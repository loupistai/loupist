use crate::app::{AppState, CommandError, PiArtifacts};

pub(crate) const MODEL_PROFILE: &str = "e2e-link-research";

fn enabled() -> bool {
    std::env::var_os("LOUPIST_E2E_SLOW_HANDOFF").as_deref() == Some(std::ffi::OsStr::new("1"))
}

fn directory(state: &AppState) -> std::path::PathBuf {
    state.artifacts.app_data_dir.join("e2e-slow-handoff")
}

pub(crate) fn prepare(state: &AppState) -> Result<(), CommandError> {
    if !enabled() {
        return Ok(());
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
    {
        crate::commands::chat_fixture::install(
            &directory(state),
            include_str!("tests/fixtures/research_open_pi.py"),
        )?;
        Ok(())
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64", debug_assertions)))]
    {
        let _ = state;
        Err(CommandError::new(
            "pi_unavailable",
            "The slow hand-off fixture requires a macOS Apple Silicon debug build",
        ))
    }
}

pub(crate) fn select(
    state: &AppState,
    model_profile_id: &str,
    host_handoff: bool,
    mut artifacts: PiArtifacts,
) -> Result<PiArtifacts, CommandError> {
    if enabled() && host_handoff && model_profile_id == MODEL_PROFILE {
        let directory = directory(state);
        artifacts.executable = directory.join("fixture-pi");
        artifacts.runtime_dir = directory.join("fixture-runtime");
        artifacts.provider_credential = None;
        if !artifacts.executable.is_file() {
            return Err(CommandError::new(
                "pi_unavailable",
                "The slow hand-off fixture is missing",
            ));
        }
    }
    Ok(artifacts)
}
