//! User-authored judgments and optional dated checks. Outcomes record facts, never advice.
use super::{
    EarlierRelation, JournalCheck, JournalDraft, JournalOutcome, JournalOutcomeDraft,
    JournalRecord, KnowledgeError, KnowledgeResult,
};
use chrono::{Datelike, NaiveDate};
use rusqlite::{params, Transaction};
use serde_json::{json, Value};

pub(crate) fn field(code: &'static str, expected: &'static str) -> KnowledgeError {
    KnowledgeError::JournalField { code, expected }
}
fn text(value: &str, max: usize, empty: bool, expected: &'static str) -> KnowledgeResult<()> {
    if value.contains('\0') || value.chars().count() > max || (!empty && value.trim().is_empty()) {
        return Err(field("journal_field", expected));
    }
    Ok(())
}
fn date(value: &str) -> KnowledgeResult<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .filter(|d| d.year() >= 100 && d.format("%Y-%m-%d").to_string() == value)
        .ok_or_else(|| field("journal_date", "journal date must be YYYY-MM-DD"))
}
pub(crate) fn journal_date(at_ms: i64) -> KnowledgeResult<NaiveDate> {
    chrono::DateTime::from_timestamp_millis(at_ms)
        .map(|d| d.date_naive())
        .ok_or_else(|| field("journal_date", "journal timestamp must be valid"))
}
pub(crate) fn validate_fields(
    judgment: &str,
    reasoning: &str,
    entities: &[String],
    check: Option<&JournalCheck>,
    thesis: Option<&super::assets::JournalThesis>,
) -> KnowledgeResult<()> {
    text(
        judgment,
        1000,
        false,
        "journal.judgment must contain 1–1000 characters",
    )?;
    text(
        reasoning,
        2000,
        true,
        "journal.reasoning must contain at most 2000 characters",
    )?;
    if entities.len() > 30 {
        return Err(field(
            "journal_entities",
            "journal.entities accepts at most 30 World entity ids",
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for entity in entities {
        text(
            entity,
            256,
            false,
            "journal.entities must contain World entity ids",
        )?;
        if !seen.insert(entity) {
            return Err(field("journal_entities", "journal.entities must be unique"));
        }
    }
    // A line on an asset page may stand without a date; every other check is dated.
    let line = thesis.is_some_and(|thesis| thesis.role == super::assets::ThesisRole::Line);
    if let Some(check) = check {
        validate_check(check, !line)?;
    }
    if let Some(thesis) = thesis {
        super::assets::validate_thesis(thesis, check.is_some())?;
    }
    Ok(())
}
/// Shape only; `validate_fields` decides whether the check needs a date.
pub(crate) fn parse_check(value: Option<&Value>) -> KnowledgeResult<Option<JournalCheck>> {
    let Some(value) = value.filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let check: JournalCheck = serde_json::from_value(value.clone()).map_err(|_| field("journal_check", "journal.check requires claim, metric, direction, threshold and byDate; unit and confidence are optional"))?;
    validate_check(&check, false)?;
    Ok(Some(check))
}
pub(crate) fn validate_check(p: &JournalCheck, dated: bool) -> KnowledgeResult<()> {
    text(
        &p.claim,
        2000,
        false,
        "journal.check.claim must contain 1–2000 characters",
    )?;
    text(
        &p.metric,
        200,
        false,
        "journal.check.metric must contain 1–200 characters",
    )?;
    if !matches!(p.direction.as_str(), "above" | "below") {
        return Err(field(
            "journal_check_direction",
            "journal.check.direction must be above or below",
        ));
    }
    if !p.threshold.is_finite() {
        return Err(field(
            "journal_check_threshold",
            "journal.check.threshold must be finite",
        ));
    }
    if let Some(unit) = &p.unit {
        text(
            unit,
            80,
            false,
            "journal.check.unit must contain 1–80 characters",
        )?;
    }
    match &p.by_date {
        Some(by_date) => {
            date(by_date)?;
        }
        None if dated => {
            return Err(field(
                "journal_check_date",
                "journal.check.byDate is required: YYYY-MM-DD",
            ))
        }
        None => {}
    }
    if p.confidence
        .is_some_and(|c| !(10..=90).contains(&c) || c % 10 != 0)
    {
        return Err(field("journal_check_confidence", "journal.check.confidence must be a user-entered percentage from 10 to 90 in steps of 10"));
    }
    Ok(())
}
pub(crate) fn parse_outcome(value: &Value) -> KnowledgeResult<JournalOutcomeDraft> {
    let outcome: JournalOutcomeDraft = serde_json::from_value(value.clone()).map_err(|_| {
        field(
            "journal_outcome",
            "journal.outcome requires observed and observedAt; note and unclear are optional",
        )
    })?;
    validate_outcome(&outcome)?;
    Ok(outcome)
}
fn validate_outcome(o: &JournalOutcomeDraft) -> KnowledgeResult<()> {
    if !o.observed.is_finite() {
        return Err(field(
            "journal_outcome_observed",
            "journal.outcome.observed must be finite",
        ));
    }
    date(&o.observed_at)?;
    if let Some(note) = &o.note {
        text(
            note,
            2000,
            false,
            "journal.outcome.note must contain 1–2000 characters",
        )?;
    }
    Ok(())
}
pub(crate) fn single_review<T>(reviews: impl IntoIterator<Item = T>) -> KnowledgeResult<T> {
    let mut reviews = reviews.into_iter();
    match (reviews.next(), reviews.next()) {
        (Some(review), None) => Ok(review),
        _ => Err(field(
            "journal_outcome_review",
            "journal.outcome requires exactly one acknowledged journal entry with relation review",
        )),
    }
}
pub(crate) fn validate_review_check(
    check: Option<&JournalCheck>,
    outcome: &JournalOutcomeDraft,
    today: NaiveDate,
) -> KnowledgeResult<()> {
    let check = check.ok_or_else(|| {
        field(
            "journal_outcome_check",
            "journal review requires an entry with a check",
        )
    })?;
    validate_outcome(outcome)?;
    let by_date = date(check.by_date.as_deref().ok_or_else(|| {
        field(
            "journal_outcome_check",
            "this line has no date to review; the asset page reads it instead",
        )
    })?)?;
    if by_date > today {
        return Err(field(
            "journal_outcome_not_due",
            "journal check is not due yet",
        ));
    }
    let observed = date(&outcome.observed_at)?;
    if observed < by_date || observed > today {
        return Err(field(
            "journal_outcome_observed_at",
            "journal.outcome.observedAt must be on or after byDate and no later than today",
        ));
    }
    Ok(())
}
pub(crate) fn dated_evidence(point: Option<&super::TimePoint>, by_date: &str, today: &str) -> bool {
    let Some(point) = point else {
        return false;
    };
    if point.approximate {
        return false;
    }
    let Some(value) = point.value.as_deref() else {
        return false;
    };
    let day = match point.precision {
        super::TimePrecision::Date => value.to_owned(),
        super::TimePrecision::Instant => match chrono::DateTime::parse_from_rfc3339(value) {
            Ok(v) => v.with_timezone(&chrono::Utc).date_naive().to_string(),
            Err(_) => return false,
        },
        _ => return false,
    };
    day.as_str() >= by_date && day.as_str() <= today
}
pub(super) fn review(
    tx: &Transaction<'_>,
    loupist_id: &str,
    draft: &JournalDraft,
    today: NaiveDate,
) -> KnowledgeResult<Option<JournalRecord>> {
    let Some(o) = &draft.outcome else {
        return Ok(None);
    };
    let earlier = single_review(
        draft
            .earlier
            .iter()
            .filter(|e| e.relation == EarlierRelation::Review),
    )?;
    let id = earlier.journal_id.as_deref().ok_or_else(|| {
        field(
            "journal_outcome_review",
            "journal.outcome requires an earlier journal entry",
        )
    })?;
    if earlier.delivered_text.trim().is_empty()
        || earlier.delivered_digest != crate::util::sha256(earlier.delivered_text.as_bytes())
        || !matches!(
            earlier.coverage.as_str(),
            "included_full" | "included_excerpt"
        )
    {
        return Err(field(
            "journal_outcome_exact_read",
            "journal.outcome requires an acknowledged exact read this turn",
        ));
    }
    let mut record = super::research::get_journal(tx, loupist_id, id)?;
    validate_review_check(record.check.as_ref(), o, today)?;
    require_unreviewed(tx, loupist_id, id)?;
    let check = record.check.as_ref().expect("validated check");
    let came_true = if check.direction == "above" {
        o.observed > check.threshold
    } else {
        o.observed < check.threshold
    };
    record.outcome = Some(JournalOutcome {
        observed: o.observed,
        observed_at: o.observed_at.clone(),
        result: if o.unclear {
            "unclear"
        } else if came_true {
            "came_true"
        } else {
            "did_not"
        }
        .into(),
        note: o.note.clone(),
        reviewed_as_of: o.reviewed_as_of.clone(),
    });
    Ok(Some(record))
}
pub(super) fn require_unreviewed(
    tx: &Transaction<'_>,
    loupist_id: &str,
    id: &str,
) -> KnowledgeResult<()> {
    let record = super::research::get_journal(tx, loupist_id, id)?;
    if record.outcome.is_some() || record.archived {
        return Err(field(
            "journal_outcome_review",
            "journal entry is archived or already reviewed",
        ));
    }
    Ok(())
}
pub(crate) fn list(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<Vec<JournalRecord>> {
    super::model::validate_loupist_id(loupist_id)?;
    let mut stmt = tx.prepare("SELECT body_json FROM knowledge_journals WHERE loupist_id=?1 ORDER BY created_at_ms DESC, journal_id")?;
    let bodies = stmt
        .query_map([loupist_id], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    bodies
        .into_iter()
        .map(|b| Ok(serde_json::from_str(&b)?))
        .collect()
}
pub(crate) fn due(
    tx: &Transaction<'_>,
    loupist_id: &str,
    today: &str,
) -> KnowledgeResult<Vec<JournalRecord>> {
    date(today)?;
    let mut rows: Vec<_> = list(tx, loupist_id)?
        .into_iter()
        .filter(|j| {
            !j.archived
                && j.outcome.is_none()
                && j.check
                    .as_ref()
                    .and_then(|c| c.by_date.as_deref())
                    .is_some_and(|by_date| by_date <= today)
        })
        .collect();
    rows.sort_by(|a, b| {
        a.check
            .as_ref()
            .and_then(|c| c.by_date.as_deref())
            .cmp(&b.check.as_ref().and_then(|c| c.by_date.as_deref()))
            .then(a.created_at_ms.cmp(&b.created_at_ms))
            .then(a.journal_id.cmp(&b.journal_id))
    });
    Ok(rows)
}
pub(super) fn context(
    tx: &Transaction<'_>,
    loupist_id: &str,
    today: &str,
    known: &mut dyn FnMut(JournalRecord) -> Option<JournalRecord>,
) -> KnowledgeResult<Value> {
    date(today)?;
    let entries: Vec<JournalRecord> = list(tx, loupist_id)?
        .into_iter()
        .filter(|j| !j.archived)
        .filter_map(&mut *known)
        .collect();
    let mut due: Vec<_> = entries
        .iter()
        .filter(|j| {
            j.outcome.is_none()
                && j.check
                    .as_ref()
                    .and_then(|c| c.by_date.as_deref())
                    .is_some_and(|by_date| by_date <= today)
        })
        .cloned()
        .collect();
    due.sort_by(|a, b| {
        a.check
            .as_ref()
            .and_then(|c| c.by_date.as_deref())
            .cmp(&b.check.as_ref().and_then(|c| c.by_date.as_deref()))
            .then(a.created_at_ms.cmp(&b.created_at_ms))
            .then(a.journal_id.cmp(&b.journal_id))
    });
    let (mut came_true, mut did_not, mut unclear, mut pending) = (0, 0, 0, 0);
    for j in entries {
        match j.outcome.as_ref().map(|o| o.result.as_str()) {
            Some("came_true") => came_true += 1,
            Some("did_not") => did_not += 1,
            Some("unclear") => unclear += 1,
            _ if j.check.is_some() => pending += 1,
            _ => {}
        }
    }
    Ok(
        json!({"dueJournals":due.into_iter().take(5).collect::<Vec<_>>(),"record":{"cameTrue":came_true,"didNot":did_not,"unclear":unclear,"pending":pending}}),
    )
}
pub(super) fn persist(tx: &Transaction<'_>, record: &JournalRecord) -> KnowledgeResult<()> {
    tx.execute("INSERT INTO knowledge_journals(journal_id,research_id,loupist_id,body_json,input_digest,created_at_ms) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(journal_id) DO UPDATE SET body_json=excluded.body_json,input_digest=excluded.input_digest", params![record.journal_id,record.research_id,record.loupist_id,serde_json::to_string(record)?,record.input_digest,record.created_at_ms])?;
    Ok(())
}
#[cfg(any(test, feature = "e2e"))]
pub(crate) fn fixture_prediction() -> JournalCheck {
    JournalCheck {
        claim: "Metric will exceed the threshold".into(),
        metric: "metric".into(),
        direction: "above".into(),
        threshold: 10.0,
        unit: None,
        by_date: Some("1970-01-02".into()),
        confidence: Some(70),
    }
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JournalInput {
    pub judgment: String,
    #[serde(default)]
    pub reasoning: String,
    #[serde(default)]
    pub entities: Vec<String>,
    pub check: Option<JournalCheck>,
    /// Tags the entry as a reason or line of an existing asset page.
    #[serde(default)]
    pub thesis: Option<super::assets::JournalThesis>,
}

/// A thesis entry belongs to a page that already exists for this Loupist.
fn require_page(
    tx: &Transaction<'_>,
    loupist_id: &str,
    thesis: Option<&super::assets::JournalThesis>,
) -> KnowledgeResult<()> {
    if let Some(thesis) = thesis {
        if super::assets::get_page(tx, loupist_id, &thesis.asset_id)?.is_none() {
            return Err(field(
                "journal_thesis",
                "journal.thesis.assetId must name an asset page",
            ));
        }
    }
    Ok(())
}
pub(crate) fn validate_entities(
    tx: &Transaction<'_>,
    loupist_id: &str,
    entities: &[String],
) -> KnowledgeResult<()> {
    if entities.is_empty() {
        return Ok(());
    }
    let world = super::lens::world_body(tx, loupist_id)?;
    for entity in entities {
        if !world
            .as_ref()
            .is_some_and(|world| world.nodes.iter().any(|node| &node.id == entity))
        {
            return Err(field(
                "journal_entity",
                "journal.entities must use current World node ids; omit an entity when no World node matches",
            ));
        }
    }
    Ok(())
}
pub(crate) fn create(
    tx: &Transaction<'_>,
    loupist_id: &str,
    input: JournalInput,
    at_ms: i64,
) -> KnowledgeResult<JournalRecord> {
    super::model::validate_loupist_id(loupist_id)?;
    super::model::validate_time(at_ms)?;
    validate_fields(
        &input.judgment,
        &input.reasoning,
        &input.entities,
        input.check.as_ref(),
        input.thesis.as_ref(),
    )?;
    validate_entities(tx, loupist_id, &input.entities)?;
    require_page(tx, loupist_id, input.thesis.as_ref())?;
    let inputs = super::JournalInputs {
        sources: vec![],
        worlds: vec![],
        lenses: vec![],
        earlier: vec![],
    };
    let record = JournalRecord {
        journal_id: super::model::mint_id("jnl"),
        research_id: String::new(),
        loupist_id: loupist_id.into(),
        judgment: input.judgment,
        reasoning: input.reasoning,
        check: input.check,
        thesis: input.thesis,
        entities: input.entities,
        outcome: None,
        input_digest: super::canonical::digest_json(&serde_json::to_value(&inputs)?)?,
        inputs,
        origin_chat_id: None,
        created_at_ms: at_ms,
        updated_at_ms: at_ms,
        archived: false,
    };
    persist(tx, &record)?;
    growth(tx, &record, "journal_created", None)?;
    Ok(record)
}
pub(crate) fn update(
    tx: &Transaction<'_>,
    loupist_id: &str,
    journal_id: &str,
    expected_updated_at_ms: i64,
    input: JournalInput,
    at_ms: i64,
) -> KnowledgeResult<JournalRecord> {
    let mut record = super::research::get_journal(tx, loupist_id, journal_id)?;
    if record.updated_at_ms != expected_updated_at_ms {
        return Err(KnowledgeError::RevisionConflict {
            expected: Some(expected_updated_at_ms.to_string()),
            actual: Some(record.updated_at_ms.to_string()),
        });
    }
    let thesis = input.thesis.clone().or_else(|| record.thesis.clone());
    validate_fields(
        &input.judgment,
        &input.reasoning,
        &input.entities,
        input.check.as_ref(),
        thesis.as_ref(),
    )?;
    validate_entities(tx, loupist_id, &input.entities)?;
    require_page(tx, loupist_id, input.thesis.as_ref())?;
    if record.outcome.is_some() && record.check != input.check {
        return Err(field(
            "journal_reviewed_check",
            "journal.check cannot change after review",
        ));
    }
    let before = record.judgment.clone();
    record.judgment = input.judgment;
    record.reasoning = input.reasoning;
    record.entities = input.entities;
    record.check = input.check;
    record.thesis = thesis;
    record.updated_at_ms = at_ms.max(record.updated_at_ms + 1);
    persist(tx, &record)?;
    growth(tx, &record, "journal_updated", Some(before))?;
    Ok(record)
}
pub(crate) fn archive(
    tx: &Transaction<'_>,
    loupist_id: &str,
    journal_id: &str,
    archived: bool,
    at_ms: i64,
) -> KnowledgeResult<JournalRecord> {
    let mut record = super::research::get_journal(tx, loupist_id, journal_id)?;
    record.archived = archived;
    record.updated_at_ms = at_ms.max(record.updated_at_ms + 1);
    persist(tx, &record)?;
    growth(
        tx,
        &record,
        if archived {
            "journal_archived"
        } else {
            "journal_restored"
        },
        None,
    )?;
    Ok(record)
}
pub(super) fn growth(
    tx: &Transaction<'_>,
    record: &JournalRecord,
    kind: &str,
    before: Option<String>,
) -> KnowledgeResult<()> {
    let mut event = super::growth::GrowthEvent::new(
        &record.loupist_id,
        &record.journal_id,
        record.updated_at_ms,
        kind,
        "user",
        &record.judgment,
    );
    event.origin_chat_id = record.origin_chat_id.clone();
    event.before_summary = before;
    event.after_summary = Some(record.judgment.clone());
    event.insert(tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::{EarlierResearch, KnowledgeStore};
    fn store() -> (tempfile::TempDir, KnowledgeStore) {
        let root = tempfile::tempdir().unwrap();
        let store = KnowledgeStore::open(root.path().join("knowledge.sqlite")).unwrap();
        (root, store)
    }
    fn input() -> JournalInput {
        JournalInput {
            judgment: "I think margins will stay above 55%.".into(),
            reasoning: String::new(),
            entities: vec![],
            check: Some(JournalCheck {
                claim: "Margins stay above 55%".into(),
                metric: "Gross margin".into(),
                direction: "above".into(),
                threshold: 55.0,
                unit: Some("%".into()),
                by_date: Some("2026-01-31".into()),
                confidence: None,
            }),
            thesis: None,
        }
    }
    #[test]
    fn entity_links_use_the_complete_current_world() {
        use crate::knowledge::{CreateRecord, GraphNode, RecordKind, SealWorld};
        let (_root, store) = store();
        let world = store
            .create_record(CreateRecord {
                loupist_id: "loupist-a".into(),
                kind: RecordKind::World,
                title: "World".into(),
                at_ms: 1,
            })
            .unwrap();
        store
            .seal_world(SealWorld {
                loupist_id: "loupist-a".into(),
                record_id: world.record_id,
                expected_revision_id: None,
                at_ms: 2,
                nodes: (0..1501)
                    .map(|i| GraphNode {
                        id: format!("entity-{i:04}"),
                        name: format!("Company {i}"),
                        aliases: vec![],
                        description: String::new(),
                        kind: "company".into(),
                        source_role: "organization".into(),
                        properties: Default::default(),
                    })
                    .collect(),
                claims: vec![],
                annotations: vec![],
                dependencies: vec![],
            })
            .unwrap();
        let mut linked = input();
        linked.entities = vec!["entity-1500".into()];
        store
            .transact(|w| create(w.tx, "loupist-a", linked.clone(), 3))
            .unwrap();
        assert!(store
            .transact(|w| create(w.tx, "loupist-b", linked.clone(), 3))
            .is_err());
        linked.entities = vec!["missing-entity".into()];
        assert!(store
            .transact(|w| create(w.tx, "loupist-a", linked, 3))
            .is_err());
    }
    fn review_draft(
        record: &JournalRecord,
        observed: f64,
        observed_at: &str,
        unclear: bool,
    ) -> JournalDraft {
        let delivered = serde_json::to_string(record).unwrap();
        JournalDraft {
            judgment: record.judgment.clone(),
            reasoning: record.reasoning.clone(),
            entities: vec![],
            check: record.check.clone(),
            thesis: None,
            outcome: Some(JournalOutcomeDraft {
                observed,
                observed_at: observed_at.into(),
                note: None,
                unclear,
                reviewed_as_of: None,
            }),
            source_receipt_ids: vec![],
            world_delivery_ids: vec![],
            lenses: vec![],
            earlier: vec![EarlierResearch {
                research_id: record.research_id.clone(),
                journal_id: Some(record.journal_id.clone()),
                relation: EarlierRelation::Review,
                delivered_digest: crate::util::sha256(delivered.as_bytes()),
                delivered_text: delivered,
                coverage: "included_full".into(),
            }],
        }
    }
    #[test]
    fn create_update_archive_are_loupist_scoped_and_reject_stale_edits() {
        let (_root, store) = store();
        let entry = store
            .transact(|w| create(w.tx, "loupist-a", input(), 1))
            .unwrap();
        assert!(store.get_journal("loupist-b", &entry.journal_id).is_err());
        let mut next = input();
        next.judgment = "I changed my mind after checking costs.".into();
        next.check = None;
        let updated = store
            .transact(|w| {
                update(
                    w.tx,
                    "loupist-a",
                    &entry.journal_id,
                    entry.updated_at_ms,
                    next.clone(),
                    2,
                )
            })
            .unwrap();
        assert_eq!(updated.judgment, next.judgment);
        assert!(updated.check.is_none());
        assert_eq!(
            store
                .transact(|w| update(w.tx, "loupist-a", &entry.journal_id, 1, next.clone(), 3))
                .unwrap_err()
                .code(),
            "revision_conflict"
        );
        assert!(store
            .transact(|w| archive(w.tx, "loupist-b", &entry.journal_id, true, 3))
            .is_err());
        store
            .transact(|w| archive(w.tx, "loupist-a", &entry.journal_id, true, 3))
            .unwrap();
        assert!(
            store
                .get_journal("loupist-a", &entry.journal_id)
                .unwrap()
                .archived
        );
        assert!(store
            .read(|w| due(w.tx, "loupist-a", "2026-02-01"))
            .unwrap()
            .is_empty());
        store
            .transact(|w| archive(w.tx, "loupist-a", &entry.journal_id, false, 4))
            .unwrap();
        assert!(
            !store
                .get_journal("loupist-a", &entry.journal_id)
                .unwrap()
                .archived
        );
    }
    #[test]
    fn validates_character_limits_optional_check_and_user_confidence_steps() {
        let (_root, store) = store();
        let create_with = |change: &dyn Fn(&mut JournalInput)| {
            let mut value = input();
            change(&mut value);
            store.transact(|w| create(w.tx, "loupist-a", value, 1))
        };
        assert!(create_with(&|v| {
            v.judgment = "한".repeat(1000);
            v.reasoning = "🙂".repeat(2000);
            v.check = None;
        })
        .is_ok());
        assert!(create_with(&|v| v.judgment = "한".repeat(1001)).is_err());
        assert!(create_with(&|v| v.reasoning = "x".repeat(2001)).is_err());
        assert!(create_with(&|v| v.entities = vec!["foreign-entity".into()]).is_err());
        for (confidence, valid) in [
            (None, true),
            (Some(10), true),
            (Some(90), true),
            (Some(5), false),
            (Some(65), false),
            (Some(100), false),
        ] {
            let mut c = input().check.unwrap();
            c.confidence = confidence;
            assert_eq!(validate_check(&c, true).is_ok(), valid, "{confidence:?}");
        }
        let check = |change: &dyn Fn(&mut JournalCheck)| {
            let mut c = input().check.unwrap();
            change(&mut c);
            validate_check(&c, true)
        };
        assert!(check(&|c| c.by_date = Some("2026-02-30".into())).is_err());
        assert!(check(&|c| c.threshold = f64::NAN).is_err());
        assert!(check(&|c| c.direction = "equal".into()).is_err());
    }
    #[test]
    fn reviews_require_due_check_exact_read_and_nonfuture_observation() {
        let (_root, store) = store();
        let entry = store
            .transact(|w| create(w.tx, "loupist-a", input(), 1))
            .unwrap();
        let today = date("2026-02-02").unwrap();
        let draft = review_draft(&entry, 56.0, "2026-01-31", false);
        assert!(store
            .read(|w| review(w.tx, "loupist-b", &draft, today))
            .is_err());
        assert_eq!(
            store
                .read(|w| review(w.tx, "loupist-a", &draft, date("2026-01-30").unwrap()))
                .unwrap_err()
                .code(),
            "journal_outcome_not_due"
        );
        for observed in ["2026-01-30", "2026-02-03"] {
            let invalid = review_draft(&entry, 56.0, observed, false);
            assert_eq!(
                store
                    .read(|w| review(w.tx, "loupist-a", &invalid, today))
                    .unwrap_err()
                    .code(),
                "journal_outcome_observed_at"
            );
        }
        let mut invalid = draft.clone();
        invalid.earlier.clear();
        assert!(store
            .read(|w| review(w.tx, "loupist-a", &invalid, today))
            .is_err());
        invalid = draft.clone();
        invalid.earlier[0].delivered_digest = "wrong".into();
        assert_eq!(
            store
                .read(|w| review(w.tx, "loupist-a", &invalid, today))
                .unwrap_err()
                .code(),
            "journal_outcome_exact_read"
        );
        invalid = draft.clone();
        invalid.outcome.as_mut().unwrap().observed = f64::INFINITY;
        assert!(store
            .read(|w| review(w.tx, "loupist-a", &invalid, today))
            .is_err());
        let reviewed = store
            .transact(|w| {
                let reviewed = review(w.tx, "loupist-a", &draft, today)?.unwrap();
                persist(w.tx, &reviewed)?;
                Ok(reviewed)
            })
            .unwrap();
        assert_eq!(reviewed.journal_id, entry.journal_id);
        assert_eq!(reviewed.judgment, entry.judgment);
        assert_eq!(reviewed.outcome.as_ref().unwrap().result, "came_true");
        assert_eq!(
            store
                .read(|w| review(w.tx, "loupist-a", &draft, today))
                .unwrap_err()
                .code(),
            "journal_outcome_review"
        );
        let mut changed = input();
        changed.check.as_mut().unwrap().threshold = 40.0;
        assert!(store
            .transact(|w| update(
                w.tx,
                "loupist-a",
                &entry.journal_id,
                reviewed.updated_at_ms,
                changed,
                3
            ))
            .is_err());
    }
    #[test]
    fn plain_record_counts_and_due_list_exclude_archived_and_resolved_entries() {
        let (_root, store) = store();
        let today = date("2026-02-02").unwrap();
        for (direction, observed, unclear, expected) in [
            ("above", 56.0, false, "came_true"),
            ("below", 54.0, false, "came_true"),
            ("above", 55.0, false, "did_not"),
            ("below", 55.0, false, "did_not"),
            ("above", 54.0, true, "unclear"),
        ] {
            let mut value = input();
            value.check.as_mut().unwrap().direction = direction.into();
            let entry = store
                .transact(|w| create(w.tx, "loupist-a", value, 1))
                .unwrap();
            let draft = review_draft(&entry, observed, "2026-01-31", unclear);
            store
                .transact(|w| {
                    let reviewed = review(w.tx, "loupist-a", &draft, today)?.unwrap();
                    assert_eq!(reviewed.outcome.as_ref().unwrap().result, expected);
                    persist(w.tx, &reviewed)
                })
                .unwrap();
        }
        let pending = store
            .transact(|w| create(w.tx, "loupist-a", input(), 2))
            .unwrap();
        let mut later = input();
        later.check.as_mut().unwrap().by_date = Some("2026-02-10".into());
        store
            .transact(|w| create(w.tx, "loupist-a", later, 3))
            .unwrap();
        let archived = store
            .transact(|w| create(w.tx, "loupist-a", input(), 4))
            .unwrap();
        store
            .transact(|w| archive(w.tx, "loupist-a", &archived.journal_id, true, 5))
            .unwrap();
        store
            .transact(|w| create(w.tx, "loupist-b", input(), 1))
            .unwrap();
        let due_entries = store
            .read(|w| due(w.tx, "loupist-a", "2026-02-02"))
            .unwrap();
        assert_eq!(due_entries.len(), 1);
        assert_eq!(due_entries[0].journal_id, pending.journal_id);
        assert!(store
            .read(|w| due(w.tx, "loupist-a", "2026-01-30"))
            .unwrap()
            .is_empty());
        let ctx = store
            .read(|w| context(w.tx, "loupist-a", "2026-02-02", &mut Some))
            .unwrap();
        assert_eq!(
            ctx["record"],
            json!({"cameTrue":2,"didNot":2,"unclear":1,"pending":2})
        );
        assert_eq!(ctx["dueJournals"][0]["journalId"], pending.journal_id);
    }
    #[test]
    fn review_evidence_needs_a_precise_date_on_or_after_the_check() {
        for (value, expected) in [
            ("2026-01-30", false),
            ("2026-01-31", true),
            ("2026-02-02", true),
            ("2026-02-03", false),
        ] {
            let point = super::super::TimePoint {
                precision: super::super::TimePrecision::Date,
                value: Some(value.into()),
                approximate: false,
            };
            assert_eq!(
                dated_evidence(Some(&point), "2026-01-31", "2026-02-02"),
                expected
            );
        }
        assert!(!dated_evidence(None, "2026-01-31", "2026-02-02"));
    }
}
