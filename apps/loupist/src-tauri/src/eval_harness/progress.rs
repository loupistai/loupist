//! Progress notes: the commentary the production progress stream showed while
//! the model worked, in order, with the time Pi streamed each one.
use crate::chat::progress::{ChatProgress, ChatProgressItem};
use serde_json::{json, Value};

/// One assistant message's streamed text and when its first character arrived.
pub(super) struct StreamedText {
    pub at_ms: Option<i64>,
    pub text: String,
}

/// `[{at_ms, text}]` from the turn's persisted progress. A turn without a
/// progress stream has no field, never an empty list. The text is what the
/// stream kept; the time is that of the streamed message it came from, null when
/// no streamed message carries it.
pub(super) fn notes(progress: Option<&ChatProgress>, streamed: &[StreamedText]) -> Option<Value> {
    let progress = progress?;
    let mut cursor = 0;
    let notes = progress
        .items
        .iter()
        .filter_map(|item| match item {
            ChatProgressItem::Commentary { text, .. } => Some(text),
            _ => None,
        })
        .map(|text| {
            let found = streamed[cursor..]
                .iter()
                .position(|message| message.text.starts_with(text.as_str()));
            let at_ms = found.and_then(|offset| {
                cursor += offset + 1;
                streamed[cursor - 1].at_ms
            });
            json!({"at_ms": at_ms, "text": text})
        })
        .collect::<Vec<_>>();
    Some(Value::Array(notes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commentary(id: &str, text: &str) -> ChatProgressItem {
        ChatProgressItem::Commentary {
            id: id.into(),
            text: text.into(),
        }
    }

    #[test]
    fn notes_follow_the_stream_and_an_absent_stream_leaves_no_field() {
        assert!(notes(None, &[]).is_none());
        let mut progress = ChatProgress {
            started_at_ms: 1,
            finished_at_ms: Some(9),
            items: vec![],
            save_activity: None,
            plan: None,
            approvals: vec![],
            questions: vec![],
        };
        assert_eq!(notes(Some(&progress), &[]), Some(json!([])));
        progress.items = vec![
            commentary("commentary-1", "Lese den Bericht."),
            commentary("commentary-2", "Vergleiche das Quartal."),
        ];
        let streamed = [
            StreamedText {
                at_ms: Some(10),
                text: "Lese den Bericht.".into(),
            },
            StreamedText {
                at_ms: Some(20),
                text: "Ohne Notiz.".into(),
            },
            StreamedText {
                at_ms: Some(30),
                text: "Vergleiche das Quartal.".into(),
            },
        ];
        assert_eq!(
            notes(Some(&progress), &streamed),
            Some(json!([
                {"at_ms": 10, "text": "Lese den Bericht."},
                {"at_ms": 30, "text": "Vergleiche das Quartal."},
            ]))
        );
        // A note no streamed message carries keeps its text without a time.
        assert_eq!(
            notes(Some(&progress), &streamed[..1]),
            Some(json!([
                {"at_ms": 10, "text": "Lese den Bericht."},
                {"at_ms": null, "text": "Vergleiche das Quartal."},
            ]))
        );
    }

    /// Pi's events feed the production projection and the trace alike.
    #[test]
    fn the_trace_times_each_note_the_production_projection_kept() {
        use crate::chat::progress::ChatProgressProjection;
        let mut projection = ChatProgressProjection::new(1);
        let mut pi = crate::eval_harness::trace::PiTrace::default();
        let rounds = [
            ("Ищу квартальную заметку Alder Rail.", "toolUse"),
            ("Читаю найденный фрагмент.", "toolUse"),
            ("Alder Rail перевезла 18 миллионов пассажиров.", "stop"),
        ];
        for (text, stop) in rounds {
            pi.observe(&json!({"type":"message_start","message":{"role":"assistant"}}));
            projection.start_assistant_turn();
            let middle = text.char_indices().nth(text.chars().count() / 2).unwrap().0;
            for delta in [&text[..middle], &text[middle..]] {
                pi.observe(&json!({"type":"message_update",
                    "assistantMessageEvent":{"type":"text_delta","delta":delta}}));
                projection.append_commentary(delta);
            }
            pi.observe(&json!({"type":"message_end",
                "message":{"role":"assistant","stopReason":stop,"content":[{"type":"text","text":text}]}}));
            projection.finish_assistant_turn(stop == "stop");
        }
        let mut message = crate::test_fixtures::chat_message(
            "notes",
            crate::domain::ChatRole::Assistant,
            rounds[2].0,
        );
        message.progress = Some(projection.snapshot());
        let record = crate::eval_harness::trace::persisted(&message, &pi);
        let recorded = record["progress_notes"].as_array().unwrap();
        let texts: Vec<_> = recorded
            .iter()
            .map(|note| note["text"].as_str().unwrap())
            .collect();
        assert_eq!(texts, [rounds[0].0, rounds[1].0]);
        assert!(
            recorded.iter().all(|note| note["at_ms"].as_i64().is_some()),
            "{record}"
        );
        message.progress = None;
        assert!(crate::eval_harness::trace::persisted(&message, &pi)
            .get("progress_notes")
            .is_none());
    }

    /// The real host and Loupist extension with a scripted SDK provider that
    /// writes a note before each of its two tool rounds.
    #[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
    #[tokio::test]
    async fn a_scripted_turn_records_both_progress_notes_in_order() {
        use crate::eval_harness::*;
        let root = tempfile::tempdir_in("/tmp").unwrap();
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let mut state = AppState::for_eval(root.path().join("app"));
        let agent_dir = state.artifacts.app_data_dir.join("pi");
        fs::create_dir_all(&agent_dir).unwrap();
        state.eval_pi_agent_dir = Some(agent_dir);
        crate::eval_harness::tests::scripted(&mut state, root.path());
        let node = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|dir| dir.join("node"))
            .find(|path| path.is_absolute() && path.is_file())
            .expect("Node from the Loupist worktree environment");
        let module = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/eval_harness/session_language_capture.mjs");
        fs::write(
            root.path().join("session-language-capture.json"),
            json!([node, module]).to_string(),
        )
        .unwrap();
        let notes = [
            "알더 레일 분기 메모를 찾아볼게요.",
            "찾은 메모의 해당 구간을 읽을게요.",
        ];
        fs::write(
            root.path().join("progress-notes.json"),
            json!(notes).to_string(),
        )
        .unwrap();
        let mut catalog = state.model_catalog().unwrap();
        catalog.models[0].provider = "custom-faux".into();
        state.set_model_catalog(catalog).unwrap();
        let suite: Suite =
            serde_json::from_str(include_str!("../../../evals/agent/suites/language-v1.json"))
                .unwrap();
        let scenario = suite
            .scenarios
            .iter()
            .find(|scenario| scenario.id == "LG-003")
            .unwrap();
        let _clock = clock::install(scenario.clock.as_deref()).unwrap();
        let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
            .await
            .unwrap();
        seed(&state, &loupist.id, &scenario.seed).unwrap();
        let chat = new_chat(&state, &loupist.id, "research").unwrap();
        let session = &scenario.sessions[0];
        let record = run_turn(
            &state,
            &chat,
            session,
            &session.turns[0],
            "fixture",
            "medium",
            60_000,
        )
        .await
        .unwrap();
        state.shutdown().await.unwrap();
        assert_eq!(record["status"], "completed", "{record}");
        let recorded = record["progress_notes"].as_array().expect("progress_notes");
        let texts: Vec<_> = recorded
            .iter()
            .map(|note| note["text"].as_str().unwrap())
            .collect();
        assert_eq!(texts, notes);
        let times: Vec<_> = recorded
            .iter()
            .map(|note| note["at_ms"].as_i64().unwrap())
            .collect();
        assert!(times[0] <= times[1], "{times:?}");
        // The answer is not a progress note.
        assert!(!texts
            .iter()
            .any(|text| record["answer"].as_str().unwrap().contains(text)));
    }
}
