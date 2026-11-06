//! Durable, tool-free method reviews. No automatic retry after a paid attempt.
use super::graph_jobs::{load_settings, GraphProcessingSettings};
use super::lens_page::{Basis, FilingOrigin, LensOp, PageDelta, PageKind, PageTarget, SectionKey};
use super::{KnowledgeError, KnowledgeResult, KnowledgeWrite, WorldSnapshotBody};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const PROMPT: &str = include_str!("../../../agent/prompts/method-learning.md");
const SIX_HOURS: i64 = 6 * 60 * 60 * 1000;
const DAY: i64 = 24 * 60 * 60 * 1000;
const MAX_INPUT: usize = 12_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewQuery {
    pub loupist_id: String,
    pub limit: Option<u32>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub review_id: String,
    pub status: String,
    pub reason: Option<String>,
    pub lens_record_id: Option<String>,
    pub rationale: Option<String>,
    pub evidence_count: u32,
    pub created_at_ms: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningSettings {
    pub learn_methods: bool,
}
pub fn settings(tx: &Transaction<'_>, loupist: &str) -> KnowledgeResult<LearningSettings> {
    super::model::validate_loupist_id(loupist)?;
    let enabled: Option<bool> = tx
        .query_row(
            "SELECT learn_methods FROM knowledge_learning_settings WHERE loupist_id=?1",
            [loupist],
            |r| r.get(0),
        )
        .optional()?;
    Ok(LearningSettings {
        learn_methods: enabled.unwrap_or(true),
    })
}
pub fn save_settings(
    tx: &Transaction<'_>,
    loupist: &str,
    enabled: bool,
) -> KnowledgeResult<LearningSettings> {
    super::model::validate_loupist_id(loupist)?;
    let value = LearningSettings {
        learn_methods: enabled,
    };
    tx.execute("INSERT INTO knowledge_learning_settings(loupist_id,learn_methods) VALUES (?1,?2) ON CONFLICT(loupist_id) DO UPDATE SET learn_methods=excluded.learn_methods",params![loupist,enabled])?;
    Ok(value)
}
pub fn list(tx: &Transaction<'_>, request: &ReviewQuery) -> KnowledgeResult<Vec<Review>> {
    super::model::validate_loupist_id(&request.loupist_id)?;
    let limit = request.limit.unwrap_or(50);
    if !(1..=200).contains(&limit) {
        return Err(KnowledgeError::Invalid(
            "learning review limit must be 1–200",
        ));
    }
    let mut q=tx.prepare("SELECT review_id,status,reason,lens_record_id,rationale,evidence_count,created_at_ms FROM knowledge_learning_reviews WHERE loupist_id=?1 AND status NOT IN ('pending','queued','running') ORDER BY created_at_ms DESC,review_id DESC LIMIT ?2")?;
    let rows = q
        .query_map(params![request.loupist_id, limit], |r| {
            Ok(Review {
                review_id: r.get(0)?,
                status: r.get(1)?,
                reason: r.get(2)?,
                lens_record_id: r.get(3)?,
                rationale: r.get(4)?,
                evidence_count: r.get(5)?,
                created_at_ms: r.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
fn enabled(
    tx: &Transaction<'_>,
    loupist: &str,
    s: &GraphProcessingSettings,
) -> KnowledgeResult<bool> {
    Ok(settings(tx, loupist)?.learn_methods
        && s.save_learning
        && s.enrichment_enabled
        && !s.paused
        && !s.inactive_loupists.iter().any(|g| g == loupist))
}
/// Called within World publication; growth rowids form a durable, same-timestamp-safe watermark.
pub(super) fn enqueue(
    tx: &Transaction<'_>,
    loupist: &str,
    world_job: &str,
    now: i64,
) -> KnowledgeResult<Option<String>> {
    let s = load_settings(tx, loupist)?;
    if !enabled(tx, loupist, &s)? {
        return Ok(None);
    }
    let active: Option<String> = tx.query_row(
        "SELECT status FROM knowledge_learning_reviews WHERE loupist_id=?1 AND status IN ('pending','queued','running')",
        [loupist], |r| r.get(0),
    ).optional()?;
    if active.is_some() {
        return admit_pending(tx, loupist, now);
    }
    let through: i64 = tx.query_row(
        "SELECT COALESCE(MAX(through_event),0) FROM knowledge_learning_reviews WHERE loupist_id=?1",
        [loupist],
        |r| r.get(0),
    )?;
    let (count,end):(i64,i64)=tx.query_row("SELECT COUNT(*),COALESCE(MAX(rowid),0) FROM knowledge_growth_events WHERE loupist_id=?1 AND rowid>?2 AND kind IN ('claim_added','claim_disputed')",params![loupist,through],|r|Ok((r.get(0)?,r.get(1)?)))?;
    if count < 5 {
        return Ok(None);
    }
    let id = super::model::mint_id("lrn");
    tx.execute("INSERT INTO knowledge_learning_reviews(review_id,loupist_id,world_job_id,status,created_at_ms,through_event,settings_json,call_budget) VALUES (?1,?2,?3,'pending',?4,?5,?6,?7)",params![id,loupist,world_job,now,end,serde_json::to_string(&s)?,s.max_llm_calls.min(1)])?;
    admit_pending(tx, loupist, now)
}

fn world_busy(tx: &Transaction<'_>, loupist: &str) -> KnowledgeResult<bool> {
    Ok(tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM knowledge_graph_outbox WHERE loupist_id=?1 AND status IN ('queued','running'))",
        [loupist], |r| r.get(0),
    )?)
}

/// Pending work does not consume the evidence watermark or a rate-limit slot.
/// Admission and the final World commit share the same SQLite transaction.
fn admit_pending(tx: &Transaction<'_>, loupist: &str, now: i64) -> KnowledgeResult<Option<String>> {
    if world_busy(tx, loupist)? {
        // A new batch may arrive after admission but before the inference claim.
        tx.execute("UPDATE knowledge_learning_reviews SET status='pending' WHERE loupist_id=?1 AND status='queued'", [loupist])?;
        return Ok(None);
    }
    let id: Option<String> = tx.query_row(
        "SELECT review_id FROM knowledge_learning_reviews WHERE loupist_id=?1 AND status='pending'",
        [loupist], |r| r.get(0),
    ).optional()?;
    let Some(id) = id else { return Ok(None) };
    if rate_limit_until(tx, loupist)? > now {
        return Ok(None);
    }
    // Include every claim committed while the review waited for the batch.
    tx.execute("UPDATE knowledge_learning_reviews SET status='queued',created_at_ms=?2,through_event=COALESCE((SELECT MAX(rowid) FROM knowledge_growth_events WHERE loupist_id=?3 AND kind IN ('claim_added','claim_disputed')),through_event) WHERE review_id=?1 AND status='pending'", params![id,now,loupist])?;
    Ok(Some(id))
}
fn rate_limit_until(tx: &Transaction<'_>, loupist: &str) -> KnowledgeResult<i64> {
    let mut q = tx.prepare("SELECT COALESCE(started_at_ms,created_at_ms) FROM knowledge_learning_reviews WHERE loupist_id=?1 AND status<>'pending' ORDER BY COALESCE(started_at_ms,created_at_ms) DESC LIMIT 4")?;
    let times = q
        .query_map([loupist], |r| r.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let cooldown = times.first().map_or(0, |at| at.saturating_add(SIX_HOURS));
    let daily = times.get(3).map_or(0, |at| at.saturating_add(DAY));
    Ok(cooldown.max(daily))
}

/// Wake at a pending review's next rate-limit boundary even if no new World
/// publication arrives. Startup reconstructs the same deadline from SQLite.
pub(super) fn next_wake_at(tx: &Transaction<'_>, now: i64) -> KnowledgeResult<Option<i64>> {
    let mut q =
        tx.prepare("SELECT loupist_id FROM knowledge_learning_reviews WHERE status='pending'")?;
    let loupists = q
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut next = None;
    for loupist in loupists {
        let at = rate_limit_until(tx, &loupist)?;
        if at > now {
            next = Some(next.map_or(at, |prior: i64| prior.min(at)));
        }
    }
    Ok(next)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Method {
    record_id: String,
    title: String,
    applicability: String,
    #[serde(skip_serializing, default)]
    digests: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Claim {
    claim_id: String,
    entity_names: Vec<String>,
    claim_text: String,
    source_date: Option<String>,
    #[serde(skip_serializing)]
    entities: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Input {
    loupist_name: String,
    focus: String,
    principles: Vec<String>,
    methods: Vec<Method>,
    claims: Vec<Claim>,
}
impl Input {
    fn text(&self) -> KnowledgeResult<String> {
        Ok(serde_json::to_string(self)?)
    }
}
fn build_input(
    tx: &Transaction<'_>,
    loupist: &str,
    start: i64,
    end: i64,
    name: &str,
    focus: &str,
) -> KnowledgeResult<Input> {
    let controls = super::memory_controls::load(tx, loupist)?;
    let principles = super::principles::principle_list(tx, loupist, false)?
        .into_iter()
        .filter(|p| !controls.paused.contains(&p.record_id))
        .map(|p| p.statement)
        .collect();
    let mut input = Input {
        loupist_name: name.chars().take(200).collect(),
        focus: focus.chars().take(1200).collect(),
        principles,
        methods: vec![],
        claims: vec![],
    };
    // Never silently omit a principle. If all cannot fit, defer without inference.
    if input.text()?.chars().count() > MAX_INPUT {
        return Err(KnowledgeError::Invalid("principles_exceed_input_limit"));
    }
    let mut q=tx.prepare("SELECT m.record_id,m.title,h.revision_id,r.payload_json FROM knowledge_lens_metadata m JOIN knowledge_record_heads h ON h.record_id=m.record_id JOIN knowledge_record_revisions r ON r.revision_id=h.revision_id WHERE m.loupist_id=?1 AND m.archived_at_ms IS NULL AND m.kind='method' ORDER BY m.updated_at_ms DESC,m.record_id LIMIT 40")?;
    for row in q.query_map([loupist], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })? {
        let (record_id, title, _revision_id, payload) = row?;
        if controls.paused.contains(&record_id) || controls.removed.contains(&record_id) {
            continue;
        }
        let page: super::lens_page::Page = serde_json::from_str(&payload)?;
        let applicability = page
            .sections
            .iter()
            .find(|section| section.key.as_str() == "use-when")
            .map(|section| section.markdown.as_str())
            .unwrap_or(&page.summary);
        input.methods.push(Method {
            record_id,
            title,
            applicability: applicability.chars().take(300).collect(),
            digests: page
                .sections
                .iter()
                .map(|section| {
                    (
                        section.key.as_str().to_owned(),
                        super::lens_page::section_digest(section),
                    )
                })
                .collect(),
        });
        if input.text()?.chars().count() > 7000 {
            input.methods.pop();
            break;
        }
    }
    let body:Option<String>=tx.query_row("SELECT s.snapshot_json FROM knowledge_world_snapshots s JOIN knowledge_record_heads h ON h.revision_id=s.revision_id JOIN knowledge_records r ON r.record_id=h.record_id WHERE r.owner_loupist_id=?1 AND r.kind='world'",[loupist],|r|r.get(0)).optional()?;
    if let Some(body) = body {
        let body: WorldSnapshotBody = serde_json::from_str(&body)?;
        let claims: BTreeMap<_, _> = body.claims.iter().map(|c| (c.id.as_str(), c)).collect();
        let names: BTreeMap<_, _> = body
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.name.as_str()))
            .collect();
        let mut q=tx.prepare("SELECT subject_id FROM knowledge_growth_events WHERE loupist_id=?1 AND rowid>?2 AND rowid<=?3 AND kind IN ('claim_added','claim_disputed') GROUP BY subject_id ORDER BY MAX(at_ms) DESC,MAX(rowid) DESC LIMIT 60")?;
        for id in q.query_map(params![loupist, start, end], |r| r.get::<_, String>(0))? {
            let id = id?;
            let Some(c) = claims.get(id.as_str()) else {
                continue;
            };
            if c.status == super::ClaimStatus::Withdrawn {
                continue;
            }
            let entities: Vec<_> = c
                .endpoints
                .iter()
                .map(|e| e.node_id.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let entity_names = entities
                .iter()
                .filter_map(|id| names.get(id.as_str()).map(|n| (*n).to_owned()))
                .collect();
            input.claims.push(Claim {
                claim_id: id,
                entity_names,
                claim_text: c.proposition.clone(),
                source_date: c.publication_time.as_ref().and_then(|t| t.value.clone()),
                entities,
            });
            if input.text()?.chars().count() > MAX_INPUT {
                input.claims.pop();
                break;
            }
        }
    }
    Ok(input)
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Output {
    action: String,
    #[serde(default)]
    target_record_id: Option<String>,
    #[serde(default)]
    title: String,
    #[serde(default)]
    applicability: String,
    #[serde(default)]
    assumptions: String,
    #[serde(default)]
    procedure: String,
    #[serde(default)]
    principles: String,
    #[serde(default)]
    limitations: String,
    #[serde(default)]
    markdown: String,
    evidence_claim_ids: Vec<String>,
    rationale: String,
}
impl Output {
    fn texts(&self) -> [&str; 6] {
        [
            &self.markdown,
            &self.applicability,
            &self.assumptions,
            &self.procedure,
            &self.principles,
            &self.limitations,
        ]
    }

    fn ops(&self, read: &BTreeMap<String, String>) -> KnowledgeResult<Vec<LensOp>> {
        let section = |key: &str, heading: &str, text: String| -> KnowledgeResult<LensOp> {
            let key = SectionKey::new(key)?;
            let expected = read.get(key.as_str()).cloned();
            Ok(LensOp::Put {
                key,
                heading: heading.into(),
                markdown: text
                    .chars()
                    .take(super::lens_page::MAX_SECTION_CHARS)
                    .collect(),
                basis: Basis::default(),
                expected,
            })
        };
        let summary: String = self
            .markdown
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(super::lens_page::MAX_SUMMARY_CHARS)
            .collect();
        Ok(vec![
            LensOp::Summary { text: summary },
            section(
                "use-when",
                "Use when",
                format!(
                    "{}\n\nAssumptions: {}",
                    self.applicability.trim(),
                    self.assumptions.trim()
                ),
            )?,
            section("steps", "Steps", self.procedure.trim().to_owned())?,
            section("checks", "Checks", self.principles.trim().to_owned())?,
            section("limits", "Limits", self.limitations.trim().to_owned())?,
        ])
    }
}
#[derive(Clone)]
pub(super) struct Attempt {
    pub review_id: String,
    pub loupist_id: String,
    pub world_job_id: String,
    pub input: Input,
    pub settings: GraphProcessingSettings,
}
pub(super) fn claim(
    tx: &Transaction<'_>,
    loupist: &str,
    name: &str,
    focus: &str,
    now: i64,
) -> KnowledgeResult<Option<Attempt>> {
    let current = load_settings(tx, loupist)?;
    if !enabled(tx, loupist, &current)? {
        return Ok(None);
    }
    admit_pending(tx, loupist, now)?;
    if world_busy(tx, loupist)? {
        return Ok(None);
    }
    let row:Option<(String,String,i64,String,u32)>=tx.query_row("SELECT review_id,world_job_id,through_event,settings_json,call_budget FROM knowledge_learning_reviews WHERE loupist_id=?1 AND status='queued' ORDER BY created_at_ms LIMIT 1",[loupist],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    let Some((id, world_job_id, end, frozen, budget)) = row else {
        return Ok(None);
    };
    // Frozen zero budgets wait just like World chunks; settings alone do not authorize a paid retry.
    if budget == 0 || current.max_llm_calls == 0 {
        return Ok(None);
    }
    let mut s: GraphProcessingSettings = serde_json::from_str(&frozen)?;
    // Like untouched World chunks, use the currently selected model while preserving the frozen call budget.
    s.helper_provider = current.helper_provider.clone();
    s.helper_model = current.helper_model.clone();
    s.thinking_level = current.thinking_level.clone();
    s.run_options = current.run_options.clone();
    s.timeout_ms = current.timeout_ms;
    s.max_llm_calls = current.max_llm_calls;
    let previous:Option<i64>=tx.query_row("SELECT MAX(started_at_ms) FROM knowledge_learning_reviews WHERE loupist_id=?1 AND review_id<>?2",params![loupist,id],|r|r.get(0))?;
    if previous.is_some_and(|t| now - t < SIX_HOURS) {
        return Ok(None);
    }
    let start:i64=tx.query_row("SELECT COALESCE(MAX(through_event),0) FROM knowledge_learning_reviews WHERE loupist_id=?1 AND review_id<>?2",params![loupist,id],|r|r.get(0))?;
    let input = match build_input(tx, loupist, start, end, name, focus) {
        Ok(i) => i,
        Err(KnowledgeError::Invalid(reason)) => {
            settle(tx, &id, "deferred", Some(reason), None, None, 0, now)?;
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    if input.claims.len() < 2 {
        settle(
            tx,
            &id,
            "deferred",
            Some("insufficient_claims"),
            None,
            None,
            0,
            now,
        )?;
        return Ok(None);
    }
    tx.execute("UPDATE knowledge_learning_reviews SET status='running',started_at_ms=?2,input_json=?3,settings_json=?4 WHERE review_id=?1 AND status='queued'",params![id,now,input.text()?,serde_json::to_string(&s)?])?;
    Ok(Some(Attempt {
        review_id: id,
        loupist_id: loupist.into(),
        world_job_id,
        input,
        settings: s,
    }))
}
#[allow(clippy::too_many_arguments)]
fn settle(
    tx: &Transaction<'_>,
    id: &str,
    status: &str,
    reason: Option<&str>,
    lens: Option<&str>,
    rationale: Option<&str>,
    count: usize,
    now: i64,
) -> KnowledgeResult<()> {
    tx.execute("UPDATE knowledge_learning_reviews SET status=?2,reason=?3,lens_record_id=?4,rationale=?5,evidence_count=?6,finished_at_ms=?7 WHERE review_id=?1",params![id,status,reason,lens,rationale,count as i64,now])?;
    Ok(())
}
pub(super) fn recover(tx: &Transaction<'_>) -> KnowledgeResult<()> {
    tx.execute("UPDATE knowledge_learning_reviews SET status='failed',reason='uncertain_paid_attempt',finished_at_ms=started_at_ms WHERE status='running'",[])?;
    Ok(())
}
pub(super) fn request(attempt: &Attempt) -> KnowledgeResult<Value> {
    Ok(
        json!({"kind":"llm","scope":{"loupist_id":attempt.loupist_id},"messages":[{"role":"system","content":PROMPT},{"role":"user","content":attempt.input.text()?}],"response_schema":{"type":"object"},"max_tokens":4096,"timeout_ms":attempt.settings.timeout_ms}),
    )
}
pub(super) fn finish(
    write: &KnowledgeWrite<'_>,
    a: &Attempt,
    reply: &super::graph_inference::InferenceReply,
    now: i64,
) -> KnowledgeResult<()> {
    use super::graph_inference::InferenceDisposition;
    let tx = write.tx;
    let running:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_learning_reviews WHERE review_id=?1 AND loupist_id=?2 AND status='running')",params![a.review_id,a.loupist_id],|r|r.get(0))?;
    if !running {
        return Err(KnowledgeError::Invalid(
            "learning attempt is no longer active",
        ));
    }
    tx.execute(
        "UPDATE knowledge_learning_reviews SET usage_json=?2,output_json=?3 WHERE review_id=?1",
        params![
            a.review_id,
            reply.body.get("usage").map(Value::to_string),
            reply.body.get("json_object").map(Value::to_string)
        ],
    )?;
    if reply.disposition != InferenceDisposition::Completed {
        return settle(
            tx,
            &a.review_id,
            "failed",
            Some(if reply.disposition == InferenceDisposition::Uncertain {
                "uncertain_paid_attempt"
            } else {
                "not_sent"
            }),
            None,
            None,
            0,
            now,
        );
    }
    if !enabled(tx, &a.loupist_id, &load_settings(tx, &a.loupist_id)?)? {
        return settle(
            tx,
            &a.review_id,
            "deferred",
            Some("learning_disabled"),
            None,
            None,
            0,
            now,
        );
    }
    let parsed = serde_json::from_value::<Output>(
        reply
            .body
            .get("json_object")
            .cloned()
            .unwrap_or(Value::Null),
    );
    // A savepoint ensures all validation and publication failures leave no partial Lens writes.
    tx.execute_batch("SAVEPOINT learning_publication")?;
    let result = parsed
        .map_err(|_| KnowledgeError::Invalid("invalid_output"))
        .and_then(|o| publish(write, a, o, now));
    match result {
        Ok(()) => tx.execute_batch("RELEASE learning_publication")?,
        Err(_) => {
            tx.execute_batch("ROLLBACK TO learning_publication; RELEASE learning_publication")?;
            settle(
                tx,
                &a.review_id,
                "deferred",
                Some("invalid_output"),
                None,
                None,
                0,
                now,
            )?;
        }
    }
    Ok(())
}
fn publish(write: &KnowledgeWrite<'_>, a: &Attempt, o: Output, now: i64) -> KnowledgeResult<()> {
    let invalid = || KnowledgeError::Invalid("invalid_output");
    if o.rationale.trim().is_empty() || o.rationale.chars().count() > 400 {
        return Err(invalid());
    }
    if o.action == "defer" {
        return settle(
            write.tx,
            &a.review_id,
            "deferred",
            Some("no_reusable_pattern"),
            None,
            Some(&o.rationale),
            0,
            now,
        );
    }
    if !matches!(o.action.as_str(), "create" | "refine")
        || o.title.trim().is_empty()
        || o.title.chars().count() > 80
    {
        return Err(invalid());
    }
    let ids: BTreeSet<_> = o.evidence_claim_ids.iter().collect();
    if ids.len() < 2
        || ids
            .iter()
            .any(|id| !a.input.claims.iter().any(|c| &c.claim_id == *id))
    {
        return Err(invalid());
    }
    if o.texts().iter().any(|s| s.trim().is_empty()) {
        return Err(invalid());
    }
    // A newly added/edited principle invalidates this bounded review's authority.
    let controls = super::memory_controls::load(write.tx, &a.loupist_id)?;
    let current: Vec<_> = super::principles::principle_list(write.tx, &a.loupist_id, false)?
        .into_iter()
        .filter(|p| !controls.paused.contains(&p.record_id))
        .map(|p| p.statement)
        .collect();
    if current != a.input.principles {
        return settle(
            write.tx,
            &a.review_id,
            "deferred",
            Some("principles_changed"),
            None,
            Some(&o.rationale),
            ids.len(),
            now,
        );
    }
    let (target, head) = if o.action == "refine" {
        let method = a
            .input
            .methods
            .iter()
            .find(|m| Some(&m.record_id) == o.target_record_id.as_ref())
            .ok_or_else(invalid)?;
        match write.read_page(&a.loupist_id, &method.record_id) {
            Ok(page) if !page.archived && page.kind == super::lens_page::LensKind::Method => {}
            _ => return Err(invalid()),
        }
        (
            PageTarget::Existing {
                record_id: method.record_id.clone(),
            },
            method.digests.clone(),
        )
    } else {
        if o.target_record_id.is_some() {
            return Err(invalid());
        }
        (
            PageTarget::Create {
                kind: PageKind::Method,
                title: o.title.clone(),
                anchors: Vec::new(),
            },
            BTreeMap::new(),
        )
    };
    let filed = super::lens::file(
        write.tx,
        &a.loupist_id,
        &FilingOrigin::Learning {
            review_id: a.review_id.clone(),
            world_job_id: a.world_job_id.clone(),
        },
        vec![PageDelta {
            target,
            ops: o.ops(&head)?,
        }],
        now,
    )?;
    let page = filed.into_iter().next().ok_or_else(invalid)?;
    let stale = page
        .outcomes
        .iter()
        .any(|op| matches!(op.outcome, super::lens_page::OpOutcome::SkippedStale { .. }));
    if !page.changed {
        return settle(
            write.tx,
            &a.review_id,
            "deferred",
            Some("method_unchanged"),
            None,
            Some(&o.rationale),
            ids.len(),
            now,
        );
    }
    let record_id = page.record_id;
    let mut entities = BTreeMap::<String, usize>::new();
    for c in a.input.claims.iter().filter(|c| ids.contains(&c.claim_id)) {
        for e in &c.entities {
            *entities.entry(e.clone()).or_default() += 1;
        }
    }
    let mut entities: Vec<_> = entities.into_iter().collect();
    entities.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let applied: BTreeSet<String> = entities.into_iter().take(30).map(|(e, _)| e).collect();
    super::lens::record_applied(
        write.tx,
        &a.loupist_id,
        &a.review_id,
        &record_id,
        &applied,
        now,
    )?;
    settle(
        write.tx,
        &a.review_id,
        if page.created { "created" } else { "refined" },
        stale.then_some("stale_head_folded"),
        Some(&record_id),
        Some(&o.rationale),
        ids.len(),
        now,
    )
}

#[cfg(test)]
mod tests;
