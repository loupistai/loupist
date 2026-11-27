//! Opt-in headless evaluations; production turn authority remains in Rust Chat.
const DEFAULT_AGENT_MODEL: &str = "openai-codex/gpt-6.2-sol";

#[cfg(test)]
mod account_fields_tests;
#[cfg(all(test, target_os = "macos", target_arch = "aarch64", debug_assertions))]
mod approvals_tests;
pub(crate) mod artifacts;
mod batch;
#[cfg(all(test, target_os = "macos", target_arch = "aarch64", debug_assertions))]
mod checks_tests;
#[cfg(test)]
mod citations;
pub(crate) mod clock;
pub mod concurrent;
mod contract;
#[cfg(test)]
mod dart_replay_tests;
#[cfg(test)]
mod followed;
#[cfg(test)]
mod growth;
#[cfg(all(test, target_os = "macos", target_arch = "aarch64", debug_assertions))]
mod judgment_commit_tests;
#[cfg(all(test, target_os = "macos", target_arch = "aarch64", debug_assertions))]
mod learning_tests;
mod observation;
pub(crate) mod replay;
#[cfg(all(test, target_os = "macos", target_arch = "aarch64", debug_assertions))]
mod replay_scenarios;
#[cfg(test)]
mod research;
mod scheduled;
mod seeds;
#[cfg(all(test, target_os = "macos", target_arch = "aarch64", debug_assertions))]
mod session_language_capture;
#[cfg(test)]
mod tests;
mod trace;
mod trial;
#[cfg(test)]
mod unattended_tests;
pub mod world;
#[cfg(test)]
mod world_staging_tests;
use crate::{
    app::{AppState, PiArtifacts},
    commands::{
        chat_runtime::{send_chat, ChatEventSink},
        ChatSendRequest,
    },
    domain::{ChatSession, LoupistCapabilityBinding, MemoryPolicy, MemorySaveSkipReason},
    knowledge::*,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Deserialize)]
struct Suite {
    pub schema: String,
    pub suite: String,
    pub version: u32,
    pub scenarios: Vec<Scenario>,
    #[serde(default)]
    pub budget: Option<Budget>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Budget {
    max_input_tokens_per_turn: u64,
    max_turns: usize,
}
impl Budget {
    fn enforce(&self, record: &mut Value) -> Option<Value> {
        let reason = self.stop_reason(record)?;
        // A failed unit whose usage is unknown, such as a run that stopped
        // before its first model round, is recorded and the batch goes on.
        if record["status"] != "completed" && reason["error"] == "budget_usage_unknown" {
            record["status"] = json!("failed");
            record["budget_note"] = reason;
            return None;
        }
        if record["error"].is_null() {
            record["error"] = reason["error"].clone();
        }
        record["status"] = json!("failed");
        record["budget_stop"] = reason.clone();
        Some(reason)
    }
    fn stop_reason(&self, record: &Value) -> Option<Value> {
        // Pi never started, so no model call spent anything: an infrastructure
        // failure of this unit that `--resume` reruns, not an unknown usage.
        if startup_failed(record) {
            return None;
        }
        let total = ["input", "cache_read", "cache_write"]
            .iter()
            .try_fold(0u64, |sum, key| {
                record["usage"][key]
                    .as_u64()
                    .and_then(|n| sum.checked_add(n))
            });
        match total {
            Some(n) if n <= self.max_input_tokens_per_turn => None,
            Some(n) => Some(
                json!({"error":"budget_exceeded", "limit":"maxInputTokensPerTurn", "maximum":self.max_input_tokens_per_turn, "observed":n}),
            ),
            None => Some(
                json!({"error":"budget_usage_unknown", "limit":"maxInputTokensPerTurn", "maximum":self.max_input_tokens_per_turn, "observed":null}),
            ),
        }
    }
}
fn startup_failed(record: &Value) -> bool {
    record["startup_failed"] == true
}

/// What one recorded turn means for the rest of the batch.
#[derive(Debug, Eq, PartialEq)]
enum AfterTurn {
    /// Run the trial's next turn.
    Continue,
    /// This trial's later turns depend on a turn that did not run: skip them
    /// and measure the next trial. `--resume` reruns the whole trial.
    NextTrial,
    /// Stop the batch: a budget stop or a provider rejection.
    StopBatch,
}

fn after_turn(record: &Value, budget_stopped: bool) -> AfterTurn {
    if budget_stopped || batch::provider_rejection(record) {
        AfterTurn::StopBatch
    } else if record["error"] == "timeout" || startup_failed(record) {
        AfterTurn::NextTrial
    } else {
        AfterTurn::Continue
    }
}

#[derive(Deserialize, Serialize)]
struct Scenario {
    #[serde(default)]
    pub clock: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub connections: Vec<String>,
    pub replay: Option<PathBuf>,
    #[serde(default)]
    pub workspace_files: BTreeMap<String, String>,
    #[serde(default)]
    pub seed: Seed,
    pub sessions: Vec<Session>,
    /// World publications of seeded Sources, scripted or extracted live.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world: Option<world::script::WorldBlock>,
}
#[derive(Default, Deserialize, Serialize)]
struct Seed {
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub lens: Vec<LensSeed>,
    #[serde(default)]
    pub sources: Vec<SourceSeed>,
    #[serde(default)]
    pub principles: Vec<String>,
    #[serde(default)]
    pub watchlist: Vec<WatchSeed>,
    /// The user's reasons and lines for followed assets, tagged with their asset pages.
    #[serde(default)]
    pub journal: Vec<JournalSeed>,
    /// Fixture packs under `evals/sharing/`, added and fully accepted before the run.
    #[serde(default)]
    pub shared_packs: Vec<String>,
    /// Research pages filed after the seed's World publications; their refs
    /// name seeded Sources and scripted facts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lens_pages: Vec<seeds::LensPageSeed>,
    /// Learning programs, turned on, each with an optional completed earlier occurrence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub programs: Vec<scheduled::ProgramSeed>,
    /// Learning requests left today when the first occurrence starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub learning_allowance: Option<u32>,
    /// A followed asset and the World company a seed publication names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asset_links: Vec<seeds::AssetLink>,
    /// Checks saved from a bundled template and turned on, with their dry run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<scheduled::CheckSeed>,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct JournalSeed {
    judgment: String,
    thesis: ThesisSeed,
    #[serde(default)]
    check: Option<crate::knowledge::JournalCheck>,
    #[serde(default)]
    created_at_ms: Option<i64>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ThesisSeed {
    asset: String,
    role: crate::knowledge::assets::ThesisRole,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WatchSeed {
    name: String,
    reference: Option<crate::commands::assets::wire::AssetReferenceFields>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LensSeedSection {
    pub key: String,
    pub heading: String,
    pub markdown: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LensSeed {
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub sections: Vec<LensSeedSection>,
}
#[derive(Deserialize, Serialize)]
struct SourceSeed {
    /// The name `world.publications`, `lens_pages` refs and replay use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub title: String,
    pub text: String,
    pub origin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub company: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<String>,
    /// YYYY-MM-DD or RFC 3339.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication_time: Option<String>,
    /// BCP 47.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}
#[derive(Deserialize, Serialize)]
struct Session {
    pub id: String,
    pub role: String,
    pub memory: String,
    pub turns: Vec<Turn>,
}
#[derive(Default, Deserialize, Serialize)]
struct Turn {
    pub user: String,
    /// Text fixtures submitted through production attachment intake, not prompt text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<TextAttachment>,
    /// Host point-in-time cutoff (YYYY-MM-DD) for this turn's tool reads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub briefing: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_price: Option<String>,
    /// Save and admit one due occurrence through the production scheduler.
    #[serde(
        default,
        deserialize_with = "scheduled::present_fixture",
        skip_serializing_if = "Option::is_none"
    )]
    pub scheduled: Option<scheduled::Fixture>,
    /// How the turn answers the host's cards. `decline`, the default, is a
    /// cautious user who says "Not now" and leaves proposals unconfirmed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approvals: Option<Approvals>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Approvals {
    /// Allow an approval card once, answer a question with its recommended
    /// option, and confirm the schedule proposals the reply saved.
    Accept,
    #[default]
    Decline,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TextAttachment {
    filename: String,
    media_type: String,
    text: String,
}

impl Turn {
    fn attachment_inputs(&self) -> Result<Vec<crate::commands::ChatAttachmentInput>> {
        let inputs = self
            .attachments
            .iter()
            .map(|attachment| {
                if !crate::commands::attachments::attachment_is_textual(&attachment.media_type) {
                    return Err("eval attachments must use a textual media_type".into());
                }
                Ok(crate::commands::ChatAttachmentInput {
                    filename: attachment.filename.clone(),
                    media_type: attachment.media_type.clone(),
                    data_base64: BASE64.encode(attachment.text.as_bytes()),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        crate::commands::attachments::prepare_chat_attachments(&inputs)
            .map_err(|error| error.message)?;
        Ok(inputs)
    }
}
/// Hidden outcomes and expectations are intentionally not deserialized into prompts.
pub struct Options {
    pub suite: PathBuf,
    pub out: PathBuf,
    pub scenarios: Vec<String>,
    pub trials: usize,
    pub model: Option<String>,
    pub thinking: String,
    pub pi_agent_dir: PathBuf,
    pub agent_assets: Option<PathBuf>,
    pub keep: bool,
    pub resume: bool,
    pub tags: Vec<String>,
    pub turn_timeout_ms: u64,
}

/// The `resources` directory holding a staged `name`: this checkout's, or in a
/// linked worktree the shared checkout's, since staging is untracked.
fn staged_resources(name: &str) -> Result<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let local = manifest.join("resources");
    if local.join(name).exists() {
        return Ok(local);
    }
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(manifest)
        .output()?;
    let git = PathBuf::from(String::from_utf8(output.stdout)?.trim());
    Ok(git
        .parent()
        .ok_or("cannot find shared checkout")?
        .join("apps/loupist/src-tauri/resources"))
}

fn artifacts() -> Result<PiArtifacts> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let runtime = if let Some(path) = std::env::var_os("LOUPIST_EVAL_PI_RUNTIME") {
        PathBuf::from(path)
    } else {
        staged_resources("pi-runtime")?.join("pi-runtime")
    };
    let agent = manifest.parent().unwrap().join("agent");
    let executable = runtime.join(if cfg!(windows) {
        "loupist-pi.exe"
    } else {
        "loupist-pi"
    });
    if !executable.is_file() {
        return Err(format!(
            "staged Pi runtime missing: {}; set LOUPIST_EVAL_PI_RUNTIME",
            runtime.display()
        )
        .into());
    }
    Ok(PiArtifacts {
        executable,
        runtime_dir: runtime,
        extension: agent.join("loupist-extension.mjs"),
        provider_extension: agent.join("loupist-provider-extension.mjs"),
        system_prompt: agent.join("SYSTEM.md"),
        provider: String::new(),
        model: String::new(),
        thinking_level: String::new(),
        run_options: Default::default(),
        provider_credential: None,
    })
}
fn configure_market_runtime(
    state: &mut AppState,
    scenario: &Scenario,
    runtime_dir: &Path,
) -> Result<()> {
    if scenario.replay.is_none()
        && scenario
            .connections
            .iter()
            .any(|id| id == "openbb.platform")
    {
        let artifacts = Arc::make_mut(&mut state.artifacts);
        artifacts.mcp_runtimes =
            crate::mcp::discover_bundled_runtimes(runtime_dir, &artifacts.process_lease_dir);
        if !artifacts.mcp_runtimes.contains_key("openbb") {
            return Err(format!(
                "bundled market-data runtime missing or invalid in {}",
                runtime_dir.display()
            )
            .into());
        }
    }
    Ok(())
}

fn memory_policy(mode: &str) -> Result<MemoryPolicy> {
    match mode {
        "remember" => Ok(MemoryPolicy {
            use_memory: true,
            update_memory: true,
        }),
        "read_only" => Ok(MemoryPolicy {
            use_memory: true,
            update_memory: false,
        }),
        "off" => Ok(MemoryPolicy {
            use_memory: false,
            update_memory: false,
        }),
        _ => Err(format!("unknown memory mode: {mode}").into()),
    }
}
fn relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("workspace/replay paths must be relative without parent traversal".into());
    }
    if path
        .components()
        .next()
        .is_some_and(|part| part.as_os_str() == ".loupist")
    {
        return Err("workspace_files cannot write .loupist".into());
    }
    Ok(())
}
fn new_chat(state: &AppState, loupist_id: &str, title: &str) -> Result<ChatSession> {
    let now = chrono::Utc::now().timestamp_millis();
    let chat = ChatSession {
        id: format!("chat-{}", uuid::Uuid::new_v4()),
        loupist_id: loupist_id.into(),
        pi_session_id: uuid::Uuid::new_v4().to_string(),
        pi_session_cache: None,
        title: title.into(),
        title_provisional: false,
        memory_policy: MemoryPolicy::default(),
        set_aside_memory: Vec::new(),
        versions: Vec::new(),
        messages: vec![],
        created_at_ms: now,
        updated_at_ms: now,
        bot_memory: Default::default(),
    };
    state.store.create_chat(&chat)?;
    Ok(chat)
}
/// The turn's message: a host-built procedure or briefing first, then the user text.
fn turn_prompt(state: &AppState, chat: &ChatSession, turn: &Turn) -> Result<String> {
    let mut parts = Vec::new();
    if turn.briefing {
        let (prompt, _) = crate::briefings::build_prompt(
            state,
            &chat.loupist_id,
            chrono::Utc::now().timestamp_millis(),
            crate::scheduled_tasks::types::BriefingEdition::Morning,
        )
        .map_err(|error| error.message)?;
        parts.push(prompt);
    }
    parts.push(turn.user.clone());
    Ok(parts.join("\n\n"))
}

#[cfg(test)]
fn seed(state: &AppState, loupist_id: &str, seed: &Seed) -> Result<seeds::Retained> {
    seed_at(state, loupist_id, seed, &BTreeMap::new())
}

/// Seed the Loupist. A Source a World publication names is retained at that
/// publication's recorded time (`recorded`, by Source id).
fn seed_at(
    state: &AppState,
    loupist_id: &str,
    seed: &Seed,
    recorded: &BTreeMap<String, i64>,
) -> Result<seeds::Retained> {
    if let Some(instructions) = &seed.instructions {
        if instructions.chars().count() > 4_000 {
            return Err("eval Loupist instructions exceed 4000 characters".into());
        }
        state.store.update_loupist_profile(
            loupist_id,
            crate::store::LoupistProfileEdit {
                instructions: Some(instructions),
                ..Default::default()
            },
            chrono::Utc::now().timestamp_millis(),
        )?;
    }
    // A read at a cutoff (a check's dry run replays earlier days) sees only
    // assets followed by then. A seeded asset is followed from the scenario's
    // start: its clock, or its earliest World publication when that is earlier.
    let followed_at = recorded
        .values()
        .fold(clock::now().timestamp_millis(), |first, at| first.min(*at));
    for entry in &seed.watchlist {
        state.store.asset_follow_name(
            loupist_id,
            &entry.name,
            entry.reference.clone(),
            crate::commands::assets::wire::AssetReferenceOrigin::Imported,
            followed_at,
        )?;
    }
    let assets = state.store.assets_list(loupist_id, false)?;
    for entry in &seed.journal {
        let matches = crate::store::user_context::matching(&assets, &entry.thesis.asset);
        let [asset] = matches.as_slice() else {
            return Err(format!(
                "journal thesis seed requires one asset: {}",
                entry.thesis.asset
            )
            .into());
        };
        let at_ms = entry
            .created_at_ms
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
        crate::commands::assets::with_assets(state, loupist_id, true, |tx| {
            let page = crate::knowledge::assets::ensure_asset_page(tx, loupist_id, asset, at_ms)?;
            crate::knowledge::journal::create(
                tx,
                loupist_id,
                crate::knowledge::journal::JournalInput {
                    judgment: entry.judgment.clone(),
                    reasoning: String::new(),
                    entities: Vec::new(),
                    check: entry.check.clone(),
                    thesis: Some(crate::knowledge::assets::JournalThesis {
                        asset_id: page.asset_id,
                        role: entry.thesis.role,
                    }),
                },
                at_ms,
            )
        })?;
    }
    for pack in &seed.shared_packs {
        if pack.is_empty()
            || !pack
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(format!("invalid shared pack fixture name {pack}").into());
        }
        let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../evals/sharing")
            .join(pack);
        crate::sharing::commands::add::seed_shared_pack(state, loupist_id, &folder)
            .map_err(|error| error.message)?;
    }
    if !seed.principles.is_empty() {
        let mut conn = state.store.lock_loupist(loupist_id)?;
        let tx = conn.transaction()?;
        for statement in &seed.principles {
            crate::knowledge::principles::principle_save(
                &tx,
                loupist_id,
                None,
                None,
                crate::knowledge::principles::PrincipleInput {
                    statement: statement.clone(),
                    why: None,
                    example: None,
                    origin: "settings".into(),
                    origin_chat_id: None,
                    origin_message_id: None,
                },
                chrono::Utc::now().timestamp_millis(),
            )?;
        }
        tx.commit()?;
    }
    let store = KnowledgeStore::for_loupist(state.store.clone(), loupist_id)?;
    let now = chrono::Utc::now().timestamp_millis();
    let mut retained = seeds::Retained::new();
    store.transact(|write| {
        for source in &seed.sources {
            let at_ms = recorded
                .get(source.id.as_deref().unwrap_or(""))
                .copied()
                .unwrap_or(now);
            let saved = seeds::retain(write, loupist_id, source, at_ms)?;
            if let Some(id) = &source.id {
                retained.insert(id.clone(), saved);
            }
        }
        for lens in &seed.lens {
            use crate::knowledge::lens_page::{
                Basis, FilingOrigin, LensOp, PageDelta, PageKind, PageTarget, SectionKey,
            };
            let mut ops = Vec::new();
            if !lens.summary.trim().is_empty() {
                ops.push(LensOp::Summary {
                    text: lens.summary.clone(),
                });
            }
            for section in &lens.sections {
                ops.push(LensOp::Put {
                    key: SectionKey::new(&section.key)?,
                    heading: section.heading.clone(),
                    markdown: section.markdown.clone(),
                    basis: Basis::default(),
                    expected: None,
                });
            }
            write.file(
                loupist_id,
                &FilingOrigin::Import {
                    pack: json!({"kind": "eval_seed"}),
                },
                vec![PageDelta {
                    target: PageTarget::Create {
                        kind: PageKind::Method,
                        title: lens.title.clone(),
                        anchors: Vec::new(),
                    },
                    ops,
                }],
                now,
            )?;
        }
        Ok(())
    })?;
    Ok(retained)
}
/// Every scenario of a suite is one this runner can stage, before any trial
/// runs. A suite states its own version; version 2 adds World publications,
/// Learning and check occurrences and their seeds.
fn validate_suite(suite: &Suite) -> Result<()> {
    if suite.schema != "loupist-agent-suite/1" || !matches!(suite.version, 1 | 2) {
        return Err("unsupported suite schema/version".into());
    }
    for scenario in &suite.scenarios {
        validate_seed(scenario)?;
        for name in scenario.workspace_files.keys() {
            relative(Path::new(name))?;
        }
        if let Some(path) = &scenario.replay {
            relative(path)?;
        }
        for session in &scenario.sessions {
            memory_policy(&session.memory)?;
            for turn in &session.turns {
                turn.attachment_inputs()?;
                scheduled::validate(scenario, session, turn)?;
            }
            if !matches!(session.role.as_str(), "train" | "probe") {
                return Err("session role must be train or probe".into());
            }
        }
    }
    Ok(())
}

/// Seeds and World publications name each other consistently, and every seed
/// key the scenario uses is one this runner stages.
fn validate_seed(scenario: &Scenario) -> Result<()> {
    let seed = &scenario.seed;
    let mut ids = std::collections::BTreeSet::new();
    for source in &seed.sources {
        if let Some(id) = &source.id {
            if !ids.insert(id.as_str()) {
                return Err(format!("{}: seed source ids are unique: {id}", scenario.id).into());
            }
        }
        if let Some(time) = &source.publication_time {
            world::time_point(time)?;
        }
    }
    trial::world_times(scenario).map_err(|error| format!("{}: {error}", scenario.id))?;
    let published: std::collections::BTreeSet<_> = scenario
        .world
        .iter()
        .flat_map(|block| &block.publications)
        .filter(|publication| publication.after == world::script::After::seed())
        .flat_map(|publication| publication.script.iter())
        .flat_map(|script| &script.facts)
        .map(|fact| fact.id.as_str())
        .collect();
    let entities: std::collections::BTreeSet<_> = scenario
        .world
        .iter()
        .flat_map(|block| &block.publications)
        .flat_map(|publication| publication.script.iter())
        .flat_map(|script| &script.entities)
        .map(|entity| entity.name.as_str())
        .collect();
    for link in &seed.asset_links {
        if !entities.contains(link.company()) {
            return Err(format!(
                "{}: an asset link names a World entity a scripted publication states: {}",
                scenario.id,
                link.company()
            )
            .into());
        }
    }
    for page in &seed.lens_pages {
        for (source, fact) in page.refs() {
            if !ids.contains(source) {
                return Err(format!(
                    "{}: lens_pages ref names no seeded Source: {source}",
                    scenario.id
                )
                .into());
            }
            if fact.is_some_and(|fact| !published.contains(fact)) {
                return Err(format!(
                    "{}: a lens_pages ref names a fact the seed's scripted publications state",
                    scenario.id
                )
                .into());
            }
        }
    }
    Ok(())
}
fn enable_connections(state: &AppState, loupist: &str, connections: &[String]) -> Result<()> {
    let catalog = crate::marketplace::bundled_catalog()?;
    // A suite's connection list is the complete external capability selection.
    for mut binding in state.store.list_loupist_capabilities(loupist)? {
        if catalog
            .entries
            .iter()
            .any(|entry| entry.id == binding.entry_id)
        {
            binding.enabled = false;
            binding.granted_permissions.clear();
            binding.updated_at_ms = chrono::Utc::now()
                .timestamp_millis()
                .max(binding.updated_at_ms + 1);
            state.store.save_loupist_capability(&binding)?;
        }
    }
    for id in connections {
        let entry = match id.as_str() {
            "opendart" => "opendart.disclosures",
            "krx" => "krx.market-data",
            "koreainvestment" | "kis" => "koreainvestment.market-data",
            "worldbank" | "world-bank" => "world-bank.indicators",
            "defillama" => "defillama.tvl",
            "web" | "web-research" => "community.web-research",
            other => other,
        };
        // Native vendors and web research read through the replay boundary.
        if replay::enabled()
            && crate::finance_data::native_vendor(entry).is_none()
            && entry != "community.web-research"
            && entry != "openbb.platform"
        {
            return Err(format!("connection has no native replay boundary: {id}").into());
        }
        let Some(catalog_entry) = catalog
            .entries
            .iter()
            .find(|candidate| candidate.id == entry)
        else {
            return Err(format!("unknown connection: {id}").into());
        };
        if entry == "koreainvestment.market-data" {
            let environment = replay::kis_environment(&state.credential_environment);
            crate::scoped_config::write_global_connector_settings(
                &state.artifacts.app_data_dir,
                entry,
                &BTreeMap::from([("environment".into(), environment.into())]),
            )?;
            crate::marketplace::connector_config::apply_pending_credential_scope(
                state,
                catalog_entry,
            )?;
        }
        let previous = state.store.get_loupist_capability(loupist, entry)?;
        let timestamp = chrono::Utc::now()
            .timestamp_millis()
            .max(previous.map_or(0, |binding| binding.updated_at_ms + 1));
        state
            .store
            .save_loupist_capability(&LoupistCapabilityBinding {
                loupist_id: loupist.into(),
                entry_id: entry.into(),
                enabled: true,
                granted_permissions: vec!["execute".into()],
                config: Default::default(),
                updated_at_ms: timestamp,
            })?;
    }
    replay::set_openbb_loupist(
        &state.credential_environment,
        connections
            .iter()
            .any(|id| id == "openbb.platform")
            .then_some(loupist),
    );
    Ok(())
}
/// Nobody watches an eval reply, so each card is answered at once instead of
/// holding the turn for the wait limit. Declined, an approval card (a new
/// website after third-party text, a native command or file) is answered "Not
/// now", as a cautious user would, and a question is left unanswered so its
/// call continues with the recommended option. Accepted, the card is allowed
/// once and the question gets its recommended option.
fn settle_waiting_cards(
    loupist_id: &str,
    event: &crate::commands::ChatStreamEvent,
    approvals: Approvals,
) {
    use crate::chat::approvals::{decide, ApprovalDecision, ApprovalState};
    use crate::chat::questions::{self, QuestionAnswer, QuestionState};
    use crate::commands::ChatStreamEvent;
    let (cards, questions) = match event {
        ChatStreamEvent::Progress { progress, .. } => {
            (Some(&progress.approvals), Some(&progress.questions))
        }
        ChatStreamEvent::ProgressPatch { patch, .. } => {
            (patch.approvals.as_ref(), patch.questions.as_ref())
        }
        _ => (None, None),
    };
    let decision = match approvals {
        Approvals::Accept => ApprovalDecision::Once,
        Approvals::Decline => ApprovalDecision::Deny,
    };
    for approval in cards.into_iter().flatten() {
        if approval.state == ApprovalState::Waiting {
            let _ = decide(loupist_id, &approval.request_id, decision);
        }
    }
    for question in questions.into_iter().flatten() {
        if question.state != QuestionState::Waiting {
            continue;
        }
        match (approvals, question.recommended) {
            (Approvals::Accept, Some(choice)) => {
                let _ = questions::answer(
                    loupist_id,
                    &question.request_id,
                    QuestionAnswer::Choice(choice),
                );
            }
            // Dropping the registered sender resolves the waiting call unanswered.
            _ => questions::forget(&question.request_id),
        }
    }
}

/// Confirm, as the user's yes on its card, each schedule proposal `reply`
/// saved. A check turns on at the version its card shows. A card the host
/// refuses to confirm, such as a check without a successful dry run, is
/// recorded with the host's reason.
fn accept_proposals(
    state: &AppState,
    loupist_id: &str,
    reply: &str,
) -> Result<(Vec<String>, Vec<Value>)> {
    let mut confirmed = Vec::new();
    let mut refused = Vec::new();
    for task in state.store.scheduled_tasks_list(loupist_id)? {
        if !task.awaiting_confirmation || task.proposed_in_message_id.as_deref() != Some(reply) {
            continue;
        }
        let answered = match &task.trigger {
            Some(check) => {
                let (version, sha256) = check.pending.as_ref().map_or(
                    (check.script.version, check.script.sha256.as_str()),
                    |pending| (pending.version, pending.sha256.as_str()),
                );
                crate::commands::triggers::approve(
                    state,
                    loupist_id,
                    &task.id,
                    task.revision,
                    version,
                    sha256,
                )
            }
            None => {
                crate::scheduled_tasks::service::confirm(state, loupist_id, &task.id, task.revision)
            }
        };
        match answered {
            Ok(_) => confirmed.push(task.id),
            Err(error) => refused
                .push(json!({"task_id": task.id, "code": error.code, "message": error.message})),
        }
    }
    Ok((confirmed, refused))
}

/// When a turn gives up. The runner's is its `--turn-timeout-ms`; a test gives
/// up at the event it proves instead, which a loaded machine cannot miss.
type Deadline = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

fn after_ms(timeout_ms: u64) -> Deadline {
    Box::pin(tokio::time::sleep(Duration::from_millis(timeout_ms)))
}

async fn run_turn(
    state: &AppState,
    chat: &ChatSession,
    session: &Session,
    turn: &Turn,
    model: &str,
    thinking: &str,
    timeout_ms: u64,
) -> Result<Value> {
    run_turn_until(
        state,
        chat,
        session,
        turn,
        model,
        thinking,
        after_ms(timeout_ms),
    )
    .await
}

/// [`run_turn`], giving up at `deadline`.
async fn run_turn_until(
    state: &AppState,
    chat: &ChatSession,
    session: &Session,
    turn: &Turn,
    model: &str,
    thinking: &str,
    deadline: Deadline,
) -> Result<Value> {
    if let Some(fixture) = &turn.scheduled {
        // Boxed: a scheduled occurrence is a large future for a caller's stack.
        return Box::pin(scheduled::run(
            state, chat, turn, fixture, model, thinking, deadline,
        ))
        .await;
    }
    let policy = memory_policy(&session.memory)?;
    let captured = Arc::new(Mutex::new(observation::TurnObservation::default()));
    let callback = captured.clone();
    // Every sink clone belongs to this turn's native runtime/broker. Its final
    // drop follows terminal persistence and Memory finalization, unlike the
    // reader lease, which is released before the canonical writer acquires.
    // Waiting for the global activity count would also wait for unrelated Chats.
    let (lifetime, mut settled) = tokio::sync::watch::channel(());
    let approvals = turn.approvals.unwrap_or_default();
    let loupist_id = chat.loupist_id.clone();
    let mut sink = ChatEventSink::new(move |event| {
        let _ = &lifetime;
        settle_waiting_cards(&loupist_id, &event, approvals);
        Ok(())
    });
    sink.eval_pi_event = Some(Arc::new(move |event| {
        callback.lock().unwrap().observe(event)
    }));
    let native_reads = captured.clone();
    sink.eval_native_read = Some(Arc::new(move |call_id, authorization| {
        native_reads
            .lock()
            .unwrap()
            .pi
            .native_read_authorized(call_id, authorization);
    }));
    let run_id = format!("eval-{}", uuid::Uuid::new_v4());
    let turn_started_ms = crate::commands::now_ms();
    let started = chrono::Utc::now().to_rfc3339();
    let clock = Instant::now();
    let run_options = model_run_options(state, model)?;
    let execution = async {
        send_chat(
            ChatSendRequest {
                run_id: run_id.clone(),
                loupist_id: chat.loupist_id.clone(),
                thread_id: chat.id.clone(),
                prompt: turn_prompt(state, chat, turn).map_err(|error| error.to_string())?,
                use_memory: policy.use_memory,
                update_memory: policy.update_memory,
                as_of: turn.as_of.clone(),
                model_profile_id: model.into(),
                thinking_level: thinking.into(),
                run_options,
                mentioned_connection_ids: vec![],
                attachments: turn
                    .attachment_inputs()
                    .map_err(|error| error.to_string())?,
                pins: vec![],
                selected_artifact: None,
            },
            sink,
            state,
            None,
        )
        .await
        .map_err(|e| e.message)?;
        let _ = settled.changed().await;
        Ok::<(), String>(())
    };
    tokio::pin!(execution);
    let result = tokio::select! {
        biased;
        done = &mut execution => Ok(done),
        () = deadline => Err(()),
    };
    let failure = match result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error),
        Err(_) => {
            let _ = state
                .cancel_run(&run_id, crate::runs::coordinator::RunKind::Chat)
                .await;
            // Keep setup alive so cancellation uses the owned shutdown path instead
            // of racing Drop's background cleanup against the next scenario/shutdown.
            let _ = tokio::time::timeout(Duration::from_secs(30), &mut execution)
                .await
                .map_err(|_| "timed-out Chat cleanup did not drain")?;
            Some("timeout".into())
        }
    };
    let stored = state
        .store
        .get_chat(&chat.id)?
        .ok_or("persisted chat missing")?;
    let message = stored
        .messages
        .last()
        .filter(|m| m.role == crate::domain::ChatRole::Assistant);
    // A task this turn saved is dated at the turn's clock.
    for task in state.store.scheduled_tasks_list(&chat.loupist_id)? {
        if task.created_at_ms >= turn_started_ms {
            scheduled::date_at_clock(state, task)?;
        }
    }
    let (confirmed, refused) = match (approvals, message) {
        (Approvals::Accept, Some(message)) => {
            accept_proposals(state, &chat.loupist_id, &message.id)?
        }
        _ => Default::default(),
    };
    let mut result = if let Some(message) = message {
        let mut result = trace::persisted(message, &captured.lock().unwrap().pi);
        trace::enrich_commits(state, chat, message, &mut result)?;
        result
    } else {
        let mut result = json!({"answer":"","status":"failed","error":"persisted assistant missing","tool_calls":[],"memory_reads":[],"memory_commits":[],"usage":{"input":null,"output":null,"cache_read":null,"cache_write":null},"performance":{"setup_ms":null,"first_text_ms":null,"answer_text_ms":null,"total_ms":null}});
        let captured = captured.lock().unwrap();
        captured.pi.record_startup_failure(&mut result);
        captured.pi.record_round_usage(&mut result);
        result
    };
    if let Some(error) = failure {
        result["status"] = json!("failed");
        result["error"] = trace::bounded_json(&json!(error));
    }
    if message.is_some_and(|message| {
        message.status == crate::domain::ChatMessageStatus::Error
            || message
                .memory_update
                .as_ref()
                .and_then(|update| update.skip_reason)
                .is_some_and(MemorySaveSkipReason::is_failure)
    }) {
        let profile = state
            .store
            .get_loupist(&chat.loupist_id)?
            .ok_or("eval Loupist missing")?;
        if let Ok(Some(failure)) = crate::runs::log::memory_commit_failure(
            &Path::new(&profile.workspace_root).join(".loupist"),
            &run_id,
        ) {
            result["memory_commit_failure"] = serde_json::to_value(failure)?;
        }
    }
    if let Some(approvals) = turn.approvals {
        result["approvals"] =
            json!({"answer": approvals, "confirmed_tasks": confirmed, "refused": refused});
    }
    // Observe canonical state independently of tool output or the assistant’s claim.
    result["lens"] = trace::lens_state(state, chat, message.map(|m| m.id.as_str()))?;
    result["scheduled_tasks"] = Value::Array(
        state
            .store
            .scheduled_tasks_list(&chat.loupist_id)?
            .iter()
            .map(trace::scheduled_task)
            .collect(),
    );
    result["followed_assets"] = json!(state.store.assets_list(&chat.loupist_id, true)?);
    if let Some(query) = &turn.replay_price {
        let rows = state.store.assets_list(&chat.loupist_id, false)?;
        let assets = crate::store::user_context::matching(&rows, query);
        let [asset] = assets.as_slice() else {
            return Err(format!("replay price requires one asset: {query}").into());
        };
        crate::commands::assets::replay_current(state, &chat.loupist_id, &asset.asset_id, None)
            .await?;
        let view = crate::commands::assets::page_for(
            state,
            crate::commands::assets::wire::AssetOpenRequest {
                loupist_id: chat.loupist_id.clone(),
                asset_id: Some(asset.asset_id.clone()),
                ticker: None,
                entity_id: None,
                stored_only: None,
            },
        )?;
        if let (Some(read), Some(price)) = (view.saved_read, view.current_price) {
            if read.state == crate::commands::assets::wire::PriceReadState::Ready {
                result["replayed_price"] = json!({"asset": asset.name, "state": read.state, "value": price.value, "date": price.date});
            }
        }
    }
    captured.lock().unwrap().write_projection(&mut result);
    observation::label_attempts(&mut result);
    result["started_at"] = json!(started);
    result["duration_ms"] = json!(clock.elapsed().as_millis() as u64);
    result["replay_fallbacks"] = json!(replay::take_fallbacks());
    Ok(result)
}
/// Every run control at its default choice. `LOUPIST_EVAL_PERFORMANCE` (for example `fast`)
/// replaces the `performance` choice for a model that offers that control, so a paid run
/// can use the provider's priority tier; a choice the model does not declare is refused.
fn model_run_options(state: &AppState, model: &str) -> Result<BTreeMap<String, String>> {
    let model = state.model_catalog()?.resolve(model)?;
    let options = with_performance(
        model
            .run_controls
            .iter()
            .map(|control| (control.id.clone(), control.default_choice.clone()))
            .collect(),
        eval_performance().as_deref(),
    );
    model.validate_run_options(&options)?;
    Ok(options)
}

/// The serving mode a paid run asks for (`LOUPIST_EVAL_PERFORMANCE`, for example `fast`),
/// for the agent's and the World's model alike; none keeps each model's default.
fn eval_performance() -> Option<String> {
    std::env::var("LOUPIST_EVAL_PERFORMANCE").ok()
}

fn with_performance(
    mut options: BTreeMap<String, String>,
    performance: Option<&str>,
) -> BTreeMap<String, String> {
    if let (Some(choice), Some(current)) = (performance, options.get_mut("performance")) {
        *current = choice.to_owned();
    }
    options
}

/// Atomically replace a complete JSONL snapshot after each record. An interruption
/// leaves either the previous complete file or the next complete file, never half a line.
fn append_atomic(path: &Path, bytes: &mut Vec<u8>, record: &Value) -> Result<()> {
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        bytes.push(b'\n');
    }
    serde_json::to_writer(&mut *bytes, record)?;
    bytes.push(b'\n');
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}
fn agent_eval_model_route(model: Option<&str>) -> Result<(&str, &str)> {
    let id = model.unwrap_or(DEFAULT_AGENT_MODEL);
    let (provider, _) = id
        .split_once('/')
        .ok_or("expected a provider/model id for the agent eval")?;
    Ok((provider, id))
}

/// One `run_python` compiled-module cache for every trial of an eval run,
/// warmed before the first trial that runs. Warming runs a fixed `pass` in a
/// disposable snapshot and makes no model call.
struct SharedPythonCache {
    root: tempfile::TempDir,
    warmed: bool,
}

impl SharedPythonCache {
    fn new() -> Result<Self> {
        Ok(Self {
            root: tempfile::Builder::new()
                .prefix("loupist-eval-python-")
                .tempdir_in(eval_temp_root())?,
            warmed: false,
        })
    }

    async fn warmed(&mut self) -> &Path {
        if !self.warmed {
            self.warmed = true;
            let started = Instant::now();
            match warm_python_cache(self.root.path()).await {
                Ok(()) => eprintln!(
                    "python cache warmed for this run in {} ms",
                    started.elapsed().as_millis()
                ),
                // A trial's first call then compiles under its own cold bound.
                Err(error) => eprintln!("python cache was not warmed: {error}"),
            }
        }
        self.root.path()
    }
}

/// Where every trial, cache and asset snapshot of a run lives: a short root,
/// by its real path. Pi's extension reads a selected Skill only at a path that
/// is its own real path, and macOS `/tmp` is a link to `/private/tmp`.
pub(super) fn eval_temp_root() -> PathBuf {
    let root = if cfg!(unix) {
        PathBuf::from("/tmp")
    } else {
        std::env::temp_dir()
    };
    fs::canonicalize(&root).unwrap_or(root)
}

async fn warm_python_cache(cache_root: &Path) -> Result<()> {
    let scratch = tempfile::Builder::new()
        .prefix("loupist-eval-python-warm-")
        .tempdir_in(eval_temp_root())?;
    let folder = |name: &str| -> Result<PathBuf> {
        let path = scratch.path().join(name);
        fs::create_dir(&path)?;
        Ok(path)
    };
    let workspace = folder("workspace")?;
    let roots = crate::agent_harness::native_policy::NativeRoots::new(
        &workspace,
        &folder("global")?,
        &folder("app-data")?,
        &workspace.join(".loupist"),
        &folder("python-scratch")?,
        &folder("attachments")?,
        Some(scratch.path()),
    );
    let result = crate::python_run::execute(
        &roots,
        cache_root,
        "pass",
        120,
        crate::web::SearchCancel::never(),
    )
    .await
    .map_err(|error| error.to_string())?;
    if result["status"] != "completed" {
        return Err(format!("the warm-up run ended {}", result["status"]).into());
    }
    Ok(())
}

/// The runner's [`trial::Recorder`]: it stamps each record with its batch,
/// enforces the suite budget and rewrites the JSONL after every record.
struct BatchWriter<'a> {
    out: &'a Path,
    bytes: Vec<u8>,
    summary: batch::Summary,
    run_id: String,
    suite: &'a str,
    budget: Option<&'a Budget>,
    thinking: &'a str,
    scenario: &'a str,
    harness: Value,
    trial: usize,
    turns_run: usize,
    budget_stop: Option<Value>,
}

impl trial::Recorder for BatchWriter<'_> {
    fn admit_turn(&mut self) -> bool {
        match self.budget {
            Some(budget) if self.turns_run >= budget.max_turns => {
                self.budget_stop = Some(
                    json!({"error":"budget_exceeded","limit":"maxTurns","maximum":budget.max_turns,"observed":self.turns_run}),
                );
                false
            }
            _ => true,
        }
    }

    fn record(
        &mut self,
        session: &Session,
        turn_index: usize,
        turn: &Turn,
        model: &crate::providers::catalog::ConfiguredModel,
        mut record: Value,
    ) -> Result<AfterTurn> {
        for (key,value) in json!({"schema":"loupist-agent-trace/1","run_id":self.run_id,"suite":self.suite,"scenario_id":self.scenario,"session_id":session.id,"session_role":session.role,"turn_index":turn_index,"trial":self.trial,"model":{"provider":model.provider,"model":model.model,"thinking":self.thinking},"harness":self.harness,"memory_mode":session.memory,"user":turn.user}).as_object().unwrap(){record[key]=value.clone();}
        record["harness"]["agent_dir_sha256"] = record
            .as_object_mut()
            .unwrap()
            .remove("agent_dir_sha256")
            .unwrap_or(Value::Null);
        record["harness"]["dirty"] = json!(batch::dirty(Path::new(env!("CARGO_MANIFEST_DIR")))?);
        // The turn cap bounds model calls; a Pi that never started made none.
        if !startup_failed(&record) {
            self.turns_run += 1;
        }
        if let Some(reason) = self.budget.and_then(|b| b.enforce(&mut record)) {
            self.budget_stop = Some(reason);
        }
        self.summary.observe(&record);
        append_atomic(self.out, &mut self.bytes, &record)?;
        Ok(after_turn(&record, self.budget_stop.is_some()))
    }
}

pub async fn run(options: Options) -> Result<()> {
    if !options.pi_agent_dir.is_absolute() || !options.pi_agent_dir.is_dir() {
        return Err("--pi-agent-dir must be an existing absolute directory (used in place)".into());
    }
    let summary = batch::Summary::default();
    if options.turn_timeout_ms == 0 {
        return Err("--turn-timeout-ms must be positive".into());
    }
    if options.resume && options.agent_assets.is_some() {
        return Err("--resume cannot be combined with --agent-assets; score each frozen candidate independently".into());
    }
    if options.resume && options.model.is_none() {
        return Err("--resume requires an explicit --model".into());
    }
    let (provider, requested_model) = agent_eval_model_route(options.model.as_deref())?;
    if options.trials == 0 {
        return Err("--trials must be positive".into());
    }
    if options.out.exists() && !options.resume {
        return Err("output already exists; choose a new --out path".into());
    }
    let (suite, optional_keys) = contract::load(&fs::read(&options.suite)?)?;
    if suite
        .budget
        .as_ref()
        .is_some_and(|b| b.max_turns == 0 || b.max_input_tokens_per_turn == 0)
    {
        return Err("suite budget caps must be positive".into());
    }
    validate_suite(&suite)?;
    for id in &options.scenarios {
        match suite.scenarios.iter().find(|s| &s.id == id) {
            None => return Err(format!("unknown scenario: {id}").into()),
            Some(s) if batch::scripted(s) => {
                return Err(
                    format!("{id} is scripted: it runs only in its eval-harness test").into(),
                )
            }
            Some(_) => {}
        }
    }
    let skipped: Vec<_> = suite
        .scenarios
        .iter()
        .filter(|s| batch::scripted(s) && batch::matches(s, &options.scenarios, &options.tags))
        .map(|s| s.id.as_str())
        .collect();
    if !skipped.is_empty() {
        eprintln!(
            "skipped scripted scenarios (they run only in their eval-harness tests): {}",
            skipped.join(", ")
        );
    }
    // Freeze replay bytes with the scenario before any provider work. Resume
    // compares these exact execution inputs, including seeds and local files.
    let inputs = suite
        .scenarios
        .iter()
        .filter(|s| batch::selected(s, &options.scenarios, &options.tags))
        .map(|scenario| {
            Ok((
                scenario,
                batch::ScenarioInput::load(scenario, &options.suite, options.turn_timeout_ms)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    if inputs.is_empty() && !skipped.is_empty() {
        eprintln!("no live scenario selected; no traces written");
        return Ok(());
    }
    let frozen =
        artifacts::FrozenArtifacts::with_override(artifacts()?, options.agent_assets.as_deref())?;
    let pi = frozen.pi.clone();
    let git = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()?;
    if !git.status.success() {
        return Err("cannot determine harness commit".into());
    }
    let harness = json!({"git_commit":String::from_utf8(git.stdout)?.trim(),"dirty":batch::dirty(Path::new(env!("CARGO_MANIFEST_DIR")))?,"system_prompt_sha256":crate::util::sha256(&fs::read(&pi.system_prompt)?),"tool_catalog_sha256":crate::util::sha256(&fs::read(pi.system_prompt.parent().unwrap().join("tool-catalog.json"))?),"agent_snapshot_sha256":frozen.sha256,"agent_assets_override":options.agent_assets.is_some(),"context_policy":frozen.context_policy,"world_prompts_sha256":crate::knowledge::world_extract::world_prompts_sha256()?,"optional_keys":optional_keys,"performance":eval_performance()});
    let bytes = if options.resume && options.out.exists() {
        fs::read(&options.out)?
    } else {
        Vec::new()
    };
    let existing = batch::records(&bytes)?;
    // Every trial's app profile is disposable; one compiled Python runtime is
    // shared so no scenario's first `run_python` pays the cold compile.
    let mut python_cache = SharedPythonCache::new()?;
    let mut host = trial::TrialHost {
        pi: pi.clone(),
        pi_agent_dir: Some(options.pi_agent_dir.clone()),
        config_toml: Some(frozen.config_toml()),
        python_cache: None,
        model: requested_model.to_owned(),
        thinking: options.thinking.clone(),
        turn_timeout_ms: options.turn_timeout_ms,
        discovery: Some(trial::Discovery {
            provider: provider.to_owned(),
            models: None,
        }),
        keep: options.keep,
    };
    let mut writer = BatchWriter {
        out: &options.out,
        bytes,
        summary,
        run_id: uuid::Uuid::new_v4().to_string(),
        suite: &suite.suite,
        budget: suite.budget.as_ref(),
        thinking: &options.thinking,
        scenario: "",
        harness: Value::Null,
        trial: 0,
        turns_run: 0,
        budget_stop: None,
    };
    'scenarios: for (scenario, inputs) in inputs {
        let mut harness = harness.clone();
        harness["scenario_input_sha256"] = json!(inputs.sha256);
        harness["clock"] = json!(scenario.clock);
        harness["language"] = json!(scenario.language);
        let _clock = clock::install(scenario.clock.as_deref())?;
        for trial in 0..options.trials {
            if options.resume
                && batch::complete(
                    &existing,
                    &suite.suite,
                    scenario,
                    trial,
                    requested_model,
                    &options.thinking,
                    &harness,
                )
            {
                eprintln!(
                    "resume: skipped {} trial={} (all turns completed)",
                    scenario.id, trial
                );
                continue;
            }
            host.python_cache = Some(python_cache.warmed().await.to_path_buf());
            writer.scenario = &scenario.id;
            writer.harness = harness.clone();
            writer.trial = trial;
            if trial::run_trial(&mut host, scenario, inputs.replay.as_ref(), &mut writer).await? {
                if writer.budget_stop.is_none() {
                    eprintln!("Stopping batch: provider authentication/quota rejection; no automatic retry.");
                }
                break 'scenarios;
            }
        }
    }
    let BatchWriter {
        bytes,
        summary,
        turns_run,
        budget_stop,
        ..
    } = writer;
    if let Some(exceeded) = budget_stop {
        let path = options.out.with_extension("budget.json");
        fs::write(
            &path,
            serde_json::to_vec_pretty(
                &json!({"error":exceeded["error"],"budget":exceeded,"turns_run":turns_run}),
            )?,
        )?;
        return Err(format!(
            "{}: {}; report {}",
            exceeded["error"].as_str().unwrap_or("budget_stop"),
            exceeded,
            path.display()
        )
        .into());
    }
    if bytes.is_empty() {
        return Err("suite selected no turns".into());
    }
    if summary.failed > 0 {
        return Err(format!(
            "{} turn(s) failed; traces saved to {}",
            summary.failed,
            options.out.display()
        )
        .into());
    }
    Ok(())
}
