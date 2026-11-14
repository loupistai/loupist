//! A scripted reader for LLM-free tests of watchers and earnings captures:
//! structured rows per tool (and symbol), retained in the reading Loupist's
//! dataset store like a real read, with every call recorded.
use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::{HostReader, ReadOutcome};
use crate::{app::AppState, knowledge::assets::plan};

pub(crate) struct Script {
    pub tool: &'static str,
    /// Matches `symbol`, `company` or KIS `fid_input_iscd`; any when None.
    pub symbol: Option<&'static str>,
    pub payload: Value,
    pub provider: Option<&'static str>,
}

#[derive(Default)]
pub(crate) struct Scripted {
    pub scripts: Vec<Script>,
    /// (Loupist, tool, arguments) of every read made.
    pub calls: Vec<(String, String, Value)>,
    /// Loupists without the grant a read needs.
    pub ungranted: BTreeSet<String>,
    pub(crate) ensured: BTreeSet<String>,
    /// Connections every Loupist holds (`fmp.market-data`…).
    pub capabilities: BTreeSet<String>,
    /// Every source fails to answer.
    pub offline: bool,
}

impl Scripted {
    pub fn script(&mut self, tool: &'static str, symbol: Option<&'static str>, payload: Value) {
        self.scripts
            .retain(|script| !(script.tool == tool && script.symbol == symbol));
        self.scripts.push(Script {
            tool,
            symbol,
            payload,
            provider: None,
        });
    }

    pub fn script_from(
        &mut self,
        tool: &'static str,
        symbol: Option<&'static str>,
        provider: &'static str,
        payload: Value,
    ) {
        self.scripts
            .retain(|script| !(script.tool == tool && script.symbol == symbol));
        self.scripts.push(Script {
            tool,
            symbol,
            payload,
            provider: Some(provider),
        });
    }

    pub fn calls_to(&self, tool: &str) -> usize {
        self.calls
            .iter()
            .filter(|(_, name, _)| name == tool)
            .count()
    }
}

fn symbol_of(arguments: &Value) -> Option<&str> {
    arguments
        .get("symbol")
        .or_else(|| arguments.get("company"))
        .or_else(|| arguments.pointer("/params/fid_input_iscd"))
        .or_else(|| arguments.get("code"))
        .or_else(|| arguments.get("id"))
        .and_then(Value::as_str)
}

impl HostReader for Scripted {
    fn ensure(&mut self, _state: &AppState, loupist_id: &str) -> bool {
        self.ensured.insert(loupist_id.to_owned());
        true
    }

    fn granted(&self, loupist_id: &str, _read: &plan::PlannedRead) -> bool {
        self.ensured.contains(loupist_id) && !self.ungranted.contains(loupist_id)
    }

    fn has(&self, _loupist_id: &str, capability: &str) -> bool {
        self.capabilities.contains(capability)
    }

    fn read(
        &mut self,
        state: &AppState,
        loupist_id: &str,
        _page: &crate::knowledge::assets::PageRecord,
        read: &plan::PlannedRead,
        allowed: Option<&'static [&'static str]>,
    ) -> impl std::future::Future<Output = ReadOutcome> + Send {
        self.calls.push((
            loupist_id.to_owned(),
            read.tool.to_owned(),
            read.arguments.clone(),
        ));
        let connection = if read.server.is_some() {
            "openbb.platform".to_owned()
        } else {
            read.connection.to_owned()
        };
        let outcome = if self.offline {
            ReadOutcome::Failed(connection)
        } else {
            let symbol = symbol_of(&read.arguments);
            match self.scripts.iter().find(|script| {
                script.tool == read.tool && (script.symbol.is_none() || script.symbol == symbol)
            }) {
                None => ReadOutcome::Failed(connection),
                Some(script)
                    if allowed.is_some_and(|allowed| {
                        !script
                            .provider
                            .is_some_and(|provider| allowed.contains(&provider))
                    }) =>
                {
                    // As the app names the connection a pinned provider needs.
                    ReadOutcome::NeedsConnection(
                        if allowed.is_some_and(|allowed| allowed.contains(&"eia")) {
                            "eia.energy"
                        } else {
                            "fmp.market-data"
                        }
                        .into(),
                    )
                }
                Some(script) => {
                    let mut payload = script.payload.clone();
                    let key = format!(
                        "scripted:{}:{}",
                        read.tool,
                        crate::util::sha256(read.arguments.to_string().as_bytes())
                    );
                    match state.store.dataset_put(
                        loupist_id,
                        &key,
                        &payload,
                        json!({"producer": "scripted", "fetched_at": chrono::Utc::now().to_rfc3339()}),
                    ) {
                        Ok(reference) => {
                            payload["dataset_ref"] = json!(reference);
                            ReadOutcome::Ok {
                                payload,
                                provider: script.provider.map(str::to_owned),
                            }
                        }
                        Err(_) => ReadOutcome::Failed(connection),
                    }
                }
            }
        };
        async move { outcome }
    }
}
