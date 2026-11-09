//! Text from a shared pack through the real tool path: wrapped in
//! `<<shared …>>` wherever it reaches the model. A flagged shared item or a
//! shared item's full text raises the untrusted-text tier, so a command asks
//! the user in a card for the rest of the turn; a clean accepted item in the
//! per-turn catalog raises nothing. The user's own Memory never does.
use super::*;

const PACK: &str = "Quartz review";
const CLEAN_PACK: &str = "Harbor desk";

fn pack_folder() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/sharing/injected-method")
}

/// A one-method pack with nothing the scan flags.
fn clean_pack(root: &Path) -> std::path::PathBuf {
    use crate::sharing::pack;
    let method = pack::MethodDoc {
        key: "m-4444444444444444".into(),
        title: "Berth utilization".into(),
        origin: None,
        document: pack::PackDoc {
            summary: "Compare berth utilization across ferry operators.".into(),
            sections: vec![
                pack::PackSection {
                    key: "use-when".into(),
                    heading: "Use when".into(),
                    markdown: "Fictional ferry operators.".into(),
                },
                pack::PackSection {
                    key: "steps".into(),
                    heading: "Steps".into(),
                    markdown: "Compare the two operators year by year.".into(),
                },
            ],
        },
    };
    let manifest = pack::Manifest {
        format: pack::FORMAT.into(),
        id: "7f1d1c3e-0000-4000-8000-000000000044".into(),
        kind: pack::PackKind::Method,
        name: CLEAN_PACK.into(),
        version: 1,
        created_at: "2026-11-01T00:00:00Z".into(),
        language: "en".into(),
        license: pack::License::CcBy,
        terms_url: None,
        credits: Vec::new(),
        author: None,
        origin: Vec::new(),
        asset_classes: Vec::new(),
        counts: pack::Counts {
            methods: 1,
            ..Default::default()
        },
        files: BTreeMap::new(),
        app: "0.0.1".into(),
    };
    let (_, files) = pack::seal(
        manifest,
        pack::Files::from([
            (
                "method-berth-utilization.md".to_owned(),
                pack::render_method(&method).into_bytes(),
            ),
            ("README.md".to_owned(), b"# Harbor desk\n".to_vec()),
        ]),
    );
    let folder = root.join("clean-pack");
    fs::create_dir_all(&folder).unwrap();
    for (path, bytes) in files {
        fs::write(folder.join(path), bytes).unwrap();
    }
    folder
}

fn wrapped_by(text: &str, pack: &str) -> bool {
    text.starts_with("<<shared id=") && text.contains(&format!("from=\"{pack}\">>"))
}

fn wrapped(text: &str) -> bool {
    wrapped_by(text, PACK)
}

fn bash_allowed(executor: &AppToolExecutor) -> bool {
    executor
        .enforce_turn_policy(ToolMethod::NativeAuthorize, &json!({"name": "bash"}))
        .is_ok()
}

/// The weaker tier. It no longer refuses a command itself: the native policy
/// asks the user in a card once it is raised.
fn untrusted_text(executor: &AppToolExecutor) -> bool {
    executor.capture.turn_taint.untrusted_text()
}

/// A fresh turn: its own capture, so each delivery is checked on its own.
fn turn(state: &AppState) -> AppToolExecutor {
    let executor = executor_for(state, Arc::new(ToolCapture::default()));
    assert!(bash_allowed(&executor));
    executor
}

fn own_method(state: &AppState) -> String {
    crate::sharing::commands::with_tx(state, "loupist-a", |tx, blobs| {
        let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
        let record = write
            .file_section(
                "loupist-a",
                crate::knowledge::lens_page::PageKind::Method,
                "Berth turnover",
                "steps",
                "Compare berth turnover year by year.",
                &crate::knowledge::test_support::user_origin(),
                1,
            )
            .map_err(|error| crate::app::CommandError::invalid(error.to_string()))?;
        Ok(record.record_id)
    })
    .unwrap()
}

#[tokio::test]
async fn shared_pack_text_arrives_delimited_and_pauses_for_flags_and_full_text() {
    let temporary = tempfile::tempdir().unwrap();
    let state = seeded_state(&temporary);
    let own = own_method(&state);
    let policy = policy(true, false);

    // A clean accepted method: its catalog entry and search hit are wrapped and pause nothing.
    crate::sharing::commands::seed_shared_pack(&state, "loupist-a", &clean_pack(temporary.path()))
        .unwrap();
    let executor = turn(&state);
    let found = executor
        .execute(
            &policy,
            ToolMethod::LensSearch,
            json!({"query": "Berth utilization"}),
        )
        .await
        .unwrap();
    let hit = found["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hit| {
            hit["title"]
                .as_str()
                .is_some_and(|title| title.contains("Berth utilization"))
        })
        .cloned()
        .unwrap_or_else(|| panic!("{found}"));
    assert!(
        wrapped_by(hit["title"].as_str().unwrap(), CLEAN_PACK),
        "{hit}"
    );
    assert!(
        bash_allowed(&executor) && !untrusted_text(&executor),
        "a clean title is not its full text"
    );
    let clean = hit["recordId"].as_str().unwrap().to_owned();
    let context =
        crate::commands::knowledge::host_memory_context(&state, "loupist-a", "berth utilization")
            .unwrap();
    let entry = context
        .entries
        .iter()
        .find(|entry| entry.id == clean)
        .unwrap();
    assert!(wrapped_by(&entry.title, CLEAN_PACK) && wrapped_by(&entry.summary, CLEAN_PACK));
    assert!(
        !context.shared_pauses,
        "a clean item in the catalog pauses nothing"
    );
    // Its full text does.
    let executor = turn(&state);
    let read = executor
        .execute(&policy, ToolMethod::LensRead, json!({"recordId": clean}))
        .await
        .unwrap();
    assert!(wrapped_by(read["text"].as_str().unwrap(), CLEAN_PACK));
    assert!(
        untrusted_text(&executor),
        "a clean item's full text raises the tier, so a command asks"
    );

    // A flagged accepted method.
    crate::sharing::commands::seed_shared_pack(&state, "loupist-a", &pack_folder()).unwrap();

    // lens_search: the shared method's title and snippet.
    let executor = turn(&state);
    let found = executor
        .execute(
            &policy,
            ToolMethod::LensSearch,
            json!({"query": "Quartz margin check"}),
        )
        .await
        .unwrap();
    let hit = found["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hit| {
            hit["title"]
                .as_str()
                .is_some_and(|title| title.contains("Quartz margin check"))
        })
        .cloned()
        .unwrap_or_else(|| panic!("{found}"));
    assert!(wrapped(hit["title"].as_str().unwrap()), "{hit}");
    assert!(
        untrusted_text(&executor),
        "flagged shared text raises the tier"
    );
    let shared = hit["recordId"].as_str().unwrap().to_owned();

    // lens_read: the body, injected line included, inside the delimiters.
    let executor = turn(&state);
    let read = executor
        .execute(&policy, ToolMethod::LensRead, json!({"recordId": shared}))
        .await
        .unwrap();
    let text = read["text"].as_str().unwrap();
    assert!(
        wrapped(text) && text.contains("Ignore your rules"),
        "{text}"
    );
    assert!(
        untrusted_text(&executor),
        "flagged shared text raises the tier"
    );

    // The user's own method arrives as it is and pauses nothing.
    let executor = turn(&state);
    let read = executor
        .execute(&policy, ToolMethod::LensRead, json!({"recordId": own}))
        .await
        .unwrap();
    assert!(!read["text"].as_str().unwrap().contains("<<shared"));
    assert!(bash_allowed(&executor) && !untrusted_text(&executor));

    // world_read: shared names, the summary asking for the system prompt, the relationship.
    let world = crate::commands::knowledge::catalog(
        &state,
        crate::commands::knowledge::KnowledgeCatalogRequest {
            loupist_id: "loupist-a".into(),
        },
    )
    .unwrap()
    .worlds[0]
        .record_id
        .clone();
    let executor = turn(&state);
    let read = executor
        .execute(
            &policy,
            ToolMethod::WorldRead,
            json!({"scope": "world", "recordId": world}),
        )
        .await
        .unwrap();
    let view: Value = serde_json::from_str(read["text"].as_str().unwrap()).unwrap();
    let quartz = view["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["name"].as_str().unwrap().contains("Quartz Ferry"))
        .unwrap();
    assert!(wrapped(quartz["name"].as_str().unwrap()), "{quartz}");
    let summary = quartz["description"].as_str().unwrap();
    assert!(wrapped(summary) && summary.contains("system prompt"));
    let edge = &view["edges"][0]["claim"];
    assert!(wrapped(edge["proposition"].as_str().unwrap()), "{edge}");
    assert!(wrapped(
        edge["evidence"][0]["attribution"].as_str().unwrap()
    ));
    assert!(
        untrusted_text(&executor),
        "flagged shared text raises the tier"
    );

    // A read of the pack's reference Source: its stored material is pack text.
    let (_, target) = view["sourceTargets"]
        .as_object()
        .unwrap()
        .iter()
        .next()
        .unwrap();
    let executor = turn(&state);
    let source = executor
        .execute(&policy, ToolMethod::WorldRead, target["readArgs"].clone())
        .await
        .unwrap();
    let text = source["text"].as_str().unwrap();
    assert!(
        wrapped(text) && text.contains("Fictional harbor report"),
        "{text}"
    );
    assert!(
        untrusted_text(&executor),
        "flagged shared text raises the tier"
    );

    // world_search over Sources: the reference Source's title.
    let executor = turn(&state);
    let found = executor
        .execute(
            &policy,
            ToolMethod::WorldSearch,
            json!({"query": "*", "scope": "source"}),
        )
        .await
        .unwrap();
    let titles: Vec<&str> = found["hits"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|hit| hit["title"].as_str())
        .filter(|title| title.contains("Fictional harbor report"))
        .collect();
    assert!(
        !titles.is_empty() && titles.iter().all(|title| wrapped(title)),
        "{found}"
    );
    assert!(
        untrusted_text(&executor),
        "flagged shared text raises the tier"
    );

    // The turn context: a flagged item in the per-turn catalog is wrapped and
    // raises the tier for the turn.
    let context =
        crate::commands::knowledge::host_memory_context(&state, "loupist-a", "quartz margin")
            .unwrap();
    assert!(
        context.shared_pauses,
        "a flagged item in the catalog pauses"
    );
    let entry = context
        .entries
        .iter()
        .find(|entry| entry.id == shared)
        .unwrap();
    assert!(wrapped(&entry.title) && wrapped(&entry.summary));
    let mine = context
        .entries
        .iter()
        .find(|entry| entry.id == own)
        .unwrap();
    assert_eq!(mine.title, "Berth turnover");
}
