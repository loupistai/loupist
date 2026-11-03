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
    /// Plugin name when this component is one connection. Built-in packs omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
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
