use super::*;
use crate::knowledge::model::{validate_research_review, ResearchReviewDraft};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::knowledge) struct Experience {
    pub question: String,
    pub checks: Vec<Observation>,
    pub next_check: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::knowledge) struct Observation {
    pub id: String,
    pub turn_id: String,
    pub question: String,
    pub disposition: crate::knowledge::CheckDisposition,
    pub finding: String,
    pub limitation: String,
    pub inputs: JournalInputs,
    pub eligible_original: bool,
}

pub(super) fn save(
    tx: &Transaction<'_>,
    request: &SaveResearch,
    review: &ResearchReviewDraft,
    admitted: &JournalInputs,
) -> KnowledgeResult<String> {
    validate_research_review(review)?;
    if request.working_note.is_none() {
        return Err(KnowledgeError::Invalid(
            "research review requires a working note",
        ));
    }
    if let Some(cutoff) = &request.as_of {
        chrono::NaiveDate::parse_from_str(cutoff, "%Y-%m-%d")
            .map_err(|_| KnowledgeError::Invalid("research cutoff is invalid"))?;
    }
    let mut checks = Vec::new();
    for (index, check) in review.checks.iter().enumerate() {
        let mut inputs = JournalInputs {
            sources: vec![],
            worlds: vec![],
            lenses: vec![],
            earlier: vec![],
        };
        for reference in &check.evidence {
            if let Some(source) = admitted
                .sources
                .iter()
                .find(|source| &source.receipt_id == reference)
            {
                inputs.sources.push(source.clone());
            } else if let Some(world) = admitted
                .worlds
                .iter()
                .find(|world| &world.delivery_id == reference)
            {
                inputs.worlds.push(world.clone());
            } else {
                return Err(KnowledgeError::Unauthorized(
                    "review evidence was not admitted by its note",
                ));
            }
        }
        if let Some(reference) = &check.method_read {
            inputs.lenses.push(
                admitted
                    .lenses
                    .iter()
                    .find(|lens| &lens.delivery_id == reference)
                    .ok_or(KnowledgeError::Unauthorized(
                        "review method was not admitted by its note",
                    ))?
                    .clone(),
            );
        }
        let mut eligible_original = false;
        for source in &inputs.sources {
            let eligible: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM knowledge_source_representations p JOIN knowledge_source_captures c ON c.record_id=p.record_id AND c.capture_id=p.capture_id JOIN knowledge_records r ON r.record_id=p.record_id WHERE p.record_id=?1 AND p.representation_id=?2 AND r.owner_loupist_id=?3 AND p.role IN ('original','provider_payload','extracted_text','structured','excerpt') AND c.coverage IN ('included_full','included_excerpt') AND c.authorship IN ('publisher','user','tool') AND c.provider_identity NOT LIKE 'shared_pack:%')",
                params![source.record_id, source.representation_id, request.loupist_id], |row| row.get(0))?;
            eligible_original |= eligible;
        }
        checks.push(Observation {
            id: format!("obs-{}-{}", request.turn_id, index),
            turn_id: request.turn_id.clone(),
            question: check.question.clone(),
            disposition: check.disposition.clone(),
            finding: check.finding.clone(),
            limitation: check.limitation.clone(),
            inputs,
            eligible_original,
        });
    }
    let body = to_value(&Experience {
        question: review.question.clone(),
        checks,
        next_check: review.next_check.clone(),
    })?;
    let digest = digest_json(&body)?;
    let existing: Option<(String,String,Option<String>,String)> = tx.query_row(
        "SELECT experience_id,body_digest,as_of,body_json FROM knowledge_research_experiences WHERE loupist_id=?1 AND turn_id=?2",
        params![request.loupist_id,request.turn_id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?))).optional()?;
    if let Some((id, prior_digest, prior_cutoff, prior_body)) = existing {
        let prior: serde_json::Value = from_json(&prior_body)?;
        if digest_json(&prior)? != prior_digest {
            return Err(KnowledgeError::DigestMismatch);
        }
        if prior_cutoff != request.as_of
            || (prior_digest != digest
                && !same_research_without_receipt_ids(
                    serde_json::from_value(prior)?,
                    serde_json::from_value(body.clone())?,
                ))
        {
            return Err(KnowledgeError::Invalid(
                "completed research experience conflicts with its turn",
            ));
        }
        return Ok(id);
    }
    let id = mint_id("exp");
    tx.execute("INSERT INTO knowledge_research_experiences(experience_id,loupist_id,turn_id,created_at_ms,as_of,body_json,body_digest) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![id,request.loupist_id,request.turn_id,request.at_ms,request.as_of,canonical_string(&body),digest])?;
    if request.as_of.is_none() {
        super::super::learning::enqueue_experiences(tx, &request.loupist_id, request.at_ms)?;
    }
    Ok(id)
}

fn same_research_without_receipt_ids(mut left: Experience, mut right: Experience) -> bool {
    for experience in [&mut left, &mut right] {
        for check in &mut experience.checks {
            for source in &mut check.inputs.sources {
                source.receipt_id.clear();
            }
            for world in &mut check.inputs.worlds {
                world.delivery_id.clear();
            }
            for lens in &mut check.inputs.lenses {
                lens.delivery_id.clear();
            }
        }
    }
    left == right
}

pub(in crate::knowledge) fn visit_recent(
    tx: &Transaction<'_>,
    loupist: &str,
    start: i64,
    end: i64,
    mut visit: impl FnMut(Experience) -> KnowledgeResult<()>,
) -> KnowledgeResult<()> {
    let mut query = tx.prepare("SELECT body_json,body_digest FROM knowledge_research_experiences WHERE loupist_id=?1 AND sequence>?2 AND sequence<=?3 AND as_of IS NULL ORDER BY sequence DESC LIMIT 24")?;
    let rows = query.query_map(params![loupist, start, end], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (body, digest) = row?;
        let value: serde_json::Value = from_json(&body)?;
        if digest_json(&value)? != digest {
            return Err(KnowledgeError::DigestMismatch);
        }
        visit(serde_json::from_value(value)?)?;
    }
    Ok(())
}
