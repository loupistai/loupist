use std::collections::BTreeMap;

use rusqlite::params;

use super::*;
use crate::knowledge::lens_page::{
    AnchorInput, Basis, BasisRef, FilingOrigin, LensOp, PageDelta, PageKind, PageTarget, SectionKey,
};
use crate::knowledge::KnowledgeStore;
use crate::scheduled_tasks::types::{
    LearningConnections, LearningDepth, LearningKeeps, LearningProgram, LearningScope,
    ReportPolicy, RunStatus, Schedule, TaskDefinition, TaskDraft, TaskKind, TaskStatus, TaskTarget,
    TaskWatch,
};

const LOUPIST: &str = "loupist-a";
const TASK: &str = "learning-harbours";

fn store() -> (tempfile::TempDir, KnowledgeStore) {
    let directory = tempfile::tempdir().unwrap();
    let store = KnowledgeStore::open(directory.path().join("knowledge.sqlite")).unwrap();
    store
        .app_store()
        .create_loupist(&crate::domain::LoupistProfile {
            root_filesystem_identity: None,
            ..crate::test_fixtures::loupist_profile(LOUPIST)
        })
        .unwrap();
    store
        .app_store()
        .scheduled_task_create_definition(
            LOUPIST,
            TASK,
            TaskDraft {
                title: "Puertos y fletes".into(),
                prompt: "Follow container volumes at the large ports.".into(),
                schedule: Schedule::Interval { every_minutes: 60 },
                target: TaskTarget::NewChat,
                model_profile_id: "fixture".into(),
                thinking_level: "medium".into(),
                run_options: BTreeMap::new(),
                use_memory: true,
                update_memory: true,
                status: TaskStatus::Active,
            },
            TaskKind::Learning,
            TaskDefinition {
                program: Some(LearningProgram {
                    depth: LearningDepth::Light,
                    scope: LearningScope {
                        connections: LearningConnections::Selected(vec![]),
                        web: false,
                    },
                    keeps: LearningKeeps {
                        sources: false,
                        lens: true,
                        asset_reads: false,
                    },
                    preset: None,
                }),
                watch: TaskWatch {
                    report: Some(ReportPolicy::WhenNew),
                    ..TaskWatch::default()
                },
                ..TaskDefinition::default()
            },
            1,
        )
        .unwrap();
    (directory, store)
}

/// One finished run of the program whose answer is `message_id`; returns its chat.
fn run(store: &KnowledgeStore, message_id: &str, at_ms: i64) -> String {
    let app = store.app_store();
    let (claim, ()) = app
        .scheduled_task_claim_with(LOUPIST, TASK, at_ms, true, |_| Ok(()))
        .unwrap()
        .unwrap();
    app.scheduled_task_finish(
        LOUPIST,
        &claim.run.run_id,
        RunStatus::Succeeded,
        None,
        at_ms + 1,
    )
    .unwrap();
    let mut connection = app.lock_loupist(LOUPIST).unwrap();
    connection
        .transaction()
        .and_then(|tx| {
            tx.execute(
                "UPDATE scheduled_task_runs SET data_json=json_set(data_json,'$.message_id',?2) WHERE run_id=?1",
                params![claim.run.run_id, message_id],
            )?;
            tx.commit()
        })
        .unwrap();
    claim.run.thread_id.unwrap()
}

fn put(key: &str, markdown: &str, expected: Option<String>) -> LensOp {
    LensOp::Put {
        key: SectionKey::new(key).unwrap(),
        heading: key.to_uppercase(),
        markdown: markdown.into(),
        basis: Basis {
            refs: vec![BasisRef::Claim {
                claim_id: "clm_1".into(),
            }],
            uncertainty: None,
        },
        expected,
    }
}

fn turn(chat_id: &str, message_id: &str) -> FilingOrigin {
    FilingOrigin::Turn {
        chat_id: chat_id.into(),
        message_id: message_id.into(),
        user_message_id: None,
        unattended: true,
    }
}

fn create(store: &KnowledgeStore, origin: FilingOrigin, title: &str, ops: Vec<LensOp>) -> String {
    store
        .transact(|write| {
            write.file(
                LOUPIST,
                &origin,
                vec![PageDelta {
                    target: PageTarget::Create {
                        kind: PageKind::Subject,
                        title: title.into(),
                        anchors: vec![AnchorInput::Name { name: title.into() }],
                    },
                    ops,
                }],
                100,
            )
        })
        .unwrap()[0]
        .record_id
        .clone()
}

fn user_edit(store: &KnowledgeStore, record_id: &str, key: &str, markdown: &str) {
    let digest = section(store, record_id, key).unwrap();
    store
        .transact(|write| {
            write.file(
                LOUPIST,
                &FilingOrigin::User {
                    chat_id: None,
                    message_id: None,
                },
                vec![PageDelta {
                    target: PageTarget::Existing {
                        record_id: record_id.into(),
                    },
                    ops: vec![put(key, markdown, Some(digest))],
                }],
                300,
            )
        })
        .unwrap();
}

/// The section's digest, or None when the page no longer holds it.
fn section(store: &KnowledgeStore, record_id: &str, key: &str) -> Option<String> {
    store
        .read_page(LOUPIST, record_id)
        .unwrap()
        .body
        .sections()
        .iter()
        .find(|section| section.key.as_str() == key)
        .map(|section| section.digest.clone())
}

fn undo_all(store: &KnowledgeStore, preview: bool) -> ProgramUndo {
    store
        .transact(|write| write.undo_program(LOUPIST, TASK, preview, 400))
        .unwrap()
}

fn log_rows(store: &KnowledgeStore) -> i64 {
    store.read(|write| write.last_log_row()).unwrap()
}

/// Two runs on two pages: Rotterdam (two sections, one edited by the user
/// since) from the first run, Busan from the second.
fn two_runs(store: &KnowledgeStore) -> (String, String) {
    let first = run(store, "answer-1", 10);
    let rotterdam = create(
        store,
        turn(&first, "answer-1"),
        "Port of Rotterdam",
        vec![
            put("volumes", "Volumes rose in the quarter.", None),
            put("rates", "Spot rates eased.", None),
        ],
    );
    let second = run(store, "answer-2", 20);
    let busan = create(
        store,
        turn(&second, "answer-2"),
        "부산항",
        vec![put("volumes", "환적 물량이 늘었다.", None)],
    );
    user_edit(
        store,
        &rotterdam,
        "rates",
        "Spot rates eased; contract rates held.",
    );
    (rotterdam, busan)
}

#[test]
fn undo_all_restores_every_run_newest_first_and_counts_a_kept_section() {
    let (_directory, store) = store();
    let (rotterdam, busan) = two_runs(&store);
    assert_eq!(
        undo_all(&store, false),
        ProgramUndo {
            runs: 2,
            undone: 2,
            kept: 1
        }
    );
    assert!(section(&store, &busan, "volumes").is_none());
    assert!(section(&store, &rotterdam, "volumes").is_none());
    assert!(
        section(&store, &rotterdam, "rates").is_some(),
        "the user's later edit is kept"
    );
}

#[test]
fn a_second_undo_all_changes_nothing() {
    let (_directory, store) = store();
    two_runs(&store);
    undo_all(&store, false);
    let rows = log_rows(&store);
    assert_eq!(
        undo_all(&store, false),
        ProgramUndo {
            runs: 0,
            undone: 0,
            kept: 0
        }
    );
    assert_eq!(log_rows(&store), rows);
}

#[test]
fn a_preview_counts_what_undo_all_does_and_changes_nothing() {
    let (_directory, store) = store();
    let (rotterdam, busan) = two_runs(&store);
    let rows = log_rows(&store);
    let preview = undo_all(&store, true);
    assert_eq!(log_rows(&store), rows);
    assert!(section(&store, &busan, "volumes").is_some());
    assert!(section(&store, &rotterdam, "volumes").is_some());
    assert_eq!(undo_all(&store, false), preview);
}

#[test]
fn a_run_undone_on_its_own_is_skipped() {
    let (_directory, store) = store();
    let (rotterdam, busan) = two_runs(&store);
    let single = store
        .transact(|write| write.undo_message(LOUPIST, "answer-2", 350))
        .unwrap();
    assert_eq!(single.undone, 1);
    assert_eq!(
        undo_all(&store, false),
        ProgramUndo {
            runs: 1,
            undone: 1,
            kept: 1
        }
    );
    assert!(section(&store, &busan, "volumes").is_none());
    assert!(section(&store, &rotterdam, "volumes").is_none());
}

#[test]
fn a_run_that_cannot_be_undone_leaves_every_run_as_it_was() {
    let (_directory, store) = store();
    let (_, busan) = two_runs(&store);
    // The oldest run names a message the per-message undo refuses, after the
    // newest run was already restored in the same transaction.
    let connection = store.app_store().lock_loupist(LOUPIST).unwrap();
    connection
        .execute(
            "UPDATE scheduled_task_runs SET data_json=json_set(data_json,'$.message_id',?1)
             WHERE task_id=?2 AND json_extract(data_json,'$.message_id')='answer-1'",
            params!["m".repeat(300), TASK],
        )
        .unwrap();
    drop(connection);
    let rows = log_rows(&store);
    assert!(store
        .transact(|write| write.undo_program(LOUPIST, TASK, false, 400))
        .is_err());
    assert_eq!(log_rows(&store), rows);
    assert!(section(&store, &busan, "volumes").is_some());
}
