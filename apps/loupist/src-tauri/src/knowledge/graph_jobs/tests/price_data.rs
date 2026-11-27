//! Price data stays out of the World queue (`knowledge::price_data`). The
//! Sources here are stored the way `source_save` stores a bundled market-data
//! read: the delivered JSON payload with its retained dataset reference.
use super::*;
use crate::knowledge::RetainedSource;

const L: &str = "loupist-a";

fn payload(dataset_id: &str, rows: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "dataset_ref": {"dataset_id": dataset_id, "revision": 1, "digest": "a".repeat(64)},
        "fetched_at": "2026-11-23T09:00:00.000Z",
        "model_delivery": {"coverage": "complete", "retained": "complete"},
        "structuredContent": {"results": rows, "provider": "yfinance"},
    }))
    .unwrap()
}

/// Hourly ETH-USD bars, long enough to span several World episodes.
fn hourly_bars(hours: usize) -> Vec<u8> {
    let rows: Vec<Value> = (0..hours)
        .map(|hour| {
            let close = 3000.0 + hour as f64 / 4.0;
            json!({"close": close, "date": format!("2026-11-{:02}T{:02}:00:00+00:00", 1 + hour / 24, hour % 24),
                "high": close + 9.5, "low": close - 11.0, "open": close - 2.5, "volume": 1200.0 + hour as f64})
        })
        .collect();
    payload("dataset-eth", json!(rows))
}

fn income_statement() -> Vec<u8> {
    payload(
        "dataset-income",
        json!([{"period_ending": "2025-12-31", "fiscal_year": 2025, "revenue": 3.0e11, "operating_income": 3.3e10, "net_income": 3.4e10}]),
    )
}

fn tool_result(store: &KnowledgeStore, bytes: &[u8]) -> (String, RetainedSource) {
    let module = source_module(store, L);
    let mut request = RetainSource {
        // The title says nothing either way.
        title: "Revenue and relationships of ETH-USD".into(),
        source_class: SourceClass::ToolResult,
        origin_locator: "ETH-USD".into(),
        provider_identity: "yfinance".into(),
        ..source_request(L, &module, bytes)
    };
    let material = request.material.as_mut().unwrap();
    material.media_type = "application/json".into();
    material.role = crate::knowledge::RepresentationRole::ProviderPayload;
    material.document_id = format!("tool.{module}");
    let retained = store.retain_source(request).unwrap();
    (module, retained)
}

fn status(store: &KnowledgeStore, record: &str) -> String {
    store
        .transact(|write| write.source_processing(L, record))
        .unwrap()
        .status
}

fn source_jobs(store: &KnowledgeStore, record: &str) -> Vec<GraphJobView> {
    store
        .graph_jobs(L)
        .unwrap()
        .into_iter()
        .filter(|job| {
            job.document
                .source_binding
                .as_ref()
                .is_some_and(|binding| binding.source_record_id == record)
        })
        .collect()
}

fn retire(store: &KnowledgeStore, known: &BTreeSet<String>) -> PriceRetirement {
    store
        .transact(|write| retire_price_jobs(write.tx, write.blobs, L, known, T + 500))
        .unwrap()
}

#[test]
fn a_saved_price_table_never_enters_the_world_queue() {
    let (_directory, store) = open();
    let settings = enabled();
    store
        .transact(|write| store_settings(write.tx, L, &settings))
        .unwrap();
    let (bars, saved) = tool_result(&store, &hourly_bars(400));
    assert!(store
        .transact(|write| write.enqueue_source_world(L, &saved, &settings, T))
        .unwrap()
        .is_none());

    let jobs = source_jobs(&store, &bars);
    assert_eq!(jobs.len(), 1, "one final row, not one per episode");
    assert_eq!(jobs[0].status, JOB_PRICE_DATA);
    assert!(jobs[0].document.body.is_empty());
    assert_eq!(jobs[0].document.extraction.max_llm_calls, 0);
    assert_eq!(store.read(|read| backlog(read.tx)).unwrap(), 0);
    assert!(store
        .read(|read| queued_loupist_heads(read.tx, &[]))
        .unwrap()
        .is_empty());
    assert_eq!(status(&store, &bars), JOB_PRICE_DATA);
    // Method learning counts it as read: World will add nothing from it.
    let cited = BTreeSet::from([bars.clone()]);
    assert_eq!(
        store
            .read(|read| processed_sources(read.tx, L, &cited))
            .unwrap(),
        cited
    );

    // Saving it again, resuming and backfilling change nothing.
    store
        .transact(|write| write.enqueue_source_world(L, &saved, &settings, T + 10))
        .unwrap();
    store
        .transact(|write| write.enqueue_existing_sources(L, &settings, T + 20))
        .unwrap();
    assert_eq!(source_jobs(&store, &bars).len(), 1);
    assert!(store
        .transact(|write| retry_source_processing(write.tx, &settings, L, &bars))
        .is_err());

    // A company statement in the same Loupist still goes to World.
    let (statement, saved) = tool_result(&store, &income_statement());
    let job = store
        .transact(|write| write.enqueue_source_world(L, &saved, &settings, T + 30))
        .unwrap()
        .expect("queued");
    assert_eq!(job.status, JOB_QUEUED);
    assert_eq!(status(&store, &statement), JOB_QUEUED);
}

#[test]
fn price_data_reads_as_such_while_world_building_is_off() {
    let (_directory, store) = open();
    let off = GraphProcessingSettings::default();
    let (bars, saved) = tool_result(&store, &hourly_bars(3));
    assert!(store
        .transact(|write| write.enqueue_source_world(L, &saved, &off, T))
        .unwrap()
        .is_none());
    assert!(source_jobs(&store, &bars).is_empty());
    assert_eq!(status(&store, &bars), JOB_PRICE_DATA);
    let (statement, _) = tool_result(&store, &income_statement());
    assert_eq!(status(&store, &statement), "disabled");
}

#[test]
fn waiting_work_for_price_tables_leaves_the_queue_once_without_a_model_call() {
    let (_directory, store) = open();
    let settings = enabled();
    store
        .transact(|write| store_settings(write.tx, L, &settings))
        .unwrap();
    let (world_module, revision) = world(&store, L);
    // Jobs queued before price data was told apart, straight through `accept_episode`.
    let queue = |bytes: &[u8]| {
        let (record, saved) = tool_result(&store, bytes);
        add(
            &store,
            &settings,
            L,
            &world_module,
            &revision,
            &record,
            &saved.revision_id,
            saved.span_id.as_deref().unwrap(),
        );
        (record, saved.revision_id, saved.span_id.unwrap())
    };
    let set = |record: &str, from: &str, to: &str, edit: &dyn Fn(&mut JobDocument)| {
        for job in source_jobs(&store, record) {
            let mut document = job.document.clone();
            edit(&mut document);
            store
                .transact(|write| write_status(write.tx, &job.job_id, from, to, &document))
                .unwrap();
        }
    };
    let (long, ..) = queue(&hourly_bars(400));
    let chunks = source_jobs(&store, &long).len();
    assert!(chunks > 1);
    // Stopped on its allowance after a paid attempt.
    let (stopped, ..) = queue(&hourly_bars(5));
    set(&stopped, JOB_QUEUED, JOB_PAUSED, &|document| {
        document.inference_attempted = true;
        document.attempt_id = Some("attempt-spent".into());
        document.extraction.max_llm_calls = 0;
    });
    let (completed, ..) = queue(&hourly_bars(6));
    set(&completed, JOB_QUEUED, JOB_COMPLETED, &|_| {});
    let (running, ..) = queue(&hourly_bars(7));
    set(&running, JOB_QUEUED, JOB_RUNNING, &|document| {
        document.attempt_id = Some("attempt-live".into());
        document.lease_expires_at_ms = Some(T + 60_000);
    });
    let (statement, statement_revision, statement_span) = queue(&income_statement());

    let first = retire(&store, &BTreeSet::new());
    assert_eq!(first.retired as usize, chunks + 1);
    for record in [&long, &stopped] {
        assert!(source_jobs(&store, record)
            .iter()
            .all(|job| job.status == JOB_PRICE_DATA));
        assert_eq!(status(&store, record), JOB_PRICE_DATA);
    }
    // Spent calls stay recorded on the row; nothing is refunded or sent again.
    let spent = &source_jobs(&store, &stopped)[0].document;
    assert!(spent.inference_attempted);
    assert_eq!(spent.attempt_id.as_deref(), Some("attempt-spent"));
    assert!(store
        .transact(|write| retry_source_processing(write.tx, &settings, L, &stopped))
        .is_err());
    // Completed and running work keeps its status; a statement keeps its place.
    assert_eq!(source_jobs(&store, &completed)[0].status, JOB_COMPLETED);
    assert_eq!(status(&store, &completed), JOB_COMPLETED);
    assert_eq!(source_jobs(&store, &running)[0].status, JOB_RUNNING);
    assert_eq!(status(&store, &statement), JOB_QUEUED);
    let heads = store
        .read(|read| queued_loupist_heads(read.tx, &[]))
        .unwrap();
    assert_eq!(heads.len(), 1);
    assert_eq!(
        heads[0]
            .document
            .source_binding
            .as_ref()
            .unwrap()
            .source_record_id,
        statement
    );
    assert!(first
        .classified
        .contains(&format!("{statement_revision}:{statement_span}")));

    // A later pass reads nothing it already read, and a fresh one changes nothing.
    let known: BTreeSet<String> = first.classified.iter().cloned().collect();
    let again = retire(&store, &known);
    assert_eq!((again.retired, again.classified.len()), (0, 0));
    let fresh = retire(&store, &BTreeSet::new());
    assert_eq!(fresh.retired, 0);
    assert_eq!(
        fresh.classified,
        vec![format!("{statement_revision}:{statement_span}")]
    );

    // The running chunk leaves the queue once it stops on its allowance.
    set(&running, JOB_RUNNING, JOB_PAUSED, &|document| {
        document.lease_expires_at_ms = None;
    });
    assert_eq!(retire(&store, &known).retired, 1);
    assert_eq!(status(&store, &running), JOB_PRICE_DATA);
}

#[test]
fn the_producing_tool_decides_when_the_rows_name_no_price_field() {
    let (_directory, store) = open();
    let settings = enabled();
    let rows = json!([{"stck_bsop_date": "20261120", "stck_clpr": "61200", "acml_vol": "1200"}]);
    store
        .transact(|write| {
            write.tx.execute(
                "INSERT INTO loupist_profiles (id, updated_at_ms, workspace_root, data_json)
                 VALUES (?1, 0, '/fixture/loupist-a', '{}')",
                [L],
            )?;
            for (dataset, operation) in [
                ("dataset-kis-bars", "domestic_stock.inquire_daily_itemchartprice"),
                ("dataset-kis-rank", "domestic_stock.volume_rank"),
            ] {
                let metadata = json!({"producer": "KisRead", "trust": "numeric",
                    "request": {"operation_id": operation, "params": {"FID_INPUT_ISCD": "005930"}}});
                write.tx.execute(
                    "INSERT INTO datasets (loupist_id, id, revision, digest, request_key, created_at_ms, metadata_json, payload)
                     VALUES (?1, ?2, 1, ?3, ?2, 0, ?4, x'7b7d')",
                    rusqlite::params![L, dataset, "d".repeat(64), metadata.to_string()],
                )?;
            }
            Ok(())
        })
        .unwrap();
    let shaped = |dataset: &str| {
        serde_json::to_vec(&json!({
            "dataset_ref": {"dataset_id": dataset, "revision": 1, "digest": "d".repeat(64)},
            "data": {"rows": rows},
        }))
        .unwrap()
    };
    let (bars, saved) = tool_result(&store, &shaped("dataset-kis-bars"));
    assert!(store
        .transact(|write| write.enqueue_source_world(L, &saved, &settings, T))
        .unwrap()
        .is_none());
    assert_eq!(status(&store, &bars), JOB_PRICE_DATA);
    let (ranking, saved) = tool_result(&store, &shaped("dataset-kis-rank"));
    assert!(store
        .transact(|write| write.enqueue_source_world(L, &saved, &settings, T + 10))
        .unwrap()
        .is_some());
    assert_eq!(status(&store, &ranking), JOB_QUEUED);
}

#[test]
fn a_price_table_composed_in_the_workspace_is_price_data() {
    let (_directory, store) = open();
    let settings = enabled();
    let module = source_module(&store, L);
    let mut request = RetainSource {
        title: "Notes".into(),
        source_class: SourceClass::UserFile,
        origin_locator: "workspace:outputs/eth-closes.csv".into(),
        provider_identity: "workspace:workspace_capture".into(),
        authorship: Authorship::Tool,
        ..source_request(
            L,
            &module,
            b"date,close,volume\n2026-11-21,3010.5,120\n2026-11-22,3075,98\n2026-11-23,3069.25,110\n",
        )
    };
    request.material.as_mut().unwrap().media_type = "text/csv".into();
    let saved = store.retain_source(request).unwrap();
    assert!(store
        .transact(|write| write.enqueue_source_world(L, &saved, &settings, T))
        .unwrap()
        .is_none());
    assert_eq!(status(&store, &module), JOB_PRICE_DATA);
}
