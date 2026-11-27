use std::collections::BTreeMap;

use super::*;
use crate::scheduled_tasks::types::{
    AllLearningConnections, DeliveryTarget, LearningProgram, TaskDefinition, TaskDraft, TaskTarget,
    TaskWatch,
};

const OWNER: &str = "loupist-a";

fn state() -> (tempfile::TempDir, AppState) {
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    state
        .store
        .create_loupist(&crate::domain::LoupistProfile {
            root_filesystem_identity: None,
            ..crate::test_fixtures::loupist_profile(OWNER)
        })
        .unwrap();
    (directory, state)
}

fn draft() -> TaskDraft {
    TaskDraft {
        title: "Semiconductor memory supply".into(),
        prompt: "Follow HBM supply contracts and capacity announcements.".into(),
        schedule: Schedule::Weekly {
            days: vec![1, 2, 3, 4, 5],
            hour: 7,
            minute: 15,
            timezone: "Asia/Seoul".into(),
        },
        target: TaskTarget::NewChat,
        model_profile_id: "fixture".into(),
        thinking_level: "medium".into(),
        run_options: BTreeMap::new(),
        use_memory: true,
        update_memory: true,
        status: TaskStatus::Paused,
    }
}

fn definition() -> TaskDefinition {
    TaskDefinition {
        program: Some(LearningProgram {
            depth: LearningDepth::Standard,
            scope: LearningScope {
                connections: LearningConnections::Selected(vec![]),
                web: false,
            },
            keeps: LearningKeeps {
                sources: true,
                lens: true,
                asset_reads: true,
            },
            preset: None,
        }),
        watch: TaskWatch {
            subjects: Some(vec![
                TaskSubject::Asset {
                    asset: "asset-sk-hynix".into(),
                },
                TaskSubject::Name {
                    name: "Mémoire à large bande".into(),
                },
            ]),
            report: Some(ReportPolicy::WhenNew),
            delivery: Some(TaskDelivery {
                to: DeliveryTarget::Bot,
                notify: None,
            }),
            ..TaskWatch::default()
        },
        ..TaskDefinition::default()
    }
}

fn program(state: &AppState, id: &str) -> Task {
    state
        .store
        .scheduled_task_create_definition(OWNER, id, draft(), TaskKind::Learning, definition(), 1)
        .unwrap()
}

fn subjects(count: usize) -> Vec<TaskSubject> {
    (0..count)
        .map(|index| TaskSubject::Name {
            name: format!("Subject {index}"),
        })
        .collect()
}

/// The field a refusal names. Every refusal is `invalid_request` with
/// `{field, reason}`, and the reason is a sentence the editor shows.
fn field(error: &CommandError) -> Option<&str> {
    let detail = error.detail.as_ref()?;
    assert_eq!(error.code, CommandErrorCode::InvalidRequest);
    assert!(detail
        .get("reason")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|reason| !reason.trim().is_empty()));
    detail.get("field")?.as_str()
}

/// The program `patch` would make, built the way `learning_program_save`
/// builds a new one, so both paths face the same validation.
fn saved_through_proposal_path(
    state: &AppState,
    id: &str,
    current: &Task,
    patch: &ProgramPatch,
) -> bool {
    let mut draft = current.draft();
    let mut definition = TaskDefinition {
        program: current.program.clone(),
        watch: current.watch.clone(),
        ..TaskDefinition::default()
    };
    let program = definition.program.as_mut().unwrap();
    draft.title = patch.title.clone().unwrap_or(draft.title);
    draft.prompt = patch.prompt.clone().unwrap_or(draft.prompt);
    draft.schedule = patch.schedule.clone().unwrap_or(draft.schedule);
    program.depth = patch.depth.unwrap_or(program.depth);
    program.scope = patch.sources.clone().unwrap_or(program.scope.clone());
    program.keeps = patch.keeps.unwrap_or(program.keeps);
    if let Some(subjects) = &patch.subjects {
        definition.watch.subjects = Some(subjects.clone());
    }
    definition.watch.report = patch.report.or(definition.watch.report);
    definition.watch.delivery = patch.delivery.or(definition.watch.delivery);
    state
        .store
        .scheduled_task_create_definition(OWNER, id, draft, TaskKind::Learning, definition, 1)
        .is_ok()
}

#[test]
fn the_editor_and_learning_program_save_share_one_validation_table() {
    let (_directory, state) = state();
    let no_asset_reads = LearningKeeps {
        sources: true,
        lens: true,
        asset_reads: false,
    };
    let only_names = vec![TaskSubject::Name {
        name: "Indice des prix à la consommation".into(),
    }];
    let rows: Vec<(&str, ProgramPatch, Option<&str>)> = vec![
        (
            "a new title",
            ProgramPatch {
                title: Some("HBM-Lieferkette".into()),
                ..ProgramPatch::default()
            },
            None,
        ),
        (
            "an empty title",
            ProgramPatch {
                title: Some("  ".into()),
                ..ProgramPatch::default()
            },
            Some("title"),
        ),
        (
            "a title over 200 bytes",
            ProgramPatch {
                title: Some("t".repeat(201)),
                ..ProgramPatch::default()
            },
            Some("title"),
        ),
        (
            "a brief of exactly 8 KiB",
            ProgramPatch {
                prompt: Some("b".repeat(8 * 1024)),
                ..ProgramPatch::default()
            },
            None,
        ),
        (
            "a brief over 8 KiB",
            ProgramPatch {
                prompt: Some("b".repeat(8 * 1024 + 1)),
                ..ProgramPatch::default()
            },
            Some("prompt"),
        ),
        (
            "an empty brief",
            ProgramPatch {
                prompt: Some(String::new()),
                ..ProgramPatch::default()
            },
            Some("prompt"),
        ),
        (
            "no subjects with asset reads off",
            ProgramPatch {
                subjects: Some(vec![]),
                keeps: Some(no_asset_reads),
                ..ProgramPatch::default()
            },
            None,
        ),
        (
            "eight subjects",
            ProgramPatch {
                subjects: Some(subjects(8)),
                keeps: Some(no_asset_reads),
                ..ProgramPatch::default()
            },
            None,
        ),
        (
            "nine subjects",
            ProgramPatch {
                subjects: Some(subjects(9)),
                ..ProgramPatch::default()
            },
            Some("subjects"),
        ),
        (
            "dropping the asset subject while keeping asset reads",
            ProgramPatch {
                subjects: Some(only_names.clone()),
                ..ProgramPatch::default()
            },
            Some("subjects"),
        ),
        (
            "asset reads after the subjects lost their asset",
            ProgramPatch {
                subjects: Some(only_names),
                keeps: Some(no_asset_reads),
                ..ProgramPatch::default()
            },
            None,
        ),
        (
            "a deep, every-run program delivered to the briefing",
            ProgramPatch {
                depth: Some(LearningDepth::Deep),
                report: Some(ReportPolicy::EveryRun),
                delivery: Some(TaskDelivery {
                    to: DeliveryTarget::Briefing,
                    notify: None,
                }),
                ..ProgramPatch::default()
            },
            None,
        ),
        (
            "a daily time in another time zone",
            ProgramPatch {
                schedule: Some(Schedule::Daily {
                    hour: 18,
                    minute: 40,
                    timezone: "America/Sao_Paulo".into(),
                }),
                ..ProgramPatch::default()
            },
            None,
        ),
        (
            "an unknown time zone",
            ProgramPatch {
                schedule: Some(Schedule::Daily {
                    hour: 6,
                    minute: 0,
                    timezone: "Mars/Olympus_Mons".into(),
                }),
                ..ProgramPatch::default()
            },
            Some("schedule"),
        ),
        (
            "an hour past the day",
            ProgramPatch {
                schedule: Some(Schedule::Daily {
                    hour: 24,
                    minute: 0,
                    timezone: "Europe/Berlin".into(),
                }),
                ..ProgramPatch::default()
            },
            Some("schedule"),
        ),
    ];
    for (index, (name, patch, refused)) in rows.into_iter().enumerate() {
        let current = program(&state, &format!("learning-row-{index}"));
        let shared =
            saved_through_proposal_path(&state, &format!("learning-new-{index}"), &current, &patch);
        let edited = update(&state, OWNER, &current.id, current.revision, patch);
        assert_eq!(
            shared,
            refused.is_none(),
            "{name}: the proposal path disagrees with the table"
        );
        match (refused, edited) {
            (None, Ok(task)) => assert_eq!(task.revision, current.revision + 1, "{name}"),
            (Some(expected), Err(error)) => {
                assert_eq!(error.code, CommandErrorCode::InvalidRequest, "{name}");
                assert_eq!(field(&error), Some(expected), "{name}");
                let unchanged = state
                    .store
                    .scheduled_task_get(OWNER, &current.id)
                    .unwrap()
                    .unwrap();
                assert_eq!(
                    unchanged, current,
                    "{name}: a refused edit changed the program"
                );
            }
            (refused, edited) => panic!("{name}: expected refusal {refused:?}, got {edited:?}"),
        }
    }
}

#[test]
fn a_stale_revision_is_refused_and_an_edit_persists() {
    let (_directory, state) = state();
    let current = program(&state, "learning-persist");
    let depth = ProgramPatch {
        depth: Some(LearningDepth::Light),
        schedule: Some(Schedule::Daily {
            hour: 21,
            minute: 5,
            timezone: "Africa/Lagos".into(),
        }),
        ..ProgramPatch::default()
    };
    let edited = update(&state, OWNER, &current.id, current.revision, depth.clone()).unwrap();
    let reloaded = state
        .store
        .scheduled_task_get(OWNER, &current.id)
        .unwrap()
        .unwrap();
    assert_eq!(reloaded.revision, current.revision + 1);
    assert_eq!(
        reloaded.program.as_ref().unwrap().depth,
        LearningDepth::Light
    );
    assert_eq!(reloaded.schedule, edited.schedule);
    assert!(
        !reloaded.awaiting_confirmation,
        "a direct edit is not a proposal"
    );

    let error = update(&state, OWNER, &current.id, current.revision, depth).unwrap_err();
    assert_eq!(error.code, CommandErrorCode::StaleRevision);
    assert_eq!(
        state
            .store
            .scheduled_task_get(OWNER, &current.id)
            .unwrap()
            .unwrap(),
        reloaded
    );

    let same = ProgramPatch {
        depth: Some(LearningDepth::Light),
        ..ProgramPatch::default()
    };
    let unchanged = update(&state, OWNER, &current.id, reloaded.revision, same).unwrap();
    assert_eq!(
        unchanged.revision, reloaded.revision,
        "a patch that changes nothing writes nothing"
    );
    let empty = update(
        &state,
        OWNER,
        &current.id,
        reloaded.revision,
        ProgramPatch::default(),
    );
    assert_eq!(empty.unwrap_err().code, CommandErrorCode::InvalidRequest);
}

#[test]
fn an_active_program_gets_its_next_run_from_the_new_schedule() {
    let (_directory, state) = state();
    let mut active = draft();
    active.status = TaskStatus::Active;
    let current = state
        .store
        .scheduled_task_create_definition(
            OWNER,
            "learning-active",
            active,
            TaskKind::Learning,
            definition(),
            1,
        )
        .unwrap();
    let schedule = Schedule::Interval { every_minutes: 90 };
    let before = now_ms();
    let edited = update(
        &state,
        OWNER,
        &current.id,
        current.revision,
        ProgramPatch {
            schedule: Some(schedule.clone()),
            ..ProgramPatch::default()
        },
    )
    .unwrap();
    let next = edited.next_run_at_ms.unwrap();
    assert!(next >= before + 90 * 60_000 && next <= now_ms() + 90 * 60_000);
    assert_eq!(edited.status, TaskStatus::Active);
}

#[test]
fn a_scope_wider_than_the_loupists_grants_is_refused() {
    let (_directory, state) = state();
    let granted = granted_connections(&state, OWNER).unwrap();
    let ungranted = "fixture.not-enabled-here";
    assert!(!granted.contains(ungranted));
    let current = program(&state, "learning-scope");
    for (name, scope) in [
        (
            "an ungranted connection",
            LearningScope {
                connections: LearningConnections::Selected(vec![ungranted.into()]),
                web: false,
            },
        ),
        (
            "web research the Loupist does not allow",
            LearningScope {
                connections: LearningConnections::Selected(vec![]),
                web: true,
            },
        ),
    ] {
        if name.starts_with("web") && granted.contains(WEB_RESEARCH) {
            continue;
        }
        let error = update(
            &state,
            OWNER,
            &current.id,
            current.revision,
            ProgramPatch {
                sources: Some(scope),
                ..ProgramPatch::default()
            },
        )
        .unwrap_err();
        assert_eq!(field(&error), Some("sources"), "{name}");
    }
    let narrowed = update(
        &state,
        OWNER,
        &current.id,
        current.revision,
        ProgramPatch {
            sources: Some(LearningScope {
                connections: LearningConnections::All(AllLearningConnections::AllEnabled),
                web: false,
            }),
            ..ProgramPatch::default()
        },
    );
    assert!(narrowed.is_ok(), "all enabled never exceeds the grants");
}

#[test]
fn widening_counts_only_what_the_saved_scope_did_not_hold() {
    let granted = BTreeSet::from(["provider.granted".to_owned()]);
    let scope = |ids: &[&str], web| LearningScope {
        connections: LearningConnections::Selected(ids.iter().map(|id| (*id).to_owned()).collect()),
        web,
    };
    let revoked = scope(&["provider.revoked"], true);
    assert!(!widens(
        &revoked,
        &scope(&["provider.revoked"], true),
        &granted
    ));
    assert!(!widens(
        &revoked,
        &scope(&["provider.granted"], false),
        &granted
    ));
    assert!(widens(
        &revoked,
        &scope(&["provider.other"], false),
        &granted
    ));
    assert!(widens(&scope(&[], false), &scope(&[], true), &granted));
    let all = LearningScope {
        connections: LearningConnections::All(AllLearningConnections::AllEnabled),
        web: false,
    };
    assert!(widens(&all, &scope(&["provider.revoked"], false), &granted));
}

#[test]
fn only_a_learning_program_is_edited() {
    let (_directory, state) = state();
    let mut custom = draft();
    custom.update_memory = false;
    let task = state
        .store
        .scheduled_task_create_definition(
            OWNER,
            "custom-1",
            custom,
            TaskKind::Custom,
            TaskDefinition::default(),
            1,
        )
        .unwrap();
    let error = update(
        &state,
        OWNER,
        &task.id,
        task.revision,
        ProgramPatch {
            title: Some("Renamed".into()),
            ..ProgramPatch::default()
        },
    )
    .unwrap_err();
    assert_eq!(error.code, CommandErrorCode::InvalidRequest);
}
