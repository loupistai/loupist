//! Durable, tool-free method reviews. No automatic retry after a paid attempt.
use super::graph_jobs::{load_settings, GraphProcessingSettings};
use super::lens_page::{
    Basis, BasisRef, FilingOrigin, LensBody, LensKind, LensOp, PageDelta, PageKind, PageTarget,
    SectionKey,
};
use super::{KnowledgeError, KnowledgeResult, KnowledgeWrite};
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
/// Called at every turn commit and after every settle; lens-log rowids form a
/// durable, same-timestamp-safe watermark (`through_event`).
pub(crate) fn enqueue(
    tx: &Transaction<'_>,
    loupist: &str,
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
    let (through, seen) = watermarks(tx, loupist, "")?;
    let end = log_head(tx, loupist)?;
    // A review settled before inference read up to `seen` without consuming it:
    // recheck only once more is filed, so a defer never requeues itself.
    if end <= seen || core(&evidence(tx, loupist, through, end)?).is_none() {
        return Ok(None);
    }
    let id = super::model::mint_id("lrn");
    // world_job_id is kept, written empty, so existing folders open without a schema bump.
    tx.execute("INSERT INTO knowledge_learning_reviews(review_id,loupist_id,world_job_id,status,created_at_ms,through_event,settings_json,call_budget) VALUES (?1,?2,'','pending',?3,?4,?5,?6)",params![id,loupist,now,end,serde_json::to_string(&s)?,s.max_llm_calls.min(1)])?;
    admit_pending(tx, loupist, now)
}

/// The evidence watermark (the head an inference attempt consumed) and the
/// head any review last read. Rows with a `world_job_id` held growth-event
/// rowids under the claim-count trigger and never count.
fn watermarks(tx: &Transaction<'_>, loupist: &str, except: &str) -> KnowledgeResult<(i64, i64)> {
    Ok(tx.query_row(
        "SELECT COALESCE(MAX(CASE WHEN started_at_ms IS NOT NULL THEN through_event END),0),COALESCE(MAX(through_event),0)
         FROM knowledge_learning_reviews WHERE loupist_id=?1 AND world_job_id='' AND review_id<>?2",
        params![loupist, except],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

fn log_head(tx: &Transaction<'_>, loupist: &str) -> KnowledgeResult<i64> {
    Ok(tx.query_row(
        "SELECT COALESCE(MAX(rowid),0) FROM knowledge_lens_log WHERE loupist_id=?1",
        [loupist],
        |r| r.get(0),
    )?)
}

/// Pending work, and work settled before inference, consumes neither the
/// evidence watermark nor a rate-limit slot.
fn admit_pending(tx: &Transaction<'_>, loupist: &str, now: i64) -> KnowledgeResult<Option<String>> {
    let id: Option<String> = tx.query_row(
        "SELECT review_id FROM knowledge_learning_reviews WHERE loupist_id=?1 AND status='pending'",
        [loupist], |r| r.get(0),
    ).optional()?;
    let Some(id) = id else { return Ok(None) };
    if rate_limit_until(tx, loupist)? > now {
        return Ok(None);
    }
    // Include every filing committed while the review waited.
    tx.execute("UPDATE knowledge_learning_reviews SET status='queued',created_at_ms=?2,through_event=?3 WHERE review_id=?1 AND status='pending'", params![id,now,log_head(tx, loupist)?])?;
    Ok(Some(id))
}
fn rate_limit_until(tx: &Transaction<'_>, loupist: &str) -> KnowledgeResult<i64> {
    let mut q = tx.prepare("SELECT started_at_ms FROM knowledge_learning_reviews WHERE loupist_id=?1 AND started_at_ms IS NOT NULL ORDER BY started_at_ms DESC LIMIT 4")?;
    let times = q
        .query_map([loupist], |r| r.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let cooldown = times.first().map_or(0, |at| at.saturating_add(SIX_HOURS));
    let daily = times.get(3).map_or(0, |at| at.saturating_add(DAY));
    Ok(cooldown.max(daily))
}

/// Wake at a pending review's next rate-limit boundary even if no later turn
/// is filed. Startup reconstructs the same deadline from SQLite.
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Method {
    record_id: String,
    title: String,
    applicability: String,
    #[serde(skip)]
    digests: BTreeMap<String, String>,
    /// Sections the user wrote; a refine never replaces them.
    #[serde(skip)]
    user_written: BTreeSet<String>,
}
/// A retained Source span a turn filed as a section's basis.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SpanRef {
    record_id: String,
    revision_digest: String,
    span_id: String,
}
/// One research section a turn filed, citing at least one preserved original.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Observation {
    id: String,
    turn: String,
    chat: String,
    page: String,
    kind: String,
    heading: String,
    text: String,
    sources: Vec<String>,
    #[serde(skip)]
    chat_id: String,
    #[serde(skip)]
    message_id: String,
    #[serde(skip)]
    page_id: String,
    #[serde(skip)]
    spans: Vec<SpanRef>,
}
/// Each Chat's cited Sources across all the evidence.
fn cited_by(all: &[Observation]) -> BTreeMap<&str, BTreeSet<&str>> {
    let mut chats: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for o in all {
        let sources = chats.entry(&o.chat_id).or_default();
        sources.extend(o.spans.iter().map(|s| s.record_id.as_str()));
    }
    chats
}
/// Two Chats are independent cases when each cites a Source the other never
/// cited. A Chat that only repeats another's Sources adds no new case.
fn independent(a: &Observation, b: &Observation, by: &BTreeMap<&str, BTreeSet<&str>>) -> bool {
    let new_to = |o: &Observation, other: &Observation| {
        let seen = by.get(other.chat_id.as_str());
        o.spans
            .iter()
            .any(|s| seen.is_none_or(|seen| !seen.contains(s.record_id.as_str())))
    };
    a.chat_id != b.chat_id && new_to(a, b) && new_to(b, a)
}
/// The method-learning gate on `cited`, judged against what each Chat cited in
/// `all`: two independent Chats and two pages. One Chat, one Source, one
/// company's page, or a Chat nested in another's Sources is too narrow to
/// generalize.
fn recurs(cited: &[&Observation], all: &[Observation]) -> bool {
    let by = cited_by(all);
    let pages: BTreeSet<_> = cited.iter().map(|o| &o.page_id).collect();
    pages.len() >= 2
        && cited
            .iter()
            .enumerate()
            .any(|(j, b)| cited[..j].iter().any(|a| independent(a, b, &by)))
}
/// The newest observations that pass the gate on their own: an independent
/// pair, plus another page when the pair shares one. None when `all` fails it.
fn core(all: &[Observation]) -> Option<BTreeSet<usize>> {
    let by = cited_by(all);
    // Independence depends only on a Chat and its Sources: try the newest of each.
    let mut kinds = BTreeSet::new();
    let firsts: Vec<usize> = (0..all.len())
        .filter(|&i| {
            let sources: BTreeSet<_> = all[i].spans.iter().map(|s| &s.record_id).collect();
            kinds.insert((&all[i].chat_id, sources))
        })
        .collect();
    let (i, j) = (0..firsts.len())
        .flat_map(|j| (0..j).map(move |i| (i, j)))
        .map(|(i, j)| (firsts[i], firsts[j]))
        .find(|&(i, j)| independent(&all[i], &all[j], &by))?;
    let mut picked = BTreeSet::from([i, j]);
    if all[i].page_id == all[j].page_id {
        picked.insert(all.iter().position(|o| o.page_id != all[i].page_id)?);
    }
    Some(picked)
}
/// Turn puts in (after, through], newest first, that are still the current
/// section of an active subject or idea page, keeping only spans whose Source
/// preserves an original. An undone, forgotten, retired or replaced put no
/// longer counts, and method pages never do: a turn's own method edits are not
/// research evidence.
fn evidence(
    tx: &Transaction<'_>,
    loupist: &str,
    after: i64,
    through: i64,
) -> KnowledgeResult<Vec<Observation>> {
    let controls = super::memory_controls::load(tx, loupist)?;
    let hidden = |id: &str| controls.paused.contains(id) || controls.removed.contains(id);
    let mut original = tx.prepare(
        "SELECT s.title FROM knowledge_source_spans sp
         JOIN knowledge_source_representations r ON r.record_id=sp.record_id AND r.representation_id=sp.representation_id
         JOIN knowledge_source_captures c ON c.record_id=sp.record_id AND c.capture_id=sp.capture_id
         JOIN knowledge_sources s ON s.record_id=c.record_id AND s.source_id=c.source_id
         WHERE sp.record_id=?1 AND sp.span_id=?2
           AND r.role IN ('original','provider_payload','extracted_text','structured','excerpt')
           AND c.coverage LIKE 'included_%' AND c.authorship IN ('publisher','user','tool')
           AND s.provider_identity NOT LIKE 'shared_pack:%'",
    )?;
    let mut head = tx.prepare(
        "SELECT r.payload_json FROM knowledge_record_heads h
         JOIN knowledge_record_revisions r ON r.revision_id=h.revision_id WHERE h.record_id=?1",
    )?;
    let mut q = tx.prepare(
        "SELECT l.op_id,l.op_json,l.chat_id,l.message_id,l.record_id,l.after_digest,m.title,m.kind
         FROM knowledge_lens_log l JOIN knowledge_lens_metadata m ON m.record_id=l.record_id
         WHERE l.loupist_id=?1 AND l.rowid>?2 AND l.rowid<=?3 AND l.origin='turn'
           AND l.outcome IN ('added','replaced') AND l.chat_id IS NOT NULL AND l.message_id IS NOT NULL
           AND m.kind IN ('subject','idea') AND m.archived_at_ms IS NULL
         ORDER BY l.rowid DESC",
    )?;
    let rows = q
        .query_map(params![loupist, after, through], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let (mut turns, mut chats) = (BTreeMap::new(), BTreeMap::new());
    let mut bodies: BTreeMap<String, Option<LensBody>> = BTreeMap::new();
    let mut out = Vec::new();
    for (id, op, chat_id, message_id, page_id, after_digest, page, kind) in rows {
        if hidden(&page_id) {
            continue;
        }
        let LensOp::Put {
            key,
            heading,
            markdown,
            basis,
            ..
        } = serde_json::from_str(&op)?
        else {
            continue;
        };
        if !bodies.contains_key(&page_id) {
            let payload: Option<String> = head.query_row([&page_id], |r| r.get(0)).optional()?;
            let body = match (payload, LensKind::parse(&kind)) {
                (Some(payload), Some(kind)) => Some(LensBody::from_payload(
                    kind,
                    serde_json::from_str(&payload)?,
                )?),
                _ => None,
            };
            bodies.insert(page_id.clone(), body);
        }
        let current = bodies[&page_id]
            .as_ref()
            .and_then(LensBody::page)
            .and_then(|p| p.section(&key))
            .map(super::lens_page::section_digest);
        if after_digest.is_none() || current != after_digest {
            continue;
        }
        let (mut spans, mut sources) = (Vec::new(), Vec::new());
        for r in basis.refs {
            let BasisRef::Span {
                record_id,
                revision_digest,
                span_id,
            } = r
            else {
                continue;
            };
            if hidden(&record_id) {
                continue;
            }
            let Some(title) = original
                .query_row(params![record_id, span_id], |r| r.get::<_, String>(0))
                .optional()?
            else {
                continue;
            };
            if !sources.contains(&title) {
                sources.push(title);
            }
            spans.push(SpanRef {
                record_id,
                revision_digest,
                span_id,
            });
        }
        if spans.is_empty() {
            continue;
        }
        let n = turns.len() + 1;
        let turn = turns
            .entry(message_id.clone())
            .or_insert_with(|| format!("t{n}"));
        let n = chats.len() + 1;
        let chat = chats
            .entry(chat_id.clone())
            .or_insert_with(|| format!("c{n}"));
        out.push(Observation {
            id,
            turn: turn.clone(),
            chat: chat.clone(),
            page,
            kind,
            heading,
            text: markdown.trim().chars().take(600).collect(),
            sources,
            chat_id,
            message_id,
            page_id,
            spans,
        });
    }
    Ok(out)
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Input {
    loupist_name: String,
    focus: String,
    principles: Vec<String>,
    methods: Vec<Method>,
    observations: Vec<Observation>,
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
        observations: vec![],
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
            user_written: page
                .sections
                .iter()
                .filter(|section| section.origin == super::lens_page::Origin::User)
                .map(|section| section.key.as_str().to_owned())
                .collect(),
        });
        if input.text()?.chars().count() > 7000 {
            input.methods.pop();
            break;
        }
    }
    // A core that passes the gate goes in first, so truncation never shows the
    // model less breadth than the evidence has. The rest fills newest first.
    let all = evidence(tx, loupist, start, end)?;
    let mut picked = core(&all).ok_or(KnowledgeError::Invalid("insufficient_turns"))?;
    input.observations = picked.iter().map(|&i| all[i].clone()).collect();
    while input.text()?.chars().count() > MAX_INPUT && input.methods.pop().is_some() {}
    if input.text()?.chars().count() > MAX_INPUT {
        return Err(KnowledgeError::Invalid("evidence_exceeds_input_limit"));
    }
    for (i, o) in all.iter().enumerate() {
        if picked.contains(&i) {
            continue;
        }
        input.observations.push(o.clone());
        if input.text()?.chars().count() > MAX_INPUT {
            break;
        }
        picked.insert(i);
    }
    input.observations = picked.into_iter().map(|i| all[i].clone()).collect();
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
    evidence_ids: Vec<String>,
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

    /// The summary and every section not in `keep`, each expecting the digest the review read.
    fn ops(
        &self,
        read: &BTreeMap<String, String>,
        keep: &BTreeSet<String>,
        basis: &Basis,
    ) -> KnowledgeResult<Vec<LensOp>> {
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
                basis: basis.clone(),
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
        let sections = [
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
        ];
        Ok(std::iter::once(LensOp::Summary { text: summary })
            .chain(
                sections
                    .into_iter()
                    .filter(|op| op.key().is_some_and(|key| !keep.contains(key.as_str()))),
            )
            .collect())
    }
}
#[derive(Clone)]
pub(super) struct Attempt {
    pub review_id: String,
    pub loupist_id: String,
    pub input: Input,
    pub settings: GraphProcessingSettings,
    /// The lens-log rows (after, through] this review read.
    after: i64,
    through: i64,
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
    let row:Option<(String,i64,String,u32)>=tx.query_row("SELECT review_id,through_event,settings_json,call_budget FROM knowledge_learning_reviews WHERE loupist_id=?1 AND status='queued' ORDER BY created_at_ms LIMIT 1",[loupist],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    let Some((id, end, frozen, budget)) = row else {
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
    let (start, _) = watermarks(tx, loupist, &id)?;
    let input = match build_input(tx, loupist, start, end, name, focus) {
        Ok(i) => i,
        Err(KnowledgeError::Invalid(reason)) => {
            settle(tx, &id, "deferred", Some(reason), None, None, 0, now)?;
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    tx.execute("UPDATE knowledge_learning_reviews SET status='running',started_at_ms=?2,input_json=?3,settings_json=?4 WHERE review_id=?1 AND status='queued'",params![id,now,input.text()?,serde_json::to_string(&s)?])?;
    Ok(Some(Attempt {
        review_id: id,
        loupist_id: loupist.into(),
        input,
        settings: s,
        after: start,
        through: end,
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
    let loupist: String = tx.query_row("UPDATE knowledge_learning_reviews SET status=?2,reason=?3,lens_record_id=?4,rationale=?5,evidence_count=?6,finished_at_ms=?7 WHERE review_id=?1 RETURNING loupist_id",params![id,status,reason,lens,rationale,count as i64,now],|r|r.get(0))?;
    // Filings committed after this review's watermark start the next one without another turn.
    enqueue(tx, &loupist, now)?;
    Ok(())
}
/// Startup. An attempt left running is failed and never retried. A review
/// queued by the claim-count trigger (non-empty `world_job_id`, a growth-event
/// watermark) is settled. Each affected Loupist then rechecks evidence filed
/// past its watermark, so nothing filed during the lost attempt is stranded.
pub(super) fn recover(tx: &Transaction<'_>, now: i64) -> KnowledgeResult<()> {
    let mut affected = BTreeSet::new();
    let mut settle_all = |sql: &str, args: &[&dyn rusqlite::ToSql]| -> KnowledgeResult<()> {
        let mut q = tx.prepare(sql)?;
        for loupist in q.query_map(args, |r| r.get::<_, String>(0))? {
            affected.insert(loupist?);
        }
        Ok(())
    };
    settle_all("UPDATE knowledge_learning_reviews SET status='failed',reason='uncertain_paid_attempt',finished_at_ms=started_at_ms WHERE status='running' RETURNING loupist_id", &[])?;
    settle_all("UPDATE knowledge_learning_reviews SET status='deferred',reason='trigger_changed',finished_at_ms=?1 WHERE status IN ('pending','queued') AND world_job_id<>'' RETURNING loupist_id", &[&now])?;
    for loupist in affected {
        enqueue(tx, &loupist, now)?;
    }
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
    let ids: BTreeSet<_> = o.evidence_ids.iter().collect();
    let shown = a
        .input
        .observations
        .iter()
        .filter(|c| ids.contains(&c.id))
        .count();
    // Re-resolve against current evidence: an observation undone, forgotten or
    // replaced since claim, or whose Sources were removed, drops out.
    let current = evidence(write.tx, &a.loupist_id, a.after, a.through)?;
    let cited: Vec<_> = current.iter().filter(|c| ids.contains(&c.id)).collect();
    if shown != ids.len() || !recurs(&cited, &current) {
        return Err(invalid());
    }
    let turns = cited
        .iter()
        .map(|c| &c.message_id)
        .collect::<BTreeSet<_>>()
        .len();
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
            turns,
            now,
        );
    }
    let (target, head, keep) = if o.action == "refine" {
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
            method.user_written.clone(),
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
            BTreeSet::new(),
        )
    };
    let ops = o.ops(&head, &keep, &span_basis(&cited))?;
    if !ops.iter().any(|op| matches!(op, LensOp::Put { .. })) {
        return settle(
            write.tx,
            &a.review_id,
            "deferred",
            Some("method_unchanged"),
            None,
            Some(&o.rationale),
            turns,
            now,
        );
    }
    let filed = super::lens::file(
        write.tx,
        &a.loupist_id,
        &FilingOrigin::Learning {
            review_id: a.review_id.clone(),
        },
        vec![PageDelta { target, ops }],
        now,
    )?;
    let page = filed.into_iter().next().ok_or_else(invalid)?;
    if page.refused_text() {
        return Err(invalid());
    }
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
            turns,
            now,
        );
    }
    let record_id = page.record_id;
    let mut entities = BTreeMap::<String, usize>::new();
    let pages: BTreeSet<_> = cited.iter().map(|c| &c.page_id).collect();
    let mut subjects = write.tx.prepare("SELECT node_id FROM knowledge_lens_anchors WHERE record_id=?1 AND role='subject' AND node_id IS NOT NULL")?;
    for page in pages {
        for node in subjects.query_map([page], |r| r.get::<_, String>(0))? {
            *entities.entry(node?).or_default() += 1;
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
        turns,
        now,
    )
}

/// The cited spans, one per Source first, so a capped basis still shows every Source it can.
fn span_basis(cited: &[&Observation]) -> Basis {
    let spans: BTreeSet<&SpanRef> = cited.iter().flat_map(|c| &c.spans).collect();
    let mut seen = BTreeSet::new();
    let (first, rest): (Vec<_>, Vec<_>) =
        spans.into_iter().partition(|s| seen.insert(&s.record_id));
    Basis {
        refs: first
            .into_iter()
            .chain(rest)
            .take(super::lens_page::MAX_BASIS_REFS)
            .map(|s| BasisRef::Span {
                record_id: s.record_id.clone(),
                revision_digest: s.revision_digest.clone(),
                span_id: s.span_id.clone(),
            })
            .collect(),
        uncertainty: None,
    }
}

#[cfg(test)]
mod tests;
