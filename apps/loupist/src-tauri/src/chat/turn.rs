use std::path::Path;

use crate::{
    app::CommandError,
    chat::execution_session::ChatExecutionSession,
    runs::coordinator::{PendingMemoryWrite, RunLease, RunRegistration},
    runs::scratch::RunScratch,
};

/// Owns every RAII resource whose lifetime is the active portion of one Chat
/// turn. The asynchronous turn task must own this value so command setup cannot
/// tear down files or unregister the run while Pi is still settling.
#[derive(Debug)]
pub(crate) struct ChatTurnResources {
    run_lease: Option<RunLease>,
    run_scratch: RunScratch,
    pi_session: ChatExecutionSession,
}

impl ChatTurnResources {
    pub(crate) fn new(
        run_lease: RunLease,
        run_scratch: RunScratch,
        pi_session: ChatExecutionSession,
    ) -> Self {
        Self {
            run_lease: Some(run_lease),
            run_scratch,
            pi_session,
        }
    }

    pub(crate) fn run_scratch_path(&self) -> &Path {
        self.run_scratch.path()
    }

    pub(crate) fn pi_session(&self) -> &ChatExecutionSession {
        &self.pi_session
    }

    pub(crate) fn pi_session_mut(&mut self) -> &mut ChatExecutionSession {
        &mut self.pi_session
    }

    /// Converts the turn from a shared Chat reader into its already-reserved
    /// FIFO Memory writer. The reservation must exist before this method drops
    /// the Chat lease, so no same-Loupist reader can enter between both phases.
    pub(crate) async fn handoff_to_memory_write(
        &mut self,
        pending: PendingMemoryWrite,
    ) -> (RunRegistration, Result<(), CommandError>) {
        self.run_lease = None;
        pending.wait_retaining_registration().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use crate::{
        domain::ChatSession,
        fs::secure_delete::SecureDeletionRoot,
        runs::coordinator::{RunCoordinator, RunKind, RunSpec, RunTarget},
    };

    fn chat() -> ChatSession {
        crate::test_fixtures::chat_session("chat-a", "loupist-a")
    }

    #[tokio::test]
    async fn async_owner_pins_all_turn_resources_until_task_exit() {
        let temporary = tempfile::tempdir().unwrap();
        let root =
            Arc::new(SecureDeletionRoot::open(&temporary.path().canonicalize().unwrap()).unwrap());
        let coordinator = RunCoordinator::default();
        let registration = coordinator
            .register(
                RunSpec {
                    run_id: "chat-run-a".into(),
                    loupist_id: "loupist-a".into(),
                    kind: RunKind::Chat,
                    target: RunTarget::ChatThread("chat-a".into()),
                },
                || Ok(()),
            )
            .unwrap();
        let run_scratch = RunScratch::create(root.clone(), "loupist-a", "chat-run-a").unwrap();
        let run_path = run_scratch.path().to_owned();
        let pi_session = ChatExecutionSession::prepare(root, &chat()).unwrap();
        let pi_path = pi_session.session_directory().to_owned();
        let resources = ChatTurnResources::new(registration.lease, run_scratch, pi_session);
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();

        let owner = tokio::spawn(async move {
            let _resources = resources;
            let _ = ready_tx.send(());
            let _ = release_rx.await;
        });

        ready_rx.await.unwrap();
        assert_eq!(coordinator.active_count(), 1);
        assert!(run_path.is_dir());
        assert!(pi_path.is_dir());

        release_tx.send(()).unwrap();
        owner.await.unwrap();
        assert_eq!(coordinator.active_count(), 0);
        assert!(!run_path.exists());
        assert!(pi_path.is_dir());
    }
}
