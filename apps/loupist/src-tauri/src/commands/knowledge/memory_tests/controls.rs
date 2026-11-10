use super::*;

#[test]
fn paused_principles_and_methods_leave_context_search_and_exact_reads_until_resumed() {
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temp.path().join("app"));
    state
        .store
        .create_loupist(&loupist_profile("loupist-a"))
        .unwrap();
    let (principle, method) = with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        let principle = crate::knowledge::principles::principle_save(
            tx,
            "loupist-a",
            None,
            None,
            crate::knowledge::PrincipleInput {
                statement: format!("Check durable cash flow {}", "in every quarter ".repeat(8)),
                why: None,
                example: None,
                origin: "settings".into(),
                origin_chat_id: None,
                origin_message_id: None,
            },
            1,
        )?;
        let write = KnowledgeWrite::bind(tx, blobs);
        let method = write.file_section(
            "loupist-a",
            crate::knowledge::lens_page::PageKind::Method,
            "Durable cash method",
            "use-when",
            "Compare durable cash flow in durable cash research",
            &crate::knowledge::test_support::user_origin(),
            2,
        )?;
        Ok((principle, method))
    })
    .unwrap();
    for (id, kind) in [
        (&principle.record_id, "principle"),
        (&method.record_id, "method"),
    ] {
        let (_, read) = host_prepare_read(&state, "loupist-a", &empty_read(id)).unwrap();
        let receipt = read.lens_receipt("exact_read").unwrap();
        assert_eq!(receipt.kind, kind);
        receipt.validate().unwrap();
        if kind == "principle" {
            assert_eq!(receipt.title, principle.statement);
        }
        with_loupist_knowledge(&state, "loupist-a", |tx, _| {
            crate::knowledge::memory_controls::pause(tx, "loupist-a", id, true)?;
            // Saving unrelated World preferences must retain item controls.
            let mut settings = crate::knowledge::load_settings(tx, "loupist-a")?;
            settings.research_focus = "Cash flow".into();
            crate::knowledge::store_settings(tx, "loupist-a", &settings)?;
            assert!(crate::knowledge::memory_controls::load(tx, "loupist-a")?
                .paused
                .contains(id));
            Ok(())
        })
        .unwrap();
        assert!(host_prepare_read(&state, "loupist-a", &empty_read(id)).is_err());
        let context = host_memory_context(&state, "loupist-a", "durable cash").unwrap();
        assert!(context.entries.iter().all(|entry| &entry.id != id));
        assert!(context.principles.iter().all(|p| &p.record_id != id));
        assert!(context
            .principle_receipts
            .iter()
            .all(|p| &p.record_id != id));
        let hits = host_search(&state, "loupist-a", "durable cash", "lens").unwrap();
        assert!(hits
            .as_array()
            .unwrap()
            .iter()
            .all(|hit| hit["recordId"].as_str() != Some(id)));
        // Renderer inspection still works, so the user can resume it.
        lens_read(
            &state,
            KnowledgeLensReadRequest {
                loupist_id: "loupist-a".into(),
                record_id: id.clone(),
                revision_id: None,
            },
        )
        .unwrap();
        with_loupist_knowledge(&state, "loupist-a", |tx, _| {
            crate::knowledge::memory_controls::pause(tx, "loupist-a", id, false)
        })
        .unwrap();
        let (_, resumed) = host_prepare_read(&state, "loupist-a", &empty_read(id)).unwrap();
        assert_eq!(resumed.lens_receipt("exact_read").unwrap().kind, kind);
        let context = host_memory_context(&state, "loupist-a", "durable cash").unwrap();
        assert!(
            context.principles.iter().any(|p| &p.record_id == id)
                || context.entries.iter().any(|entry| &entry.id == id)
        );
    }
}

#[test]
fn context_only_delivery_counts_once_per_completed_turn_and_records_kind() {
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temp.path().join("app"));
    state
        .store
        .create_loupist(&loupist_profile("loupist-a"))
        .unwrap();
    let p = with_loupist_knowledge(&state, "loupist-a", |tx, _| {
        crate::knowledge::principles::principle_save(
            tx,
            "loupist-a",
            None,
            None,
            crate::knowledge::PrincipleInput {
                statement: "Check operating cash flow".into(),
                why: None,
                example: None,
                origin: "settings".into(),
                origin_chat_id: None,
                origin_message_id: None,
            },
            1,
        )
    })
    .unwrap();
    let context = host_memory_context(&state, "loupist-a", "cash").unwrap();
    assert!(context.principles_block.contains(&p.statement));
    let receipt = &context.principle_receipts[0];
    assert_eq!(receipt.kind, "principle");
    assert_eq!(receipt.delivery, "context");
    with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        super::super::memory_usage::persist_deliveries(tx, blobs, "loupist-a", "chat-1", "turn-1", &[receipt.clone(), receipt.clone()], 10)?;
        let (kind, delivery): (String, String) = tx.query_row("SELECT json_extract(document_json,'$.kind'),json_extract(document_json,'$.delivery') FROM knowledge_lens_deliveries LIMIT 1", [], |r| Ok((r.get(0)?,r.get(1)?)))?;
        assert_eq!((kind.as_str(),delivery.as_str()), ("principle","context"));
        Ok(())
    }).unwrap();
    let usage = super::super::memory_usage::usage(&state, "loupist-a").unwrap();
    assert_eq!(usage[0].record_id, p.record_id);
    assert_eq!(usage[0].used_count, 1);
    assert_eq!(usage[0].used_in[0].chat_id, "chat-1");
}

#[test]
fn a_source_still_waiting_for_the_world_is_marked_in_search() {
    let temp = tempfile::tempdir().unwrap();
    let state = AppState::for_test(temp.path().join("app"));
    state
        .store
        .create_loupist(&loupist_profile("loupist-a"))
        .unwrap();
    let job = with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        let settings = crate::knowledge::GraphProcessingSettings {
            enrichment_enabled: true,
            helper_provider: Some("fixture".into()),
            helper_model: Some("fixture-local".into()),
            ..Default::default()
        };
        crate::knowledge::store_settings(tx, "loupist-a", &settings)?;
        let write = KnowledgeWrite::bind(tx, blobs);
        let saved = write.retain_new_source(text_source(
            "loupist-a",
            "example-publisher",
            b"Turbine maker Volta expands its Ohio plant.",
        ))?;
        write.enqueue_source_world("loupist-a", &saved, &settings, 7)
    })
    .unwrap()
    .expect("the Source is queued for the World");
    let marked = |state: &AppState| {
        let hits = host_search(state, "loupist-a", "Volta turbine plant", "source").unwrap();
        let hit = hits
            .as_array()
            .unwrap()
            .iter()
            .find(|hit| hit["scope"] == "source")
            .cloned()
            .expect("the Source is found before the World has it");
        hit.get("mapPending").cloned()
    };
    assert_eq!(marked(&state), Some(serde_json::json!(true)));
    with_loupist_knowledge(&state, "loupist-a", |tx, _| {
        tx.execute(
            "UPDATE knowledge_graph_outbox SET status = 'completed' WHERE job_id = ?1",
            [&job.job_id],
        )?;
        Ok(())
    })
    .unwrap();
    assert_eq!(marked(&state), None);
}
