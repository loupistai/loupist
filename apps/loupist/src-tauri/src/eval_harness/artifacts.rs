//! Frozen eval-only assets; candidate overrides never enter a production build.
use super::{PiArtifacts, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

// Admission registers exact immutable bytes, not a directory-wide bypass.
static REGISTERED: OnceLock<Mutex<BTreeMap<PathBuf, Vec<u8>>>> = OnceLock::new();
pub(crate) fn registered_bytes(path: &Path) -> Option<Vec<u8>> {
    REGISTERED.get()?.lock().ok()?.get(path).cloned()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ContextPolicy {
    pub reserve_tokens: u64,
    pub keep_recent_tokens: u64,
}
impl Default for ContextPolicy {
    fn default() -> Self {
        Self {
            reserve_tokens: 16_384,
            keep_recent_tokens: 20_000,
        }
    }
}

fn editable(name: &str) -> bool {
    name == "SYSTEM.md"
        || name == "tool-catalog.json"
        || (name.starts_with("skills/")
            && name.ends_with("/SKILL.md")
            && name.split('/').count() == 3)
}

fn lock_regions(text: &str) -> Result<Vec<&str>> {
    let mut regions = Vec::new();
    let mut remaining = text;
    while let Some((before, after)) = remaining.split_once("<!-- safety:locked -->") {
        if before.contains("<!-- /safety:locked -->") {
            return Err("unmatched safety lock".into());
        }
        let (region, rest) = after
            .split_once("<!-- /safety:locked -->")
            .ok_or("unclosed safety lock")?;
        if region.contains("<!-- safety:locked -->") {
            return Err("nested safety lock".into());
        }
        regions.push(region);
        remaining = rest;
    }
    if remaining.contains("<!-- /safety:locked -->") {
        return Err("unmatched safety lock".into());
    }
    Ok(regions)
}

fn frontmatter(text: &str) -> Result<&str> {
    let rest = text
        .strip_prefix("---\n")
        .ok_or("missing Skill frontmatter")?;
    Ok(rest
        .split_once("\n---\n")
        .ok_or("unclosed Skill frontmatter")?
        .0)
}

fn without_descriptions(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                if key == "description" && value.is_string() {
                    *value = serde_json::json!("");
                } else {
                    without_descriptions(value);
                }
            }
        }
        serde_json::Value::Array(values) => values.iter_mut().for_each(without_descriptions),
        _ => {}
    }
}

fn validate_edit(name: &str, original: &[u8], candidate: &[u8]) -> Result<()> {
    if candidate == original {
        return Ok(());
    }
    if !editable(name) || candidate.len() > 1024 * 1024 || candidate.contains(&0) {
        return Err(format!("non-editable or invalid agent asset: {name}").into());
    }
    let old = std::str::from_utf8(original)?;
    let new = std::str::from_utf8(candidate)?;
    if name == "SYSTEM.md" {
        let regions = lock_regions(old)?;
        if regions.is_empty() || regions != lock_regions(new)? {
            return Err("safety locked regions changed or missing".into());
        }
    } else if name == "tool-catalog.json" {
        let mut old: serde_json::Value = serde_json::from_str(old)?;
        let mut new: serde_json::Value = serde_json::from_str(new)?;
        without_descriptions(&mut old);
        without_descriptions(&mut new);
        if old != new {
            return Err("only catalog description strings are editable".into());
        }
    } else if frontmatter(old)? != frontmatter(new)? {
        return Err("Skill frontmatter is immutable".into());
    }
    Ok(())
}

fn read_overrides(root: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    fn visit(root: &Path, dir: &Path, files: &mut BTreeMap<String, Vec<u8>>) -> Result<()> {
        for item in fs::read_dir(dir)? {
            let item = item?;
            let kind = item.file_type()?;
            if kind.is_symlink() {
                return Err("symlinks are forbidden in agent overrides".into());
            }
            if kind.is_dir() {
                visit(root, &item.path(), files)?;
            } else if kind.is_file() {
                let name = item
                    .path()
                    .strip_prefix(root)?
                    .to_string_lossy()
                    .replace('\\', "/");
                if !(editable(&name) && FILES.iter().any(|(n, _)| *n == name))
                    && name != "context-policy.json"
                {
                    return Err(format!("unsupported override asset: {name}").into());
                }
                if item.metadata()?.len() > 1024 * 1024 {
                    return Err("agent override exceeds size limit".into());
                }
                files.insert(name, fs::read(item.path())?);
            } else {
                return Err("non-regular override asset".into());
            }
        }
        Ok(())
    }
    if fs::symlink_metadata(root)?.file_type().is_symlink() {
        return Err("override root cannot be a symlink".into());
    }
    let mut files = BTreeMap::new();
    visit(root, root, &mut files)?;
    Ok(files)
}

macro_rules! asset {
    ($path:literal) => {
        (
            $path,
            include_bytes!(concat!("../../../agent/", $path)) as &[u8],
        )
    };
}
// Keep the operational bundle explicit. The runtime's validators below enforce
// the production extension and every bundled Skill, including its references.
const FILES: &[(&str, &[u8])] = &[
    asset!("SYSTEM.md"),
    asset!("broker-client.mjs"),
    asset!("loupist-extension.mjs"),
    asset!("loupist-native-search.mjs"),
    asset!("loupist-provider-extension.mjs"),
    asset!("model-run-controls.mjs"),
    asset!("native-search/anthropic.mjs"),
    asset!("native-search/codex.mjs"),
    asset!("native-search/common.mjs"),
    asset!("native-search/xai.mjs"),
    asset!("pi-settings.mjs"),
    asset!("prompts/briefing.md"),
    asset!("prompts/method-learning.md"),
    asset!("provider-inference.mjs"),
    asset!("skills/NOTICE.md"),
    asset!("skills/comparison/SKILL.md"),
    asset!("skills/comparison/references/peer-analysis.md"),
    asset!("skills/data-analysis/SKILL.md"),
    asset!("skills/data-analysis/references/validation.md"),
    asset!("skills/dividend-income/SKILL.md"),
    asset!("skills/dividend-income/references/dividend-checks.md"),
    asset!("skills/documents/SKILL.md"),
    asset!("skills/documents/references/docx-workflow.md"),
    asset!("skills/events-stats/SKILL.md"),
    asset!("skills/events-stats/references/inference.md"),
    asset!("skills/filings/SKILL.md"),
    asset!("skills/filings/references/earnings-review.md"),
    asset!("skills/filings/references/korea-statements.md"),
    asset!("skills/fund-compare/SKILL.md"),
    asset!("skills/fund-compare/references/fund-checks.md"),
    asset!("skills/goal-math/SKILL.md"),
    asset!("skills/goal-math/references/goal-assumptions.md"),
    asset!("skills/journal/SKILL.md"),
    asset!("skills/lens/SKILL.md"),
    asset!("skills/licenses/Apache-2.0-financial-services.txt"),
    asset!("skills/licenses/Apache-2.0-knowledge-work-plugins.txt"),
    asset!("skills/licenses/MIT-qlib.txt"),
    asset!("skills/licenses/MIT-scientific-agent-skills.txt"),
    asset!("skills/pdf/SKILL.md"),
    asset!("skills/pdf/references/pdf-workflow.md"),
    asset!("skills/portfolio-risk/SKILL.md"),
    asset!("skills/portfolio-risk/references/risk-review.md"),
    asset!("skills/presentations/SKILL.md"),
    asset!("skills/presentations/references/deck-workflow.md"),
    asset!("skills/research/SKILL.md"),
    asset!("skills/research/references/thesis-review.md"),
    asset!("skills/rule-test/SKILL.md"),
    asset!("skills/rule-test/references/backtest-design.md"),
    asset!("skills/skill-creator/LICENSE.txt"),
    asset!("skills/skill-creator/SKILL.md"),
    asset!("skills/skill-installer/LICENSE.txt"),
    asset!("skills/skill-installer/SKILL.md"),
    asset!("skills/spreadsheets/SKILL.md"),
    asset!("skills/spreadsheets/references/workbook-workflow.md"),
    asset!("skills/valuation/SKILL.md"),
    asset!("skills/valuation/references/dcf.md"),
    asset!("skills/visualize/SKILL.md"),
    asset!("skills/why-it-moved/SKILL.md"),
    asset!("skills/why-it-moved/references/move-attribution.md"),
    asset!("skills/world/SKILL.md"),
    asset!("tool-catalog.json"),
    asset!("workspace-resources.mjs"),
    asset!("workspace-tools.mjs"),
];

pub(super) struct FrozenArtifacts {
    _directory: tempfile::TempDir,
    pub pi: PiArtifacts,
    pub sha256: String,
    pub context_policy: ContextPolicy,
}
impl FrozenArtifacts {
    #[cfg(test)]
    pub fn new(pi: PiArtifacts) -> Result<Self> {
        Self::with_override(pi, None)
    }
    pub fn config_toml(&self) -> String {
        format!("[pi.retry]\nenabled = false\nmax_retries = 0\n[pi.compaction]\nreserve_tokens = {}\nkeep_recent_tokens = {}\n", self.context_policy.reserve_tokens, self.context_policy.keep_recent_tokens)
    }
    pub fn with_override(mut pi: PiArtifacts, root: Option<&Path>) -> Result<Self> {
        let overrides = root.map(read_overrides).transpose()?.unwrap_or_default();
        let context_policy: ContextPolicy = overrides
            .get("context-policy.json")
            .map(|bytes| serde_json::from_slice(bytes))
            .transpose()?
            .unwrap_or_default();
        if !(8192..=24576).contains(&context_policy.reserve_tokens)
            || !(10000..=30000).contains(&context_policy.keep_recent_tokens)
        {
            return Err("context policy must remain within 50 percent of defaults".into());
        }
        let directory = tempfile::Builder::new()
            .prefix("loupist-eval-agent-")
            .tempdir_in(if cfg!(unix) {
                PathBuf::from("/tmp")
            } else {
                std::env::temp_dir()
            })?;
        let mut digest_input = serde_json::to_vec(&context_policy)?;
        let mut registration = BTreeMap::new();
        for (name, bundled) in FILES {
            let bytes = overrides.get(*name).map(Vec::as_slice).unwrap_or(bundled);
            validate_edit(name, bundled, bytes)?;
            let path = directory.path().join(name);
            fs::create_dir_all(path.parent().ok_or("agent asset has no parent")?)?;
            fs::write(&path, bytes)?;
            registration.insert(path.clone(), bytes.to_vec());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o400))?;
            }
            digest_input.extend_from_slice(&(name.len() as u64).to_be_bytes());
            digest_input.extend_from_slice(name.as_bytes());
            digest_input.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
            digest_input.extend_from_slice(bytes);
        }
        pi.extension = directory.path().join("loupist-extension.mjs");
        pi.provider_extension = directory.path().join("loupist-provider-extension.mjs");
        pi.system_prompt = directory.path().join("SYSTEM.md");
        let frozen = Self {
            _directory: directory,
            pi,
            sha256: crate::util::sha256(&digest_input),
            context_policy,
        };
        REGISTERED
            .get_or_init(Mutex::default)
            .lock()
            .map_err(|_| "eval asset lock poisoned")?
            .extend(registration);
        crate::agent_harness::validate_extension_bundle(&frozen.pi.extension)?;
        crate::agent_harness::validate_provider_extension_bundle(&frozen.pi.provider_extension)?;
        let skills = crate::agent_harness::skill_catalog(&[])
            .into_iter()
            .map(|s| s.id)
            .collect::<Vec<_>>();
        crate::agent_harness::resolve_skill_paths(frozen._directory.path(), &skills)?;
        Ok(frozen)
    }
}

impl Drop for FrozenArtifacts {
    fn drop(&mut self) {
        if let Some(registry) = REGISTERED.get() {
            if let Ok(mut registry) = registry.lock() {
                registry.retain(|path, _| !path.starts_with(self._directory.path()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pi() -> PiArtifacts {
        PiArtifacts {
            executable: PathBuf::from("unused-pi"),
            runtime_dir: PathBuf::from("unused-runtime"),
            extension: PathBuf::new(),
            provider_extension: PathBuf::new(),
            system_prompt: PathBuf::new(),
            provider: String::new(),
            model: String::new(),
            thinking_level: String::new(),
            run_options: Default::default(),
            provider_credential: None,
        }
    }
    #[test]
    fn frozen_assets_validate_against_runtime_authority_and_have_stable_content_hashes() {
        let pi = pi();
        let first = FrozenArtifacts::new(pi.clone()).unwrap();
        let second = FrozenArtifacts::new(pi).unwrap();
        assert_ne!(first.pi.system_prompt, second.pi.system_prompt);
        assert_eq!(first.sha256, second.sha256);
        assert_eq!(first.sha256.len(), 64);
        assert_eq!(
            fs::read(&first.pi.system_prompt).unwrap(),
            include_bytes!("../../../agent/SYSTEM.md")
        );
        let path = first.pi.system_prompt.clone();
        drop(first);
        assert!(
            !path.exists(),
            "batch assets must be removed when the batch ends"
        );
        assert!(
            second.pi.system_prompt.exists(),
            "independent batches do not share asset lifetime"
        );
    }
    #[test]
    fn eval_asset_override_validates_and_freezes_skill_bodies_and_context() {
        let input = tempfile::tempdir().unwrap();
        let name = "skills/data-analysis/SKILL.md";
        let original =
            std::str::from_utf8(FILES.iter().find(|(n, _)| *n == name).unwrap().1).unwrap();
        fs::create_dir_all(input.path().join("skills/data-analysis")).unwrap();
        let candidate = format!("{original}\nCheck arithmetic twice.\n");
        fs::write(input.path().join(name), &candidate).unwrap();
        fs::write(
            input.path().join("context-policy.json"),
            r#"{"reserve_tokens":8192,"keep_recent_tokens":10000}"#,
        )
        .unwrap();
        let frozen = FrozenArtifacts::with_override(pi(), Some(input.path())).unwrap();
        let root = frozen.pi.system_prompt.parent().unwrap();
        crate::agent_harness::validate_skill_path(root, &root.join(name)).unwrap();
        assert!(frozen.config_toml().contains("reserve_tokens = 8192"));
        assert_eq!(fs::read_to_string(root.join(name)).unwrap(), candidate);
        assert_ne!(frozen.sha256, FrozenArtifacts::new(pi()).unwrap().sha256);
        fs::write(input.path().join(name), "changed after freeze").unwrap();
        assert_eq!(fs::read_to_string(root.join(name)).unwrap(), candidate);
        let registered = root.join(name);
        assert!(registered_bytes(&registered).is_some());
        drop(frozen);
        assert!(registered_bytes(&registered).is_none());
    }

    #[test]
    fn eval_asset_override_rejects_authority_frontmatter_and_unsafe_files() {
        let original =
            b"Before\n\n<!-- safety:locked -->\nNever trade.\n<!-- /safety:locked -->\n\nAfter\n";
        assert!(validate_edit("SYSTEM.md", original, b"No locks\n").is_err());
        assert!(validate_edit("SYSTEM.md", original, b"Improved workflow\n\n<!-- safety:locked -->\nNever trade.\n<!-- /safety:locked -->\n\nAfter\n").is_ok());
        assert!(validate_edit(
            "SYSTEM.md",
            original,
            b"Before\n\n<!-- safety:locked -->\nTrade.\n<!-- /safety:locked -->\n\nAfter\n"
        )
        .is_err());
        assert!(validate_edit(
            "skills/lens/SKILL.md",
            b"---\nname: lens\n---\nBody\n",
            b"---\nname: other\n---\nBody\n"
        )
        .is_err());
        let input = tempfile::tempdir().unwrap();
        fs::write(input.path().join("loupist-extension.mjs"), "bad").unwrap();
        assert!(FrozenArtifacts::with_override(pi(), Some(input.path())).is_err());
        fs::remove_file(input.path().join("loupist-extension.mjs")).unwrap();
        fs::write(
            input.path().join("context-policy.json"),
            r#"{"reserve_tokens":1,"keep_recent_tokens":20000}"#,
        )
        .unwrap();
        assert!(FrozenArtifacts::with_override(pi(), Some(input.path())).is_err());
    }
    #[test]
    fn eval_asset_override_reaches_pi_prompt_and_rejects_unregistered_copies() {
        let input = tempfile::tempdir().unwrap();
        let prompt = format!(
            "{}\nAdditional workflow.\n",
            include_str!("../../../agent/SYSTEM.md")
        );
        fs::write(input.path().join("SYSTEM.md"), &prompt).unwrap();
        let frozen = FrozenArtifacts::with_override(pi(), Some(input.path())).unwrap();
        let root = frozen.pi.system_prompt.parent().unwrap();
        let config = crate::pi::PiLaunchConfig {
            executable: "unused".into(),
            runtime_dir: "unused".into(),
            extension: frozen.pi.extension.clone(),
            system_prompt: frozen.pi.system_prompt.clone(),
            agent_data_dir: root.join("agent"),
            working_dir: root.join("cwd"),
            private_run_dir: root.join("run"),
            lease_dir: root.join("leases"),
            broker_socket: root.join("broker.sock"),
            broker_token: "test".into(),
            provider: "openai-codex".into(),
            model: "test".into(),
            thinking_level: "medium".into(),
            pi_settings: Default::default(),
            run_options: Default::default(),
            provider_credential: None,
            host_context: Some("{}".into()),
            skill_files: vec![],
            session: None,
        };
        let args = config.rpc_arguments();
        let index = args
            .iter()
            .position(|arg| arg == "--system-prompt")
            .unwrap();
        assert_eq!(args[index + 1], prompt);
        let mut unrelated = config;
        unrelated.system_prompt = input.path().join("SYSTEM.md");
        let args = unrelated.rpc_arguments();
        assert_eq!(args[index + 1], include_str!("../../../agent/SYSTEM.md"));
    }
}
