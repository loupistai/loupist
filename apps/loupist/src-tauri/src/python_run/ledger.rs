//! Data exchanged between isolated worker starts. The worker never dispatches
//! a host operation. Its request file is treated as untrusted input.
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

use crate::{broker::BrokerError, datasets::DatasetRef};

pub(crate) const MAX_ROUNDS: u32 = 4;
pub(crate) const MAX_CALLS: usize = 32;
pub(crate) const MAX_REQUEST_BYTES: usize = 256 * 1024;
pub(crate) const MAX_LEDGER_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const BOOTSTRAP: &str = include_str!("loupist_bootstrap.py");

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReadCall {
    pub(crate) tool: String,
    pub(crate) arguments: Map<String, Value>,
}

impl ReadCall {
    pub(crate) fn key(&self) -> String {
        // Nested argument objects are sorted as well, independent of order on the wire.
        let mut value = serde_json::to_value(self).expect("JSON read call");
        value.sort_all_objects();
        value.to_string()
    }

    pub(crate) fn valid(&self) -> bool {
        !self.tool.is_empty() && self.tool.len() <= 128 && self.key().len() <= 8 * 1024
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ReadSummary {
    pub tool: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub citation_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub as_of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub dataset: Option<DatasetRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum Response {
    Ok {
        value: Value,
    },
    Refused {
        code: String,
    },
    Unavailable {
        code: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_after: Option<u64>,
    },
}

impl Response {
    pub(crate) fn refused(code: &str) -> Self {
        Self::Refused {
            code: code.to_owned(),
        }
    }

    pub(crate) fn unavailable(code: &str) -> Self {
        Self::Unavailable {
            code: code.to_owned(),
            retry_after: None,
        }
    }

    pub(crate) fn rate_limited(retry_after: Option<u64>) -> Self {
        Self::Unavailable {
            code: "rate_limited".into(),
            retry_after,
        }
    }

    pub(crate) fn status(&self) -> &str {
        match self {
            Self::Ok { .. } => "ok",
            Self::Refused { code } | Self::Unavailable { code, .. } => code,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct LedgerEntry {
    pub(crate) call: ReadCall,
    pub(crate) response: Response,
}

fn empty_state() -> Value {
    Value::Object(Map::new())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RunLedger {
    pub(crate) id: String,
    pub(crate) now: String,
    pub(crate) args: Map<String, Value>,
    #[serde(default = "empty_state")]
    pub(crate) state: Value,
    pub(crate) inputs: BTreeMap<String, ReadCall>,
    pub(crate) entries: Vec<LedgerEntry>,
    pub(crate) final_round: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) bar_dates: BTreeMap<String, [String; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) check_stale_days: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunParams {
    pub(crate) code: Option<String>,
    pub(crate) script: Option<String>,
    #[serde(default)]
    pub(crate) args: Map<String, Value>,
    #[serde(default)]
    pub(crate) inputs: Vec<NamedInput>,
    #[serde(default = "default_timeout")]
    pub(crate) timeout_seconds: u64,
}

fn default_timeout() -> u64 {
    super::DEFAULT_TIMEOUT_SECONDS
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NamedInput {
    name: String,
    call: Option<ReadCall>,
    dataset: Option<DatasetInput>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DatasetInput {
    dataset_id: String,
    revision: u32,
    pointer: Option<String>,
}

impl RunParams {
    pub(crate) fn parse(params: Value) -> Result<Self, BrokerError> {
        let object = params.as_object().ok_or(BrokerError::Malformed)?;
        if object.get("code").is_some_and(|value| !value.is_string()) {
            return Err(BrokerError::invalid("code", "a Python program"));
        }
        if object.get("timeout_seconds").is_some_and(|value| {
            value
                .as_u64()
                .is_none_or(|seconds| !(1..=super::MAX_TIMEOUT_SECONDS).contains(&seconds))
        }) {
            return Err(BrokerError::invalid(
                "timeout_seconds",
                "an integer from 1 to 120",
            ));
        }
        let request: Self = serde_json::from_value(params).map_err(|_| BrokerError::Malformed)?;
        if request.code.is_some() == request.script.is_some() {
            return Err(BrokerError::invalid(
                "code",
                "provide exactly one of code or script",
            ));
        }
        if let Some(code) = &request.code {
            super::validate_code(code, request.timeout_seconds)?;
        } else {
            super::validate_code("pass", request.timeout_seconds)?;
        }
        if request.inputs.len() > MAX_CALLS
            || Value::Object(request.args.clone()).to_string().len() > 8 * 1024
        {
            return Err(BrokerError::invalid(
                "inputs",
                "at most 32 inputs and 8 KiB of args",
            ));
        }
        Ok(request)
    }
}

pub(crate) fn named_inputs(
    inputs: Vec<NamedInput>,
) -> Result<BTreeMap<String, ReadCall>, BrokerError> {
    let mut named = BTreeMap::new();
    for input in inputs {
        let call = match (input.call, input.dataset) {
            (Some(call), None) => call,
            (None, Some(dataset)) => ReadCall {
                tool: "dataset".into(),
                arguments: serde_json::json!({
                    "dataset_id": dataset.dataset_id, "revision": dataset.revision, "pointer": dataset.pointer,
                }).as_object().expect("JSON object").clone(),
            },
            _ => return Err(BrokerError::invalid("inputs", "each input needs one call or dataset")),
        };
        if !call.valid()
            || input.name.is_empty()
            || input.name.len() > 64
            || named.insert(input.name, call).is_some()
        {
            return Err(BrokerError::invalid(
                "inputs",
                "use unique names and bounded read calls",
            ));
        }
    }
    Ok(named)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct PythonStep {
    pub code: String,
    pub stdout: String,
    pub stderr: String,
    pub status: String,
    pub reads: Vec<ReadSummary>,
    pub rounds: u32,
}

pub(crate) fn bootstrap_digest() -> String {
    crate::util::sha256(BOOTSTRAP.as_bytes())
}

/// A Library test or a replay has no working files, attachments or persistent
/// output. Its per-start snapshot remains subject to the same worker policy.
pub(crate) fn empty_roots() -> Result<
    (
        tempfile::TempDir,
        crate::agent_harness::native_policy::NativeRoots,
    ),
    BrokerError,
> {
    let folder = tempfile::tempdir()
        .map_err(|_| BrokerError::Execution("Python workspace is unavailable".into()))?;
    let work = folder.path().join("workspace");
    std::fs::create_dir(&work)
        .map_err(|_| BrokerError::Execution("Python workspace is unavailable".into()))?;
    let absent = folder.path().join("unavailable");
    let roots = crate::agent_harness::native_policy::NativeRoots::new(
        &work, &absent, &absent, &absent, &absent, &absent, None,
    );
    Ok((folder, roots))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_ledger_keys_ignore_nested_object_order_and_keep_values() {
        let first: ReadCall =
            serde_json::from_str(r#"{"tool":"data","arguments":{"a":{"c":2,"b":1},"b":[1,2]}}"#)
                .unwrap();
        let second: ReadCall =
            serde_json::from_str(r#"{"tool":"data","arguments":{"b":[1,2],"a":{"b":1,"c":2}}}"#)
                .unwrap();
        assert_eq!(first.key(), second.key());
        assert_ne!(
            first.key(),
            r#"{"arguments":{"a":{"b":1,"c":2},"b":[2,1]},"tool":"data"}"#
        );
    }

    #[test]
    fn python_ledger_inputs_require_exactly_one_source_and_unique_names() {
        let parse = |value| RunParams::parse(value).unwrap().inputs;
        let call = serde_json::json!({"name":"x","call":{"tool":"data","arguments":{}}});
        assert!(named_inputs(parse(
            serde_json::json!({"code":"pass","inputs":[call.clone(),call]})
        ))
        .is_err());
        assert!(named_inputs(parse(
            serde_json::json!({"code":"pass","inputs":[{"name":"x"}]})
        ))
        .is_err());
        assert!(RunParams::parse(serde_json::json!({"code":"pass","script":"x"})).is_err());
    }
}
