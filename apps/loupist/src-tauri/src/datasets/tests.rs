use super::*;
use crate::store::SqliteStore;

fn fixture() -> SqliteStore {
    let store = SqliteStore::open_in_memory().unwrap();
    for id in ["reader-a", "reader-b"] {
        store
            .create_loupist(&crate::test_fixtures::loupist_profile(id))
            .unwrap();
    }
    store
}

#[test]
fn dataset_description_has_one_global_budget_and_preserves_section_pointers() {
    let store = fixture();
    // Control characters triple in size when JSON-escaped; the budget counts that.
    let value = "\u{0000}x".repeat(400);
    let row = (0..30)
        .map(|index| (format!("value_{index}"), json!(value)))
        .collect::<serde_json::Map<_, _>>();
    let payload = (0..80)
        .map(|index| {
            (
                format!("table/{index}"),
                json!([row.clone(), row.clone(), row.clone()]),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    let reference = store
        .dataset_put(
            "reader-a",
            "many-tables",
            &Value::Object(payload),
            json!({"unit":"USD million"}),
        )
        .unwrap();
    let description = store
        .dataset_describe("reader-a", &reference.dataset_id, 1)
        .unwrap();
    assert!(serde_json::to_vec(&description).unwrap().len() <= 64 * 1024);
    let section = &description["sections"][0];
    assert_eq!(section["pointer"], "/table~10");
    let exact = store
        .dataset_query(
            "reader-a",
            &reference.dataset_id,
            1,
            section["pointer"].as_str(),
            0,
            1,
            &["value_0".into()],
        )
        .unwrap();
    assert_eq!(exact["rows"][0]["value_0"], value);
}

#[test]
fn replacing_a_dataset_rejects_stale_refs_and_keeps_only_current_bytes() {
    let store = fixture();
    let old = store
        .dataset_put("reader-a", "report", &json!([1]), json!({}))
        .unwrap();
    let current = store
        .dataset_put("reader-a", "report", &json!([2]), json!({}))
        .unwrap();
    assert_eq!(old.dataset_id, current.dataset_id);
    assert_ne!(old.revision, current.revision);
    assert!(store
        .dataset("reader-a", &old.dataset_id, old.revision)
        .is_err());
    assert!(store
        .dataset("reader-b", &current.dataset_id, current.revision)
        .is_err());
    assert_eq!(
        store
            .dataset("reader-a", &current.dataset_id, current.revision)
            .unwrap()
            .1,
        json!([2])
    );
    assert_eq!(
        store
            .dataset_put("reader-a", "report", &json!([2]), json!({}))
            .unwrap(),
        current
    );
    let count: i64 = store
        .lock_loupist("reader-a")
        .unwrap()
        .query_row("SELECT count(*) FROM datasets", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn stale_datasets_are_refetched_without_reading_invalid_payloads() {
    let store = fixture();
    let earlier = (chrono::Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
    let reference = store
        .dataset_put(
            "reader-a",
            "quote",
            &json!({"close":"10"}),
            json!({"fetched_at": earlier}),
        )
        .unwrap();
    store
        .lock_loupist("reader-a")
        .unwrap()
        .execute(
            "UPDATE datasets SET payload=x'ff' WHERE id=?1",
            [&reference.dataset_id],
        )
        .unwrap();
    assert!(store
        .dataset_fresh("reader-a", "quote", 60)
        .unwrap()
        .is_none());
    // A reusable revision still has to pass the exact byte-integrity check.
    assert!(store
        .dataset_fresh("reader-a", "quote", 3 * 60 * 60)
        .is_err());
    assert!(store
        .dataset("reader-a", &reference.dataset_id, reference.revision)
        .is_err());
}

#[test]
fn identical_refetches_keep_the_revision_and_max_age_selects_only_recent_fetches() {
    let store = fixture();
    let earlier = (chrono::Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
    let first = store
        .dataset_put(
            "reader-a",
            "quote",
            &json!({"close": "10"}),
            json!({"fetched_at": earlier}),
        )
        .unwrap();
    // Two hours old: a one-hour max_age refetches, a three-hour one reuses.
    assert!(store
        .dataset_fresh("reader-a", "quote", 60 * 60)
        .unwrap()
        .is_none());
    let reused = store
        .dataset_fresh("reader-a", "quote", 3 * 60 * 60)
        .unwrap()
        .unwrap();
    assert_eq!(reused.0, first);
    assert_eq!(reused.1, json!({"close": "10"}));

    // Identical bytes keep their revision but record the newer fetch time.
    let now = chrono::Utc::now().to_rfc3339();
    let refetched = store
        .dataset_put(
            "reader-a",
            "quote",
            &json!({"close": "10"}),
            json!({"fetched_at": now}),
        )
        .unwrap();
    assert_eq!(refetched, first);
    let fresh = store
        .dataset_fresh("reader-a", "quote", 60)
        .unwrap()
        .unwrap();
    assert_eq!(fresh.2["fetched_at"], now);
    let read = store
        .dataset_query(
            "reader-a",
            &first.dataset_id,
            first.revision,
            None,
            0,
            1,
            &[],
        )
        .unwrap();
    assert_eq!(read["fetched_at"], now);
    assert_eq!(
        store
            .dataset_describe("reader-a", &first.dataset_id, first.revision)
            .unwrap()["fetched_at"],
        now
    );

    // Undated revisions (local imports) and other Loupists never satisfy max_age.
    store
        .dataset_put(
            "reader-a",
            "import",
            &json!([1]),
            json!({"origin": "local_file"}),
        )
        .unwrap();
    assert!(store
        .dataset_fresh("reader-a", "import", 60)
        .unwrap()
        .is_none());
    assert!(store
        .dataset_fresh("reader-b", "quote", 60)
        .unwrap()
        .is_none());
}
