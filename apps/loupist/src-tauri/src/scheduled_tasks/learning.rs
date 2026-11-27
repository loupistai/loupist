//! Context and bundled definitions for learning runs.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    app::{AppState, CommandError},
    commands::map_store,
    knowledge::{KnowledgeStore, RecordKind, WorldSnapshotBody},
};

use super::types::{
    LearningConnections, LearningDepth, LearningKeeps, LearningProgram, ReportPolicy, Run,
    Schedule, Task, TaskDelivery, TaskSubject,
};

#[derive(Debug)]
pub(crate) struct TurnContext {
    pub(crate) body: Value,
    pub(crate) asset_ids: Vec<String>,
    pub(crate) keeps: LearningKeeps,
    pub(crate) untrusted: bool,
}

pub(crate) fn allows_connection(program: &LearningProgram, id: &str) -> bool {
    if id == "community.web-research" {
        return program.scope.web;
    }
    match &program.scope.connections {
        LearningConnections::All(_) => true,
        LearningConnections::Selected(ids) => ids.iter().any(|selected| selected == id),
    }
}

fn model_title(text: &str) -> String {
    use crate::custom_connections::taint::{delimited, fresh_id};
    delimited(
        &crate::util::shorten(text, 200),
        "record title",
        &fresh_id(),
    )
}

pub(crate) fn build_context(
    state: &AppState,
    task: &Task,
    run: &Run,
    program: &LearningProgram,
    language: &str,
) -> Result<TurnContext, CommandError> {
    let owner = &task.loupist_id;
    let earlier = state
        .store
        .scheduled_task_runs(owner, &task.id)
        .map_err(map_store)?;
    let completed: Vec<_> = earlier
        .iter()
        .filter(|candidate| super::is_previous_success(candidate, run))
        .collect();
    let messages: Vec<_> = completed
        .iter()
        .filter_map(|run| run.message_id.clone())
        .collect();
    let digs =
        crate::commands::knowledge::learned_digests(state, owner, &messages).unwrap_or_default();
    let assets = state.store.assets_list(owner, true).map_err(map_store)?;
    let knowledge = KnowledgeStore::for_loupist(state.store.clone(), owner).ok();
    let lens_pages = knowledge
        .as_ref()
        .and_then(|store| store.read(|read| read.lens_index(owner)).ok())
        .unwrap_or_default();
    let subjects = task.watch.subjects.as_deref().unwrap_or_default();
    let world = subjects
        .iter()
        .any(|subject| matches!(subject, TaskSubject::World { .. }))
        .then(|| {
            knowledge
                .as_ref()?
                .read(|read| {
                    for record in read.list_records(owner)? {
                        if record.kind == RecordKind::World {
                            if let Some(revision) = record.revision_id {
                                let head = read.get_revision(owner, &revision)?;
                                if let Ok(snapshot) =
                                    serde_json::from_value::<WorldSnapshotBody>(head.payload)
                                {
                                    return Ok(Some(snapshot));
                                }
                            }
                        }
                    }
                    Ok(None)
                })
                .ok()
                .flatten()
        })
        .flatten();
    let mut asset_ids = Vec::new();
    let subjects: Vec<_> = subjects
        .iter()
        .filter_map(|subject| match subject {
            TaskSubject::Asset { asset } => {
                let row = assets.iter().find(|row| row.asset_id == *asset)?;
                asset_ids.push(asset.clone());
                Some(json!({ "asset": asset, "name": model_title(&row.name) }))
            }
            TaskSubject::World { world: id } => {
                let node = world.as_ref()?.nodes.iter().find(|node| node.id == *id)?;
                Some(json!({ "world": id, "name": model_title(&node.name) }))
            }
            TaskSubject::Lens { lens } => {
                let page = lens_pages.iter().find(|page| page.record_id == *lens)?;
                Some(json!({ "lens": lens, "name": model_title(&page.title) }))
            }
            TaskSubject::Name { name } => Some(json!({ "name": name })),
        })
        .collect();
    let mut seen = BTreeSet::new();
    let maintained_pages: Vec<_> = digs
        .iter()
        .flat_map(|digest| &digest.pages)
        .filter(|page| seen.insert(&page.record_id))
        .filter_map(|filed| {
            lens_pages
                .iter()
                .find(|page| page.record_id == filed.record_id)
        })
        .take(16)
        .map(|page| json!({"id": page.record_id, "title": model_title(&page.title)}))
        .collect();
    let connection = state.store.lock_loupist(owner).map_err(map_store)?;
    let mut source_titles = connection
        .prepare(
            "SELECT DISTINCT r.title FROM knowledge_growth_events g
         JOIN knowledge_records r ON r.record_id=g.record_id
         WHERE g.loupist_id=?1 AND g.origin_message_id=?2 AND g.kind='source_retained'
         ORDER BY r.title LIMIT 12",
        )
        .map_err(crate::commands::map_internal)?;
    let mut previous_runs = Vec::new();
    for earlier in completed.iter().take(5) {
        let mut sources = Vec::new();
        let mut pages = Vec::new();
        if let Some(message) = earlier.message_id.as_deref() {
            if let Some(digest) = digs.iter().find(|digest| digest.message_id == message) {
                pages.extend(digest.pages.iter().take(16).map(|page| json!({
                    "id": page.record_id,
                    "sections": page.outcomes.iter().filter_map(|op| op.key.as_deref()).take(8).collect::<BTreeSet<_>>(),
                })));
            }
            let rows = source_titles
                .query_map(rusqlite::params![owner, message], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(crate::commands::map_internal)?;
            sources.extend(rows.filter_map(Result::ok).map(|title| model_title(&title)));
        }
        previous_runs.push(json!({
            "startedAtMs": earlier.started_at_ms,
            "outcome": earlier.budget.map_or(super::types::BudgetOutcome::None, |budget| budget.outcome),
            "quiet": earlier.quiet,
            "pages": pages,
            "sources": sources,
        }));
    }
    let mut language = language.to_owned();
    crate::agent_harness::interface::resolve_language(&mut language);
    let scope = json!({
        "connections": match &program.scope.connections {
            LearningConnections::All(_) => json!("all_enabled"),
            LearningConnections::Selected(ids) => json!(ids.len()),
        },
        "web": program.scope.web,
    });
    Ok(TurnContext {
        asset_ids,
        keeps: program.keeps,
        untrusted: !messages.is_empty() || world.is_some(),
        body: json!({
            "date": crate::local_time::today().to_string(),
            "appLanguage": language,
            "program": {
                "id": task.id, "title": task.title, "subjects": subjects, "depth": program.depth,
                "scope": scope, "keeps": program.keeps,
                "report": task.watch.report.unwrap_or(ReportPolicy::WhenNew),
            },
            "firstRun": completed.is_empty(),
            "previousRuns": previous_runs,
            "maintainedPages": maintained_pages,
            "budget": run.budget.map(|budget| json!({ "requests": budget.requests })),
        }),
    })
}

/// A Loupist's learning settings, from its config.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LearningSettings {
    /// Requests automatic learning runs may use per local day; 0 turns them off.
    pub(crate) daily_requests: u32,
    /// Learning programs that may be active at once.
    pub(crate) program_cap: u32,
}

impl Default for LearningSettings {
    fn default() -> Self {
        Self {
            daily_requests: super::types::DEFAULT_LEARNING_DAILY_REQUESTS,
            program_cap: super::types::DEFAULT_LEARNING_PROGRAM_CAP,
        }
    }
}

/// Refuse a learning allowance below a light update's hard budget, other
/// than 0, and a program cap outside 1 to 50.
pub(crate) fn check_settings(
    daily_requests: Option<u32>,
    program_cap: Option<u32>,
) -> Result<(), CommandError> {
    let minimum = LearningProgram::light_update_hard_budget();
    if let Some(requests) = daily_requests.filter(|requests| (1..minimum).contains(requests)) {
        return Err(CommandError::with_detail(
            "invalid_configuration",
            format!("learning_daily_requests must be zero or at least {minimum}, not {requests}"),
            json!({"reason": "below_learning_update", "minimum": minimum}),
        ));
    }
    let maximum = super::types::MAX_LEARNING_PROGRAM_CAP;
    if let Some(cap) = program_cap.filter(|cap| !(1..=maximum).contains(cap)) {
        return Err(CommandError::with_detail(
            "invalid_configuration",
            format!("learning_program_cap must be between 1 and {maximum}, not {cap}"),
            json!({"reason": "learning_program_cap", "minimum": 1, "maximum": maximum}),
        ));
    }
    Ok(())
}

/// The learning settings of the Loupist whose database `connection` holds.
/// A Loupist folder's database, `.loupist/loupist.sqlite3`, sits beside its
/// config file. Any other database, such as a test's single database, has the
/// defaults. An invalid config refuses, as it blocks the Loupist's runs.
pub(crate) fn settings_beside(
    connection: &rusqlite::Connection,
) -> crate::store::StoreResult<LearningSettings> {
    let named = |path: Option<&Path>, name: &str| {
        path.and_then(Path::file_name) == Some(std::ffi::OsStr::new(name))
    };
    let database = connection.path().map(Path::new);
    let folder = database.and_then(Path::parent);
    let Some(folder) =
        folder.filter(|_| named(database, "loupist.sqlite3") && named(folder, ".loupist"))
    else {
        return Ok(LearningSettings::default());
    };
    crate::scoped_config::learning_settings(folder)
        .map_err(|error| crate::store::StoreError::Invalid(error.message))
}

/// A bundled program template. The host fills its slots once, at creation; no
/// preset file states a time zone, so activation supplies the user's.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LearningPreset {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) brief: String,
    /// Where the brief's model text is still a working sentence. Never sent.
    #[serde(default)]
    #[allow(dead_code, reason = "documentation inside the preset file")]
    notes: Option<String>,
    /// The text slots the caller fills, by name.
    pub(crate) slots: BTreeMap<String, SlotSpec>,
    pub(crate) subject_kind: SubjectKind,
    /// For a `name` preset, the slot whose value becomes the program's subject.
    #[serde(default)]
    pub(crate) subject_slot: Option<String>,
    pub(crate) schedule: PresetSchedule,
    pub(crate) program: PresetProgram,
    pub(crate) report: ReportPolicy,
    pub(crate) delivery: TaskDelivery,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SlotSpec {
    pub(crate) max_chars: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PresetProgram {
    pub(crate) depth: LearningDepth,
    pub(crate) scope: super::types::LearningScope,
    pub(crate) keeps: LearningKeeps,
}

/// A preset's cadence without a zone.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PresetSchedule {
    Daily { hour: u8, minute: u8 },
    Weekly { days: Vec<u8>, hour: u8, minute: u8 },
}

impl PresetSchedule {
    pub(crate) fn in_zone(self, zone: chrono_tz::Tz) -> Schedule {
        let timezone = zone.to_string();
        match self {
            Self::Daily { hour, minute } => Schedule::Daily {
                hour,
                minute,
                timezone,
            },
            Self::Weekly { days, hour, minute } => Schedule::Weekly {
                days,
                hour,
                minute,
                timezone,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SubjectKind {
    /// One followed asset; fills `{asset}`, `{name}` and `{reference}`.
    Asset,
    /// The `subject_slot` value becomes a `{name}` subject.
    Name,
    /// No subject.
    None,
}

/// The followed asset an `asset` preset is about.
pub(crate) struct PresetAsset<'a> {
    pub(crate) id: &'a str,
    pub(crate) name: &'a str,
    pub(crate) reference: &'a str,
}

/// A preset with its slots filled, ready to become a learning task.
#[derive(Debug, PartialEq)]
pub(crate) struct FilledProgram {
    pub(crate) title: String,
    pub(crate) prompt: String,
    pub(crate) schedule: Schedule,
    pub(crate) subjects: Vec<TaskSubject>,
    pub(crate) program: LearningProgram,
    pub(crate) report: ReportPolicy,
    pub(crate) delivery: TaskDelivery,
}

fn slot_error(preset: &str, slot: &str, reason: &str, message: &str) -> CommandError {
    CommandError::with_detail(
        crate::app::CommandErrorCode::PresetSlot,
        message,
        json!({"preset": preset, "slot": slot, "reason": reason}),
    )
}

impl LearningPreset {
    /// Slot names the asset subject fills.
    const ASSET_SLOTS: [&'static str; 3] = ["asset", "name", "reference"];

    fn validate(&self, id: &str) -> Result<(), CommandError> {
        let invalid =
            |message: &str| CommandError::internal(format!("Learning preset {id}: {message}"));
        if self.id != id {
            return Err(CommandError::invalid("Learning preset identity is invalid"));
        }
        let asset_slots: &[&str] = if self.subject_kind == SubjectKind::Asset {
            &Self::ASSET_SLOTS
        } else {
            &[]
        };
        if self
            .slots
            .keys()
            .any(|name| asset_slots.contains(&name.as_str()))
        {
            return Err(invalid("a text slot shadows a subject slot"));
        }
        for text in [&self.title, &self.brief] {
            for name in slot_names(text) {
                if !self.slots.contains_key(name) && !asset_slots.contains(&name) {
                    return Err(invalid("a template names an undeclared slot"));
                }
            }
        }
        if self
            .slots
            .values()
            .any(|slot| slot.max_chars == 0 || slot.max_chars > 1000)
        {
            return Err(invalid("a slot bound is out of range"));
        }
        match (self.subject_kind, self.subject_slot.as_deref()) {
            (SubjectKind::Name, Some(slot)) if self.slots.contains_key(slot) => {}
            (SubjectKind::Asset | SubjectKind::None, None) => {}
            _ => return Err(invalid("the subject slot does not match the subject kind")),
        }
        if self.program.keeps.asset_reads && self.subject_kind != SubjectKind::Asset {
            return Err(invalid("only an asset preset keeps asset reads"));
        }
        if let PresetSchedule::Weekly { days, .. } = &self.schedule {
            if days.is_empty() || days.iter().any(|day| *day > 6) {
                return Err(invalid("weekly days are out of range"));
            }
        }
        Ok(())
    }

    /// Fill the slots once. Every given slot must be declared and every
    /// declared slot given; `zone` is the user's time zone at activation.
    pub(crate) fn fill(
        &self,
        asset: Option<PresetAsset<'_>>,
        slots: &BTreeMap<String, String>,
        zone: chrono_tz::Tz,
    ) -> Result<FilledProgram, CommandError> {
        let id = self.id.as_str();
        if let Some(name) = slots.keys().find(|name| !self.slots.contains_key(*name)) {
            return Err(slot_error(
                id,
                name,
                "unknown",
                "This preset has no slot of that name",
            ));
        }
        let mut values = BTreeMap::new();
        for (name, spec) in &self.slots {
            let value = slots
                .get(name)
                .map(|value| value.trim())
                .unwrap_or_default();
            if value.is_empty() {
                return Err(slot_error(
                    id,
                    name,
                    "missing",
                    "This preset needs a value for the slot",
                ));
            }
            if value.chars().count() > spec.max_chars || value.chars().any(char::is_control) {
                return Err(slot_error(
                    id,
                    name,
                    "invalid",
                    "The slot value must be one line within its length",
                ));
            }
            values.insert(name.as_str(), value);
        }
        let subjects = match (self.subject_kind, &asset) {
            (SubjectKind::Asset, Some(asset)) => {
                values.insert("asset", asset.name);
                values.insert("name", asset.name);
                values.insert("reference", asset.reference);
                vec![TaskSubject::Asset {
                    asset: asset.id.to_owned(),
                }]
            }
            (SubjectKind::Asset, None) => {
                return Err(CommandError::invalid(
                    "This learning preset needs one asset subject",
                ))
            }
            (_, Some(_)) => {
                return Err(CommandError::invalid(
                    "This learning preset takes no asset subject",
                ))
            }
            (SubjectKind::Name, None) => vec![TaskSubject::Name {
                name: values[self.subject_slot.as_deref().unwrap_or_default()].to_owned(),
            }],
            (SubjectKind::None, None) => Vec::new(),
        };
        let render = |text: &str| {
            let mut chunks = text.split('{');
            let mut result = chunks.next().unwrap_or_default().to_owned();
            for chunk in chunks {
                if let Some((slot, suffix)) = chunk.split_once('}') {
                    result.push_str(values.get(slot).copied().unwrap_or_default());
                    result.push_str(suffix);
                }
            }
            result
        };
        let title = render(&self.title);
        let title = if title.chars().count() > 200 {
            let cut: String = title.chars().take(199).collect();
            format!("{}…", cut.trim_end())
        } else {
            title
        };
        Ok(FilledProgram {
            title,
            prompt: render(&self.brief),
            schedule: self.schedule.clone().in_zone(zone),
            subjects,
            program: LearningProgram {
                depth: self.program.depth,
                scope: self.program.scope.clone(),
                keeps: self.program.keeps,
                preset: Some(self.id.clone()),
            },
            report: self.report,
            delivery: self.delivery,
        })
    }
}

fn slot_names(text: &str) -> impl Iterator<Item = &str> {
    text.split('{')
        .skip(1)
        .filter_map(|chunk| chunk.split_once('}').map(|(name, _)| name))
}

pub(crate) fn load_preset(
    agent_root: &Path,
    id: super::types::LearningPresetId,
) -> Result<LearningPreset, CommandError> {
    let id = id.as_str();
    let definition = crate::fs::artifact_trust::read_private_regular_file_bounded(
        &agent_root
            .join("learning-presets")
            .join(format!("{id}.json")),
        16 * 1024,
    )
    .map_err(crate::commands::map_internal)?;
    let preset: LearningPreset =
        serde_json::from_slice(&definition).map_err(crate::commands::map_internal)?;
    preset.validate(id)?;
    Ok(preset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_asset_preset_loads_with_weekday_and_delivery_defaults() {
        let agent_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../agent");
        let preset = load_preset(
            &agent_root,
            super::super::types::LearningPresetId::AssetDaily,
        )
        .unwrap();
        assert!(matches!(preset.subject_kind, SubjectKind::Asset));
        assert_eq!(
            preset.schedule,
            PresetSchedule::Weekly {
                days: vec![1, 2, 3, 4, 5],
                hour: 6,
                minute: 30
            }
        );
        assert_eq!(preset.report, ReportPolicy::WhenNew);
        assert_eq!(
            preset.delivery.to,
            super::super::types::DeliveryTarget::Briefing
        );
    }
}
