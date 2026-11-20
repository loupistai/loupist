//! Journal tool input conversion. Canonical check/review rules live in knowledge::journal.
use super::*;
use crate::knowledge::assets::ThesisRole;

pub(super) fn journal_argument_error(error: crate::knowledge::KnowledgeError) -> BrokerError {
    match error {
        crate::knowledge::KnowledgeError::JournalField { code, expected } => BrokerError::invalid(
            expected.split_whitespace().next().unwrap_or("journal"),
            format!("{code}: {expected}"),
        ),
        error => knowledge_error(error),
    }
}
pub(super) type ThesisInput = (String, ThesisRole);

pub(super) fn parse_research_journal(
    value: &Value,
    at_ms: i64,
) -> Result<(StagedResearchJournal, Option<ThesisInput>), BrokerError> {
    let object = exact_object(
        value,
        &["judgment"],
        &[
            "reasoning",
            "entities",
            "check",
            "thesis",
            "sourceReads",
            "worldReads",
            "lensReads",
        ],
    )?;
    let thesis = parse_thesis_target(object.get("thesis"))?;
    let journal = StagedResearchJournal {
        thesis: None,
        at_ms,
        judgment: bounded_text(object.get("judgment"), 1, 1000)?,
        reasoning: optional_bounded_text(object.get("reasoning"), 0, 2000)?.unwrap_or_default(),
        entities: optional_unique_strings(object.get("entities"), 30, 256).map_err(|_| {
            BrokerError::invalid(
                "journal.entities",
                "at most 30 unique nonempty World node ids, each at most 256 characters",
            )
        })?,
        check: crate::knowledge::journal::parse_check(object.get("check"))
            .map_err(journal_argument_error)?,
        outcome: None,
        earlier: Vec::new(),
        source_reads: optional_unique_strings(object.get("sourceReads"), 32, 128)?,
        world_reads: optional_unique_strings(object.get("worldReads"), 8, 128)?,
        lens_reads: optional_unique_strings(object.get("lensReads"), 8, 128)?,
    };
    // A reason or line is the user's own words about an asset they follow; the
    // asset page is its context, so it needs no separate Memory read.
    if thesis.is_none()
        && journal.source_reads.is_empty()
        && journal.world_reads.is_empty()
        && journal.lens_reads.is_empty()
    {
        return Err(BrokerError::invalid(
            "journal",
            "at least one acknowledged sourceReads, worldReads or lensReads reference",
        ));
    }
    Ok((journal, thesis))
}

/// `thesis: {asset, role}` names the page through an id, ticker or name.
fn parse_thesis_target(value: Option<&Value>) -> Result<Option<ThesisInput>, BrokerError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let object = exact_object(value, &["asset", "role"], &[])?;
    let asset = bounded_text(object.get("asset"), 1, 256)?;
    let role = match object.get("role").and_then(Value::as_str) {
        Some("reason") => ThesisRole::Reason,
        Some("line") => ThesisRole::Line,
        _ => return Err(BrokerError::invalid("thesis.role", "reason or line")),
    };
    Ok(Some((asset, role)))
}
