//! `earnings_snapshot_save` (finance-earnings-snapshot.md §3.3): a reply
//! records guidance, consensus, a broker's estimate, the reported figures or
//! the call's tone for one results event, checked here against this turn's
//! acknowledged evidence and committed with the reply. Differences are never
//! taken from the model: Rust computes them when the card is read.
use chrono::NaiveDate;

use super::authority::bounded_text;
use super::*;
use crate::knowledge::{
    assets::{self, AssetEvidence},
    earnings::{self, NewRow, Side, Stage, Unit, ValueKind},
    numbers,
};

/// Rows one call may save.
const MAX_ROWS: usize = 12;
const SCALES: &[f64] = &[1.0, 1e3, 1e4, 1e6, 1e8, 1e9, 1e12];

/// One staged save, committed with the reply.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EarningsSave {
    pub(crate) asset_id: String,
    pub(crate) event_id: String,
    pub(crate) rows: Vec<NewRow>,
    pub(crate) tone: Option<(String, Vec<String>, Vec<AssetEvidence>)>,
}

fn printed_decimals(text: &str) -> usize {
    text.split_once('.')
        .map_or(0, |(_, fraction)| fraction.len())
}

/// A value as the model wrote it: a JSON number or a decimal string.
fn value_text(value: Option<&Value>, field: &str) -> Result<Option<(f64, String)>, BrokerError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            let text = number.to_string();
            let parsed = number
                .as_f64()
                .filter(|value| value.is_finite())
                .ok_or_else(|| BrokerError::invalid(field, "a finite number"))?;
            Ok(Some((parsed, text)))
        }
        Some(Value::String(text)) => {
            let trimmed = text.trim().replace(',', "");
            let parsed = trimmed
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite() && value.abs() <= 1e15)
                .ok_or_else(|| BrokerError::invalid(field, "a decimal number such as \"1.27\""))?;
            Ok(Some((parsed, trimmed)))
        }
        Some(_) => Err(BrokerError::invalid(field, "a number")),
    }
}

fn base(value: f64, scale: f64) -> String {
    let text = format!("{}", value * scale);
    text.strip_suffix(".0").map(str::to_owned).unwrap_or(text)
}

impl AppToolExecutor {
    /// `earnings_snapshot_save`: checked against this turn's evidence, staged
    /// until the reply commits.
    pub(super) async fn stage_earnings_save(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        if policy.as_of.is_some() || self.capture.as_of_turn() {
            return Err(BrokerError::invalid(
                "earnings_snapshot_save",
                "earnings_as_of: an as-of answer cannot record results; the host captures late expectations under its own cutoff",
            ));
        }
        if !policy.propose_memory_updates || !policy.use_memory {
            return Err(BrokerError::invalid(
                "earnings_snapshot_save",
                "earnings_private: saving is off in this chat, so results cannot be recorded",
            ));
        }
        let object = exact_object(
            &params,
            &["assetId", "side"],
            &["eventId", "stage", "rows", "tone"],
        )?;
        let asset_id = object
            .get("assetId")
            .and_then(Value::as_str)
            .filter(|id| assets::valid_asset_id(id))
            .ok_or_else(|| BrokerError::invalid("assetId", "the assetId asset_page_read returned"))?
            .to_owned();
        let side = object
            .get("side")
            .and_then(Value::as_str)
            .and_then(Side::parse)
            .ok_or_else(|| BrokerError::invalid("side", "expected, reported or guidance_new"))?;
        let stage = match object.get("stage") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_str()
                    .and_then(Stage::parse)
                    .ok_or_else(|| BrokerError::invalid("stage", "preliminary or final"))?,
            ),
        };
        if stage.is_some() && side != Side::Reported {
            return Err(BrokerError::invalid(
                "stage",
                "only reported rows have a stage",
            ));
        }
        let loupist = policy.loupist_id.clone();
        let requested_event = object
            .get("eventId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let (page, event, existing) =
            crate::commands::assets::with_assets(&self.state, &loupist, false, |tx| {
                let page = assets::get_page(tx, &loupist, &asset_id)?;
                let events = earnings::events_for(tx, &loupist, &asset_id)?;
                let event = match (&requested_event, &page) {
                    (Some(id), _) => events.iter().find(|event| &event.event_id == id).cloned(),
                    (None, Some(page)) => earnings::current_event(
                        &events,
                        earnings::dates::listing_today(&page.market, chrono::Utc::now()),
                    )
                    .cloned(),
                    _ => None,
                };
                let existing = match &event {
                    Some(event) => earnings::rows(tx, &loupist, &event.event_id)?,
                    None => Vec::new(),
                };
                Ok((page, event, existing))
            })
            .map_err(|error| BrokerError::Execution(error.message))?;
        let page = page.ok_or_else(|| {
            BrokerError::invalid("assetId", "earnings_no_page: read the asset page first")
        })?;
        let event = event.ok_or_else(|| {
            BrokerError::invalid(
                "eventId",
                "earnings_no_event: this asset has no results event near today; asset_page_read lists its events",
            )
        })?;
        let today = earnings::dates::listing_today(&page.market, chrono::Utc::now());
        let report_date = event.reported_on();
        let rows = object
            .get("rows")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if rows.len() > MAX_ROWS {
            return Err(BrokerError::invalid("rows", "at most 12 rows in one call"));
        }
        let tone = object.get("tone").filter(|tone| !tone.is_null());
        if rows.is_empty() && tone.is_none() {
            return Err(BrokerError::invalid("rows", "1–12 rows, or a tone"));
        }
        if side == Side::Expected {
            earnings::expected_writable(&event, &existing, today, None).map_err(|code| {
                BrokerError::invalid(
                    "side",
                    format!("{code}: expectations are recorded only before the report date and are locked after it"),
                )
            })?;
        }
        let registry = self.capture.run_results.lock().await;
        let mut staged = Vec::new();
        let mut keys: Vec<(String, String, String, Option<Stage>)> = existing
            .iter()
            .filter(|row| row.side == Side::Reported)
            .map(|row| {
                (
                    row.measure.clone(),
                    row.basis.clone(),
                    row.period.clone(),
                    row.stage,
                )
            })
            .collect();
        for (index, item) in rows.iter().enumerate() {
            let at = |field: &str| format!("rows[{index}].{field}");
            let row = exact_object(
                item,
                &["measure", "unit", "kind", "evidence"],
                &[
                    "basis", "period", "low", "high", "point", "scale", "currency",
                ],
            )?;
            let measure = row
                .get("measure")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|measure| earnings::valid_measure(measure))
                .ok_or_else(|| {
                    BrokerError::invalid(
                        at("measure"),
                        "revenue, operatingIncome, operatingMargin, netIncome, eps, fcf, or label:<your words>",
                    )
                })?
                .to_owned();
            let unit = row
                .get("unit")
                .and_then(Value::as_str)
                .and_then(Unit::parse)
                .ok_or_else(|| {
                    BrokerError::invalid(at("unit"), "currency, percent, per_share or number")
                })?;
            let kind = row
                .get("kind")
                .and_then(Value::as_str)
                .and_then(ValueKind::parse)
                .ok_or_else(|| {
                    BrokerError::invalid(
                        at("kind"),
                        "guidance, consensus, broker_estimate or reported",
                    )
                })?;
            let allowed = match side {
                Side::Expected => matches!(
                    kind,
                    ValueKind::Guidance | ValueKind::Consensus | ValueKind::BrokerEstimate
                ),
                Side::Reported => kind == ValueKind::Reported,
                Side::GuidanceNew => kind == ValueKind::Guidance,
            };
            if !allowed {
                return Err(BrokerError::invalid(
                    at("kind"),
                    if kind == ValueKind::Implied {
                        "earnings_implied_host: the price-implied range comes from the host's reverse DCF".to_owned()
                    } else {
                        format!(
                            "{} rows take kind {}",
                            side.as_str(),
                            match side {
                                Side::Expected => "guidance, consensus or broker_estimate",
                                Side::Reported => "reported",
                                Side::GuidanceNew => "guidance",
                            }
                        )
                    },
                ));
            }
            let basis = optional_bounded_text_value(row.get("basis"), 40, &at("basis"))?;
            let period = optional_bounded_text_value(row.get("period"), 40, &at("period"))?;
            if side == Side::GuidanceNew && period.is_none() {
                return Err(BrokerError::invalid(
                    at("period"),
                    "new guidance names the period it guides, such as \"Q4 2026\"",
                ));
            }
            let period = period.unwrap_or_else(|| event.period.clone());
            let scale = match row.get("scale") {
                None | Some(Value::Null) => 1.0,
                Some(value) => value
                    .as_f64()
                    .filter(|scale| SCALES.iter().any(|known| (known - scale).abs() < 1e-9))
                    .ok_or_else(|| {
                        BrokerError::invalid(at("scale"), "1, 1e3, 1e4, 1e6, 1e8, 1e9 or 1e12")
                    })?,
            };
            if unit == Unit::Percent && scale != 1.0 {
                return Err(BrokerError::invalid(at("scale"), "a percent has no scale"));
            }
            let currency = match row.get("currency") {
                None | Some(Value::Null) => None,
                Some(value) => Some(
                    value
                        .as_str()
                        .map(str::trim)
                        .filter(|code| {
                            code.len() == 3 && code.bytes().all(|b| b.is_ascii_alphabetic())
                        })
                        .ok_or_else(|| {
                            BrokerError::invalid(at("currency"), "an ISO code such as USD or KRW")
                        })?
                        .to_ascii_uppercase(),
                ),
            };
            let low = value_text(row.get("low"), &at("low"))?;
            let high = value_text(row.get("high"), &at("high"))?;
            let point = value_text(row.get("point"), &at("point"))?;
            if point.is_none() && (low.is_none() || high.is_none()) {
                return Err(BrokerError::invalid(
                    at("point"),
                    "a point, or low and high",
                ));
            }
            if let (Some((low, _)), Some((high, _))) = (&low, &high) {
                if low > high {
                    return Err(BrokerError::invalid(at("low"), "low at most high"));
                }
            }
            let urls: Vec<String> = row
                .get("evidence")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            if urls.is_empty() || urls.len() > assets::MAX_EVIDENCE {
                return Err(BrokerError::invalid(
                    at("evidence"),
                    "1–3 citation_url values from this turn",
                ));
            }
            let mut evidence = Vec::new();
            let mut candidates = Vec::new();
            let mut dates: Vec<NaiveDate> = Vec::new();
            for (position, url) in urls.iter().enumerate() {
                let result = evidence::cited_result(&registry, url).map_err(|why| {
                    BrokerError::invalid(at("evidence"), format!("evidence[{position}]: {why}"))
                })?;
                let provider = result
                    .producer
                    .provider
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                match kind {
                    ValueKind::Consensus if !matches!(provider.as_str(), "fmp" | "intrinio") => {
                        return Err(BrokerError::invalid(
                            at("kind"),
                            "earnings_consensus_provider: consensus is recorded only from FMP or Intrinio results; best-effort sources aren't used for expectations",
                        ));
                    }
                    ValueKind::BrokerEstimate if result.producer.tool_name != "kis_read" => {
                        return Err(BrokerError::invalid(
                            at("kind"),
                            "earnings_broker_source: one broker's estimate comes only from a Korea Investment read",
                        ));
                    }
                    _ => {}
                }
                let dated = Self::evidence_day(&registry, result, today)
                    .or_else(|| receipt_day(result, today));
                match (side, dated) {
                    (Side::Expected, Some(day)) if day >= report_date => {
                        return Err(BrokerError::invalid(
                            at("evidence"),
                            format!("earnings_expected_late: evidence[{position}] is dated {day}, on or after the report date {report_date}; expectations must be published before it"),
                        ));
                    }
                    (Side::Reported | Side::GuidanceNew, Some(day)) if day < report_date => {
                        return Err(BrokerError::invalid(
                            at("evidence"),
                            format!("earnings_reported_early: evidence[{position}] is dated {day}, before the report date {report_date}"),
                        ));
                    }
                    _ => {}
                }
                if let Some(day) = dated {
                    dates.push(day);
                }
                candidates.extend(numbers::data_candidates(&result.payload));
                let projected = evidence::project(result);
                evidence.push(AssetEvidence {
                    tool: projected.tool_name,
                    provider: projected.provider,
                    title: projected.title,
                    url: projected.url,
                    dataset: projected.dataset_ref,
                    pointer: None,
                    record_id: None,
                    world_id: None,
                    citation_url: Some(url.clone()),
                    date: dated.map(|day| day.to_string()),
                    retrieved_at: Some(projected.retrieved_at),
                });
            }
            // Reported values need at least one source dated on or after the report date.
            if matches!(side, Side::Reported | Side::GuidanceNew) && dates.is_empty() {
                return Err(BrokerError::invalid(
                    at("evidence"),
                    "earnings_evidence_undated: no cited source carries a date, so none shows the reported results",
                ));
            }
            // Every value must be printed in a cited source (finance-show-the-math.md §7.3).
            let printed_unit = match unit {
                Unit::Percent => numbers::Unit::Percent,
                Unit::Number => numbers::Unit::Plain,
                Unit::Currency | Unit::PerShare => numbers::Unit::Amount(scale),
            };
            for (field, value) in [("low", &low), ("high", &high), ("point", &point)] {
                let Some((number, text)) = value else {
                    continue;
                };
                let printed = numbers::Printed {
                    value: *number,
                    unit: printed_unit,
                    precision: 0.5 * 10f64.powi(-(printed_decimals(text) as i32)),
                };
                let (verdict, _) = numbers::check(&printed, &candidates, false);
                if !matches!(
                    verdict,
                    numbers::Verdict::Matched | numbers::Verdict::Rounded
                ) {
                    return Err(BrokerError::invalid(
                        at(field),
                        format!("earnings_value_not_found: {text} is not printed in a cited source in this unit and scale"),
                    ));
                }
            }
            let stage = (side == Side::Reported).then(|| stage.unwrap_or(Stage::Preliminary));
            let key = (
                measure.clone(),
                basis.clone().unwrap_or_default(),
                period.clone(),
                stage,
            );
            if side == Side::Reported {
                if keys.contains(&key) {
                    return Err(BrokerError::invalid(
                        at("measure"),
                        "earnings_reported_exists: this measure is already recorded for this basis, period and stage",
                    ));
                }
                keys.push(key);
            }
            let fact_date = dates.iter().max().copied().unwrap_or(today);
            staged.push(NewRow {
                side,
                measure,
                basis: basis.unwrap_or_default(),
                period,
                low: low.as_ref().map(|(value, _)| base(*value, scale)),
                high: high.as_ref().map(|(value, _)| base(*value, scale)),
                point: point.as_ref().map(|(value, _)| base(*value, scale)),
                unit,
                currency,
                kind,
                stage,
                provider: (kind == ValueKind::Consensus || kind == ValueKind::BrokerEstimate)
                    .then(|| evidence.first().and_then(|item| item.provider.clone()))
                    .flatten(),
                evidence,
                extra: json!({"printed": {"low": low.map(|(_, text)| text), "high": high.map(|(_, text)| text), "point": point.map(|(_, text)| text), "scale": scale}}),
                fact_date,
                captured_as_of: None,
            });
        }
        let tone = match tone {
            None => None,
            Some(value) => {
                let tone = exact_object(value, &["text", "spans", "evidence"], &[])?;
                let text =
                    bounded_text(tone.get("text"), 1, earnings::MAX_TONE_CHARS).map_err(|_| {
                        BrokerError::invalid("tone.text", "one sentence of at most 200 characters")
                    })?;
                let spans: Vec<String> = tone
                    .get("spans")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                if spans.is_empty()
                    || spans.len() > 2
                    || spans
                        .iter()
                        .any(|span| span.trim().is_empty() || !text.contains(span.as_str()))
                {
                    return Err(BrokerError::invalid(
                        "tone.spans",
                        "1–2 exact quotes that appear in the tone sentence",
                    ));
                }
                let urls: Vec<String> = tone
                    .get("evidence")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                if urls.is_empty() || urls.len() > assets::MAX_EVIDENCE {
                    return Err(BrokerError::invalid(
                        "tone.evidence",
                        "1–3 citation_url values from this turn",
                    ));
                }
                let mut evidence = Vec::new();
                let mut texts = String::new();
                for url in &urls {
                    let result = evidence::cited_result(&registry, url)
                        .map_err(|why| BrokerError::invalid("tone.evidence", why))?;
                    // Tone comes only from a call transcript, read or retained as a Source.
                    let transcript = result.producer.tool_name.contains("transcript")
                        || (result.producer.origin == RunResultOrigin::SavedContext
                            && result.producer.tool_name == "world_read");
                    if !transcript {
                        return Err(BrokerError::invalid(
                            "tone.evidence",
                            "earnings_tone_source: the call's tone comes only from a transcript (FMP) or one the user retained as a Source",
                        ));
                    }
                    collect_text(&result.payload, &mut texts);
                    let projected = evidence::project(result);
                    evidence.push(AssetEvidence {
                        tool: projected.tool_name,
                        provider: projected.provider,
                        title: projected.title,
                        url: projected.url,
                        dataset: projected.dataset_ref,
                        pointer: None,
                        record_id: None,
                        world_id: None,
                        citation_url: Some(url.clone()),
                        date: None,
                        retrieved_at: Some(projected.retrieved_at),
                    });
                }
                if spans.iter().any(|span| !texts.contains(span.as_str())) {
                    return Err(BrokerError::invalid(
                        "tone.spans",
                        "earnings_tone_span: each quote must appear exactly in the cited transcript",
                    ));
                }
                Some((text, spans, evidence))
            }
        };
        drop(registry);
        let save = EarningsSave {
            asset_id: asset_id.clone(),
            event_id: event.event_id.clone(),
            rows: staged,
            tone,
        };
        let count = save.rows.len();
        self.capture
            .stage(delivery_id, move |pending| pending.earnings.push(save))
            .await;
        Ok(json!({
            "status": "staged",
            "assetId": asset_id,
            "eventId": event.event_id,
            "period": event.period,
            "rows": count,
            "note": "Checked against the evidence now; saved when this reply finishes. The app computes every difference and revision itself and shows them on the results card; describe them from asset_page_read, never compute your own."
        }))
    }
}

/// An OpenDART receipt number starts with its filing date (YYYYMMDD).
fn receipt_day(result: &RunResult, today: NaiveDate) -> Option<NaiveDate> {
    if !result.producer.tool_name.starts_with("dart_") {
        return None;
    }
    [
        "/query/rcept_no",
        "/data/filing/filing_id",
        "/filing/filing_id",
    ]
    .iter()
    .find_map(|pointer| result.payload.pointer(pointer).and_then(Value::as_str))
    .and_then(|id| id.get(..8))
    .and_then(|digits| NaiveDate::parse_from_str(digits, "%Y%m%d").ok())
    .filter(|day| *day <= today)
}

fn optional_bounded_text_value(
    value: Option<&Value>,
    max: usize,
    field: &str,
) -> Result<Option<String>, BrokerError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text))
            if !text.trim().is_empty()
                && text.chars().count() <= max
                && !text.chars().any(char::is_control) =>
        {
            Ok(Some(text.trim().to_owned()))
        }
        Some(_) => Err(BrokerError::invalid(
            field,
            format!("text of at most {max} characters"),
        )),
    }
}

/// Every string in a payload, for finding exact quotes.
fn collect_text(value: &Value, out: &mut String) {
    match value {
        Value::String(text) => {
            if out.len() < 4 * 1024 * 1024 {
                out.push_str(text);
                out.push('\n');
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect_text(item, out)),
        Value::Object(map) => map.values().for_each(|item| collect_text(item, out)),
        _ => {}
    }
}

/// Commits a reply's staged saves in its transaction. A save the event no
/// longer accepts (expectations locked meanwhile, a duplicate reported row)
/// is left out; the rest are stored with the reply as their origin.
pub(crate) fn commit_turn(
    tx: &rusqlite::Transaction<'_>,
    loupist_id: &str,
    message_id: &str,
    at_ms: i64,
    saves: &[EarningsSave],
    unattended: bool,
) -> crate::knowledge::KnowledgeResult<usize> {
    let origin = if unattended {
        earnings::Origin::Briefing
    } else {
        earnings::Origin::Chat
    };
    let mut stored = 0;
    for save in saves {
        let Some(event) = earnings::get_event(tx, loupist_id, &save.event_id)? else {
            continue;
        };
        let Some(page) = assets::get_page(tx, loupist_id, &save.asset_id)? else {
            continue;
        };
        let today = earnings::dates::listing_today(&page.market, chrono::Utc::now());
        let existing = earnings::rows(tx, loupist_id, &event.event_id)?;
        let rows: Vec<NewRow> = save
            .rows
            .iter()
            .filter(|row| {
                row.side != Side::Expected
                    || earnings::expected_writable(&event, &existing, today, None).is_ok()
            })
            .filter(|row| {
                row.side != Side::Reported
                    || !existing.iter().any(|known| {
                        known.side == Side::Reported
                            && known.measure == row.measure
                            && known.basis == row.basis
                            && known.period == row.period
                            && known.stage == row.stage
                    })
            })
            .cloned()
            .collect();
        stored += earnings::insert_rows(
            tx,
            loupist_id,
            &event.event_id,
            &rows,
            origin,
            Some(message_id),
            at_ms,
        )?;
        if let Some((text, spans, evidence)) = &save.tone {
            earnings::set_tone(
                tx,
                loupist_id,
                &event.event_id,
                text,
                spans,
                evidence,
                at_ms,
            )?;
        }
    }
    Ok(stored)
}
