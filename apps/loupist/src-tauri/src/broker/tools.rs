//! Shared static tool contracts. Rust owns execution and authority; the Pi
//! adapter reads the same bundled catalog for names, schemas and broker routing.
use serde::Deserialize;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ToolPermission {
    Always,
    MemoryRead,
    MemoryWrite,
}

impl ToolPermission {
    pub fn allows(self, read: bool, write: bool) -> bool {
        match self {
            Self::Always => true,
            Self::MemoryRead => read,
            Self::MemoryWrite => write,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentToolSpec {
    pub name: String,
    pub method: Option<String>,
    pub permission: ToolPermission,
    pub component: Option<String>,
    // Model-facing fields are read by the Pi adapter only.
    #[serde(rename = "label")]
    _label: Option<String>,
    #[serde(rename = "description")]
    _description: Option<String>,
    // A catalog maintainer note, omitted from the registered tool definition.
    #[serde(rename = "_comment")]
    _comment: Option<String>,
    #[serde(rename = "parameters")]
    _parameters: Option<serde_json::Value>,
    /// Display rules for Chat progress. The Pi adapter registers only name,
    /// label, description and parameters, so this block never reaches the model.
    #[serde(default)]
    pub progress: Option<ToolProgressSpec>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ToolProgressSpec {
    pub category: String,
    pub operation: String,
    pub running: String,
    pub done: String,
    #[serde(default)]
    pub target_args: Vec<String>,
    /// path, command, join, join_flat, count, or web.
    #[serde(default)]
    pub target: Option<String>,
    /// web, memory, files, finance, artifact, compute, or capability.
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentSpec {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Ordinary built-ins start active unless the catalog makes them on-demand.
    #[serde(default = "preload_by_default")]
    pub preload: bool,
    /// Plugin name when this component is one connection. Built-in packs omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
}

fn preload_by_default() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCatalog {
    pub tools: Vec<AgentToolSpec>,
    pub components: Vec<ComponentSpec>,
}

pub static TOOL_CATALOG: LazyLock<ToolCatalog> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../agent/tool-catalog.json"))
        .expect("bundled tool catalog must be valid")
});

pub(super) async fn enforce_run_budget(
    policy: &super::ToolPolicy,
    method: super::ToolMethod,
    params: &serde_json::Value,
    call_id: Option<&str>,
) -> Result<(), super::BrokerError> {
    use super::{
        tool_class::{PythonExposure, Reach},
        BrokerError,
        ToolMethod::*,
    };
    let Some(budget) = crate::runs::budget::for_chat(&policy.loupist_id, &policy.session_id) else {
        return Ok(());
    };
    if let Some(learning) = budget.learning() {
        let permitted = match method {
            SourceSave => learning.keeps.sources,
            LensFile => learning.keeps.lens,
            ThesisUpdate | PriceReadSave => {
                learning.keeps.asset_reads
                    && params["assetId"]
                        .as_str()
                        .is_some_and(|id| learning.asset_ids.iter().any(|asset| asset == id))
            }
            _ => true,
        };
        if !permitted {
            return Err(BrokerError::MethodDenied);
        }
    }
    if matches!(
        method,
        LensRead | LensFile | SourceSave | ThesisUpdate | PriceReadSave
    ) {
        return Ok(());
    }
    let safety = method.safety();
    let reads = safety.reach == Reach::Outbound
        || safety.python != PythonExposure::NoPython
        || method.producer().is_some()
        || matches!(method, McpConnect | McpCall);
    if reads {
        if let Some(call_id) = call_id {
            if !budget.wait_for_call(call_id).await {
                return Err(BrokerError::MethodDenied);
            }
        }
        if !budget.refusal() {
            return Ok(());
        }
        return Err(BrokerError::MethodDenied.with_next(
            "This run reached its request limit: the host refuses further data reads and still accepts the run's saves. Make the saves the evidence in hand supports and write the result.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod budget_tests {
    use super::*;
    use crate::{
        broker::{ToolMethod, ToolPolicy},
        runs::budget::{for_chat, BudgetLease, GRACE_ROUNDS},
        scheduled_tasks::types::{BudgetOutcome, LearningBudget, RunBudget},
    };
    use serde_json::json;
    use std::time::Duration;
    use tokio::time::Instant;

    #[tokio::test]
    async fn generated_rounds_follow_soft_hard_and_grace_boundaries() {
        let policy = ToolPolicy {
            loupist_id: "budget-oracle".into(),
            session_id: "scripted-lease".into(),
            use_memory: true,
            propose_memory_updates: true,
            memory_proposal_budget: 1,
            as_of: None,
            display_currency: None,
        };
        let protected = [
            ToolMethod::LensRead,
            ToolMethod::LensFile,
            ToolMethod::SourceSave,
            ToolMethod::ThesisUpdate,
            ToolMethod::PriceReadSave,
        ];
        let external = [
            ToolMethod::KrxDailyQuote,
            ToolMethod::WebFetch,
            ToolMethod::McpCall,
            ToolMethod::PythonRun,
        ];
        for soft in 1..=7 {
            for hard in soft + 1..=12 {
                let total = hard + GRACE_ROUNDS;
                let cap = RunBudget::freeze(LearningBudget { soft, hard }, total, total);
                let lease = BudgetLease::bind(
                    &policy.loupist_id,
                    &policy.session_id,
                    cap,
                    Duration::ZERO,
                    None,
                );
                let budget = for_chat(&policy.loupist_id, &policy.session_id).unwrap();
                let start = Instant::now();
                let args = json!({});
                for round in 0..=total {
                    let tick = start + Duration::from_secs(u64::from(round));
                    budget.observe(round, tick, std::iter::empty());
                    assert_eq!(budget.exhausted(tick), round >= total);
                    for method in external {
                        assert_eq!(
                            enforce_run_budget(&policy, method, &args, None)
                                .await
                                .is_err(),
                            round >= hard
                        );
                    }
                    for method in protected {
                        assert!(enforce_run_budget(&policy, method, &args, None)
                            .await
                            .is_ok());
                    }
                    if round == total {
                        budget.mark_cancelled();
                    }
                    let expected = if round >= total {
                        BudgetOutcome::Cancelled
                    } else if round >= hard {
                        BudgetOutcome::Refused
                    } else if round >= soft {
                        BudgetOutcome::Soft
                    } else {
                        BudgetOutcome::None
                    };
                    assert_eq!(budget.outcome(), expected);
                }
                let grace_ends = start + Duration::from_secs(u64::from(hard) + 300);
                assert_eq!(budget.deadline(), grace_ends);
                drop(lease);
                assert!(for_chat(&policy.loupist_id, &policy.session_id).is_none());
            }
        }
    }
}
