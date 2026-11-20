//! Compare matched inputs at Pi's provider callback, using the real host and
//! extension with a scripted SDK provider. No changes to turn composition.
use super::*;

#[tokio::test]
async fn session_language_capture_lg003_at_both_appearances() {
    let ko: Suite =
        serde_json::from_str(include_str!("../../../evals/agent/suites/language-v1.json")).unwrap();
    let en: Suite = serde_json::from_str(include_str!(
        "../../../evals/agent/suites/session-language-v1.json"
    ))
    .unwrap();
    let catalog: Value =
        serde_json::from_str(include_str!("../../../agent/tool-catalog.json")).unwrap();
    let on_demand = catalog["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == "interface_labels");

    for (suite, id, lang) in [(ko, "LG-003", "ko"), (en, "LG-003-EN", "en")] {
        let scenario = suite
            .scenarios
            .iter()
            .find(|scenario| scenario.id == id)
            .unwrap();
        assert_eq!(scenario.language.as_deref(), Some(lang));
        let _clock = clock::install(scenario.clock.as_deref()).unwrap();
        let root = tempfile::tempdir_in("/tmp").unwrap();
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let mut state = AppState::for_eval(root.path().join("app"));
        let agent_dir = state.artifacts.app_data_dir.join("pi");
        fs::create_dir_all(&agent_dir).unwrap();
        state.eval_pi_agent_dir = Some(agent_dir);
        super::tests::scripted(&mut state, root.path());
        fs::write(
            state.artifacts.app_data_dir.join("config.toml"),
            format!("[appearance]\nlanguage = {lang:?}\ndisplay_currency = \"USD\"\n"),
        )
        .unwrap();
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
        let mut catalog = state.model_catalog().unwrap();
        catalog.models[0].provider = "custom-faux".into();
        state.set_model_catalog(catalog).unwrap();
        let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
            .await
            .unwrap();
        seed(&state, &loupist.id, &scenario.seed).unwrap();
        let chat = new_chat(&state, &loupist.id, "research").unwrap();
        let session = &scenario.sessions[0];
        let turn = &session.turns[0];
        let record = run_turn(&state, &chat, session, turn, "fixture", "medium", 60_000)
            .await
            .unwrap();
        state.shutdown().await.unwrap();
        assert_eq!(record["status"], "completed", "{record}");
        assert_eq!(record["model_rounds"], 3, "{record}");
        assert_eq!(record["observed_model"], "custom-faux/fixture-model");
        assert_eq!(record["resolved_appearance"], lang);
        assert_eq!(record["display_currency"], "USD");
        assert_eq!(record["interface_label_calls"], 0);
        let calls = record["tool_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["name"], "world_search");
        assert_eq!(calls[1]["name"], "world_read");
        let source = &scenario.seed.sources[0];
        assert!(record["memory_reads"]
            .as_array()
            .unwrap()
            .iter()
            .any(|read| {
                read["kind"] == "source"
                    && read["access"] == "exact_read"
                    && read["title"] == source.title
            }));

        for phase in ["first.json", "after-read.json"] {
            let path = root.path().join(phase);
            let bytes = fs::read(&path).unwrap();
            let request: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(request["options"]["reasoning"], "medium");
            let messages = request["context"]["messages"].as_array().unwrap();
            let [user] = messages
                .iter()
                .filter(|message| message["role"] == "user")
                .collect::<Vec<_>>()[..]
            else {
                panic!("expected one user message");
            };
            let prompt = user["content"][0]["text"].as_str().unwrap();
            let (_, current) = prompt.rsplit_once("<current_user_message>\n").unwrap();
            assert_eq!(current, format!("{}\n</current_user_message>", turn.user));
            let envelope: Value = serde_json::from_str(
                prompt
                    .split_once("<turn_envelope>\n")
                    .unwrap()
                    .1
                    .split_once("\n</turn_envelope>")
                    .unwrap()
                    .0,
            )
            .unwrap();
            assert_eq!(envelope["chat_surface"]["surface"], "session");
            let interface = &envelope["interface"];
            if on_demand {
                assert_eq!(interface, &json!({"display_currency":"USD"}), "{phase}");
            } else {
                assert_eq!(interface["language"], lang);
                assert_eq!(interface["display_currency"], "USD");
                assert_eq!(interface["names"].as_object().unwrap().len(), 5);
            }
            if phase == "after-read.json" {
                let read = messages
                    .iter()
                    .find(|message| {
                        message["role"] == "toolResult" && message["toolName"] == "world_read"
                    })
                    .unwrap();
                let read: Value =
                    serde_json::from_str(read["content"][0]["text"].as_str().unwrap()).unwrap();
                assert_eq!(
                    read["text"]
                        .as_str()
                        .and_then(crate::custom_connections::taint::delimited_body),
                    Some(source.text.as_str())
                );
                assert!(read["citation_url"]
                    .as_str()
                    .is_some_and(|url| record["answer"].as_str().unwrap().contains(url)));
            }
            if let Some(output) = std::env::var_os("F3_CAPTURE_DIR").map(PathBuf::from) {
                fs::create_dir_all(&output).unwrap();
                fs::write(output.join(format!("{lang}-{phase}")), bytes).unwrap();
            }
        }
    }
}
