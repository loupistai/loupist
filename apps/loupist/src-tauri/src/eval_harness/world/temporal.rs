use super::{AppState, Result, Scenario};
use crate::knowledge::lens_page::{
    Basis, BasisRef, FilingOrigin, LensOp, PageDelta, PageKind, PageTarget, SectionKey,
};
use crate::knowledge::{GraphClaim, GraphNode, KnowledgeStore, WorldGraphQuery};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PreFile {
    pub(super) after_step: String,
    source_id: String,
    claim: ClaimSelector,
    title: String,
    summary: String,
    section: Section,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Section {
    key: SectionKey,
    heading: String,
    markdown: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ClaimSelector {
    between: Option<[String; 2]>,
    proposition: Option<String>,
}

impl ClaimSelector {
    fn regex(pattern: &str) -> Result<Regex> {
        Ok(RegexBuilder::new(pattern).case_insensitive(true).build()?)
    }

    fn matches(&self, claim: &GraphClaim, nodes: &BTreeMap<String, String>) -> Result<bool> {
        if let Some(pattern) = &self.proposition {
            if !Self::regex(pattern)?.is_match(&claim.proposition) {
                return Ok(false);
            }
        }
        if let Some([left, right]) = &self.between {
            let (left, right) = (Self::regex(left)?, Self::regex(right)?);
            let [a, b] = claim.endpoints.as_slice() else {
                return Ok(false);
            };
            let (Some(a), Some(b)) = (nodes.get(&a.node_id), nodes.get(&b.node_id)) else {
                return Ok(false);
            };
            return Ok(
                (left.is_match(a) && right.is_match(b)) || (left.is_match(b) && right.is_match(a))
            );
        }
        Ok(true)
    }
}

pub(super) fn validate(scenario: &Scenario) -> Result<()> {
    if let Some(filing) = &scenario.pre_file {
        let step = scenario
            .steps
            .iter()
            .find(|step| step.id == filing.after_step)
            .ok_or("temporal pre-file step is absent")?;
        if !step
            .sources
            .iter()
            .any(|source| source.id == filing.source_id)
            || filing.summary.trim().is_empty()
            || (filing.claim.between.is_none() && filing.claim.proposition.is_none())
        {
            return Err(
                "temporal pre-file requires a Source in its step, a summary and a claim selector"
                    .into(),
            );
        }
        for pattern in filing
            .claim
            .between
            .iter()
            .flatten()
            .chain(&filing.claim.proposition)
        {
            ClaimSelector::regex(pattern)?;
        }
    }
    for step in &scenario.steps {
        for day in &step.as_of_reads {
            let date = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d")?;
            if date.format("%Y-%m-%d").to_string() != *day {
                return Err("temporal read requires a canonical cutoff date".into());
            }
        }
    }
    Ok(())
}

fn world(snapshot: &Value) -> Result<&Value> {
    snapshot["records"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|row| row["record"]["kind"] == "world")
        .ok_or_else(|| "World snapshot is absent".into())
}

pub(super) fn file(
    state: &AppState,
    owner: &str,
    fixture: &PreFile,
    ingestion: &Value,
    snapshot: &Value,
) -> Result<Value> {
    let saved = ingestion
        .as_array()
        .into_iter()
        .flatten()
        .find(|row| row["fixture_id"] == fixture.source_id)
        .ok_or("pre-file Source was not ingested")?;
    let source: crate::knowledge::RetainedSource = serde_json::from_value(saved["saved"].clone())?;
    let span = source.span_id.ok_or("pre-file Source lacks a span")?;
    let payload = &world(snapshot)?["snapshot"]["payload"];
    let nodes: Vec<GraphNode> = serde_json::from_value(payload["nodes"].clone())?;
    let nodes: BTreeMap<_, _> = nodes.into_iter().map(|node| (node.id, node.name)).collect();
    let claims: Vec<GraphClaim> = serde_json::from_value(payload["claims"].clone())?;
    let mut matched = Vec::new();
    for claim in claims {
        if claim.evidence.iter().any(|item| item.span_id == span)
            && fixture.claim.matches(&claim, &nodes)?
        {
            matched.push(claim.id);
        }
    }
    let [claim] = matched.as_slice() else {
        return Err(format!("pre-file selector matched {} claims", matched.len()).into());
    };
    let store = KnowledgeStore::for_loupist(state.store.clone(), owner)?;
    let receipt = store.transact(|write| {
        write.file(
            owner,
            &FilingOrigin::User {
                chat_id: None,
                message_id: None,
            },
            vec![PageDelta {
                target: PageTarget::Create {
                    kind: PageKind::Subject,
                    title: fixture.title.clone(),
                    anchors: Vec::new(),
                },
                ops: vec![
                    LensOp::Summary {
                        text: fixture.summary.clone(),
                    },
                    LensOp::Put {
                        key: fixture.section.key.clone(),
                        heading: fixture.section.heading.clone(),
                        markdown: fixture.section.markdown.clone(),
                        basis: Basis {
                            refs: vec![
                                BasisRef::Claim {
                                    claim_id: claim.clone(),
                                },
                                BasisRef::Span {
                                    record_id: source.record_id.clone(),
                                    revision_digest: source.revision_digest.clone(),
                                    span_id: span.clone(),
                                },
                            ],
                            uncertainty: None,
                        },
                        expected: None,
                    },
                ],
            }],
            chrono::Utc::now().timestamp_millis(),
        )
    })?;
    if receipt.len() != 1
        || !receipt[0].created
        || !receipt[0]
            .outcomes
            .iter()
            .all(|item| item.outcome.changed())
    {
        return Err("pre-file lens was not stored".into());
    }
    Ok(json!({"record_id": receipt[0].record_id, "claim_id": claim, "source_span_id": span}))
}

pub(super) fn cutoff_views(
    state: &AppState,
    owner: &str,
    days: &[String],
    snapshot: &Value,
) -> Result<Value> {
    let world = world(snapshot)?;
    let mut views = Map::new();
    for day in days {
        let mut cursor = None;
        let mut edges = Vec::new();
        loop {
            let query: WorldGraphQuery = serde_json::from_value(json!({
                "loupistId": owner, "recordId": world["record"]["recordId"],
                "revisionId": world["record"]["revisionId"], "asOf": day,
                "cursor": cursor,
            }))?;
            let result = crate::commands::knowledge::world_query(state, query)
                .map_err(|err| format!("World cutoff read failed: {err:?}"))?;
            edges.extend(result.view.edges);
            cursor = result.view.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        views.insert(day.clone(), json!({"edges": edges}));
    }
    Ok(Value::Object(views))
}

#[cfg(test)]
mod tests {
    #[test]
    fn v3_corpus_keeps_the_backfill_orders_lens_filing_and_cutoffs() {
        let suite: super::super::Suite = serde_json::from_str(include_str!(
            "../../../../evals/world/temporal-corpus-v3.json"
        ))
        .unwrap();
        super::super::validate(&suite, &super::super::Options::default()).unwrap();
        assert_eq!(suite.scenarios.len(), 12);
        let orders: std::collections::BTreeSet<_> = suite
            .scenarios
            .iter()
            .filter(|scenario| scenario.id.contains("DUP"))
            .map(|scenario| {
                scenario
                    .steps
                    .iter()
                    .flat_map(|step| step.sources.iter().map(|source| &source.id))
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(orders.len(), 3);
        assert_eq!(
            suite
                .scenarios
                .iter()
                .filter(|scenario| scenario.pre_file.is_some())
                .count(),
            9
        );
        assert_eq!(
            suite
                .scenarios
                .iter()
                .flat_map(|scenario| &scenario.steps)
                .flat_map(|step| &step.as_of_reads)
                .count(),
            4
        );
    }
}
