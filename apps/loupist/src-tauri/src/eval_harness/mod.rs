//! Opt-in headless evaluations; production turn authority remains in Rust Chat.
pub(crate) mod artifacts;
mod batch;
pub mod briefing;
#[cfg(test)]
mod citations;
pub mod concurrent;
#[cfg(test)]
mod dart_replay_tests;
#[cfg(test)]
mod finance_seed_tests;
#[cfg(test)]
mod followed;
#[cfg(test)]
mod growth;
#[cfg(test)]
mod holdings_tests;
pub(crate) mod replay;
#[cfg(all(test, target_os = "macos", target_arch = "aarch64", debug_assertions))]
mod replay_scenarios;
#[cfg(test)]
mod research;
#[cfg(test)]
mod tests;
mod trace;
pub mod world;
use crate::{
    app::{AppState, PiArtifacts},
    commands::{
        chat_runtime::{send_chat, ChatEventSink},
        ChatSendRequest,
    },
    domain::{ChatSession, LoupistCapabilityBinding, MemoryPolicy},
    knowledge::*,
    store::LoupistStore,
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
        if record["error"].is_null() {
            record["error"] = reason["error"].clone();
        }
        record["status"] = json!("failed");
        record["budget_stop"] = reason.clone();
        Some(reason)
    }
    fn stop_reason(&self, record: &Value) -> Option<Value> {
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
#[derive(Deserialize, Serialize)]
struct Scenario {
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
    pub holdings_csv: Option<String>,
    /// An exchange balance file, imported through the crypto preview and commit.
    #[serde(default)]
    pub holdings_crypto_csv: Option<String>,
    #[serde(default)]
    pub watchlist: Vec<WatchSeed>,
    /// The user's reasons and lines for followed assets, tagged with their asset pages.
    #[serde(default)]
    pub journal: Vec<JournalSeed>,
    /// Runs the asset pages' host refresh (no model call) before the first turn.
    #[serde(default)]
    pub refresh_assets: bool,
    /// Results events with their rows, as the host or an earlier reply recorded them.
    #[serde(default)]
    pub earnings: Vec<EarningsSeed>,
    /// Runs one local watch check (no model call) before the first turn; the
    /// watchers' starting point lies before `baseline_before`.
    #[serde(default)]
    pub watch_check: Option<WatchCheckSeed>,
    /// Fixture packs under `evals/sharing/`, added and fully accepted before the run.
    #[serde(default)]
    pub shared_packs: Vec<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WatchCheckSeed {
    baseline_before: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EarningsSeed {
    symbol: String,
    market: String,
    period: String,
    report_date: String,
    #[serde(default)]
    released: Option<EarningsReleaseSeed>,
    #[serde(default)]
    rows: Vec<EarningsRowSeed>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EarningsReleaseSeed {
    date: String,
    title: String,
    filing_id: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EarningsRowSeed {
    side: String,
    measure: String,
    #[serde(default)]
    low: Option<String>,
    #[serde(default)]
    high: Option<String>,
    #[serde(default)]
    point: Option<String>,
    unit: String,
    #[serde(default)]
    currency: Option<String>,
    kind: String,
    fact_date: String,
    source_title: String,
    source_url: String,
}
/// Which results procedure a turn runs (the host builds its prompt).
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EarningsPromptSeed {
    symbol: String,
    market: String,
    period: String,
    side: String,
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
    symbol: String,
    market: String,
    role: crate::knowledge::assets::ThesisRole,
}
#[derive(Deserialize, Serialize)]
struct WatchSeed {
    symbol: String,
    market: String,
    display_name: Option<String>,
}
#[derive(Deserialize, Serialize)]
struct LensSeedSection {
    pub key: String,
    pub heading: String,
    pub markdown: String,
}
#[derive(Deserialize, Serialize)]
struct LensSeed {
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub sections: Vec<LensSeedSection>,
}
#[derive(Deserialize, Serialize)]
struct SourceSeed {
    pub title: String,
    pub text: String,
    pub origin: String,
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
    /// Runs the host-built results procedure for one event before the user text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub earnings_prompt: Option<EarningsPromptSeed>,
    /// Sends the host-built briefing prompt (with its watch alerts) before the user text.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub briefing: bool,
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
        memory_policy: MemoryPolicy::default(),
        set_aside_memory: Vec::new(),
        versions: Vec::new(),
        messages: vec![],
        created_at_ms: now,
        updated_at_ms: now,
    };
    state.store.create_chat(&chat)?;
    Ok(chat)
}
fn seed_day(value: &str) -> Result<chrono::NaiveDate> {
    Ok(chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")?)
}

/// A results event and its rows (fictional sources), for the earnings evals.
fn seed_earnings(
    state: &AppState,
    loupist_id: &str,
    watchlist: &[WatchSeed],
    entry: &EarningsSeed,
) -> Result<()> {
    use crate::knowledge::earnings;
    let name = watchlist
        .iter()
        .find(|item| item.symbol == entry.symbol && item.market == entry.market)
        .and_then(|item| item.display_name.clone())
        .unwrap_or_else(|| entry.symbol.clone());
    let report_date = seed_day(&entry.report_date)?;
    let mut rows = Vec::new();
    for row in &entry.rows {
        rows.push(earnings::NewRow {
            side: earnings::Side::parse(&row.side).ok_or("unknown earnings side")?,
            measure: row.measure.clone(),
            basis: String::new(),
            period: entry.period.clone(),
            low: row.low.clone(),
            high: row.high.clone(),
            point: row.point.clone(),
            unit: earnings::Unit::parse(&row.unit).ok_or("unknown earnings unit")?,
            currency: row.currency.clone(),
            kind: earnings::ValueKind::parse(&row.kind).ok_or("unknown earnings kind")?,
            stage: None,
            provider: None,
            evidence: vec![crate::knowledge::assets::AssetEvidence {
                tool: "web_fetch".into(),
                title: row.source_title.clone(),
                url: Some(row.source_url.clone()),
                date: Some(row.fact_date.clone()),
                ..Default::default()
            }],
            extra: json!({}),
            fact_date: seed_day(&row.fact_date)?,
            captured_as_of: None,
        });
    }
    crate::commands::assets::with_assets(state, loupist_id, true, |tx| {
        let page = crate::knowledge::assets::ensure_page(
            tx,
            loupist_id,
            &crate::knowledge::assets::AssetIdentity {
                symbol: entry.symbol.clone(),
                market: entry.market.clone(),
                name: name.clone(),
            },
            None,
            1,
        )?;
        let end = earnings::dates::label_end(&entry.period);
        let (event, _) = earnings::upsert_event(
            tx,
            loupist_id,
            &page.asset_id,
            &entry.period,
            end,
            report_date,
            earnings::DateSource::Calendar,
            1,
        )?;
        earnings::insert_rows(
            tx,
            loupist_id,
            &event.event_id,
            &rows,
            earnings::Origin::Host,
            None,
            2,
        )?;
        if !rows.is_empty() {
            earnings::mark_before(tx, loupist_id, &event.event_id, 2, None)?;
        }
        if let Some(release) = &entry.released {
            let current = earnings::get_event(tx, loupist_id, &event.event_id)?
                .ok_or(crate::knowledge::KnowledgeError::NotFound)?;
            earnings::set_release(
                tx,
                loupist_id,
                &current,
                &earnings::FilingRef {
                    form: None,
                    title: release.title.clone(),
                    date: release.date.clone(),
                    filing_id: release.filing_id.clone(),
                    evidence: crate::knowledge::assets::AssetEvidence {
                        tool: "dart_filing_search".into(),
                        title: "OpenDART".into(),
                        url: Some(format!(
                            "https://dart.fss.or.kr/dsaf001/main.do?rcpNo={}",
                            release.filing_id
                        )),
                        date: Some(release.date.clone()),
                        ..Default::default()
                    },
                },
                3,
            )?;
        }
        Ok(())
    })?;
    Ok(())
}

/// One local watch check whose starting point lies before `baseline_before`:
/// filings dated on or after it are new alerts.
async fn seed_watch_check(
    state: &AppState,
    loupist_id: &str,
    check: &WatchCheckSeed,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    use crate::knowledge::{assets, watch};
    let before = seed_day(&check.baseline_before)?;
    // Proactive consent: the weekday morning update is on (Auto model).
    state.store.scheduled_task_create_kind(
        loupist_id,
        &format!("briefing-{loupist_id}"),
        crate::scheduled_tasks::types::TaskDraft {
            title: "Briefings".into(),
            prompt: crate::briefings::PROMPT.into(),
            schedule: crate::briefings::default_schedule("Asia/Seoul".into()),
            target: crate::scheduled_tasks::types::TaskTarget::Dedicated,
            model_profile_id: String::new(),
            thinking_level: String::new(),
            run_options: Default::default(),
            use_memory: true,
            update_memory: true,
            status: crate::scheduled_tasks::types::TaskStatus::Active,
        },
        crate::scheduled_tasks::types::TaskKind::Briefing,
        now.timestamp_millis(),
    )?;
    crate::commands::watch::on_consent(state, loupist_id).map_err(|error| error.message)?;
    let pages =
        crate::commands::watch::followed_pages(state, loupist_id).map_err(|error| error.message)?;
    crate::commands::assets::with_assets(state, loupist_id, true, |tx| {
        for page in &pages {
            // A starting point: one earlier filing, the route's baseline done.
            let marker = assets::NewObservation {
                kind: assets::ObservationKind::Filing,
                fact_date: (before - chrono::Duration::days(1)).to_string(),
                dedupe_key: "filing:eval-baseline".into(),
                facts: json!({"title": "Earlier filing (eval starting point)", "filingId": "eval-baseline"}),
                evidence: assets::AssetEvidence {
                    tool: "dart_filing_search".into(),
                    title: "Eval starting point".into(),
                    url: Some("https://fictional.example/eval-baseline".into()),
                    ..Default::default()
                },
            };
            assets::record_observations(tx, loupist_id, &page.asset_id, vec![marker], assets::ObservationOrigin::Refresh, None, 1)?;
            for route in [watch::Route::Filings, watch::Route::Calendar, watch::Route::Dividends] {
                watch::record_route(tx, loupist_id, &page.asset_id, route, "ok", "seed", 1)?;
            }
        }
        Ok(())
    })
    .map_err(|error| error.message)?;
    let runtime = state.scheduled.watch_runtime();
    let alerts = crate::commands::watch::run_pass(
        state,
        &runtime,
        crate::commands::watch::Scope::Loupist(loupist_id.into()),
        now,
    )
    .await
    .map_err(|error| error.message)?;
    eprintln!(
        "watch check found {} alert(s) before the first turn",
        alerts.values().sum::<usize>()
    );
    Ok(())
}

/// The turn's message: a host-built procedure or briefing first, then the user text.
fn turn_prompt(state: &AppState, chat: &ChatSession, turn: &Turn, since_ms: i64) -> Result<String> {
    let mut parts = Vec::new();
    if turn.briefing {
        let (prompt, _) = crate::briefings::build_prompt(
            state,
            &chat.loupist_id,
            Some(since_ms),
            chrono::Utc::now().timestamp_millis(),
        )
        .map_err(|error| error.message)?;
        parts.push(prompt);
    }
    if let Some(procedure) = &turn.earnings_prompt {
        let asset_id = crate::knowledge::assets::asset_id(&procedure.symbol, &procedure.market);
        let event_id = crate::knowledge::earnings::event_id(&asset_id, &procedure.period);
        parts.push(
            crate::commands::earnings::capture_prompt(
                state,
                &chat.loupist_id,
                &asset_id,
                &event_id,
                procedure.side == "after",
            )
            .map_err(|error| error.message)?,
        );
    }
    parts.push(turn.user.clone());
    Ok(parts.join("\n\n"))
}

fn seed(state: &AppState, loupist_id: &str, seed: &Seed) -> Result<()> {
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
    for entry in &seed.watchlist {
        state
            .store
            .watchlist_add(&entry.symbol, &entry.market, entry.display_name.as_deref())?;
    }
    for entry in &seed.journal {
        let name = seed
            .watchlist
            .iter()
            .find(|item| item.symbol == entry.thesis.symbol && item.market == entry.thesis.market)
            .and_then(|item| item.display_name.clone())
            .unwrap_or_else(|| entry.thesis.symbol.clone());
        let identity = crate::knowledge::assets::AssetIdentity {
            symbol: entry.thesis.symbol.clone(),
            market: entry.thesis.market.clone(),
            name,
        };
        let at_ms = entry
            .created_at_ms
            .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
        crate::commands::assets::with_assets(state, loupist_id, true, |tx| {
            let page =
                crate::knowledge::assets::ensure_page(tx, loupist_id, &identity, None, at_ms)?;
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
        crate::sharing::commands::seed_shared_pack(state, loupist_id, &folder)
            .map_err(|error| error.message)?;
    }
    for entry in &seed.earnings {
        seed_earnings(state, loupist_id, &seed.watchlist, entry)?;
    }
    if let Some(csv) = &seed.holdings_csv {
        let preview = state.store.preview_csv_bytes(csv.as_bytes())?;
        if !preview.errors.is_empty() {
            return Err("invalid eval holdings CSV".into());
        }
        state.store.holdings_commit_csv(&preview.preview_id)?;
    }
    if let Some(csv) = &seed.holdings_crypto_csv {
        let preview = state.store.preview_crypto_bytes(csv.as_bytes())?;
        if !preview.errors.is_empty() || preview.trades_only || preview.mapping_headers.is_some() {
            return Err("invalid eval crypto holdings CSV".into());
        }
        state.store.holdings_commit_csv(&preview.preview_id)?;
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
    store.transact(|write| {
        for source in &seed.sources {
            let record = write.create_record(CreateRecord {
                loupist_id: loupist_id.into(),
                kind: RecordKind::Source,
                title: source.title.clone(),
                at_ms: now,
            })?;
            write.retain_source(RetainSource {
                origin: None,
                loupist_id: loupist_id.into(),
                record_id: record.record_id,
                expected_revision_id: None,
                at_ms: now,
                source_id: None,
                title: source.title.clone(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: source.origin.clone(),
                provider_identity: "eval-fixture".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                publication_time: None,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: uuid::Uuid::new_v4().to_string(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: vec![],
                    bytes: source.text.as_bytes().to_vec(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: source.text.len() as u64,
                    },
                    context_note: "Fictional eval source".into(),
                }),
            })?;
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
        if replay::enabled()
            && !matches!(
                entry,
                "opendart.disclosures"
                    | "krx.market-data"
                    | "koreainvestment.market-data"
                    | "world-bank.indicators"
                    | "defillama.tvl"
                    | "community.web-research"
            )
        {
            return Err(format!("connection has no native replay boundary: {id}").into());
        }
        if !catalog
            .entries
            .iter()
            .any(|candidate| candidate.id == entry)
        {
            return Err(format!("unknown connection: {id}").into());
        }
        if entry == "koreainvestment.market-data" {
            crate::scoped_config::write_global_connector_settings(
                &state.artifacts.app_data_dir,
                entry,
                &BTreeMap::from([("environment".into(), "demo".into())]),
            )?;
            crate::marketplace::connector_config::reconcile_connector_configs(state)?;
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
    Ok(())
}
/// Nobody watches an eval reply, so an approval card (a new website after
/// third-party text, a native command or file) is answered "Not now" at once,
/// as a cautious user would, and a question is left unanswered so its call
/// continues with the recommended option, instead of holding the turn for the
/// wait limit.
fn settle_waiting_cards(loupist_id: &str, event: &crate::commands::ChatStreamEvent) {
    use crate::chat::approvals::{decide, ApprovalDecision, ApprovalState};
    use crate::chat::questions::{self, QuestionState};
    use crate::commands::ChatStreamEvent;
    let (approvals, questions) = match event {
        ChatStreamEvent::Progress { progress, .. } => {
            (Some(&progress.approvals), Some(&progress.questions))
        }
        ChatStreamEvent::ProgressPatch { patch, .. } => {
            (patch.approvals.as_ref(), patch.questions.as_ref())
        }
        _ => (None, None),
    };
    for approval in approvals.into_iter().flatten() {
        if approval.state == ApprovalState::Waiting {
            let _ = decide(loupist_id, &approval.request_id, ApprovalDecision::Deny);
        }
    }
    for question in questions.into_iter().flatten() {
        if question.state == QuestionState::Waiting {
            // Dropping the registered sender resolves the waiting call unanswered.
            questions::forget(&question.request_id);
        }
    }
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
    let policy = memory_policy(&session.memory)?;
    let captured = Arc::new(Mutex::new(trace::PiTrace::default()));
    let callback = captured.clone();
    // Every sink clone belongs to this turn's native runtime/broker. Its final
    // drop follows terminal persistence and Memory finalization, unlike the
    // reader lease, which is released before the canonical writer acquires.
    // Waiting for the global activity count would also wait for unrelated Chats.
    let (lifetime, mut settled) = tokio::sync::watch::channel(());
    let loupist_id = chat.loupist_id.clone();
    let mut sink = ChatEventSink::new(move |event| {
        let _ = &lifetime;
        settle_waiting_cards(&loupist_id, &event);
        Ok(())
    });
    sink.eval_pi_event = Some(Arc::new(move |event| {
        callback.lock().unwrap().observe(event)
    }));
    let run_id = format!("eval-{}", uuid::Uuid::new_v4());
    let started = chrono::Utc::now().to_rfc3339();
    let clock = Instant::now();
    let run_options = state
        .model_catalog()?
        .resolve(model)?
        .run_controls
        .into_iter()
        .map(|control| (control.id, control.default_choice))
        .collect();
    let execution = async {
        send_chat(
            ChatSendRequest {
                run_id: run_id.clone(),
                loupist_id: chat.loupist_id.clone(),
                thread_id: chat.id.clone(),
                prompt: turn_prompt(state, chat, turn, chat.created_at_ms - 60_000)
                    .map_err(|error| error.to_string())?,
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
    let result = tokio::time::timeout(Duration::from_millis(timeout_ms), &mut execution).await;
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
    let mut result = if let Some(message) = message {
        let mut result = trace::persisted(message, &captured.lock().unwrap());
        trace::enrich_commits(state, chat, message, &mut result)?;
        result
    } else {
        json!({"answer":"","status":"failed","error":"persisted assistant missing","tool_calls":[],"memory_reads":[],"memory_commits":[],"usage":{"input":null,"output":null,"cache_read":null,"cache_write":null},"performance":{"setup_ms":null,"first_text_ms":null,"total_ms":null}})
    };
    if let Some(error) = failure {
        result["status"] = json!("failed");
        result["error"] = trace::bounded_json(&json!(error));
    }
    // Observe canonical state independently of tool output or the assistant’s claim.
    result["lens"] = trace::lens_state(state, chat, message.map(|m| m.id.as_str()))?;
    result["scheduled_tasks"] = json!(state.store.scheduled_tasks_list(&chat.loupist_id)?.iter().map(|task| {
        trace::bounded_json(&json!({"task_id":task.id,"title":task.title,"status":task.status,"revision":task.revision,
            "schedule":task.schedule,"target":task.target,"session_thread_id":task.session_thread_id,
            "model_profile_id":task.model_profile_id,"thinking_level":task.thinking_level,
            "use_memory":task.use_memory,"update_memory":task.update_memory,
            "next_run_at_ms":task.next_run_at_ms,"awaiting_confirmation":task.awaiting_confirmation,"prompt":task.prompt.chars().take(1500).collect::<String>(),
            "prompt_sha256":crate::util::sha256(task.prompt.as_bytes())}))
    }).collect::<Vec<_>>());
    result["started_at"] = json!(started);
    result["duration_ms"] = json!(clock.elapsed().as_millis() as u64);
    Ok(result)
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
pub async fn run(options: Options) -> Result<()> {
    if !options.pi_agent_dir.is_absolute() || !options.pi_agent_dir.is_dir() {
        return Err("--pi-agent-dir must be an existing absolute directory (used in place)".into());
    }
    let mut summary = batch::Summary::default();
    if options.turn_timeout_ms == 0 {
        return Err("--turn-timeout-ms must be positive".into());
    }
    if options.resume && options.agent_assets.is_some() {
        return Err("--resume cannot be combined with --agent-assets; score each frozen candidate independently".into());
    }
    if options.resume && options.model.is_none() {
        return Err("--resume requires an explicit --model".into());
    }
    if options.trials == 0 {
        return Err("--trials must be positive".into());
    }
    if options.out.exists() && !options.resume {
        return Err("output already exists; choose a new --out path".into());
    }
    let suite: Suite = serde_json::from_slice(&fs::read(&options.suite)?)?;
    if suite.schema != "loupist-agent-suite/1" || suite.version != 1 {
        return Err("unsupported suite schema/version".into());
    }
    if suite
        .budget
        .as_ref()
        .is_some_and(|b| b.max_turns == 0 || b.max_input_tokens_per_turn == 0)
    {
        return Err("suite budget caps must be positive".into());
    }
    for scenario in &suite.scenarios {
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
            }
            if !matches!(session.role.as_str(), "train" | "probe") {
                return Err("session role must be train or probe".into());
            }
        }
    }
    for id in &options.scenarios {
        if !suite.scenarios.iter().any(|s| &s.id == id) {
            return Err(format!("unknown scenario: {id}").into());
        }
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
    let frozen =
        artifacts::FrozenArtifacts::with_override(artifacts()?, options.agent_assets.as_deref())?;
    let pi = frozen.pi.clone();
    let run_id = uuid::Uuid::new_v4().to_string();
    let git = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()?;
    if !git.status.success() {
        return Err("cannot determine harness commit".into());
    }
    let harness = json!({"git_commit":String::from_utf8(git.stdout)?.trim(),"dirty":batch::dirty(Path::new(env!("CARGO_MANIFEST_DIR")))?,"system_prompt_sha256":crate::util::sha256(&fs::read(&pi.system_prompt)?),"tool_catalog_sha256":crate::util::sha256(&fs::read(pi.system_prompt.parent().unwrap().join("tool-catalog.json"))?),"agent_snapshot_sha256":frozen.sha256,"agent_assets_override":options.agent_assets.is_some(),"context_policy":frozen.context_policy});
    let mut bytes = if options.resume && options.out.exists() {
        fs::read(&options.out)?
    } else {
        Vec::new()
    };
    let existing = batch::records(&bytes)?;
    let mut models = None;
    let mut turns_run = 0usize;
    let mut budget_stop = None;
    'scenarios: for (scenario, inputs) in inputs {
        let mut harness = harness.clone();
        harness["scenario_input_sha256"] = json!(inputs.sha256);
        for trial in 0..options.trials {
            if options.resume
                && batch::complete(
                    &existing,
                    &suite.suite,
                    scenario,
                    trial,
                    options.model.as_deref().unwrap(),
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
            let temporary = tempfile::Builder::new()
                .prefix("loupist-eval-")
                .tempdir_in(if cfg!(unix) {
                    PathBuf::from("/tmp")
                } else {
                    std::env::temp_dir()
                })?;
            let workspace = temporary.path().join("workspace");
            fs::create_dir(&workspace)?;
            for (name, contents) in &scenario.workspace_files {
                relative(Path::new(name))?;
                let path = workspace.join(name);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(path, contents)?;
            }
            let mut state = AppState::for_eval(temporary.path().join("app"));
            // Only this disposable app profile is changed; paid eval attempts never retry.
            fs::write(
                state.artifacts.app_data_dir.join("config.toml"),
                frozen.config_toml(),
            )?;
            state.eval_pi_agent_dir = Some(options.pi_agent_dir.clone());
            Arc::make_mut(&mut state.artifacts).pi = Some(pi.clone());
            let provider = options
                .model
                .as_deref()
                .and_then(|value| value.split_once('/').map(|pair| pair.0))
                .unwrap_or("openai-codex");
            if models.is_none() {
                let available = crate::providers::connection::eval_models(&state, provider).await?;
                eprintln!(
                    "discovered models: {}",
                    available
                        .iter()
                        .map(|m| m.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                models = Some(available);
            }
            let discovered = models.as_ref().unwrap();
            let selected = if let Some(id) = &options.model {
                discovered.iter().find(|m| &m.id == id)
            } else {
                discovered
                    .iter()
                    .filter(|m| {
                        m.input.iter().any(|input| input == "text")
                            && m.thinking_levels.contains(&options.thinking)
                    })
                    .max_by_key(|m| &m.id)
            }
            .ok_or("requested model/thinking not found in Pi model discovery")?;
            state.set_model_catalog(crate::providers::catalog::ModelCatalog {
                models: discovered.clone(),
            })?;
            eprintln!(
                "eval model {}/{} thinking={}",
                selected.provider, selected.model, options.thinking
            );
            let replay = if let Some(replay) = &inputs.replay {
                Some(replay.clone().install(&state.credential_environment)?)
            } else {
                None
            };
            let loupist =
                crate::commands::loupist::create_folder_loupist(&state, &workspace).await?;
            seed(&state, &loupist.id, &scenario.seed)?;
            enable_connections(&state, &loupist.id, &scenario.connections)?;
            if scenario.seed.refresh_assets {
                // The host refresh reads through replay like any tool; a source it
                // cannot reach is recorded on the page, never fatal.
                let refreshed = crate::commands::assets::refresh_followed(
                    &state,
                    &loupist.id,
                    crate::commands::assets::BRIEFING_BUDGET,
                    None,
                )
                .await?;
                eprintln!("refreshed {refreshed} asset page(s) before the first turn");
            }
            if let Some(check) = &scenario.seed.watch_check {
                seed_watch_check(&state, &loupist.id, check, chrono::Utc::now()).await?;
            }
            let mut stop_batch = false;
            'sessions: for session in &scenario.sessions {
                memory_policy(&session.memory)?;
                let chat = if session.id == "bot" {
                    crate::chat::home::ensure_home_thread(&state, &loupist.id)
                        .map_err(|error| error.message)?
                } else {
                    new_chat(&state, &loupist.id, &session.id)?
                };
                for (turn_index, turn) in session.turns.iter().enumerate() {
                    if let Some(budget) = &suite.budget {
                        if turns_run >= budget.max_turns {
                            budget_stop = Some(
                                json!({"error":"budget_exceeded","limit":"maxTurns","maximum":budget.max_turns,"observed":turns_run}),
                            );
                            stop_batch = true;
                            break 'sessions;
                        }
                    }
                    let mut record = run_turn(
                        &state,
                        &chat,
                        session,
                        turn,
                        &selected.id,
                        &options.thinking,
                        options.turn_timeout_ms,
                    )
                    .await?;
                    for (key,value) in json!({"schema":"loupist-agent-trace/1","run_id":run_id,"suite":suite.suite,"scenario_id":scenario.id,"session_id":session.id,"session_role":session.role,"turn_index":turn_index,"trial":trial,"model":{"provider":selected.provider,"model":selected.model,"thinking":options.thinking},"harness":harness,"memory_mode":session.memory,"user":turn.user}).as_object().unwrap(){record[key]=value.clone();}
                    record["harness"]["agent_dir_sha256"] = record
                        .as_object_mut()
                        .unwrap()
                        .remove("agent_dir_sha256")
                        .unwrap_or(Value::Null);
                    record["harness"]["dirty"] =
                        json!(batch::dirty(Path::new(env!("CARGO_MANIFEST_DIR")))?);
                    turns_run += 1;
                    if let Some(reason) = suite.budget.as_ref().and_then(|b| b.enforce(&mut record))
                    {
                        budget_stop = Some(reason);
                    }
                    summary.observe(&record);
                    append_atomic(&options.out, &mut bytes, &record)?;
                    stop_batch = budget_stop.is_some() || batch::provider_rejection(&record);
                    let timed_out = record["error"] == "timeout";
                    if stop_batch || timed_out {
                        break 'sessions;
                    }
                }
            }
            state.shutdown().await?;
            drop(state);
            drop(replay);
            if options.keep {
                eprintln!("kept {}", temporary.keep().display());
            }
            if stop_batch {
                if budget_stop.is_none() {
                    eprintln!("Stopping batch: provider authentication/quota rejection; no automatic retry.");
                }
                break 'scenarios;
            }
            // A timeout aborts this Loupist's dependent sessions only. Each next
            // trial starts in a fresh Loupist and must still be measured.
        }
    }
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
