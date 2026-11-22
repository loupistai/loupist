//! Reconcile the canonical transcript before this host can use a folder.

use rusqlite::{params, Connection, TransactionBehavior};

use crate::{
    commands::{new_id, now_ms},
    domain::{ChatMessage, ChatMessageStatus, ChatRole, ChatSession, STOPPED_REPLY_CONTENT},
};

use super::{from_json, to_json, StoreError, StoreResult};

/// Called under the folder's instance lock, before the connection is published.
/// A request and any accepted steers are durable before the reply. An answer
/// awaiting Memory is already a Complete message with its interrupted receipt.
pub(super) fn stop_interrupted_turns(
    connection: &mut Connection,
    loupist_id: &str,
) -> StoreResult<()> {
    let interrupted = {
        let mut statement = connection.prepare(
            "SELECT id, data_json FROM chat_sessions WHERE loupist_id = ?1
             AND json_valid(data_json)
             AND (json_extract(data_json, '$.messages[#-1].role') = 'user' OR id = ?2)",
        )?;
        let bot_id = crate::chat::home::home_thread_id(loupist_id);
        let rows = statement.query_map(params![loupist_id, bot_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut interrupted = Vec::new();
        for row in rows {
            let (id, json) = row?;
            let Ok(mut chat) = from_json::<ChatSession>(&json) else {
                continue;
            };
            if chat.id != id || chat.loupist_id != loupist_id {
                continue;
            }
            // Bot may already show an independent delivery from a previous
            // launch. It does not answer an interrupted request of its own.
            let Some(request) = chat
                .messages
                .iter()
                .rfind(|message| !message.steer && !message.is_bot_post())
                .filter(|message| message.role == ChatRole::User)
            else {
                continue;
            };
            let finished_at_ms = now_ms().max(chat.updated_at_ms.saturating_add(1));
            let reply = ChatMessage::terminal_reply(
                new_id("message"),
                ChatMessageStatus::Aborted,
                STOPPED_REPLY_CONTENT.into(),
                finished_at_ms,
                request.as_of.clone(),
            )
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
            chat.messages.push(reply);
            chat.updated_at_ms = finished_at_ms;
            chat.pi_session_cache = None;
            if chat.validate().is_ok() {
                interrupted.push((chat, json));
            }
        }
        interrupted
    };
    if interrupted.is_empty() {
        return Ok(());
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    for (chat, expected) in interrupted {
        let changed = transaction.execute(
            "UPDATE chat_sessions SET updated_at_ms = ?2, data_json = ?3
             WHERE id = ?1 AND loupist_id = ?4 AND data_json = ?5",
            params![
                chat.id,
                chat.updated_at_ms,
                to_json(&chat)?,
                loupist_id,
                expected
            ],
        )?;
        if changed != 1 {
            return Err(StoreError::Conflict(
                "Chat changed during interruption recovery",
            ));
        }
    }
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod crash_recovery_tests {
    use super::*;
    use crate::{
        app::AppState,
        chat::{approvals, home, questions},
        domain::prepare_chat_retry,
        test_fixtures::{chat_message, chat_session, create_folder_loupist},
    };

    const LOUPIST: &str = "recovery-test";

    struct Fixture {
        root: tempfile::TempDir,
        state: AppState,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let state = AppState::for_folder_test(root.path().join("app"));
            create_folder_loupist(&state.store, LOUPIST, &root.path().join("workspace"));
            Self { root, state }
        }

        fn restart(self) -> Self {
            self.state.close_for_restart_test();
            Self {
                state: AppState::for_folder_test(self.root.path().join("app")),
                root: self.root,
            }
        }

        fn read(&self, id: &str) -> ChatSession {
            self.state
                .store
                .get_chat_in_loupist(LOUPIST, id)
                .unwrap()
                .unwrap()
        }
    }

    fn interrupted_request(id: &str) -> ChatSession {
        let mut chat = chat_session(id, LOUPIST);
        let mut request = chat_message("request", ChatRole::User, "Check the annual report.");
        request.as_of = Some("2025-12-31".into());
        chat.messages.push(request);
        chat
    }

    fn assert_stopped(original: &ChatSession, recovered: &ChatSession) {
        assert_eq!(recovered.messages.len(), original.messages.len() + 1);
        assert_eq!(
            recovered.messages[..original.messages.len()],
            original.messages
        );
        let reply = recovered.messages.last().unwrap();
        assert_eq!(reply.role, ChatRole::Assistant);
        assert_eq!(reply.status, ChatMessageStatus::Aborted);
        assert_eq!(reply.content, STOPPED_REPLY_CONTENT);
        assert_eq!(reply.as_of, original.messages[0].as_of);
        assert!(reply.progress.is_none());
    }

    #[test]
    fn research_session_recovers_once_and_preserves_any_already_stopped_draft() {
        let fixture = Fixture::new();
        let mut interrupted = interrupted_request("interrupted");
        let mut steer = chat_message("steer", ChatRole::User, "Focus on cash flow.");
        steer.steer = true;
        interrupted.messages.push(steer.clone());
        let mut already_stopped = interrupted_request("already-stopped");
        let mut draft = chat_message("draft", ChatRole::Assistant, "Cash flow rose.");
        draft.status = ChatMessageStatus::Aborted;
        already_stopped.messages.push(draft);
        fixture.state.store.create_chat(&interrupted).unwrap();
        fixture.state.store.create_chat(&already_stopped).unwrap();
        assert_eq!(fixture.read(&interrupted.id), interrupted);

        let fixture = fixture.restart();
        let recovered = fixture.read(&interrupted.id);
        assert_stopped(&interrupted, &recovered);
        let (prompt, after_retry) =
            prepare_chat_retry(&recovered, recovered.updated_at_ms).unwrap();
        assert_eq!(prompt, interrupted.messages[0].content);
        assert_eq!(after_retry.messages, [steer]);
        assert_eq!(fixture.read(&already_stopped.id), already_stopped);
        assert_eq!(fixture.restart().read(&interrupted.id), recovered);
    }

    #[test]
    fn a_bot_turn_recovers_with_an_independent_post_and_retry_keeps_that_post() {
        let fixture = Fixture::new();
        let mut chat = interrupted_request(&home::home_thread_id(LOUPIST));
        let mut post =
            home::host_message("vocabulary", "Bot now tracks this topic.".into(), 2).unwrap();
        post.vocabulary = Some(crate::domain::HomeVocabulary {
            change_id: "change".into(),
            producer: "fixture".into(),
            labels: vec!["Cash flow".into()],
            labels_ko: vec![],
        });
        chat.messages.push(post.clone());
        fixture.state.store.create_chat(&chat).unwrap();

        let fixture = fixture.restart();
        let recovered = fixture.read(&chat.id);
        assert_stopped(&chat, &recovered);
        // Bot can post while the user is away. The recovered reply remains the
        // one the user can retry, and neither delivery is removed by doing so.
        let mut after_post = recovered.clone();
        let mut later = post.clone();
        later.id = "later-delivery".into();
        after_post.messages.push(later.clone());
        let (prompt, after_retry) =
            prepare_chat_retry(&after_post, recovered.updated_at_ms).unwrap();
        assert_eq!(prompt, chat.messages[0].content);
        assert_eq!(after_retry.messages, [post, later]);
    }

    #[test]
    fn a_chat_parked_on_a_question_recovers_without_a_live_question() {
        let fixture = Fixture::new();
        let chat = interrupted_request("question");
        fixture.state.store.create_chat(&chat).unwrap();
        let question = questions::register("recovery-question", LOUPIST, Some(&chat.id), None, 2);
        assert!(questions::waiting_chats(LOUPIST).contains_key(&chat.id));
        // A process exit loses the in-memory response channel; only the
        // admitted transcript is present when the new host opens the folder.
        drop(question);
        let fixture = fixture.restart();
        assert_stopped(&chat, &fixture.read(&chat.id));
        assert!(questions::waiting_chats(LOUPIST).is_empty());
    }

    #[test]
    fn a_chat_parked_on_approval_recovers_without_a_live_approval() {
        let fixture = Fixture::new();
        let chat = interrupted_request("approval");
        fixture.state.store.create_chat(&chat).unwrap();
        let approval = approvals::register("recovery-approval", LOUPIST);
        drop(approval);
        let fixture = fixture.restart();
        assert_stopped(&chat, &fixture.read(&chat.id));
        assert!(matches!(
            approvals::decide(
                LOUPIST,
                "recovery-approval",
                approvals::ApprovalDecision::Once
            ),
            Err(approvals::DecideError::NotWaiting)
        ));
    }
}
