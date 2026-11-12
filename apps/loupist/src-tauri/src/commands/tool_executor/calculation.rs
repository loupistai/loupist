use super::*;
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

/// The units a cited input can be printed in, by what it measures
/// ([`calculations::input_kind`], which also words the input for readers): a
/// `price` is a per-share price in base currency units, printed with or
/// without its currency.
fn input_units(
    name: &str,
    amount: crate::knowledge::numbers::Unit,
) -> Vec<crate::knowledge::numbers::Unit> {
    use crate::knowledge::numbers::Unit;
    match calculations::input_kind(name) {
        calculations::InputKind::Count => vec![Unit::Plain],
        calculations::InputKind::Price => vec![Unit::Amount(1.0), Unit::Plain],
        calculations::InputKind::Percent => vec![Unit::Percent],
        calculations::InputKind::Amount => vec![amount],
    }
}

/// How a cited input stands against its evidence, read in each of its units.
fn cited_status(
    value: Option<&Value>,
    units: &[crate::knowledge::numbers::Unit],
    payload: &Value,
) -> crate::domain::NumberCheckStatus {
    use crate::domain::NumberCheckStatus;
    use crate::knowledge::numbers::{self, Verdict};
    let candidates = numbers::data_candidates(payload);
    let verdicts: Vec<Verdict> = units
        .iter()
        .filter_map(|unit| printed_input(value?, *unit))
        .map(|printed| numbers::check(&printed, &candidates, false).0)
        .collect();
    if verdicts.contains(&Verdict::Matched) {
        NumberCheckStatus::Matched
    } else if verdicts.contains(&Verdict::Rounded) {
        NumberCheckStatus::Rounded
    } else {
        NumberCheckStatus::NotFound
    }
}

/// The input as the model wrote it, read in `unit`, with half a unit of its
/// last digit.
fn printed_input(
    value: &Value,
    unit: crate::knowledge::numbers::Unit,
) -> Option<crate::knowledge::numbers::Printed> {
    let text = match value {
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.trim().to_owned(),
        _ => return None,
    };
    let number = text
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())?;
    let decimals = text.split_once('.').map_or(0, |(_, fraction)| {
        fraction.bytes().take_while(u8::is_ascii_digit).count()
    });
    Some(crate::knowledge::numbers::Printed {
        value: number,
        unit,
        precision: 0.5 * 10f64.powi(-(decimals as i32)),
    })
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
        let calculated = calculations::calculate(&params, &mut datasets);
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
        let unit =
            crate::knowledge::numbers::Unit::of_line(params.get("unit").and_then(Value::as_str))
                .unwrap_or(crate::knowledge::numbers::Unit::Plain);
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
            let status = cited_status(
                params.pointer(&format!("/inputs/{name}")),
                &input_units(name, unit),
                &source.payload,
            );
            input["provenance"] = json!({
                "kind": "cited",
                "evidence_id": evidence::evidence_id(&source.result_ref),
                "status": status,
            });
        }
        if !upstream.is_empty() {
            result[RESOLVED_UPSTREAM_FIELD] = json!(upstream);
        }
        Ok(result)
    }
}
