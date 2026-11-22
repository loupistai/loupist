use super::*;
use crate::agent_harness::native_policy::NativeRoots;

#[tokio::test]
async fn workspace_capture_retains_exact_bytes_with_local_attribution_after_acknowledgment() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = bare_executor(&temporary);
    let directory = temporary.path().join("user-research");
    fs::create_dir(&directory).unwrap();
    let contents = "period,revenue\n2026-Q2,500\n";
    fs::write(directory.join("filing.csv"), contents).unwrap();
    seed_profile(
        executor.state.store.as_ref(),
        &profile("loupist-a", &directory, 1),
    );
    executor
        .state
        .store
        .create_chat(&chat("chat-a", "loupist-a", 1))
        .unwrap();
    let policy = policy(true, true);
    let result = executor
        .execute_for_delivery(
            &policy,
            ToolMethod::WorkspaceCapture,
            json!({"path":"filing.csv"}),
            "capture",
        )
        .await
        .unwrap();
    assert!(result["next"].as_str().unwrap().contains("source_save"));
    let read_only = executor
        .execute_for_delivery(
            &super::policy(true, false),
            ToolMethod::WorkspaceCapture,
            json!({"path":"filing.csv"}),
            "read-only",
        )
        .await
        .unwrap();
    assert!(read_only.get("next").is_none());
    let reference = result["result_ref"].as_str().unwrap();
    let save = json!({"result_ref": reference, "title":"Local filing", "coverage":"included_full"});
    assert!(executor
        .stage_source_save(&policy, save.clone(), "save")
        .await
        .is_err());
    executor.commit_delivery(&policy, "capture").await.unwrap();
    let results = executor.run_results_list(json!({})).await.unwrap();
    assert_eq!(results["results"][0]["source_save_allowed"], true);
    // A subsequent filesystem edit cannot change the captured evidence.
    fs::write(directory.join("filing.csv"), "changed").unwrap();
    executor
        .stage_source_save(&policy, save, "save")
        .await
        .unwrap();
    executor.commit_delivery(&policy, "save").await.unwrap();
    let staged = executor.capture.staged_sources.lock().await;
    assert_eq!(staged.len(), 1);
    assert_eq!(staged[0].bytes, contents.as_bytes());
    assert_eq!(
        staged[0].source_class,
        crate::knowledge::SourceClass::UserFile
    );
    assert_eq!(staged[0].origin, "workspace:filing.csv");
    assert_eq!(staged[0].media_type, "text/csv");
    assert!(executor
        .workspace_capture(json!({"path":"filing.csv","expected_sha256":result["content_sha256"]}))
        .is_err());
    assert!(executor
        .workspace_capture(json!({"path":"../outside"}))
        .is_err());
    // What a native read is refused stays out of reach here too.
    fs::create_dir_all(directory.join(".loupist")).unwrap();
    fs::write(directory.join(".loupist/activity.log"), "private").unwrap();
    fs::create_dir_all(directory.join(".git")).unwrap();
    fs::write(directory.join(".git/config"), "[remote]").unwrap();
    for protected in [".loupist/activity.log", ".git/config", "notes/.env"] {
        let error = executor
            .workspace_capture(json!({"path": protected}))
            .unwrap_err();
        assert!(
            matches!(&error, BrokerError::WithNext { source, .. } if matches!(**source, BrokerError::ProtectedPath)),
            "{protected}: {error:?}"
        );
    }
}

#[tokio::test]
async fn c014_workspace_capture_projects_attachment_names_and_message_dates() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let workspace = temporary.path().join("loupist");
    let app = temporary.path().join("app");
    let attachments = workspace.join(".loupist/loupists/loupist-a/attachments/chat-a");
    let original = state.store.get_chat("chat-a").unwrap().unwrap();
    let mut attached = original.clone();
    let contents = "period,revenue\n2026-Q2,500\n";
    for (index, (name, millis)) in [
        ("audit-note.txt", 1_795_269_600_000),
        ("audit-units.csv", 1_795_269_720_000),
        ("audit-units.csv", 1_795_269_840_000),
    ]
    .into_iter()
    .enumerate()
    {
        let message_id = format!("message-{index}");
        let attachment = crate::domain::ChatAttachment {
            id: format!("attachment-{index}"),
            filename: name.into(),
            media_type: "text/plain".into(),
            size_bytes: contents.len() as u64,
            content_sha256: sha256(contents.as_bytes()),
        };
        let folder = attachments.join(&message_id);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(&attachment.id), contents).unwrap();
        let mut message =
            crate::test_fixtures::chat_message(&message_id, crate::domain::ChatRole::User, "File");
        message.created_at_ms = millis;
        message.attachments.push(attachment);
        attached.messages.push(message);
    }
    state.store.replace_chat(&original, &attached).unwrap();
    fs::create_dir_all(workspace.join("outputs")).unwrap();
    fs::write(workspace.join("outputs/report.csv"), contents).unwrap();
    let streamed = Arc::new(std::sync::Mutex::new(Vec::new()));
    let live = streamed.clone();
    let capture = Arc::new(ToolCapture {
        bound_chat_id: Some("chat-a".into()),
        evidence_sink: Some(Arc::new(move |evidence, _| {
            live.lock().unwrap().push(evidence.snapshot);
        })),
        file_labels: crate::chat::local_files::FileLabels::for_chat(&attached),
        native: NativeGate::for_turn(NativeRoots::new(
            &workspace,
            &app,
            &app,
            &workspace.join(".loupist"),
            &app.join("scratch"),
            &attachments,
            None,
        )),
        ..ToolCapture::default()
    });
    let executor = executor_for(&state, capture.clone());
    let mut results = Vec::new();
    for path in [
        ".loupist/loupists/loupist-a/attachments/chat-a/message-0/attachment-0",
        ".loupist/loupists/loupist-a/attachments/chat-a/message-1/attachment-1",
        ".loupist/loupists/loupist-a/attachments/chat-a/message-2/attachment-2",
        "outputs/report.csv",
    ] {
        results.push(
            executor
                .execute(
                    &policy(true, true),
                    ToolMethod::WorkspaceCapture,
                    json!({"path":path}),
                )
                .await
                .unwrap(),
        );
    }
    let stored = capture.evidence_refs.lock().await.clone();
    let refs = streamed.lock().unwrap().clone();
    let mut before_duplicate = attached.clone();
    before_duplicate
        .messages
        .retain(|message| message.id != "message-2");
    let mut first = stored[1].clone();
    crate::chat::local_files::FileLabels::for_chat(&before_duplicate).project_evidence(&mut first);
    assert_eq!(first.title, "audit-units.csv");
    assert_eq!(refs.len(), 4);
    assert_eq!(refs[0].title, "audit-note.txt");
    assert_eq!(refs[1].title, "audit-units.csv, attached Nov 21 14:02 UTC");
    assert_eq!(refs[2].title, "audit-units.csv, attached Nov 21 14:04 UTC");
    assert_eq!(refs[3].title, "outputs/report.csv");
    let expected = &refs[1].title;
    executor
        .stage_source_save(
            &policy(true, true),
            json!({
                "result_ref": results[1]["result_ref"],
                "title": results[1]["path"],
                "coverage": "included_full"
            }),
            "save-attachment",
        )
        .await
        .unwrap();
    executor
        .commit_delivery(&policy(true, true), "save-attachment")
        .await
        .unwrap();
    assert_eq!(capture.staged_sources.lock().await[0].title, *expected);
    let mut old = refs[1].clone();
    old.title = ".loupist/loupists/loupist-a/attachments/chat-a/message-1/attachment-1".into();
    let mut answer = crate::test_fixtures::chat_message(
        "answer",
        crate::domain::ChatRole::Assistant,
        "Read the file.",
    );
    let mut progress = crate::chat::progress::ChatProgressProjection::new(1);
    progress.start_tool(
        "legacy",
        "workspace_capture",
        &json!({"path":old.title}),
        None,
        2,
    );
    progress.finish_tool("legacy", false, 3, None);
    answer.progress = progress.finish(4, false);
    answer.evidence_refs = vec![old, stored[2].clone()];
    attached.messages.push(answer);
    let restored = crate::commands::types::chat_dto(&attached).unwrap();
    let restored = restored.messages.last().unwrap();
    assert_eq!(restored.evidence_refs[0].snapshot.title, *expected);
    assert_eq!(restored.evidence_refs[1].snapshot.title, refs[2].title);
    assert!(matches!(&restored.progress.as_ref().unwrap().items[0],
        crate::chat::progress::ChatProgressItem::Tool { target: Some(title), .. } if title == expected));
}
