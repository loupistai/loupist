use super::*;

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
