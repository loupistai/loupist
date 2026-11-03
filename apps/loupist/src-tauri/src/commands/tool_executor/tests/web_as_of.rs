use super::*;
use crate::web::script;

fn article(metadata: &str) -> String {
    format!(
        "<html><head><title>Issuer correction</title>{metadata}</head><body><article>\
         <h1>Issuer correction</h1><p>Corrected quarterly operating profit is USD 4 million, \
         replacing the previous USD 5 million. Consolidated revenue remains USD 50 million. \
         The release covers the first quarter and does not provide a future profit forecast.</p>\
         </article></body></html>"
    )
}

#[tokio::test]
async fn web_as_of_admits_a_page_published_before_the_cutoff_only_after_acknowledgment() {
    let _script = script::install(vec![(
        "example.com/published-correction",
        vec![script::ok(
            "text/html",
            article(r#"<meta property="article:published_time" content="2026-04-25T09:00:00Z">"#),
        )],
    )]);
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor_for(&seeded_state(&temporary), Arc::new(ToolCapture::default()));
    let policy = policy(true, true);
    let fetched = executor
        .execute_for_delivery(
            &policy,
            ToolMethod::WebFetch,
            json!({"url":"https://example.com/published-correction", "as_of":"2026-04-30"}),
            "accepted-fetch",
        )
        .await
        .unwrap();
    assert_eq!(fetched["published_at"], "2026-04-25T09:00:00+00:00");
    let result_ref = fetched["result_ref"].as_str().unwrap();
    assert!(executor.capture.run_result(result_ref).await.is_none());
    assert!(executor.capture.evidence_refs.lock().await.is_empty());
    executor
        .commit_delivery(&policy, "accepted-fetch")
        .await
        .unwrap();
    assert!(executor.capture.run_result(result_ref).await.is_some());
    assert_eq!(executor.capture.evidence_refs.lock().await.len(), 1);
    executor
        .execute(
            &policy,
            ToolMethod::SourceSave,
            json!({"result_ref":result_ref,"title":"Issuer correction","coverage":"included_full"}),
        )
        .await
        .unwrap();

    // The acknowledged snapshot still has to satisfy a tighter cutoff, without refetching.
    assert!(executor
        .execute(
            &policy,
            ToolMethod::WebFetch,
            json!({"source_id":fetched["source_id"], "as_of":"2026-04-24"}),
        )
        .await
        .is_err());
    assert_eq!(executor.capture.evidence_refs.lock().await.len(), 1);
    assert_eq!(script::requests().len(), 1);
}

#[tokio::test]
async fn web_as_of_rejects_undated_modified_only_and_future_pages_without_save_authority() {
    for (name, metadata) in [
        ("undated", ""),
        (
            "modified-only",
            r#"<meta property="article:modified_time" content="2026-04-25T09:00:00Z">"#,
        ),
        (
            "future",
            r#"<meta property="article:published_time" content="2026-05-01T09:00:00Z">"#,
        ),
    ] {
        let url = format!("https://example.com/as-of-{name}");
        let _script = script::install(vec![(
            &url,
            vec![script::ok("text/html", article(metadata))],
        )]);
        let temporary = tempfile::tempdir().unwrap();
        let executor = executor_for(&seeded_state(&temporary), Arc::new(ToolCapture::default()));
        let policy = policy(true, true);
        let result = executor
            .execute_for_delivery(
                &policy,
                ToolMethod::WebFetch,
                json!({"url":url, "as_of":"2026-04-30"}),
                "rejected-fetch",
            )
            .await;
        assert!(result.is_err(), "{name}: {result:?}");
        // Even an erroneous acknowledgment cannot confer evidence or save authority.
        executor
            .commit_delivery(&policy, "rejected-fetch")
            .await
            .unwrap();
        assert!(
            executor.capture.web_sources.lock().await.is_empty(),
            "{name}"
        );
        assert!(
            executor.capture.run_results.lock().await.results.is_empty(),
            "{name}"
        );
        assert!(
            executor.capture.evidence_refs.lock().await.is_empty(),
            "{name}"
        );
    }
}
