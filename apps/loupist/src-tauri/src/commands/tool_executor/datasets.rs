use super::*;

/// Agent arguments for `dataset_describe` and `dataset_query`. The Loupist comes
/// from the run, never from the model.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DatasetQueryRequest {
    dataset_id: String,
    revision: u32,
    #[serde(default)]
    pointer: Option<String>,
    #[serde(default)]
    offset: usize,
    #[serde(default = "query_limit")]
    limit: usize,
    #[serde(default)]
    columns: Vec<String>,
}

fn query_limit() -> usize {
    100
}

const DATASET_AS_OF_NEXT: &str = "Fetch the data again in this turn, where the as-of cutoff bounds the read, and use the new dataset_ref. Do not answer from a revision fetched without this cutoff.";

/// The cutoff a revision was fetched under: native reads record `as_of`,
/// bundled MCP reads record it in their captured request.
fn revision_cutoff(metadata: &Value) -> Option<chrono::NaiveDate> {
    metadata
        .get("as_of")
        .and_then(Value::as_str)
        .or_else(|| metadata.pointer("/request/as_of").and_then(Value::as_str))
        .and_then(|value| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
}

/// Under a turn cutoff, only a revision fetched under a cutoff on or before it
/// is usable: one fetched without a cutoff (for example earlier in the Chat)
/// can hold rows that were not yet known.
pub(super) fn ensure_within_cutoff(
    dataset_id: &str,
    revision: u32,
    metadata: &Value,
    cutoff: Option<chrono::NaiveDate>,
) -> Result<(), BrokerError> {
    let Some(cutoff) = cutoff else {
        return Ok(());
    };
    let fetched = match revision_cutoff(metadata) {
        Some(fetched) if fetched <= cutoff => return Ok(()),
        Some(fetched) => format!("under the later as-of cutoff {fetched}"),
        None => "without an as-of cutoff".to_owned(),
    };
    Err(BrokerError::Execution(format!(
        "as_of_unsupported: dataset {dataset_id} revision {revision} was fetched {fetched}, so it cannot show only what was known on {cutoff}"
    ))
    .with_next(DATASET_AS_OF_NEXT))
}

impl AppToolExecutor {
    /// Refuses a revision the turn's as-of cutoff cannot use.
    pub(super) fn ensure_dataset_within_cutoff(
        &self,
        policy: &ToolPolicy,
        dataset_id: &str,
        revision: u32,
    ) -> Result<(), BrokerError> {
        let Some(cutoff) = parse_as_of_date(policy.as_of.as_deref())? else {
            return Ok(());
        };
        let metadata = self
            .state
            .store
            .dataset_metadata(&self.loupist_id, dataset_id, revision)
            .map_err(command_error)?;
        ensure_within_cutoff(dataset_id, revision, &metadata, Some(cutoff))
    }

    pub(super) fn dataset_tool(
        &self,
        policy: &ToolPolicy,
        method: ToolMethod,
        params: Value,
    ) -> Result<Value, BrokerError> {
        let object = params.as_object().ok_or(BrokerError::Malformed)?;
        if object.contains_key("loupist_id") {
            return Err(BrokerError::MethodDenied);
        }
        fn parse<T: serde::de::DeserializeOwned>(v: Value) -> Result<T, BrokerError> {
            serde_json::from_value(v).map_err(|_| BrokerError::Malformed)
        }
        fn map<T>(r: Result<T, crate::app::CommandError>) -> Result<T, BrokerError> {
            r.map_err(command_error)
        }
        match method {
            ToolMethod::DatasetDescribe => {
                let r: DatasetQueryRequest = parse(params)?;
                self.ensure_dataset_within_cutoff(policy, &r.dataset_id, r.revision)?;
                map(self
                    .state
                    .store
                    .dataset_describe(&self.loupist_id, &r.dataset_id, r.revision))
            }
            ToolMethod::DatasetQuery => {
                let r: DatasetQueryRequest = parse(params)?;
                self.ensure_dataset_within_cutoff(policy, &r.dataset_id, r.revision)?;
                map(self.state.store.dataset_query(
                    &self.loupist_id,
                    &r.dataset_id,
                    r.revision,
                    r.pointer.as_deref(),
                    r.offset,
                    r.limit,
                    &r.columns,
                ))
            }
            _ => Err(BrokerError::MethodDenied),
        }
    }
}
