use crate::{
    app::{AppState, CommandError},
    runs::coordinator::{PendingMemoryWrite, RunRegistration, RunTarget},
};

use super::new_id;

pub(super) fn reserve_chat_memory_finalization(
    state: &AppState,
    loupist_id: &str,
) -> Result<(String, PendingMemoryWrite), CommandError> {
    let write_id = new_id("memory-write");
    let pending_writer = state.reserve_memory_write(
        write_id.clone(),
        loupist_id.to_owned(),
        RunTarget::MemoryWriteSession(write_id.clone()),
    )?;
    Ok((write_id, pending_writer))
}

pub(super) fn apply_chat_memory_update_with_registered_finalize<T>(
    state: &AppState,
    loupist_id: &str,
    // The caller retains the writer through terminal persistence and emission,
    // including rollback/failure. Observers must not see an idle run first.
    _memory_writer: &RunRegistration,
    finalize: impl FnOnce() -> Result<T, CommandError>,
) -> Result<T, CommandError> {
    state.ensure_loupist_available(loupist_id)?;
    finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn failed_finalization_keeps_writer_active_until_terminal_handling_finishes() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        crate::commands::tests::seed_profile(
            state.store.as_ref(),
            &crate::commands::tests::profile("loupist-finalization", &workspace, 1),
        );
        let (_, pending) =
            reserve_chat_memory_finalization(&state, "loupist-finalization").unwrap();
        let writer = pending.wait().await.unwrap();
        let result: Result<(), CommandError> = apply_chat_memory_update_with_registered_finalize(
            &state,
            "loupist-finalization",
            &writer,
            || Err(CommandError::internal("fixture transaction failure")),
        );
        assert!(result.is_err());
        assert_eq!(
            state.run_coordinator.active_count(),
            1,
            "rollback must not announce idle before the failed assistant can be persisted"
        );
        drop(writer);
        assert_eq!(state.run_coordinator.active_count(), 0);
    }
}
