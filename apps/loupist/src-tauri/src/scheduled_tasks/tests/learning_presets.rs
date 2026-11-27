//! Bundled learning presets: slots, subjects, zones and the stored program.
use std::{collections::BTreeMap, path::Path};

use super::{draft, fixture};
use crate::scheduled_tasks::{
    learning::{load_preset, PresetAsset, SubjectKind},
    types::*,
};

fn agent_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../agent")
}

/// Fictional values from several markets and scripts.
fn sample(slot: &str) -> &'static str {
    match slot {
        "theme" => "Énergie solaire en Afrique de l'Ouest",
        "series" => "Índice ficticio de precios al consumidor",
        "question" => "架空の地方銀行は来期に配当を増やせるか?",
        "market" => "Bolsa Ficticia de Valparaíso",
        other => panic!("no sample for slot {other}"),
    }
}

#[test]
fn every_bundled_preset_fills_its_slots_and_saves_as_a_learning_program() {
    let store = fixture();
    let zone = chrono_tz::Asia::Kolkata;
    for preset_id in LearningPresetId::ALL {
        let id = preset_id.as_str();
        let preset = load_preset(&agent_root(), preset_id).unwrap();
        let slots: BTreeMap<_, _> = preset
            .slots
            .keys()
            .map(|name| (name.clone(), sample(name).to_owned()))
            .collect();
        let asset = (preset.subject_kind == SubjectKind::Asset).then_some(PresetAsset {
            id: "asset-nordvik",
            name: "Nordvik Kraft ASA",
            reference: "NVK",
        });
        let filled = preset.fill(asset, &slots, zone).unwrap();
        for text in [&filled.title, &filled.prompt] {
            assert!(!text.contains(['{', '}']), "{id}: unfilled slot in {text}");
            assert!(!text.contains("TODO"), "{id}: working note in model text");
        }
        for value in slots.values() {
            assert!(filled.prompt.contains(value.as_str()), "{id}");
        }
        assert_eq!(filled.program.preset.as_deref(), Some(id));
        match &filled.schedule {
            Schedule::Daily { timezone, .. } | Schedule::Weekly { timezone, .. } => {
                assert_eq!(timezone, "Asia/Kolkata", "{id}")
            }
            other => panic!("{id}: {other:?}"),
        }
        match preset.subject_kind {
            SubjectKind::Asset => {
                assert!(filled.title.contains("Nordvik Kraft ASA"));
                assert_eq!(
                    filled.subjects,
                    [TaskSubject::Asset {
                        asset: "asset-nordvik".into()
                    }]
                );
            }
            SubjectKind::Name => assert!(matches!(
                filled.subjects.as_slice(),
                [TaskSubject::Name { name }] if slots.values().any(|value| value == name)
            )),
            SubjectKind::None => assert!(filled.subjects.is_empty()),
        }
        let saved = store
            .scheduled_task_create_definition(
                "loupist-a",
                &format!("learning-{id}"),
                TaskDraft {
                    title: filled.title,
                    prompt: filled.prompt,
                    schedule: filled.schedule,
                    update_memory: true,
                    status: TaskStatus::Paused,
                    ..draft()
                },
                TaskKind::Learning,
                TaskDefinition {
                    program: Some(filled.program),
                    watch: TaskWatch {
                        report: Some(filled.report),
                        delivery: Some(filled.delivery),
                        subjects: Some(filled.subjects),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                1,
            )
            .unwrap_or_else(|error| panic!("{id}: {error}"));
        let stored = store
            .scheduled_task_get("loupist-a", &saved.id)
            .unwrap()
            .unwrap();
        assert_eq!(
            stored.program.and_then(|program| program.preset).as_deref(),
            Some(id)
        );
    }
}

#[test]
fn a_preset_refuses_an_unknown_or_missing_slot_with_a_typed_error() {
    let zone = chrono_tz::Europe::Oslo;
    let market = load_preset(&agent_root(), LearningPresetId::Market).unwrap();
    let refusal = |preset: &crate::scheduled_tasks::learning::LearningPreset,
                   asset: Option<PresetAsset<'_>>,
                   slots: &[(&str, &str)]| {
        let slots = slots
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        preset.fill(asset, &slots, zone).unwrap_err()
    };
    let unknown = refusal(
        &market,
        None,
        &[("market", "Fiktiv Børs"), ("exchange", "X")],
    );
    assert_eq!(unknown.code, crate::app::CommandErrorCode::PresetSlot);
    let detail = unknown.detail.unwrap();
    assert_eq!(
        (&detail["preset"], &detail["slot"], &detail["reason"]),
        (&"market".into(), &"exchange".into(), &"unknown".into())
    );
    // The market preset has no default market.
    for slots in [&[][..], &[("market", "  ")][..]] {
        let missing = refusal(&market, None, slots);
        assert_eq!(missing.code, crate::app::CommandErrorCode::PresetSlot);
        assert_eq!(missing.detail.unwrap()["reason"], "missing");
    }
    let multiline = refusal(&market, None, &[("market", "Fiktiv\nBørs")]);
    assert_eq!(multiline.detail.unwrap()["reason"], "invalid");
    let daily = load_preset(&agent_root(), LearningPresetId::AssetDaily).unwrap();
    let asset = || {
        Some(PresetAsset {
            id: "asset-k",
            name: "가상전자",
            reference: "000000",
        })
    };
    assert_eq!(
        refusal(&daily, asset(), &[("asset", "Other")]).code,
        crate::app::CommandErrorCode::PresetSlot
    );
    assert_eq!(
        refusal(&daily, None, &[]).code,
        crate::app::CommandErrorCode::InvalidRequest
    );
    assert_eq!(
        refusal(&market, asset(), &[("market", "Fiktiv Børs")]).code,
        crate::app::CommandErrorCode::InvalidRequest
    );
}

#[test]
fn a_preset_file_never_states_a_time_zone() {
    let root = tempfile::tempdir().unwrap();
    let presets = root.path().join("learning-presets");
    std::fs::create_dir_all(&presets).unwrap();
    let mut definition: serde_json::Value = serde_json::from_slice(
        &std::fs::read(agent_root().join("learning-presets/theme.json")).unwrap(),
    )
    .unwrap();
    assert!(load_preset(&agent_root(), LearningPresetId::Theme).is_ok());
    definition["schedule"]["timezone"] = "UTC".into();
    let path = presets.join("theme.json");
    std::fs::write(&path, serde_json::to_vec(&definition).unwrap()).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    assert!(load_preset(root.path(), LearningPresetId::Theme).is_err());
}

#[test]
fn a_program_round_trips_with_and_without_its_preset() {
    let program = |preset: Option<&str>| LearningProgram {
        depth: LearningDepth::Light,
        scope: LearningScope {
            connections: LearningConnections::Selected(vec!["fictional-statistics".into()]),
            web: false,
        },
        keeps: LearningKeeps {
            sources: true,
            lens: false,
            asset_reads: false,
        },
        preset: preset.map(str::to_owned),
    };
    let store = fixture();
    for (id, preset) in [("learning-plain", None), ("learning-themed", Some("theme"))] {
        let created = store
            .scheduled_task_create_definition(
                "loupist-b",
                id,
                TaskDraft {
                    update_memory: true,
                    ..draft()
                },
                TaskKind::Learning,
                TaskDefinition {
                    program: Some(program(preset)),
                    watch: TaskWatch {
                        report: Some(ReportPolicy::EveryRun),
                        delivery: Some(TaskDelivery::default()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                1,
            )
            .unwrap();
        let stored = store.scheduled_task_get("loupist-b", id).unwrap().unwrap();
        assert_eq!(stored, created);
        assert_eq!(stored.program, Some(program(preset)));
        let wire = serde_json::to_value(&stored).unwrap();
        assert_eq!(
            wire["program"].get("preset"),
            preset.map(Into::into).as_ref()
        );
        assert_eq!(serde_json::from_value::<Task>(wire).unwrap(), stored);
    }
    // A program stored before presets were kept reads as having none.
    let older: LearningProgram = serde_json::from_value(serde_json::json!({
        "depth": "standard",
        "scope": {"connections": "all_enabled", "web": true},
        "keeps": {"sources": true, "lens": true, "asset_reads": false}
    }))
    .unwrap();
    assert_eq!(older.preset, None);
}
