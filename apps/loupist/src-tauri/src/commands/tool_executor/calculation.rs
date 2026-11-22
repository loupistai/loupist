use super::*;
use crate::domain::NumberCheckStatus;
use crate::knowledge::numbers::{self, Place, Unit, Verdict};
use crate::{calculations, datasets::DatasetRef};

/// Dataset operands for one `calculate` call: this Loupist's revisions only,
/// each read once and checked against its stored digest and, under an as-of
/// cutoff, against the cutoff it was fetched under.
struct PinnedDatasets<'a> {
    store: &'a crate::store::SqliteStore,
    loupist_id: &'a str,
    cutoff: Option<chrono::NaiveDate>,
    refused: Option<BrokerError>,
    read: BTreeMap<(String, u32), (DatasetRef, Arc<Value>)>,
}

impl PinnedDatasets<'_> {
    fn cell_currency(&self, provenance: &Value) -> Option<&str> {
        let dataset_id = provenance["dataset_id"].as_str()?.to_owned();
        let revision = u32::try_from(provenance["revision"].as_u64()?).ok()?;
        let pointer = provenance["pointer"].as_str()?;
        let (_, payload) = self.read.get(&(dataset_id, revision))?;
        amount_currency(payload, pointer)
    }
}

impl calculations::DatasetSource for PinnedDatasets<'_> {
    fn dataset(
        &mut self,
        dataset_id: &str,
        revision: u32,
    ) -> Result<(DatasetRef, Arc<Value>), String> {
        let key = (dataset_id.to_owned(), revision);
        if let Some(found) = self.read.get(&key) {
            return Ok(found.clone());
        }
        let (reference, payload, metadata) = self
            .store
            .dataset(self.loupist_id, dataset_id, revision)
            .map_err(command_message)?;
        if let Err(refused) =
            super::datasets::ensure_within_cutoff(dataset_id, revision, &metadata, self.cutoff)
        {
            let message = refused.to_string();
            self.refused = Some(refused);
            return Err(message);
        }
        let found = (reference, Arc::new(payload));
        self.read.insert(key, found.clone());
        Ok(found)
    }
}

/// Cited inputs at most per calculation.
const MAX_INPUT_SOURCES: usize = 16;

/// Removes `input_sources` (`{ <input name>: <citation_url> }`) from the
/// request; each name must be one of the request's inputs.
fn take_input_sources(params: &mut Value) -> Result<BTreeMap<String, String>, BrokerError> {
    let Some(object) = params.as_object_mut() else {
        return Ok(BTreeMap::new());
    };
    let Some(sources) = object.remove("input_sources") else {
        return Ok(BTreeMap::new());
    };
    let sources = sources
        .as_object()
        .filter(|sources| sources.len() <= MAX_INPUT_SOURCES)
        .ok_or_else(|| {
            BrokerError::invalid(
                "input_sources",
                format!("an object of at most {MAX_INPUT_SOURCES} input names, each mapped to a citation_url from this turn"),
            )
        })?;
    let inputs = object.get("inputs").and_then(Value::as_object);
    let mut cited = BTreeMap::new();
    for (name, url) in sources {
        let field = format!("input_sources.{name}");
        if !inputs.is_some_and(|inputs| inputs.contains_key(name)) {
            return Err(BrokerError::invalid(
                field,
                "not an input of this calculation",
            ));
        }
        let url = url
            .as_str()
            .filter(|url| url.starts_with("#evidence-") && url.len() <= 256)
            .ok_or_else(|| {
                BrokerError::invalid(field.clone(), "not a citation_url from this turn")
            })?;
        cited.insert(name.clone(), url.to_owned());
    }
    Ok(cited)
}

/// One way a cited input can be printed in its evidence.
#[derive(Clone, Copy, Debug, PartialEq)]
enum InputReading {
    In(Unit),
    /// An amount the request gives no `unit`: the number as the evidence
    /// prints it, in the evidence's own scale (`52` for `52억원`).
    AsPrinted,
}

/// The readings of a cited input, by what it measures
/// ([`calculations::input_kind`], which also words the input for readers): a
/// `price` is a per-share price in base currency units, printed with or
/// without its currency; an amount is in the request's `unit`, or as printed
/// when the request states none.
fn input_units(name: &str, amount: Option<Unit>) -> Vec<InputReading> {
    use InputReading::{AsPrinted, In};
    match calculations::input_kind(name) {
        calculations::InputKind::Count | calculations::InputKind::Rate => vec![In(Unit::Plain)],
        calculations::InputKind::Price => vec![In(Unit::Amount(1.0)), In(Unit::Plain)],
        calculations::InputKind::Percent => vec![In(Unit::Percent)],
        calculations::InputKind::Amount => vec![amount.map_or(AsPrinted, In)],
    }
}

/// How a cited input stands against its evidence, in each of its readings.
fn cited_status(
    value: Option<&Value>,
    readings: &[InputReading],
    payload: &Value,
) -> (NumberCheckStatus, Option<String>) {
    let candidates = numbers::data_candidates(payload);
    let verdicts: Vec<_> = readings
        .iter()
        .filter_map(|reading| match reading {
            InputReading::In(unit) => {
                let printed = printed_input(value?, *unit, false)?;
                Some(numbers::check(&printed, &candidates, false))
            }
            InputReading::AsPrinted => {
                let printed = printed_input(value?, Unit::Plain, false)?;
                Some(numbers::check(
                    &printed,
                    &numbers::as_printed(&candidates),
                    false,
                ))
            }
        })
        .collect();
    for (verdict, status) in [
        (Verdict::Matched, NumberCheckStatus::Matched),
        (Verdict::Rounded, NumberCheckStatus::Rounded),
    ] {
        if let Some((_, index)) = verdicts.iter().find(|(found, _)| *found == verdict) {
            let pointer = index.and_then(|index| match &candidates[index].place {
                Place::Pointer(pointer) => Some(pointer.clone()),
                Place::Span { .. } => None,
            });
            return (status, pointer);
        }
    }
    (NumberCheckStatus::NotFound, None)
}

/// The input as the model wrote it, read in `unit`, with half a unit of its
/// last digit.
fn printed_input(value: &Value, unit: Unit, exponent_precision: bool) -> Option<numbers::Printed> {
    let text = match value {
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.trim().to_owned(),
        _ => return None,
    };
    let number = text
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())?;
    // Only conversion rates interpret exponents when choosing the tolerance.
    let exponent = match text.split_once(['e', 'E']) {
        Some((_, exponent)) if exponent_precision => exponent.parse::<i32>().ok()?,
        _ => 0,
    };
    let decimals = text.split_once('.').map_or(0, |(_, fraction)| {
        fraction.bytes().take_while(u8::is_ascii_digit).count()
    });
    Some(numbers::Printed {
        value: number,
        unit,
        currency: None,
        precision: 0.5 * 10f64.powi(exponent.saturating_sub(decimals as i32)),
        signed: text.starts_with(['+', '-']),
    })
}

fn conversion_rate_basis(source: &RunResult) -> Option<calculations::ConversionRate<'_>> {
    if source.producer.tool_name != "fx_rate" {
        return None;
    }
    Some(calculations::ConversionRate {
        date: source.payload["date"].as_str()?,
        source_id: source.payload.pointer("/source/id")?.as_str()?,
        source: source
            .payload
            .get("source")
            .filter(|source| source.is_object())?,
        revision: source.payload["revision"].as_str()?,
    })
}

fn check_conversion_rate(
    value: Option<&Value>,
    source: &RunResult,
    conversion: &Value,
) -> Result<(), BrokerError> {
    let payload = &source.payload;
    let from = conversion["from"].as_str().unwrap_or_default();
    let to = conversion["to"].as_str().unwrap_or_default();
    let wrong_result = || {
        BrokerError::invalid(
            "input_sources.rate",
            format!(
                "expected the citation_url of an fx_rate result from {from} to {to} in this turn"
            ),
        )
    };
    if conversion_rate_basis(source).is_none() {
        return Err(wrong_result());
    }
    let cited_from = payload["from"].as_str().ok_or_else(wrong_result)?;
    let cited_to = payload["to"].as_str().ok_or_else(wrong_result)?;
    if cited_from != from || cited_to != to {
        return Err(BrokerError::invalid(
            "input_sources.rate",
            format!("the cited fx_rate is from {cited_from} to {cited_to}. This conversion needs an fx_rate result from {from} to {to}"),
        ));
    }
    let published = payload["to_per_from"]
        .as_f64()
        .filter(|rate| *rate > 0.0)
        .ok_or_else(wrong_result)?;
    let published_precision =
        crate::finance_data::fx::published_rate_precision(published).ok_or_else(wrong_result)?;
    let agrees = value
        .and_then(|value| printed_input(value, Unit::Plain, true))
        .is_some_and(|printed| {
            let slack = f64::EPSILON * printed.value.abs().max(published.abs());
            (printed.value - published).abs() <= printed.precision.min(published_precision) + slack
        });
    if !agrees {
        return Err(BrokerError::invalid(
            "inputs.rate",
            format!("the cited fx_rate result states {published} for to_per_from"),
        ));
    }
    Ok(())
}

fn amount_currency<'a>(payload: &'a Value, pointer: &str) -> Option<&'a str> {
    let (parent, field) = super::number_checks::split_pointer(pointer)?;
    let row = payload.pointer(parent)?;
    [
        format!("{field}_currency").as_str(),
        "currency",
        "reported_currency",
        "currency_code",
    ]
    .into_iter()
    .filter_map(|key| row.get(key)?.as_str().map(str::trim))
    .find(|currency| !currency.is_empty())
}

fn check_amount_currency(
    input: &mut Value,
    currency: Option<&str>,
    from: &str,
) -> Result<(), BrokerError> {
    match currency {
        Some(currency) if currency != from && currency != from.to_ascii_lowercase() => {
            return Err(BrokerError::invalid(
                "input_sources.amount",
                format!("the source states this value in {currency}; unit says {from}"),
            ));
        }
        None => input["currency_basis"] = json!("declared"),
        _ => {}
    }
    Ok(())
}

impl AppToolExecutor {
    pub(super) async fn calculate(
        &self,
        policy: &ToolPolicy,
        mut params: Value,
    ) -> Result<Value, BrokerError> {
        if params.get("loupist_id").is_some() {
            return Err(BrokerError::invalid(
                "loupist_id",
                "Omit this field; calculate uses this Chat's Loupist.",
            ));
        }
        let input_sources = take_input_sources(&mut params)?;
        // A cited input must name this turn's acknowledged evidence.
        let cited: Vec<(String, RunResult)> = {
            let registry = self.capture.run_results.lock().await;
            input_sources
                .iter()
                .map(|(name, url)| {
                    evidence::cited_result(&registry, url)
                        .map(|result| (name.clone(), result.clone()))
                        .map_err(|why| BrokerError::invalid(format!("input_sources.{name}"), why))
                })
                .collect::<Result<_, _>>()?
        };
        let mut datasets = PinnedDatasets {
            store: self.state.store.as_ref(),
            loupist_id: &self.loupist_id,
            cutoff: parse_as_of_date(policy.as_of.as_deref())?,
            refused: None,
            read: BTreeMap::new(),
        };
        let rate_basis = cited
            .iter()
            .find(|(name, _)| name == "rate")
            .and_then(|(_, source)| conversion_rate_basis(source));
        let calculated = calculations::calculate_with_rate(&params, &mut datasets, rate_basis);
        if let Some(refused) = datasets.refused.take() {
            return Err(refused);
        }
        let mut result = calculated.map_err(|error| {
            match error {
                // Ownership, absence and integrity failures come from the store.
                calculations::CalcError::Unavailable { .. } => {
                    BrokerError::Execution(error.to_string())
                }
                _ => BrokerError::invalid(error.field(), error.detail()),
            }
        })?;
        let mut upstream = Vec::new();
        if !datasets.read.is_empty() {
            // A calculation over a pinned revision descends from the reads that produced it.
            let registry = self.capture.run_results.lock().await;
            upstream = registry
                .results
                .values()
                .filter(|entry| {
                    entry.payload.get("dataset_ref").is_some_and(|reference| {
                        datasets.read.keys().any(|(id, revision)| {
                            reference.get("dataset_id").and_then(Value::as_str) == Some(id)
                                && reference.get("revision").and_then(Value::as_u64)
                                    == Some(u64::from(*revision))
                        })
                    })
                })
                .map(|entry| entry.result_ref.clone())
                .collect::<Vec<_>>();
        }
        // Each cited input is checked against its evidence; the evidence joins the lineage.
        let unit = params
            .get("unit")
            .and_then(Value::as_str)
            .map(|unit| Unit::of_line(Some(unit)).unwrap_or(Unit::Plain));
        let conversion = result.get("conversion").cloned();
        for (name, source) in &cited {
            if !upstream.contains(&source.result_ref) {
                upstream.push(source.result_ref.clone());
            }
            let Some(input) = result
                .get_mut("inputs")
                .and_then(Value::as_array_mut)
                .and_then(|inputs| {
                    inputs.iter_mut().find(|input| {
                        input.get("name").and_then(Value::as_str) == Some(name.as_str())
                    })
                })
            else {
                continue;
            };
            // A dataset cell keeps its pinned provenance.
            if input.pointer("/provenance/kind").and_then(Value::as_str) != Some("provided") {
                continue;
            }
            let value = params.pointer(&format!("/inputs/{name}"));
            let status = if let Some(conversion) = conversion.as_ref().filter(|_| name == "rate") {
                check_conversion_rate(value, source, conversion)?;
                NumberCheckStatus::Matched
            } else {
                let (status, pointer) =
                    cited_status(value, &input_units(name, unit), &source.payload);
                if let Some(from) = conversion
                    .as_ref()
                    .filter(|_| {
                        name == "amount"
                            && matches!(
                                status,
                                NumberCheckStatus::Matched | NumberCheckStatus::Rounded
                            )
                    })
                    .and_then(|conversion| conversion["from"].as_str())
                {
                    let currency = pointer
                        .as_deref()
                        .and_then(|pointer| amount_currency(&source.payload, pointer));
                    check_amount_currency(input, currency, from)?;
                }
                status
            };
            input["provenance"] = json!({
                "kind": "cited",
                "evidence_id": evidence::evidence_id(&source.result_ref),
                "status": status,
            });
        }
        if let Some(from) = conversion
            .as_ref()
            .and_then(|conversion| conversion["from"].as_str())
        {
            let pinned_amount =
                result["inputs"]
                    .as_array_mut()
                    .into_iter()
                    .flatten()
                    .find(|input| {
                        input["name"] == "amount" && input["provenance"]["kind"] == "dataset"
                    });
            if let Some(input) = pinned_amount {
                let currency = datasets.cell_currency(&input["provenance"]);
                check_amount_currency(input, currency, from)?;
            }
        }
        if !upstream.is_empty() {
            result[RESOLVED_UPSTREAM_FIELD] = json!(upstream);
        }
        Ok(result)
    }
}
