//! Headless release preflight: the real host launch paths, no app profile or model call.
use std::{collections::BTreeMap, ffi::OsString, path::Path, time::Duration};

use serde_json::json;

use crate::{
    agent_harness::native_policy::NativeRoots,
    mcp::{discover_bundled_runtimes, McpLaunchConfig, McpSession},
    pi::{PiEvent, PiProcess, PiSupportLaunchConfig},
    web::SearchCancel,
};

pub(crate) const FLAG: &str = "--check-bundled-helpers";

pub(crate) fn requested(
    arguments: impl IntoIterator<Item = OsString>,
) -> Option<Result<(), String>> {
    let mut arguments = arguments.into_iter().skip(1);
    if arguments.next().as_deref() != Some(std::ffi::OsStr::new(FLAG)) {
        return None;
    }
    if arguments.next().is_some() {
        return Some(Err(
            "helper preflight accepts no additional arguments".into()
        ));
    }
    if cfg!(debug_assertions) {
        return Some(Err("helper preflight requires a release build".into()));
    }
    Some(run())
}

fn run() -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|error| error.to_string())?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let resources = executable
        .parent()
        .and_then(Path::parent)
        .ok_or("app Contents directory is missing")?
        .join("Resources");
    let temporary =
        crate::fs::artifact_trust::private_temporary_directory("loupist-helper-preflight-")
            .map_err(|error| error.to_string())?;
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?
        .block_on(check_helpers(&resources, temporary.path()))
}

async fn check_helpers(resources: &Path, scratch: &Path) -> Result<(), String> {
    let pi_runtime = resources.join("pi-runtime");
    let leases = scratch.join("leases");
    // This uses Pi's real metadata/tree checks, signature gate and process owner.
    // get_state is local RPC; it never requests a model response or signs in.
    let pi = PiProcess::spawn_support(PiSupportLaunchConfig {
        executable: pi_runtime.join("loupist-pi"),
        runtime_dir: pi_runtime.clone(),
        extension: resources.join("loupist-agent/loupist-provider-extension.mjs"),
        agent_data_dir: scratch.join("pi-agent"),
        private_working_dir: scratch.join("pi-cwd"),
        lease_dir: leases.clone(),
        result_file: scratch.join("pi-result.json"),
        request_file: None,
        provider_credential: None,
        mutation_api_key: None,
    })
    .await
    .map_err(|error| format!("Pi helper preflight: {error}"))?;
    let mut events = pi.subscribe();
    let id = pi.get_state().await.map_err(|error| error.to_string())?;
    let state = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            match events.recv().await {
                Ok(PiEvent::Rpc { payload }) if payload["id"].as_u64() == Some(id) => {
                    return payload["success"].as_bool() == Some(true)
                }
                Ok(PiEvent::Exited | PiEvent::ProtocolError { .. }) | Err(_) => return false,
                _ => {}
            }
        }
    })
    .await
    .unwrap_or(false);
    pi.shutdown_settled()
        .await
        .map_err(|error| error.to_string())?;
    if !state {
        return Err("Pi did not answer local RPC".into());
    }

    // Use the same bundled MCP entry point as the app, including the trust gate,
    // private bootstrap, protocol negotiation and tool inventory. No provider is
    // enabled and no network host is admitted; available_categories is local.
    let runtime = discover_bundled_runtimes(&pi_runtime, &leases)
        .remove("openbb")
        .ok_or("bundled OpenBB manifest is unavailable")?;
    let working = scratch.join("openbb");
    // The sidecar refuses a scratch directory that is not private (mode 0700);
    // the app creates it this way before every run.
    crate::fs::artifact_trust::ensure_private_directory(&working)
        .map_err(|_| "OpenBB helper preflight: scratch directory is not private")?;
    let (session, tools) = McpSession::spawn(McpLaunchConfig {
        server_id: "openbb".into(), executable: runtime.executable, arguments: Vec::new(),
        private_working_dir: working.clone(), lease_dir: leases,
        environment: BTreeMap::new(), bootstrap: json!({
            "type": "loupist.bootstrap", "protocol_version": 1, "run_id": "release-helper-preflight",
            "scratch_dir": working, "credentials": {}, "settings": {
                "allowed_categories": runtime.allowed_categories,
                "enabled_provider_ids": [], "allowed_network_hosts": [], "provider_config": {}
            }
        }),
    }).await.map_err(|error| format!("OpenBB helper preflight: {error}"))?;
    let inventory = tools
        .iter()
        .map(|tool| tool.name.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let result = session
        .call_tool("available_categories", json!({}), Duration::from_secs(15))
        .await;
    session
        .shutdown(Duration::from_secs(2))
        .await
        .map_err(|error| error.to_string())?;
    if inventory != runtime.control_tool_names
        || result.map_err(|error| error.to_string())?.is_error
    {
        return Err("OpenBB local tool preflight failed".into());
    }

    let workspace = scratch.join("workspace");
    let app_data = scratch.join("app-data");
    let attachments = scratch.join("attachments");
    let run_scratch = scratch.join("python-scratch");
    let global = scratch.join("global");
    for path in [&workspace, &app_data, &attachments, &run_scratch, &global] {
        std::fs::create_dir(path).map_err(|error| error.to_string())?;
    }
    let roots = NativeRoots::new(
        &workspace,
        &global,
        &app_data,
        &workspace.join(".loupist"),
        &run_scratch,
        &attachments,
        Some(scratch),
    );
    // The actual run_python snapshot, worker trust check and supervisor execute
    // this constant in WASI, with only disposable state and no writable grants.
    let result = crate::python_run::execute(
        &roots,
        &scratch.join("cache"),
        "print('loupist-release-helper-ok')",
        120,
        SearchCancel::never(),
    )
    .await
    .map_err(|error| format!("Python helper preflight: {error}"))?;
    if result["status"] != "completed"
        || result["stdout"].as_str().map(str::trim) != Some("loupist-release-helper-ok")
    {
        return Err("Python worker did not complete the offline probe".into());
    }
    println!("Release helper preflight passed: Pi RPC, OpenBB local tool, run_python");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_exact_headless_flag_is_recognized_and_debug_cannot_qualify() {
        let args = |values: &[&str]| values.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(requested(args(&["app"])).is_none());
        assert!(requested(args(&["app", "--other"])).is_none());
        assert!(requested(args(&["app", FLAG, "/arbitrary/helper"]))
            .unwrap()
            .is_err());
        assert!(requested(args(&["app", FLAG]))
            .unwrap()
            .unwrap_err()
            .contains("release build"));
    }

    /// The same helper sequence the release build runs from the mounted DMG,
    /// against the runtimes staged in this checkout. Debug builds have no
    /// signature gate, so this covers everything except the signer check: the
    /// first release dry run failed here on a scratch directory the sidecar
    /// refused. Skipped where the runtimes are not staged.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_helper_sequence_passes_against_the_staged_runtimes() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let staged = manifest.join("resources/pi-runtime");
        if !staged.join("loupist-pi").is_file() || !staged.join("openbb/loupist-openbb").is_file() {
            eprintln!("skipped: runtimes are not staged in this checkout");
            return;
        }
        let temporary =
            crate::fs::artifact_trust::private_temporary_directory("loupist-helper-test-").unwrap();
        let resources = temporary.path().join("Resources");
        std::fs::create_dir(&resources).unwrap();
        std::os::unix::fs::symlink(&staged, resources.join("pi-runtime")).unwrap();
        std::os::unix::fs::symlink(manifest.join("../agent"), resources.join("loupist-agent"))
            .unwrap();
        let scratch = temporary.path().join("state");
        crate::fs::artifact_trust::ensure_private_directory(&scratch).unwrap();
        check_helpers(&resources, &scratch).await.unwrap();
    }
}
