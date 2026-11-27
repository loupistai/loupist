//! A scenario's World publications. `scripted` answers every extraction stage
//! from what the scenario says the Source states, so the production claim,
//! projection, supersession, episodes, growth events and Lens marks run with no
//! model call; `live` sends the same job to the World model.
use crate::{
    app::AppState,
    knowledge::world_extract::hints::FollowedInstrument,
    knowledge::{
        GraphProcessingSettings, InferenceDisposition, InferenceReply, KnowledgeStore, RecordKind,
        RetainedSource, WorldSnapshotBody, CONFLICTS_WITH_CLAIM,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};

type Result<T> = super::Result<T>;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorldBlock {
    pub mode: Mode,
    #[serde(default)]
    pub publications: Vec<Publication>,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Mode {
    Scripted,
    Live,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Publication {
    /// A seeded Source's `id`.
    pub source: String,
    pub after: After,
    /// When the publication is recorded, if not at its Source's publication
    /// time: a late notice is published in one month and learned in another.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_time: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<Script>,
}

/// `"seed"`, or `{session, turn}`: the 0-based turn of that session.
#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub(crate) enum After {
    Named(String),
    Turn { session: String, turn: usize },
}

impl After {
    pub(crate) fn seed() -> Self {
        Self::Named("seed".into())
    }

    pub(crate) fn turn(session: &str, turn: usize) -> Self {
        Self::Turn {
            session: session.into(),
            turn,
        }
    }
}

/// What the Source states, as data.
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Script {
    #[serde(default)]
    pub entities: Vec<Entity>,
    #[serde(default)]
    pub facts: Vec<Fact>,
    /// By the id of a fact of this publication; the named facts may belong to
    /// any publication of the scenario.
    #[serde(default)]
    pub resolutions: BTreeMap<String, Resolution>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entity {
    pub name: String,
    /// An entity type the extraction offers, such as `Organization` or `Person`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Listing or identifying codes as the Source writes them. A bare code is
    /// the listing ticker; `exchange:`, `identifier:` and `original_name:`
    /// prefixes fill those fields instead.
    #[serde(default)]
    pub codes: Vec<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Fact {
    pub id: String,
    pub subject: String,
    pub object: String,
    pub relation: String,
    pub text: String,
    #[serde(default)]
    pub valid_at: Option<String>,
    #[serde(default)]
    pub invalid_at: Option<String>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Resolution {
    #[serde(default)]
    pub duplicates: Vec<String>,
    #[serde(default)]
    pub contradicts: Vec<String>,
    #[serde(default)]
    pub same_assertion: Vec<String>,
}

/// Every reference in the block names something the scenario has.
pub(crate) fn validate(
    block: &WorldBlock,
    sources: &BTreeSet<&str>,
    turns: &BTreeMap<&str, usize>,
) -> Result<()> {
    let mut facts = BTreeSet::new();
    let mut published = BTreeSet::new();
    for publication in &block.publications {
        if !sources.contains(publication.source.as_str()) {
            return Err(format!(
                "world publication names no seeded Source: {}",
                publication.source
            )
            .into());
        }
        if !published.insert(publication.source.as_str()) {
            return Err(format!("a Source is published once: {}", publication.source).into());
        }
        match &publication.after {
            After::Named(name) if name == "seed" => {}
            After::Named(name) => {
                return Err(
                    format!("world publication after must be seed or a turn: {name}").into(),
                )
            }
            After::Turn { session, turn } => {
                if turns
                    .get(session.as_str())
                    .is_none_or(|count| turn >= count)
                {
                    return Err(
                        format!("world publication after names no turn: {session}#{turn}").into(),
                    );
                }
            }
        }
        match (block.mode, &publication.script) {
            (Mode::Scripted, None) => {
                return Err(format!(
                    "a scripted publication needs its script: {}",
                    publication.source
                )
                .into())
            }
            (Mode::Live, Some(_)) => {
                return Err(
                    "a live publication is extracted by the World model; drop its script".into(),
                )
            }
            _ => {}
        }
        let Some(script) = &publication.script else {
            continue;
        };
        let names: BTreeSet<_> = script.entities.iter().map(|e| e.name.as_str()).collect();
        if names.len() != script.entities.len() {
            return Err(format!("entity names repeat in {}", publication.source).into());
        }
        let own: BTreeSet<_> = script.facts.iter().map(|f| f.id.as_str()).collect();
        for fact in &script.facts {
            if !facts.insert(fact.id.clone()) {
                return Err(format!("fact ids are unique per scenario: {}", fact.id).into());
            }
            if !names.contains(fact.subject.as_str()) || !names.contains(fact.object.as_str()) {
                return Err(format!("fact {} names an entity its script lacks", fact.id).into());
            }
            for time in fact.valid_at.iter().chain(&fact.invalid_at) {
                if super::time_point(time).is_err() {
                    return Err(format!("fact {} has an invalid time: {time}", fact.id).into());
                }
            }
        }
        for (id, resolution) in &script.resolutions {
            if !own.contains(id.as_str()) {
                return Err(format!(
                    "a resolution is keyed by a fact of its own publication: {id}"
                )
                .into());
            }
            for named in resolution
                .duplicates
                .iter()
                .chain(&resolution.contradicts)
                .chain(&resolution.same_assertion)
            {
                if !facts.contains(named) {
                    return Err(
                        format!("resolution {id} names an unknown or later fact: {named}").into(),
                    );
                }
            }
            if resolution
                .same_assertion
                .iter()
                .any(|named| !resolution.contradicts.contains(named))
            {
                return Err(
                    format!("resolution {id}: same_assertion must also be contradicted").into(),
                );
            }
        }
    }
    Ok(())
}

/// The World provider name of scripted jobs. No provider offers it, so the
/// background supervisor never claims one of these jobs for a model call.
const SCRIPTED_PROVIDER: &str = "eval-scripted";

/// The instant a publication is recorded at: its `recorded_time`, else its
/// Source's `publication_time`. A date is that day's start at the scenario
/// clock's offset, which stands for the user's time zone.
pub(crate) fn recorded_ms(value: &str, offset: chrono::FixedOffset) -> Result<i64> {
    if let Ok(instant) = chrono::DateTime::parse_from_rfc3339(value) {
        return Ok(instant.timestamp_millis());
    }
    let day = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| format!("a recorded time is RFC 3339 or YYYY-MM-DD: {value}"))?;
    Ok(day
        .and_hms_opt(0, 0, 0)
        .expect("midnight")
        .and_local_timezone(offset)
        .single()
        .ok_or("the recorded day has no single start")?
        .timestamp_millis())
}

struct Staged {
    publication: Publication,
    recorded_ms: i64,
    job_id: Option<String>,
}

/// One trial's World publications.
pub(crate) struct Staging {
    mode: Mode,
    assets: PathBuf,
    /// In recorded order.
    publications: Vec<Staged>,
    /// Every scripted fact staged so far, by id, and the World claim it became.
    facts: BTreeMap<String, Fact>,
    claims: BTreeMap<String, String>,
    staged: Vec<Value>,
    /// Every stage's system message, for a test of what opens a World call.
    #[cfg(test)]
    pub(crate) systems: Vec<String>,
}

impl Staging {
    /// `recorded` gives each publication's recorded instant, by Source id.
    pub(crate) fn new(
        block: &WorldBlock,
        recorded: &BTreeMap<String, i64>,
        assets: PathBuf,
    ) -> Result<Self> {
        let mut publications = block
            .publications
            .iter()
            .map(|publication| {
                Ok(Staged {
                    recorded_ms: *recorded
                        .get(&publication.source)
                        .ok_or_else(|| format!("{} has no recorded time", publication.source))?,
                    publication: publication.clone(),
                    job_id: None,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        publications.sort_by_key(|staged| staged.recorded_ms);
        Ok(Self {
            mode: block.mode,
            assets,
            publications,
            facts: BTreeMap::new(),
            claims: BTreeMap::new(),
            staged: Vec::new(),
            #[cfg(test)]
            systems: Vec::new(),
        })
    }

    /// The World settings this trial's publications use.
    pub(crate) fn settings(
        &self,
        model: &crate::providers::catalog::ConfiguredModel,
        thinking: &str,
    ) -> GraphProcessingSettings {
        let (provider, name, profile) = match self.mode {
            Mode::Scripted => (
                SCRIPTED_PROVIDER.to_owned(),
                "scripted".to_owned(),
                format!("{SCRIPTED_PROVIDER}/scripted"),
            ),
            Mode::Live => (
                model.provider.clone(),
                model.model.clone(),
                model.id.clone(),
            ),
        };
        GraphProcessingSettings {
            enrichment_enabled: true,
            helper_provider: Some(provider),
            helper_model: Some(name),
            helper_model_profile_id: Some(profile),
            world_model_choice: crate::knowledge::WorldModelChoice::Chosen,
            thinking_level: thinking.to_owned(),
            run_options: match self.mode {
                Mode::Scripted => Default::default(),
                Mode::Live => model
                    .run_controls
                    .iter()
                    .map(|control| (control.id.clone(), control.default_choice.clone()))
                    .collect(),
            },
            ..Default::default()
        }
    }

    /// The World claim a scripted fact became, once its publication is staged.
    pub(crate) fn claim(&self, fact: &str) -> Option<&str> {
        self.claims.get(fact).map(String::as_str)
    }

    /// Queue every scripted publication's World job at seed, at its recorded
    /// time, so a publication placed after a turn is pending until then. A
    /// live job is queued at its publication, since the supervisor would
    /// otherwise extract it at once.
    pub(crate) fn queue(
        &mut self,
        state: &AppState,
        loupist_id: &str,
        retained: &super::super::seeds::Retained,
    ) -> Result<()> {
        if self.mode != Mode::Scripted {
            return Ok(());
        }
        let store = KnowledgeStore::for_loupist(state.store.clone(), loupist_id)?;
        let settings = saved_settings(state, loupist_id)?;
        for staged in &mut self.publications {
            let source = retained
                .get(&staged.publication.source)
                .ok_or("a published Source was not seeded")?;
            let job = store
                .transact(|write| {
                    write.enqueue_source_world(loupist_id, source, &settings, staged.recorded_ms)
                })?
                .ok_or("the Source has no text span to extract")?;
            staged.job_id = Some(job.job_id);
        }
        Ok(())
    }

    /// Publish everything placed at `after`, in recorded order, each under its
    /// recorded clock.
    pub(crate) async fn publish(
        &mut self,
        state: &AppState,
        loupist_id: &str,
        retained: &super::super::seeds::Retained,
        after: &After,
        timeout_ms: u64,
    ) -> Result<()> {
        let store = KnowledgeStore::for_loupist(state.store.clone(), loupist_id)?;
        let settings = saved_settings(state, loupist_id)?;
        let due: Vec<_> = (0..self.publications.len())
            .filter(|&index| self.publications[index].publication.after == *after)
            .collect();
        for index in due {
            let recorded_ms = self.publications[index].recorded_ms;
            let publication = self.publications[index].publication.clone();
            let source = retained
                .get(&publication.source)
                .ok_or("a published Source was not seeded")?;
            // At the scenario clock's offset, which stands for the user's time zone.
            let offset = super::super::clock::offset()
                .unwrap_or_else(|| chrono::FixedOffset::east_opt(0).expect("UTC"));
            let instant = chrono::DateTime::from_timestamp_millis(recorded_ms)
                .ok_or("a recorded time is out of range")?
                .with_timezone(&offset)
                .to_rfc3339();
            let _clock = super::super::clock::install(Some(&instant))?;
            let job_id = match self.publications[index].job_id.clone() {
                Some(job_id) => job_id,
                None => {
                    store
                        .transact(|write| {
                            write.enqueue_source_world(loupist_id, source, &settings, recorded_ms)
                        })?
                        .ok_or("the Source has no text span to extract")?
                        .job_id
                }
            };
            let mut record = json!({
                "source": publication.source, "after": publication.after, "mode": self.mode,
                "recorded_at": instant, "job_id": job_id,
            });
            let status = match (&publication.script, self.mode) {
                (Some(script), Mode::Scripted) => {
                    for fact in &script.facts {
                        self.facts.insert(fact.id.clone(), fact.clone());
                    }
                    let (status, calls, unresolved, _systems) = drive(
                        &store,
                        &followed(state, loupist_id),
                        &settings,
                        &job_id,
                        recorded_ms,
                        script,
                        &self.facts,
                        &self.assets,
                    )
                    .await?;
                    record["stage_answers"] = json!(calls);
                    record["unresolved"] = json!(unresolved);
                    #[cfg(test)]
                    self.systems.extend(_systems);
                    status
                }
                _ => drain(state, &store, loupist_id, &job_id, timeout_ms).await?,
            };
            record["status"] = json!(status);
            if !matches!(status.as_str(), "completed" | "reconciled_without_retry") {
                return Err(
                    format!("World publication of {} ended {status}", publication.source).into(),
                );
            }
            if let Some(script) = &publication.script {
                self.bind_claims(&store, loupist_id, script, source)?;
            }
            self.staged.push(record);
        }
        Ok(())
    }

    /// Extract every other queued Source, such as one the agent saved, with
    /// an empty script: a scripted World states only what its scenario says.
    pub(crate) async fn settle_unscripted(
        &mut self,
        state: &AppState,
        loupist_id: &str,
    ) -> Result<()> {
        if self.mode != Mode::Scripted {
            return Ok(());
        }
        let store = KnowledgeStore::for_loupist(state.store.clone(), loupist_id)?;
        let settings = saved_settings(state, loupist_id)?;
        let ours: BTreeSet<_> = self
            .publications
            .iter()
            .filter_map(|staged| staged.job_id.clone())
            .collect();
        for job in store.graph_jobs(loupist_id)? {
            if job.status != "queued" || ours.contains(&job.job_id) {
                continue;
            }
            let (status, _, _, _) = drive(
                &store,
                &followed(state, loupist_id),
                &settings,
                &job.job_id,
                crate::commands::now_ms(),
                &Script::default(),
                &self.facts,
                &self.assets,
            )
            .await?;
            self.staged
                .push(json!({"job_id": job.job_id, "mode": "unscripted", "status": status}));
        }
        Ok(())
    }

    fn bind_claims(
        &mut self,
        store: &KnowledgeStore,
        loupist_id: &str,
        script: &Script,
        source: &RetainedSource,
    ) -> Result<()> {
        let span = source.span_id.as_deref().ok_or("the Source has no span")?;
        let claims = head(store, loupist_id)?
            .map(|body| body.claims)
            .unwrap_or_default();
        for fact in &script.facts {
            let matched: Vec<_> = claims
                .iter()
                .filter(|claim| {
                    same_text(&claim.proposition, &fact.text)
                        && claim.evidence.iter().any(|binding| binding.span_id == span)
                })
                .collect();
            match matched.as_slice() {
                [claim] => {
                    self.claims.insert(fact.id.clone(), claim.id.clone());
                }
                // A duplicate the script resolved merges into an earlier claim.
                [] => {}
                _ => return Err(format!("fact {} became several claims", fact.id).into()),
            }
        }
        Ok(())
    }

    /// The `world_staged` trace record: what was published and the World it left.
    pub(crate) fn trace(&self, state: &AppState, loupist_id: &str) -> Result<Value> {
        let store = KnowledgeStore::for_loupist(state.store.clone(), loupist_id)?;
        let claims = head(&store, loupist_id)?
            .map(|body| body.claims)
            .unwrap_or_default();
        Ok(super::super::trace::world_staged(
            &self.staged,
            claims.len(),
            claims
                .iter()
                .filter(|claim| claim.invalid_at.is_some())
                .count(),
            claims
                .iter()
                .filter(|claim| claim.qualifier(CONFLICTS_WITH_CLAIM).is_some())
                .count(),
        ))
    }
}

/// What the Loupist follows, named to a World job as the production supervisor names it.
fn followed(state: &AppState, loupist_id: &str) -> Vec<FollowedInstrument> {
    state
        .store
        .map_context(loupist_id)
        .map(|context| {
            context
                .watchlist
                .into_iter()
                .map(|item| {
                    let symbol = item.symbol;
                    let name = item
                        .display_name
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or_else(|| symbol.clone());
                    FollowedInstrument { name, symbol }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn saved_settings(state: &AppState, loupist_id: &str) -> Result<GraphProcessingSettings> {
    let connection = state.store.lock_loupist(loupist_id)?;
    Ok(crate::knowledge::load_settings(&connection, loupist_id)?)
}

/// Run one World job through the production pipeline, answering its stages from `script`.
#[allow(clippy::too_many_arguments)]
async fn drive(
    store: &KnowledgeStore,
    followed: &[FollowedInstrument],
    settings: &GraphProcessingSettings,
    job_id: &str,
    at_ms: i64,
    script: &Script,
    facts: &BTreeMap<String, Fact>,
    assets: &Path,
) -> Result<(String, BTreeMap<String, u32>, Vec<String>, Vec<String>)> {
    let responder = std::sync::Mutex::new(Responder {
        script,
        facts,
        assets,
        unresolved: Vec::new(),
        calls: BTreeMap::new(),
        systems: Vec::new(),
    });
    let mut settings = settings.clone();
    let view = crate::knowledge::world_extract::drive_job_with_account(
        store,
        &mut settings,
        job_id,
        // The scripted callback stands in for the provider; this only admits
        // the helper.
        "api_key",
        at_ms,
        followed,
        assets,
        None,
        None,
        |_| {},
        |request: Value| std::future::ready(responder.lock().unwrap().answer(&request)),
    )
    .await?;
    let responder = responder.into_inner().unwrap();
    Ok((
        view.status,
        responder.calls,
        responder.unresolved,
        responder.systems,
    ))
}

/// The sealed World head's claims and nodes, when a World exists.
pub(crate) fn head(store: &KnowledgeStore, loupist_id: &str) -> Result<Option<WorldSnapshotBody>> {
    for record in store.list_records(loupist_id)? {
        if record.kind != RecordKind::World {
            continue;
        }
        let Some(revision) = &record.revision_id else {
            continue;
        };
        let snapshot = store.record_snapshot(loupist_id, revision)?;
        return Ok(Some(serde_json::from_value(snapshot.payload)?));
    }
    Ok(None)
}

/// Drive a live job through the production supervisor until it settles.
async fn drain(
    state: &AppState,
    store: &KnowledgeStore,
    loupist_id: &str,
    job_id: &str,
    timeout_ms: u64,
) -> Result<String> {
    let started = std::time::Instant::now();
    loop {
        let status = store
            .graph_jobs(loupist_id)?
            .into_iter()
            .find(|job| job.job_id == job_id)
            .map(|job| job.status)
            .ok_or("the World job disappeared")?;
        if !matches!(status.as_str(), "queued" | "running") {
            return Ok(status);
        }
        let remaining = Duration::from_millis(timeout_ms).saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Ok("world_stage_timeout".into());
        }
        match tokio::time::timeout(remaining, state.graph.pump_once()).await {
            Ok(Ok(Some(_))) => {}
            Ok(Ok(None)) => tokio::time::sleep(Duration::from_millis(100)).await,
            Ok(Err(error)) => return Err(error.to_string().into()),
            Err(_) => return Ok("world_stage_timeout".into()),
        }
    }
}

fn same_text(left: &str, right: &str) -> bool {
    left.split_whitespace().eq(right.split_whitespace())
}

/// Answers each World stage of one publication from its script.
struct Responder<'a> {
    script: &'a Script,
    /// Every scripted fact of the scenario staged so far, this publication's included.
    facts: &'a BTreeMap<String, Fact>,
    assets: &'a Path,
    /// A resolution the pipeline gave no chance to apply: the fact it names was
    /// not among the candidates the stage offered.
    unresolved: Vec<String>,
    calls: BTreeMap<String, u32>,
    /// The system message of each stage request.
    systems: Vec<String>,
}

impl Responder<'_> {
    fn answer(&mut self, request: &Value) -> InferenceReply {
        if request["kind"] == "embedding" {
            let inputs: Vec<String> =
                serde_json::from_value(request["inputs"].clone()).unwrap_or_default();
            return match crate::knowledge::embed_local_batch(self.assets, &inputs) {
                Ok(vectors) => {
                    completed(json!({"status": "ok", "json_object": null, "vectors": vectors}))
                }
                Err(_) => InferenceReply {
                    disposition: InferenceDisposition::NotSent,
                    body: json!({"status": "not_sent"}),
                },
            };
        }
        let title = request["response_schema"]["title"]
            .as_str()
            .unwrap_or("")
            .to_owned();
        *self.calls.entry(title.clone()).or_default() += 1;
        if let Some(system) = request["messages"][0]["content"].as_str() {
            self.systems.push(system.to_owned());
        }
        let user = request["messages"][1]["content"].as_str().unwrap_or("");
        let value = match title.as_str() {
            "ExtractedEntities" => self.entities(user),
            "NodeResolutions" => resolve_nodes(user),
            "ExtractedEdges" => self.edges(),
            "EdgeDuplicate" => self.resolve_edge(user),
            "EdgeResolutionBatch" => {
                let cases = first_json(user).unwrap_or(Value::Null);
                let mut reply = serde_json::Map::new();
                for case in request["response_schema"]["required"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    let Some(case) = case.as_str() else { continue };
                    let content = cases[case]
                        .as_array()
                        .and_then(|messages| {
                            messages.iter().find(|message| message["role"] == "user")
                        })
                        .and_then(|message| message["content"].as_str())
                        .unwrap_or("");
                    reply.insert(case.to_owned(), self.resolve_edge(content));
                }
                Value::Object(reply)
            }
            "CoreAttributeBatch" => self.attributes(user),
            "SummarizedEntities" => self.summaries(),
            _ => {
                return InferenceReply {
                    disposition: InferenceDisposition::NotSent,
                    body: json!({"status": "not_sent"}),
                }
            }
        };
        completed(json!({
            "status": "ok", "json_object": value,
            "usage": {"input_tokens": 0, "output_tokens": 0, "certainty": "accounted"},
        }))
    }

    fn entities(&self, user: &str) -> Value {
        let types = entity_types(user);
        let entities: Vec<_> = self
            .script
            .entities
            .iter()
            .map(|entity| {
                json!({
                    "name": entity.name,
                    "entity_type_id": types.get(entity.kind.as_str()).copied().unwrap_or(0),
                    "episode_indices": [0],
                })
            })
            .collect();
        json!({"extracted_entities": entities})
    }

    fn edges(&self) -> Value {
        let edges: Vec<_> = self
            .script
            .facts
            .iter()
            .map(|fact| {
                json!({
                    "source_entity_name": fact.subject, "target_entity_name": fact.object,
                    "relation_type": fact.relation, "fact": fact.text,
                    "valid_at": fact.valid_at, "invalid_at": fact.invalid_at,
                    "episode_indices": [0],
                })
            })
            .collect();
        json!({"edges": edges})
    }

    fn resolve_edge(&mut self, user: &str) -> Value {
        let empty =
            json!({"duplicate_facts": [], "contradicted_facts": [], "same_assertion_facts": []});
        let Some(new) = section(user, "<NEW FACT>", "</NEW FACT>") else {
            self.unresolved
                .push("a resolver request without its new fact".into());
            return empty;
        };
        // The new fact is its text, or a literal with its validity fields
        // when an ending is being resolved.
        let Some(fact) =
            self.script.facts.iter().find(|fact| {
                same_text(&fact.text, new) || !indexed_entry(new, &fact.text).is_empty()
            })
        else {
            self.unresolved
                .push(format!("an unscripted new fact: {new}"));
            return empty;
        };
        let Some(resolution) = self.script.resolutions.get(&fact.id) else {
            return empty;
        };
        let existing = section(user, "<EXISTING FACTS>", "</EXISTING FACTS>").unwrap_or("");
        let candidates = section(
            user,
            "<FACT INVALIDATION CANDIDATES>",
            "</FACT INVALIDATION CANDIDATES>",
        )
        .unwrap_or("");
        let mut indices = |named: &[String], lists: &[&str]| -> Vec<i64> {
            let mut found = Vec::new();
            for id in named {
                let Some(text) = self.facts.get(id).map(|fact| fact.text.as_str()) else {
                    continue;
                };
                let hits: Vec<_> = lists.iter().flat_map(|list| indexed(list, text)).collect();
                if hits.is_empty() {
                    self.unresolved
                        .push(format!("{}: {id} was not a candidate", fact.id));
                }
                found.extend(hits);
            }
            found.sort_unstable();
            found.dedup();
            found
        };
        json!({
            "duplicate_facts": indices(&resolution.duplicates, &[existing]),
            "contradicted_facts": indices(&resolution.contradicts, &[existing, candidates]),
            "same_assertion_facts": indices(&resolution.same_assertion, &[existing, candidates]),
        })
    }

    fn attributes(&self, user: &str) -> Value {
        let request = first_json(user).unwrap_or(Value::Null);
        let rows: Vec<_> = request["entities"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| {
                let entity = self
                    .script
                    .entities
                    .iter()
                    .find(|entity| Some(entity.name.as_str()) == row["name"].as_str())?;
                let offered = row["fields"].as_object()?;
                let mut attributes = serde_json::Map::new();
                for code in &entity.codes {
                    let (field, value) = match code.split_once(':') {
                        Some((field, value))
                            if matches!(
                                field,
                                "ticker" | "exchange" | "identifier" | "original_name"
                            ) =>
                        {
                            (field, value)
                        }
                        _ => ("ticker", code.as_str()),
                    };
                    if offered.contains_key(field) && !attributes.contains_key(field) {
                        attributes.insert(field.to_owned(), json!(value));
                    }
                }
                (!attributes.is_empty())
                    .then(|| json!({"index": row["index"], "attributes": attributes}))
            })
            .collect();
        json!({"entities": rows})
    }

    fn summaries(&self) -> Value {
        let summaries: Vec<_> = self
            .script
            .entities
            .iter()
            .filter_map(|entity| {
                let said: Vec<_> = self
                    .script
                    .facts
                    .iter()
                    .filter(|fact| fact.subject == entity.name || fact.object == entity.name)
                    .map(|fact| fact.text.as_str())
                    .collect();
                (!said.is_empty()).then(|| {
                    json!({"name": entity.name, "summary": crate::util::shorten(&said.join(" "), 900)})
                })
            })
            .collect();
        json!({"summaries": summaries})
    }
}

fn completed(body: Value) -> InferenceReply {
    InferenceReply {
        disposition: InferenceDisposition::Completed,
        body,
    }
}

/// The first JSON value of a stage message that may carry more text after it.
fn first_json(text: &str) -> Option<Value> {
    serde_json::Deserializer::from_str(text.trim_start())
        .into_iter::<Value>()
        .next()?
        .ok()
}

fn section<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let end = start + text[start..].find(close)?;
    Some(text[start..end].trim())
}

/// The extraction's entity type ids by name, from its `<ENTITY TYPES>` list.
fn entity_types(user: &str) -> BTreeMap<&str, i64> {
    let mut types = BTreeMap::new();
    let mut rest = section(user, "<ENTITY TYPES>", "</ENTITY TYPES>").unwrap_or("");
    while let Some(at) = rest.find("'entity_type_id': ") {
        rest = &rest[at + "'entity_type_id': ".len()..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        let Some(name) = rest
            .find("'entity_type_name': '")
            .map(|at| &rest[at + "'entity_type_name': '".len()..])
            .and_then(|name| name.split_once('\'').map(|(name, _)| name))
        else {
            break;
        };
        if let Ok(id) = digits.parse() {
            types.insert(name, id);
        }
    }
    types
}

/// Answer `dedupe_nodes` by name: a candidate with the same name is the duplicate.
fn resolve_nodes(user: &str) -> Value {
    let list = |open, close| {
        section(user, open, close)
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .unwrap_or(Value::Null)
    };
    let entities = list("<ENTITIES>", "</ENTITIES>");
    let existing = list("<EXISTING ENTITIES>", "</EXISTING ENTITIES>");
    let fold = |name: &str| {
        name.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let resolutions: Vec<_> = entities
        .as_array()
        .into_iter()
        .flatten()
        .map(|entity| {
            let name = entity["name"].as_str().unwrap_or("");
            let duplicate = existing
                .as_array()
                .into_iter()
                .flatten()
                .find(|candidate| {
                    candidate["name"]
                        .as_str()
                        .is_some_and(|other| fold(other) == fold(name))
                })
                .and_then(|candidate| candidate["candidate_id"].as_i64())
                .unwrap_or(-1);
            json!({"id": entity["id"], "name": name, "duplicate_candidate_id": duplicate})
        })
        .collect();
    json!({"entity_resolutions": resolutions})
}

/// The `idx` of every entry of a resolver fact list whose fact is `text`. The
/// list is printed as Python literals, `[{'idx': 0, 'fact': '...'}, ...]`,
/// with validity fields after the fact when an ending is being resolved.
fn indexed(list: &str, text: &str) -> Vec<i64> {
    indexed_entry(list, text)
        .into_iter()
        .filter_map(|start| {
            let entry = &list[..start];
            let at = entry.rfind("{'idx': ")? + "{'idx': ".len();
            entry[at..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .ok()
        })
        .collect()
}

/// Where each `'fact': <text>` field of a Python literal starts.
fn indexed_entry(literal: &str, text: &str) -> Vec<usize> {
    let needle = format!("'fact': {}", python_repr(text));
    literal
        .match_indices(&needle)
        .filter(|(at, _)| matches!(literal[at + needle.len()..].chars().next(), Some(',' | '}')))
        .map(|(at, _)| at)
        .collect()
}

/// Python's `repr` of a string, as the resolver prompt prints its facts.
fn python_repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch == quote => {
                out.push('\\');
                out.push(ch);
            }
            ch if (ch as u32) < 0x20 || ch as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out.push(quote);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolver_lists_are_read_back_by_python_literal() {
        let list = r#"[{'idx': 0, 'fact': 'A supplies B with \'cells\' and "modules".'}, {'idx': 1, 'fact': "Lila Trent's tenure ended."}, {'idx': 2, 'fact': 'Plain fact.'}]"#;
        assert_eq!(
            indexed(list, "A supplies B with 'cells' and \"modules\"."),
            [0]
        );
        assert_eq!(indexed(list, "Lila Trent's tenure ended."), [1]);
        assert_eq!(indexed(list, "Plain fact."), [2]);
        assert!(indexed(list, "Absent.").is_empty());
        let with_validity = "[{'idx': 3, 'fact': 'Old supply.', 'relation_type': 'SUPPLIES', 'valid_at': '2026-06-01'}]";
        assert_eq!(indexed(with_validity, "Old supply."), [3]);
        assert!(indexed(with_validity, "Old supply").is_empty());
    }

    #[test]
    fn entity_types_are_read_from_the_extraction_list() {
        let user = "<ENTITY TYPES>\n[{'entity_type_id': 0, 'entity_type_name': 'Entity', 'entity_type_description': 'x'}, {'entity_type_id': 1, 'entity_type_name': 'Organization', 'entity_type_description': 'y'}]\n</ENTITY TYPES>";
        let types = entity_types(user);
        assert_eq!(types.get("Organization"), Some(&1));
        assert_eq!(types.get("Entity"), Some(&0));
    }
}
