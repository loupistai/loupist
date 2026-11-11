//! Grid cards at finalization (finance-korea-retail-disagreement.md §2.2). A
//! paragraph of the answer that is only a link to a retained `dcf_grid` or
//! `reverse_dcf` result becomes a card built from the host's own result:
//! the grid, the reference growth it is read against, the price and inputs
//! with their sources. The answer's text never supplies a value.
use super::*;
use crate::domain::{
    answer_checks::clip, ChatEvidenceRef, NumberCheckStatus, SensitivityCard, SensitivityGrid,
    SensitivityInput, SensitivityPrice, SensitivityReference, MAX_SENSITIVITY_CARDS,
};
use crate::knowledge::numbers::{self, Place};
use number_checks::EvidenceLink;

const MAX_SUBJECT_CHARS: usize = 80;
const MAX_FORMULAS: usize = 8;
const MAX_INPUTS: usize = 12;
/// Row fields that date a price row (KRX, KIS, OpenBB).
const DATE_FIELDS: &[&str] = &[
    "date",
    "stck_bsop_date",
    "bsop_date",
    "trade_date",
    "TRD_DD",
    "BAS_DD",
];

/// Links that are the only content of a top-level paragraph, with their
/// index among all evidence links (the renderer replaces that paragraph).
pub(in crate::commands) fn paragraph_links(content: &str) -> Vec<EvidenceLink> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    let mut out = Vec::new();
    let mut index = 0_u32;
    let mut nested = 0_usize;
    // (links in this paragraph, other visible text)
    let mut paragraph: Option<(Vec<EvidenceLink>, bool)> = None;
    let mut open: Option<(String, String)> = None;
    for event in Parser::new(content) {
        match event {
            Event::Start(
                Tag::Item | Tag::BlockQuote(_) | Tag::Table(_) | Tag::FootnoteDefinition(_),
            ) => nested += 1,
            Event::End(
                TagEnd::Item | TagEnd::BlockQuote(_) | TagEnd::Table | TagEnd::FootnoteDefinition,
            ) => nested = nested.saturating_sub(1),
            Event::Start(Tag::Paragraph) => paragraph = Some((Vec::new(), nested > 0)),
            Event::End(TagEnd::Paragraph) => {
                if let Some((mut links, other)) = paragraph.take() {
                    if !other && links.len() == 1 {
                        out.push(links.remove(0));
                    }
                }
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                let id = dest_url
                    .strip_prefix("#evidence-")
                    .filter(|id| {
                        !id.is_empty()
                            && id.len() <= 128
                            && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                    })
                    .map(str::to_owned);
                match id {
                    Some(id) => open = Some((id, String::new())),
                    None => {
                        if let Some((_, other)) = paragraph.as_mut() {
                            *other = true;
                        }
                    }
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some((evidence_id, text)) = open.take() {
                    let link = EvidenceLink {
                        index,
                        text,
                        evidence_id,
                    };
                    index += 1;
                    if let Some((links, _)) = paragraph.as_mut() {
                        links.push(link);
                    }
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some((_, label)) = open.as_mut() {
                    label.push_str(&text);
                } else if !text.trim().is_empty() {
                    if let Some((_, other)) = paragraph.as_mut() {
                        *other = true;
                    }
                }
            }
            Event::InlineHtml(_) | Event::Html(_) | Event::FootnoteReference(_) => {
                if let Some((_, other)) = paragraph.as_mut() {
                    *other = true;
                }
            }
            _ => {}
        }
    }
    out
}

/// Where a calculation input came from: the evidence it was cited from or
/// read out of, and Rust's check of a cited value.
pub(in crate::commands) fn input_source(
    input: &Value,
    registry: &RunResultRegistry,
) -> (Option<String>, Option<NumberCheckStatus>) {
    let provenance = input.get("provenance").unwrap_or(&Value::Null);
    match provenance.get("kind").and_then(Value::as_str) {
        Some("cited") => (
            provenance
                .get("evidence_id")
                .and_then(Value::as_str)
                .map(str::to_owned),
            provenance
                .get("status")
                .cloned()
                .and_then(|status| serde_json::from_value(status).ok()),
        ),
        Some("dataset") => (
            registry
                .values()
                .find(|result| {
                    !matches!(
                        result.producer.tool_name.as_str(),
                        "dataset_query" | "dataset_describe" | "calculate"
                    ) && result.payload.get("dataset_ref").is_some_and(|reference| {
                        reference.get("dataset_id") == provenance.get("dataset_id")
                            && reference.get("revision") == provenance.get("revision")
                    })
                })
                .map(|result| evidence::evidence_id(&result.result_ref)),
            None,
        ),
        _ => (None, None),
    }
}

fn by_evidence<'a>(registry: &'a RunResultRegistry, evidence_id: &str) -> Option<&'a RunResult> {
    registry
        .values()
        .find(|result| evidence::evidence_id(&result.result_ref) == evidence_id)
}

fn calendar_day(text: &str) -> Option<String> {
    let text = text.trim();
    // `get` because byte 10 of provider text can fall inside a character.
    let day = text
        .get(..10)
        .and_then(|prefix| chrono::NaiveDate::parse_from_str(prefix, "%Y-%m-%d").ok())
        .or_else(|| {
            (text.len() == 8)
                .then(|| chrono::NaiveDate::parse_from_str(text, "%Y%m%d").ok())
                .flatten()
        })?;
    Some(day.to_string())
}

/// The date of the row a cited price was read from, else the evidence's
/// publication date.
fn price_date(
    price: f64,
    result: &RunResult,
    evidence: Option<&ChatEvidenceRef>,
) -> Option<String> {
    let tolerance = 0.005_f64.max(price.abs() * 1e-9);
    let row_date = numbers::data_candidates(&result.payload)
        .into_iter()
        .filter(|candidate| (candidate.value - price).abs() <= tolerance)
        .find_map(|candidate| {
            let Place::Pointer(pointer) = candidate.place else {
                return None;
            };
            let (parent, _) = pointer.rsplit_once('/')?;
            let row = result.payload.pointer(parent)?;
            DATE_FIELDS
                .iter()
                .find_map(|key| row.get(*key).and_then(Value::as_str).and_then(calendar_day))
        });
    row_date.or_else(|| {
        evidence
            .and_then(|evidence| evidence.published_at.as_deref())
            .and_then(calendar_day)
    })
}

/// The years a cited `cagr` result spans.
fn reference_years(result: &RunResult) -> Option<u32> {
    if result.producer.tool_name != "calculate" || result.payload["operation"] != "cagr" {
        return None;
    }
    let years = result.payload["inputs"]
        .as_array()?
        .iter()
        .find(|input| input["name"] == "years")?["value"]
        .as_f64()?;
    ((1.0..=200.0).contains(&years) && years.fract() == 0.0).then_some(years as u32)
}

fn number_list(value: &Value) -> Option<Vec<f64>> {
    value.as_array()?.iter().map(Value::as_f64).collect()
}

/// A grid from one retained `dcf_grid` or `reverse_dcf` result. Inputs carry
/// evidence ids only when `registry` holds the results they were cited from.
pub(in crate::commands) fn sensitivity_grid(
    payload: &Value,
    registry: &RunResultRegistry,
    evidence: &[ChatEvidenceRef],
) -> Option<SensitivityGrid> {
    let operation = payload.get("operation").and_then(Value::as_str)?;
    if !matches!(operation, "dcf_grid" | "reverse_dcf") {
        return None;
    }
    let grid = payload.get("grid")?;
    let row_values = number_list(&grid["row_values"])?;
    let column_values = number_list(&grid["column_values"])?;
    let values = grid["values"]
        .as_array()?
        .iter()
        .map(|row| {
            row.as_array()
                .map(|cells| cells.iter().map(Value::as_f64).collect::<Vec<_>>())
        })
        .collect::<Option<Vec<_>>>()?;
    let above_reference = grid.get("above_reference").and_then(|rows| {
        rows.as_array()?
            .iter()
            .map(|row| {
                row.as_array().map(|cells| {
                    cells
                        .iter()
                        .map(|cell| cell.as_bool() == Some(true))
                        .collect()
                })
            })
            .collect::<Option<Vec<Vec<bool>>>>()
    });
    let amount_unit = grid
        .get("amount_unit")
        .and_then(Value::as_str)
        .map(|unit| clip(unit, 64));
    let currency = amount_unit.as_deref().and_then(|unit| {
        let code = unit.split_whitespace().next()?;
        (code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase())).then(|| code.to_owned())
    });
    let inputs = payload.get("inputs").and_then(Value::as_array)?;
    let evidence_of = |id: &Option<String>| {
        id.as_deref()
            .and_then(|id| evidence.iter().find(|item| item.id == id))
    };
    let mut price = None;
    let mut reference = None;
    let mut listed = Vec::new();
    for input in inputs {
        let name = input
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let value = input.get("value").and_then(Value::as_f64);
        let display = clip(
            input
                .get("display")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            128,
        );
        let label = input
            .get("label")
            .and_then(Value::as_str)
            .map(|label| clip(label, 128));
        let (evidence_id, status) = input_source(input, registry);
        match (name, value) {
            ("price", Some(value)) => {
                let date = evidence_id
                    .as_deref()
                    .and_then(|id| by_evidence(registry, id))
                    .and_then(|result| price_date(value, result, evidence_of(&evidence_id)));
                price = Some(SensitivityPrice {
                    value,
                    display: clip(&display, 64),
                    currency: currency.clone(),
                    date,
                    evidence_id,
                    status,
                });
            }
            ("reference_growth_pct", Some(value)) => {
                let years = evidence_id
                    .as_deref()
                    .and_then(|id| by_evidence(registry, id))
                    .and_then(reference_years);
                reference = Some(SensitivityReference {
                    value_pct: value,
                    // The period the reference covers ("FY2020–FY2025"), as labelled.
                    label: input
                        .get("period")
                        .and_then(Value::as_str)
                        .map(|period| clip(period, 128)),
                    years,
                    evidence_id,
                    status,
                });
            }
            _ => listed.push(SensitivityInput {
                name: clip(name, 128),
                label,
                display,
                evidence_id,
                status,
            }),
        }
    }
    listed.truncate(MAX_INPUTS);
    let formulas = payload
        .get("formulas")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .take(MAX_FORMULAS)
        .map(|line| clip(line, 2048))
        .collect();
    let grid = SensitivityGrid {
        operation: operation.to_owned(),
        row_values,
        column_values,
        values,
        unit: clip(
            grid.get("unit").and_then(Value::as_str).unwrap_or_default(),
            64,
        ),
        forecast_years: grid
            .get("forecast_years")
            .and_then(Value::as_u64)
            .and_then(|years| u32::try_from(years).ok())
            .unwrap_or(5),
        reference: if operation == "reverse_dcf" {
            reference
        } else {
            None
        },
        above_reference: if operation == "reverse_dcf" {
            above_reference
        } else {
            None
        },
        price: if operation == "reverse_dcf" {
            price
        } else {
            None
        },
        amount_unit,
        inputs: listed,
        formulas,
    };
    grid.valid().then_some(grid)
}

impl ToolCapture {
    /// A card for each paragraph that is only a link to a grid this turn
    /// computed, in answer order.
    pub(in crate::commands) async fn sensitivity_cards(
        &self,
        content: &str,
        evidence: &[ChatEvidenceRef],
    ) -> Vec<SensitivityCard> {
        let links = paragraph_links(content);
        if links.is_empty() {
            return Vec::new();
        }
        let registry = self.run_results.lock().await;
        let mut cards: Vec<SensitivityCard> = Vec::new();
        for link in links {
            if cards.len() >= MAX_SENSITIVITY_CARDS {
                break;
            }
            let Some(result) = by_evidence(&registry, &link.evidence_id) else {
                continue;
            };
            if result.producer.tool_name != "calculate" {
                continue;
            }
            let subject: String = link
                .text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(MAX_SUBJECT_CHARS)
                .collect();
            if subject.is_empty() {
                continue;
            }
            if let Some(grid) = sensitivity_grid(&result.payload, &registry, evidence) {
                cards.push(SensitivityCard {
                    grid,
                    link_index: link.index,
                    evidence_id: link.evidence_id,
                    subject,
                });
            }
        }
        cards
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_paragraph_that_is_one_evidence_link_becomes_a_card() {
        let content = "Margin was [8.4%](#evidence-ev_a).\n\n[Hanul Precision](#evidence-ev_grid)\n\n- [Inside a list](#evidence-ev_list)\n\n> [Quoted](#evidence-ev_quote)\n\n[Two](#evidence-ev_b) [links](#evidence-ev_c)\n\n  [Solsup Foods](#evidence-ev_second)  \n";
        let links = paragraph_links(content);
        let found: Vec<_> = links
            .iter()
            .map(|link| (link.index, link.evidence_id.as_str(), link.text.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                (1, "ev_grid", "Hanul Precision"),
                (6, "ev_second", "Solsup Foods")
            ]
        );
    }

    fn calculate(payload: Value) -> RunResult {
        RunResult {
            result_ref: "result:calc".into(),
            producer: RunResultProducer {
                origin: RunResultOrigin::Derived,
                runtime_id: "host-calculation".into(),
                tool_name: "calculate".into(),
                provider: None,
            },
            origin: None,
            request_digest: "a".repeat(64),
            response_digest: "b".repeat(64),
            retrieved_at: "2026-10-31T00:00:00Z".into(),
            payload,
            model_delivery: None,
            warnings: vec![],
            upstream_result_refs: vec![],
        }
    }

    #[test]
    fn a_reverse_grid_keeps_its_reference_price_and_inputs_from_the_host_result() {
        let mut datasets = NoDatasets;
        let payload = crate::calculations::calculate(
            &json!({"operation": "reverse_dcf", "wacc_pct": [8, 10, 12], "terminal_growth_pct": [2, 3],
                "inputs": {"cash_flow": 412, "price": 41.2, "shares": 98.2, "net_debt": 310, "reference_growth_pct": 8},
                "labels": {"cash_flow": "free cash flow FY2025", "reference_growth_pct": "FY2020–FY2025"},
                "unit": "USD millions"}),
            &mut datasets,
        )
        .unwrap();
        let grid = sensitivity_grid(&payload, &RunResultRegistry::default(), &[]).unwrap();
        assert_eq!(grid.operation, "reverse_dcf");
        assert_eq!(grid.row_values, [8.0, 10.0, 12.0]);
        assert_eq!(grid.values.len(), 3);
        assert_eq!(grid.forecast_years, 5);
        assert_eq!(grid.reference.as_ref().unwrap().value_pct, 8.0);
        assert_eq!(
            grid.reference.as_ref().unwrap().label.as_deref(),
            Some("FY2020–FY2025")
        );
        assert_eq!(grid.above_reference.as_ref().unwrap().len(), 3);
        let price = grid.price.as_ref().unwrap();
        assert_eq!(price.value, 41.2);
        assert_eq!(price.currency.as_deref(), Some("USD"));
        let names: Vec<_> = grid
            .inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect();
        assert_eq!(names, ["cash_flow", "shares", "net_debt"]);
        assert_eq!(
            grid.inputs[0].label.as_deref(),
            Some("free cash flow FY2025")
        );
        // A value grid has no reference, shading or price.
        let value = crate::calculations::calculate(
            &json!({"operation": "dcf_grid", "wacc_pct": [9], "terminal_growth_pct": [2],
                "inputs": {"cash_flow": 100, "growth_pct": 4, "shares": 10}}),
            &mut datasets,
        )
        .unwrap();
        let grid = sensitivity_grid(&value, &RunResultRegistry::default(), &[]).unwrap();
        assert!(grid.reference.is_none() && grid.above_reference.is_none() && grid.price.is_none());
        assert_eq!(grid.unit, "per share");
        // Other calculations are not grids.
        let ratio = crate::calculations::calculate(
            &json!({"operation": "ratio", "inputs": {"numerator": 1, "denominator": 2}}),
            &mut datasets,
        )
        .unwrap();
        assert!(sensitivity_grid(&ratio, &RunResultRegistry::default(), &[]).is_none());
    }

    #[tokio::test]
    async fn cards_come_from_the_retained_grid_with_the_dated_price_row() {
        let quote = RunResult {
            result_ref: "result:quote".into(),
            producer: RunResultProducer {
                origin: RunResultOrigin::AcquiredOriginal,
                runtime_id: "native-finance".into(),
                tool_name: "krx_daily_quote".into(),
                provider: Some("krx.market-data".into()),
            },
            payload: json!({"dataset_ref": {"dataset_id": "ds_quote", "revision": 1}, "data": {"bars": [
                {"date": "2026-10-30", "close": "40.1"},
                {"date": "2026-10-31", "close": "41.2"}
            ]}}),
            ..calculate(Value::Null)
        };
        let quote_id = evidence::evidence_id("result:quote");
        let mut payload = crate::calculations::calculate(
            &json!({"operation": "reverse_dcf", "wacc_pct": [9, 10], "terminal_growth_pct": [2, 3],
                "inputs": {"cash_flow": 412, "price": 41.2, "shares": 98.2}}),
            &mut NoDatasets,
        )
        .unwrap();
        for input in payload["inputs"].as_array_mut().unwrap() {
            if input["name"] == "price" {
                input["provenance"] =
                    json!({"kind": "cited", "evidence_id": quote_id, "status": "matched"});
            }
        }
        let capture = ToolCapture::default();
        {
            let mut registry = capture.run_results.lock().await;
            registry.insert(quote).unwrap();
            registry.insert(calculate(payload)).unwrap();
        }
        let calc_id = evidence::evidence_id("result:calc");
        let content = format!(
            "The close was [41.20](#evidence-{quote_id}).\n\n[Hanul Precision](#evidence-{calc_id})\n\n[The quote](#evidence-{quote_id})\n"
        );
        let cards = capture.sensitivity_cards(&content, &[]).await;
        assert_eq!(cards.len(), 1, "only the grid link makes a card");
        let card = &cards[0];
        assert_eq!(card.link_index, 1);
        assert_eq!(card.subject, "Hanul Precision");
        assert_eq!(card.evidence_id, calc_id);
        let price = card.grid.price.as_ref().unwrap();
        assert_eq!(price.date.as_deref(), Some("2026-10-31"));
        assert_eq!(price.evidence_id.as_deref(), Some(quote_id.as_str()));
        assert_eq!(price.status, Some(NumberCheckStatus::Matched));
        crate::domain::answer_checks::validate_sensitivity_cards(&cards).unwrap();
    }

    struct NoDatasets;

    impl crate::calculations::DatasetSource for NoDatasets {
        fn dataset(
            &mut self,
            _: &str,
            _: u32,
        ) -> Result<(crate::datasets::DatasetRef, Arc<Value>), String> {
            Err("no datasets".into())
        }
    }
}
