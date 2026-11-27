use std::collections::BTreeMap;

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
    (directory, store)
}

/// A learning program and one finished run whose answer is `message_id`,
/// recorded on the run as the scheduler records a completed run's answer.
fn learning_run(
    store: &KnowledgeStore,
    task_id: &str,
    title: &str,
    message_id: &str,
) -> (String, String) {
    let app = store.app_store();
    let task = app
        .scheduled_task_create_definition(
            LOUPIST,
            task_id,
            TaskDraft {
                title: title.into(),
                prompt: "Follow the policy rate decisions and the statements after them.".into(),
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
    let (claim, ()) = app
        .scheduled_task_claim_with(LOUPIST, &task.id, 2, true, |_| Ok(()))
        .unwrap()
        .unwrap();
    let run = claim.run;
    app.scheduled_task_finish(LOUPIST, &run.run_id, RunStatus::Succeeded, None, 3)
        .unwrap();
    let mut connection = app.lock_loupist(LOUPIST).unwrap();
    connection
        .transaction()
        .and_then(|tx| {
            tx.execute(
                "UPDATE scheduled_task_runs SET data_json=json_set(data_json,'$.message_id',?2) WHERE run_id=?1",
                params![run.run_id, message_id],
            )?;
            tx.commit()
        })
        .unwrap();
    (run.thread_id.unwrap(), run.run_id)
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

fn run_turn(chat_id: &str, message_id: &str) -> FilingOrigin {
    FilingOrigin::Turn {
        chat_id: chat_id.into(),
        message_id: message_id.into(),
        user_message_id: None,
        unattended: true,
    }
}

fn marks(store: &KnowledgeStore, record_id: &str) -> BTreeMap<String, LearnedMark> {
    store
        .read(|write| learned_marks(write.tx, LOUPIST, record_id))
        .unwrap()
}

#[test]
fn a_learning_runs_op_is_marked_until_a_later_op_changes_its_section() {
    let (_directory, store) = store();
    let (chat, run_id) = learning_run(&store, "learning-rates", "Taux directeurs", "answer-rates");
    let filed = store
        .transact(|write| {
            write.file(
                LOUPIST,
                &run_turn(&chat, "answer-rates"),
                vec![PageDelta {
                    target: PageTarget::Create {
                        kind: PageKind::Subject,
                        title: "Banco Central do Brasil".into(),
                        anchors: vec![AnchorInput::Name {
                            name: "Banco Central do Brasil".into(),
                        }],
                    },
                    ops: vec![
                        put("decisions", "The Selic rate was held.", None),
                        put("guidance", "The statement kept its wording.", None),
                    ],
                }],
                100,
            )
        })
        .unwrap();
    let record_id = filed[0].record_id.clone();
    let filed_marks = marks(&store, &record_id);
    assert_eq!(filed_marks.len(), 2);
    assert_eq!(
        filed_marks["decisions"],
        LearnedMark {
            at_ms: 100,
            task_id: "learning-rates".into(),
            run_id: run_id.clone(),
            program_title: "Taux directeurs".into(),
        }
    );

    let page = store.read_page(LOUPIST, &record_id).unwrap();
    let decisions = page
        .body
        .sections()
        .iter()
        .find(|section| section.key.as_str() == "decisions")
        .unwrap();
    assert_eq!(
        decisions.learned.as_ref(),
        Some(&filed_marks["decisions"]),
        "the page view carries the mark"
    );
    let digest = decisions.digest.clone();
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
                        record_id: record_id.clone(),
                    },
                    ops: vec![put("decisions", "Held, with two dissents.", Some(digest))],
                }],
                200,
            )
        })
        .unwrap();
    let after_edit = marks(&store, &record_id);
    assert!(store
        .read_page(LOUPIST, &record_id)
        .unwrap()
        .body
        .sections()
        .iter()
        .any(|section| section.key.as_str() == "decisions" && section.learned.is_none()));
    assert!(
        !after_edit.contains_key("decisions"),
        "a later op clears the mark"
    );
    assert_eq!(
        after_edit["guidance"].run_id, run_id,
        "other sections keep theirs"
    );
}

#[test]
fn an_op_from_another_chat_or_message_is_not_a_learned_mark() {
    let (_directory, store) = store();
    let (chat, _) = learning_run(&store, "learning-ports", "港湾の取扱量", "answer-ports");
    let file = |origin: FilingOrigin, title: &str| {
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
                        ops: vec![put("volumes", "Container volumes rose.", None)],
                    }],
                    100,
                )
            })
            .unwrap()[0]
            .record_id
            .clone()
    };
    let elsewhere = file(
        run_turn("chat-research", "answer-ports"),
        "Port of Rotterdam",
    );
    assert!(
        marks(&store, &elsewhere).is_empty(),
        "the chat must be the run's"
    );
    let other_message = file(run_turn(&chat, "answer-unrelated"), "Port of Busan");
    assert!(
        marks(&store, &other_message).is_empty(),
        "the message must be the run's"
    );
}
