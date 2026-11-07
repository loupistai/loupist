use std::path::{Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

mod skill_mentions;
pub(crate) mod workspace;
pub use skill_mentions::explicit_skill_names;
pub use workspace::AgentWorkspaceSnapshot;

pub const RESEARCH_SKILL_ID: &str = "research";
pub const WORLD_SKILL_ID: &str = "world";
pub const LENS_SKILL_ID: &str = "lens";
pub const JOURNAL_SKILL_ID: &str = "journal";
pub const VALUATION_SKILL_ID: &str = "valuation";
pub const COMPARISON_SKILL_ID: &str = "comparison";
pub const RULE_TEST_SKILL_ID: &str = "rule-test";
pub const FILINGS_SKILL_ID: &str = "filings";
pub const VISUALIZE_SKILL_ID: &str = "visualize";
pub const PORTFOLIO_RISK_SKILL_ID: &str = "portfolio-risk";
pub const EVENTS_STATS_SKILL_ID: &str = "events-stats";
pub const DATA_ANALYSIS_SKILL_ID: &str = "data-analysis";
pub const DIVIDEND_INCOME_SKILL_ID: &str = "dividend-income";
pub const FUND_COMPARE_SKILL_ID: &str = "fund-compare";
pub const WHY_IT_MOVED_SKILL_ID: &str = "why-it-moved";
pub const GOAL_MATH_SKILL_ID: &str = "goal-math";

pub const PDF_SKILL_ID: &str = "pdf";
pub const DOCUMENTS_SKILL_ID: &str = "documents";
pub const SPREADSHEETS_SKILL_ID: &str = "spreadsheets";
pub const PRESENTATIONS_SKILL_ID: &str = "presentations";

const HARNESS_SCHEMA: &str = "loupist-harness/1";
const RUNTIME_SCHEMA: &str = "loupist-agent-runtime/1";
const EXTENSION_ENTRYPOINT: &str = "loupist-extension.mjs";
const PROVIDER_EXTENSION_ENTRYPOINT: &str = "loupist-provider-extension.mjs";
const EXTENSION_FILES: &[(&str, &[u8])] = &[
    (
        "tool-catalog.json",
        include_bytes!("../../agent/tool-catalog.json"),
    ),
    (
        EXTENSION_ENTRYPOINT,
        include_bytes!("../../agent/loupist-extension.mjs"),
    ),
    (
        "broker-client.mjs",
        include_bytes!("../../agent/broker-client.mjs"),
    ),
    (
        "workspace-tools.mjs",
        include_bytes!("../../agent/workspace-tools.mjs"),
    ),
    (
        "workspace-resources.mjs",
        include_bytes!("../../agent/workspace-resources.mjs"),
    ),
    (
        "pi-settings.mjs",
        include_bytes!("../../agent/pi-settings.mjs"),
    ),
    (
        "model-run-controls.mjs",
        include_bytes!("../../agent/model-run-controls.mjs"),
    ),
];
const PROVIDER_EXTENSION_FILES: &[(&str, &[u8])] = &[
    (
        PROVIDER_EXTENSION_ENTRYPOINT,
        include_bytes!("../../agent/loupist-provider-extension.mjs"),
    ),
    (
        "provider-inference.mjs",
        include_bytes!("../../agent/provider-inference.mjs"),
    ),
    (
        "model-run-controls.mjs",
        include_bytes!("../../agent/model-run-controls.mjs"),
    ),
    (
        "loupist-native-search.mjs",
        include_bytes!("../../agent/loupist-native-search.mjs"),
    ),
    (
        "native-search/common.mjs",
        include_bytes!("../../agent/native-search/common.mjs"),
    ),
    (
        "native-search/codex.mjs",
        include_bytes!("../../agent/native-search/codex.mjs"),
    ),
    (
        "native-search/anthropic.mjs",
        include_bytes!("../../agent/native-search/anthropic.mjs"),
    ),
    (
        "native-search/xai.mjs",
        include_bytes!("../../agent/native-search/xai.mjs"),
    ),
];

/// Optional workflows. Each entry has its own default; authoring starts off.
const SELECTABLE_SKILLS: &[BundledSkill] = &[
    BundledSkill {
        id: "skill-creator",
        name: "Skill Creator",
        default_enabled: false,
        relative_path: "skills/skill-creator/SKILL.md",
        content: include_bytes!("../../agent/skills/skill-creator/SKILL.md"),
        support_files: &[(
            "skills/skill-creator/LICENSE.txt",
            include_bytes!("../../agent/skills/skill-creator/LICENSE.txt"),
        )],
    },
    BundledSkill {
        id: "skill-installer",
        name: "Skill Installer",
        default_enabled: false,
        relative_path: "skills/skill-installer/SKILL.md",
        content: include_bytes!("../../agent/skills/skill-installer/SKILL.md"),
        support_files: &[(
            "skills/skill-installer/LICENSE.txt",
            include_bytes!("../../agent/skills/skill-installer/LICENSE.txt"),
        )],
    },
    // Everyday questions come first so `/` lists them before the specialist workflows.
    BundledSkill {
        id: DIVIDEND_INCOME_SKILL_ID,
        name: "Dividend income",
        default_enabled: true,
        relative_path: "skills/dividend-income/SKILL.md",
        content: include_bytes!("../../agent/skills/dividend-income/SKILL.md"),
        support_files: &[(
            "skills/dividend-income/references/dividend-checks.md",
            include_bytes!("../../agent/skills/dividend-income/references/dividend-checks.md"),
        )],
    },
    BundledSkill {
        id: FUND_COMPARE_SKILL_ID,
        name: "Fund comparison",
        default_enabled: true,
        relative_path: "skills/fund-compare/SKILL.md",
        content: include_bytes!("../../agent/skills/fund-compare/SKILL.md"),
        support_files: &[(
            "skills/fund-compare/references/fund-checks.md",
            include_bytes!("../../agent/skills/fund-compare/references/fund-checks.md"),
        )],
    },
    BundledSkill {
        id: WHY_IT_MOVED_SKILL_ID,
        name: "Why it moved",
        default_enabled: true,
        relative_path: "skills/why-it-moved/SKILL.md",
        content: include_bytes!("../../agent/skills/why-it-moved/SKILL.md"),
        support_files: &[(
            "skills/why-it-moved/references/move-attribution.md",
            include_bytes!("../../agent/skills/why-it-moved/references/move-attribution.md"),
        )],
    },
    BundledSkill {
        id: GOAL_MATH_SKILL_ID,
        name: "Goal math",
        default_enabled: true,
        relative_path: "skills/goal-math/SKILL.md",
        content: include_bytes!("../../agent/skills/goal-math/SKILL.md"),
        support_files: &[(
            "skills/goal-math/references/goal-assumptions.md",
            include_bytes!("../../agent/skills/goal-math/references/goal-assumptions.md"),
        )],
    },
    BundledSkill {
        id: RESEARCH_SKILL_ID,
        name: "Research",
        default_enabled: true,
        relative_path: "skills/research/SKILL.md",
        content: include_bytes!("../../agent/skills/research/SKILL.md"),
        support_files: &[(
            "skills/research/references/thesis-review.md",
            include_bytes!("../../agent/skills/research/references/thesis-review.md"),
        )],
    },
    BundledSkill {
        id: VALUATION_SKILL_ID,
        name: "Valuation",
        default_enabled: true,
        relative_path: "skills/valuation/SKILL.md",
        content: include_bytes!("../../agent/skills/valuation/SKILL.md"),
        support_files: &[(
            "skills/valuation/references/dcf.md",
            include_bytes!("../../agent/skills/valuation/references/dcf.md"),
        )],
    },
    BundledSkill {
        id: COMPARISON_SKILL_ID,
        name: "Peer comparison",
        default_enabled: true,
        relative_path: "skills/comparison/SKILL.md",
        content: include_bytes!("../../agent/skills/comparison/SKILL.md"),
        support_files: &[(
            "skills/comparison/references/peer-analysis.md",
            include_bytes!("../../agent/skills/comparison/references/peer-analysis.md"),
        )],
    },
    BundledSkill {
        id: FILINGS_SKILL_ID,
        name: "Filings and earnings",
        default_enabled: true,
        relative_path: "skills/filings/SKILL.md",
        content: include_bytes!("../../agent/skills/filings/SKILL.md"),
        support_files: &[
            (
                "skills/filings/references/earnings-review.md",
                include_bytes!("../../agent/skills/filings/references/earnings-review.md"),
            ),
            (
                "skills/filings/references/korea-statements.md",
                include_bytes!("../../agent/skills/filings/references/korea-statements.md"),
            ),
        ],
    },
    BundledSkill {
        id: RULE_TEST_SKILL_ID,
        name: "Backtesting",
        default_enabled: true,
        relative_path: "skills/rule-test/SKILL.md",
        content: include_bytes!("../../agent/skills/rule-test/SKILL.md"),
        support_files: &[(
            "skills/rule-test/references/backtest-design.md",
            include_bytes!("../../agent/skills/rule-test/references/backtest-design.md"),
        )],
    },
    BundledSkill {
        id: PORTFOLIO_RISK_SKILL_ID,
        name: "Portfolio and risk",
        default_enabled: true,
        relative_path: "skills/portfolio-risk/SKILL.md",
        content: include_bytes!("../../agent/skills/portfolio-risk/SKILL.md"),
        support_files: &[(
            "skills/portfolio-risk/references/risk-review.md",
            include_bytes!("../../agent/skills/portfolio-risk/references/risk-review.md"),
        )],
    },
    BundledSkill {
        id: EVENTS_STATS_SKILL_ID,
        name: "Events and statistics",
        default_enabled: true,
        relative_path: "skills/events-stats/SKILL.md",
        content: include_bytes!("../../agent/skills/events-stats/SKILL.md"),
        support_files: &[(
            "skills/events-stats/references/inference.md",
            include_bytes!("../../agent/skills/events-stats/references/inference.md"),
        )],
    },
    BundledSkill {
        id: DATA_ANALYSIS_SKILL_ID,
        name: "Dataset analysis",
        default_enabled: true,
        relative_path: "skills/data-analysis/SKILL.md",
        content: include_bytes!("../../agent/skills/data-analysis/SKILL.md"),
        support_files: &[(
            "skills/data-analysis/references/validation.md",
            include_bytes!("../../agent/skills/data-analysis/references/validation.md"),
        )],
    },
    BundledSkill {
        id: PDF_SKILL_ID,
        name: "PDF",
        default_enabled: true,
        relative_path: "skills/pdf/SKILL.md",
        content: include_bytes!("../../agent/skills/pdf/SKILL.md"),
        support_files: &[(
            "skills/pdf/references/pdf-workflow.md",
            include_bytes!("../../agent/skills/pdf/references/pdf-workflow.md"),
        )],
    },
    BundledSkill {
        id: DOCUMENTS_SKILL_ID,
        name: "Word documents",
        default_enabled: true,
        relative_path: "skills/documents/SKILL.md",
        content: include_bytes!("../../agent/skills/documents/SKILL.md"),
        support_files: &[(
            "skills/documents/references/docx-workflow.md",
            include_bytes!("../../agent/skills/documents/references/docx-workflow.md"),
        )],
    },
    BundledSkill {
        id: SPREADSHEETS_SKILL_ID,
        name: "Spreadsheets",
        default_enabled: true,
        relative_path: "skills/spreadsheets/SKILL.md",
        content: include_bytes!("../../agent/skills/spreadsheets/SKILL.md"),
        support_files: &[(
            "skills/spreadsheets/references/workbook-workflow.md",
            include_bytes!("../../agent/skills/spreadsheets/references/workbook-workflow.md"),
        )],
    },
    BundledSkill {
        id: PRESENTATIONS_SKILL_ID,
        name: "Presentations",
        default_enabled: true,
        relative_path: "skills/presentations/SKILL.md",
        content: include_bytes!("../../agent/skills/presentations/SKILL.md"),
        support_files: &[(
            "skills/presentations/references/deck-workflow.md",
            include_bytes!("../../agent/skills/presentations/references/deck-workflow.md"),
        )],
    },
];

/// App procedures are always discoverable; their bodies load only when needed.
/// They do not grant connection access or Memory permissions.
const ALWAYS_ON_SKILLS: &[BundledSkill] = &[
    BundledSkill {
        id: WORLD_SKILL_ID,
        name: "World",
        default_enabled: true,
        relative_path: "skills/world/SKILL.md",
        content: include_bytes!("../../agent/skills/world/SKILL.md"),
        support_files: &[],
    },
    BundledSkill {
        id: LENS_SKILL_ID,
        name: "Lens",
        default_enabled: true,
        relative_path: "skills/lens/SKILL.md",
        content: include_bytes!("../../agent/skills/lens/SKILL.md"),
        support_files: &[],
    },
    BundledSkill {
        id: JOURNAL_SKILL_ID,
        name: "Journal",
        default_enabled: true,
        relative_path: "skills/journal/SKILL.md",
        content: include_bytes!("../../agent/skills/journal/SKILL.md"),
        support_files: &[],
    },
    BundledSkill {
        id: VISUALIZE_SKILL_ID,
        name: "Visualize",
        default_enabled: true,
        relative_path: "skills/visualize/SKILL.md",
        content: include_bytes!("../../agent/skills/visualize/SKILL.md"),
        support_files: &[],
    },
];

#[derive(Clone, Copy)]
struct BundledSkill {
    id: &'static str,
    name: &'static str,
    default_enabled: bool,
    relative_path: &'static str,
    content: &'static [u8],
    support_files: &'static [(&'static str, &'static [u8])],
}

fn skill_files(skill: &BundledSkill) -> impl Iterator<Item = (&'static str, &'static [u8])> + '_ {
    std::iter::once((skill.relative_path, skill.content)).chain(skill.support_files.iter().copied())
}

fn hash_skill(digest: &mut Sha256, skill: &BundledSkill) {
    digest.update([0]);
    digest.update(skill.id.as_bytes());
    for (path, content) in skill_files(skill) {
        digest.update([0]);
        digest.update(path.as_bytes());
        digest.update([0]);
        digest.update(content);
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AgentSkillSummary {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub fixed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentHarnessSnapshot {
    pub schema: String,
    pub mode: String,
    pub skill_ids: Vec<String>,
    pub capability_ids: Vec<String>,
    pub digest: String,
}

/// The exact Pi surface for one Chat run. Core tools start active. Bundled
/// component tools are registered with Pi but remain inactive until the agent
/// discovers and loads that component through the capability tools. Rust derives
/// both sets from this Chat run and the Loupist's sealed authority snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentRuntimeProfile {
    pub schema: String,
    pub mode: String,
    pub memory_access_enabled: bool,
    pub memory_update_enabled: bool,
    /// Exact enabled capability bindings represented by this run profile.
    /// Components may aggregate several providers behind one semantic tool.
    pub capability_ids: Vec<String>,
    pub core_tool_names: Vec<String>,
    pub components: Vec<AgentRuntimeComponent>,
    /// Host-owned names and purposes of unavailable bundled connections.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unavailable_connections: Vec<UnavailableConnection>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UnavailableConnection {
    pub name: String,
    pub description: String,
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AgentRuntimeComponent {
    pub id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    pub name: String,
    pub description: String,
    pub tool_names: Vec<String>,
    /// Plugin name when this component is a connection. Built-in packs omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// The host's bundled MCP runtime, which activates a default tool set on
    /// load and discovers the rest; a custom server exposes all its approved
    /// tools on connect. Only Rust sets it, and only for that runtime.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bundled: bool,
}

const MAX_PURPOSE_CHARS: usize = 160;

/// A component's purpose, cut at the last sentence end that fits, else the
/// last word, so the index never ends mid-word.
fn connection_purpose(description: &str) -> String {
    let text = description.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= MAX_PURPOSE_CHARS {
        return text;
    }
    let limit = text
        .char_indices()
        .nth(MAX_PURPOSE_CHARS)
        .map_or(text.len(), |(index, _)| index);
    let head = &text[..limit];
    let sentence_end = head
        .char_indices()
        .filter(|(_, character)| matches!(character, '.' | '!' | '?'))
        .map(|(index, character)| index + character.len_utf8())
        .filter(|end| head[*end..].chars().next().is_none_or(char::is_whitespace))
        .last();
    match sentence_end {
        Some(end) => head[..end].to_owned(),
        None => match head.rfind(char::is_whitespace) {
            Some(space) => head[..space].trim_end_matches([',', ';']).to_owned(),
            None => head.to_owned(),
        },
    }
}

impl AgentRuntimeProfile {
    pub fn new(
        mode: &str,
        use_memory: bool,
        propose_memory_updates: bool,
        capability_ids: &[String],
    ) -> Result<Self, AgentHarnessError> {
        if mode != "chat" {
            return Err(AgentHarnessError::Context);
        }
        use crate::broker::tools::TOOL_CATALOG;
        let capabilities = normalize_capability_ids(capability_ids)?;
        let allowed = |tool: &&crate::broker::tools::AgentToolSpec| {
            tool.permission.allows(use_memory, propose_memory_updates)
        };
        let core_tools = TOOL_CATALOG
            .tools
            .iter()
            .filter(allowed)
            .filter(|tool| tool.component.is_none())
            .map(|tool| tool.name.clone())
            .collect();
        let market =
            crate::marketplace::bundled_plugins().map_err(|_| AgentHarnessError::Context)?;
        let enabled_plugins = market
            .entries
            .iter()
            .filter(|entry| capabilities.iter().any(|id| id == &entry.id))
            .map(|entry| entry.plugin.clone())
            .collect::<std::collections::BTreeSet<_>>();
        let mut components = Vec::new();
        for component in &TOOL_CATALOG.components {
            if component
                .connection
                .as_ref()
                .is_some_and(|plugin| !enabled_plugins.contains(plugin))
            {
                continue;
            }
            let tool_names: Vec<_> = TOOL_CATALOG
                .tools
                .iter()
                .filter(allowed)
                .filter(|tool| tool.component.as_deref() == Some(component.id.as_str()))
                .map(|tool| tool.name.clone())
                .collect();
            if tool_names.is_empty() {
                continue;
            }
            components.push(AgentRuntimeComponent {
                id: component.id.clone(),
                kind: "tool".into(),
                server_id: None,
                name: component.name.clone(),
                description: connection_purpose(&component.description),
                tool_names,
                connection: component.connection.clone(),
                bundled: false,
            });
        }
        components.extend(mcp_runtime_components(&capabilities, &market)?);
        components.extend(capabilities.iter().filter(|id| crate::custom_connections::service::valid_id(id)).map(|id| AgentRuntimeComponent {
            id: id.clone(), kind: "mcp".into(), server_id: Some(id.clone()), name: "Custom data source".into(),
            description: "User-configured read-only data source. Load to discover approved tools and their connection name.".into(),
            tool_names: Vec::new(), connection: Some(id.clone()), bundled: false,
        }));
        let unavailable_connections = market
            .plugins
            .iter()
            .filter(|plugin| !enabled_plugins.contains(&plugin.name))
            .map(|plugin| UnavailableConnection {
                id: plugin.name.clone(),
                name: plugin.display_name.clone(),
                description: connection_purpose(&plugin.short_description),
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        Ok(Self {
            schema: RUNTIME_SCHEMA.to_owned(),
            mode: mode.to_owned(),
            memory_access_enabled: use_memory,
            memory_update_enabled: propose_memory_updates,
            capability_ids: capabilities,
            core_tool_names: core_tools,
            components,
            unavailable_connections,
        })
    }

    /// Unattended (scheduled) runs see only the briefing allowlist: no native
    /// tools and no schedule management. The broker enforces the same policy.
    pub fn with_schedule_management(mut self, enabled: bool) -> Self {
        if !enabled {
            self.core_tool_names
                .retain(|name| crate::briefings::policy::allowed_name(name));
            for component in &mut self.components {
                component
                    .tool_names
                    .retain(|name| crate::briefings::policy::allowed_name(name));
            }
            self.components
                .retain(|component| component.kind == "mcp" || !component.tool_names.is_empty());
            if self.components.is_empty() {
                self.core_tool_names
                    .retain(|name| name != "capability_load");
            }
        }
        self
    }
}

fn mcp_runtime_components(
    capabilities: &[String],
    market: &crate::marketplace::BundledPluginIndex,
) -> Result<Vec<AgentRuntimeComponent>, AgentHarnessError> {
    let enabled = capabilities
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    let plugins = market
        .plugins
        .iter()
        .map(|plugin| (plugin.name.as_str(), plugin))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut servers = std::collections::BTreeMap::<String, String>::new();
    for entry in &market.entries {
        if !enabled.contains(&entry.id)
            || entry.runtime.kind != crate::marketplace::MarketplaceRuntimeKind::BundledMcp
        {
            continue;
        }
        let Some(server_id) = entry.runtime.server_id.clone() else {
            return Err(AgentHarnessError::Context);
        };
        let server = servers
            .entry(entry.plugin.clone())
            .or_insert_with(|| server_id);
        if *server != entry.runtime.server_id.as_deref().unwrap_or_default() {
            return Err(AgentHarnessError::Context);
        }
    }
    let mut namespaces = std::collections::BTreeSet::new();
    if servers
        .values()
        .map(|server_id| server_id.replace(['.', '-'], "_"))
        .any(|namespace| !namespaces.insert(namespace))
    {
        return Err(AgentHarnessError::Context);
    }
    servers
        .into_iter()
        .map(|(plugin, server_id)| {
            let summary = plugins
                .get(plugin.as_str())
                .ok_or(AgentHarnessError::Context)?;
            Ok(AgentRuntimeComponent {
                id: plugin.clone(),
                kind: "mcp".into(),
                server_id: Some(server_id),
                name: summary.display_name.clone(),
                description: connection_purpose(&summary.short_description),
                tool_names: Vec::new(),
                connection: Some(plugin),
                bundled: true,
            })
        })
        .collect()
}

impl AgentHarnessSnapshot {
    pub fn validate(&self) -> Result<(), AgentHarnessError> {
        if self.schema != HARNESS_SCHEMA || self.mode != "chat" {
            return Err(AgentHarnessError::Context);
        }
        let mut seen_skills = std::collections::BTreeSet::new();
        for id in &self.skill_ids {
            // Stored Chat locks are historical. They must stay readable after a
            // bundled Skill is removed; only new runs require the current catalog.
            if !historical_skill_id_is_valid(id) || !seen_skills.insert(id.as_str()) {
                return Err(AgentHarnessError::Skill);
            }
        }
        if normalize_capability_ids(&self.capability_ids)? != self.capability_ids
            || self.digest.len() != 64
            || !self
                .digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(AgentHarnessError::Context);
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum AgentHarnessError {
    #[error("workspace instructions are unavailable, invalid UTF-8, or exceed the bounded resource limits")]
    Workspace,
    #[error("unknown or duplicate agent skill")]
    Skill,
    #[error("bundled agent skill is missing or modified: {0}")]
    SkillFile(PathBuf),
    #[error("bundled agent extension is missing or modified: {0}")]
    ExtensionFile(PathBuf),
    #[error("agent harness context is malformed")]
    Context,
}

pub fn extension_bundle_sha256() -> String {
    let mut digest = Sha256::new();
    digest.update(b"loupist-agent-extension-bundle/1");
    for (name, content) in EXTENSION_FILES {
        digest.update([0]);
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update(content);
    }
    hex::encode(digest.finalize())
}

pub fn validate_extension_bundle(entrypoint: &Path) -> Result<(), AgentHarnessError> {
    if entrypoint.file_name().and_then(|name| name.to_str()) != Some(EXTENSION_ENTRYPOINT) {
        return Err(AgentHarnessError::ExtensionFile(entrypoint.to_path_buf()));
    }
    let root = entrypoint
        .parent()
        .ok_or_else(|| AgentHarnessError::ExtensionFile(entrypoint.to_path_buf()))?;
    validate_agent_files(root, EXTENSION_FILES)
}

pub fn validate_provider_extension_bundle(entrypoint: &Path) -> Result<(), AgentHarnessError> {
    if entrypoint.file_name().and_then(|name| name.to_str()) != Some(PROVIDER_EXTENSION_ENTRYPOINT)
    {
        return Err(AgentHarnessError::ExtensionFile(entrypoint.to_path_buf()));
    }
    let root = entrypoint
        .parent()
        .ok_or_else(|| AgentHarnessError::ExtensionFile(entrypoint.to_path_buf()))?;
    validate_agent_files(root, PROVIDER_EXTENSION_FILES)
}

fn validate_agent_files(root: &Path, files: &[(&str, &[u8])]) -> Result<(), AgentHarnessError> {
    for (name, expected) in files {
        #[cfg(feature = "eval-harness")]
        let registered = crate::eval_harness::artifacts::registered_bytes(&root.join(name));
        #[cfg(feature = "eval-harness")]
        let expected = &registered.as_deref().unwrap_or(expected);
        let relative = Path::new(name);
        let mut ancestor = relative.parent();
        while let Some(parent) = ancestor {
            if parent.as_os_str().is_empty() {
                break;
            }
            let path = root.join(parent);
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|_| AgentHarnessError::ExtensionFile(path.clone()))?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(AgentHarnessError::ExtensionFile(path));
            }
            ancestor = parent.parent();
        }
        let path = root.join(name);
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|_| AgentHarnessError::ExtensionFile(path.clone()))?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || std::fs::read(&path).ok().as_deref() != Some(*expected)
        {
            return Err(AgentHarnessError::ExtensionFile(path));
        }
    }
    Ok(())
}

pub fn default_skill_ids() -> Vec<String> {
    SELECTABLE_SKILLS
        .iter()
        .map(|skill| skill.id.to_owned())
        .collect()
}

/// Included Skills use the catalog flag. A Skill outside that catalog stays enabled.
pub fn skill_default_enabled(id: &str) -> bool {
    SELECTABLE_SKILLS
        .iter()
        .find(|skill| skill.id == id)
        .map(|skill| skill.default_enabled)
        .unwrap_or(true)
}

pub fn always_on_skill_ids() -> Vec<String> {
    ALWAYS_ON_SKILLS
        .iter()
        .map(|skill| skill.id.to_owned())
        .collect()
}

/// Skills Pi receives for a Chat run: always-on method cards, then the Loupist's
/// optional workflows. Only optional IDs are stored as Loupist selections.
pub fn run_skill_ids(selectable: &[String]) -> Result<Vec<String>, AgentHarnessError> {
    let selectable = normalize_selectable_skill_ids(selectable)?;
    let mut ids = always_on_skill_ids();
    for id in selectable {
        if ids.iter().any(|seen| seen == &id) {
            return Err(AgentHarnessError::Skill);
        }
        ids.push(id);
    }
    Ok(ids)
}

pub fn normalize_selectable_skill_ids(
    requested: &[String],
) -> Result<Vec<String>, AgentHarnessError> {
    let mut normalized = Vec::new();
    for skill in SELECTABLE_SKILLS {
        if requested.iter().any(|id| id == skill.id) {
            normalized.push(skill.id.to_owned());
        }
    }
    if normalized.len() != requested.len() {
        return Err(AgentHarnessError::Skill);
    }
    Ok(normalized)
}

pub(crate) fn bundled_skill_file_names(id: &str) -> Vec<String> {
    all_skills()
        .find(|skill| skill.id == id)
        .map(|skill| {
            let prefix = format!("skills/{id}/");
            skill_files(skill)
                .map(|(path, _)| path.strip_prefix(&prefix).unwrap_or(path).to_owned())
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn bundled_skill_content(id: &str) -> Option<&'static [u8]> {
    all_skills()
        .find(|skill| skill.id == id)
        .map(|skill| skill.content)
}

pub fn skill_catalog(enabled_skill_ids: &[String]) -> Vec<AgentSkillSummary> {
    all_skills()
        .map(|skill| {
            let fixed = ALWAYS_ON_SKILLS.iter().any(|fixed| fixed.id == skill.id);
            let (_, description) = bundled_skill_frontmatter(skill);
            AgentSkillSummary {
                id: skill.id.to_owned(),
                name: skill.name.to_owned(),
                description,
                enabled: fixed || enabled_skill_ids.iter().any(|id| id == skill.id),
                fixed,
            }
        })
        .collect()
}

pub fn resolve_skill_paths(
    agent_root: &Path,
    skill_ids: &[String],
) -> Result<Vec<PathBuf>, AgentHarnessError> {
    let mut paths = Vec::with_capacity(skill_ids.len());
    for id in skill_ids {
        let skill = bundled_skill(id)?;
        if paths
            .iter()
            .any(|existing| existing == &agent_root.join(skill.relative_path))
        {
            return Err(AgentHarnessError::Skill);
        }
        let path = agent_root.join(skill.relative_path);
        validate_skill_path(agent_root, &path)?;
        paths.push(path);
    }
    Ok(paths)
}

pub fn validate_skill_path(agent_root: &Path, path: &Path) -> Result<(), AgentHarnessError> {
    let skill = all_skills()
        .find(|skill| agent_root.join(skill.relative_path) == path)
        .ok_or(AgentHarnessError::SkillFile(path.to_path_buf()))?;
    let files = skill_files(skill).collect::<Vec<_>>();
    validate_agent_files(agent_root, &files)
        .map_err(|_| AgentHarnessError::SkillFile(path.to_path_buf()))
}

pub fn snapshot(
    mode: &str,
    skill_ids: &[String],
    capability_ids: &[String],
) -> Result<AgentHarnessSnapshot, AgentHarnessError> {
    if mode != "chat" {
        return Err(AgentHarnessError::Context);
    }
    let mut normalized_skills = Vec::with_capacity(skill_ids.len());
    for id in skill_ids {
        selectable_skill(id)?;
        if normalized_skills.iter().any(|seen| seen == id) {
            return Err(AgentHarnessError::Skill);
        }
        normalized_skills.push(id.clone());
    }
    let normalized_capabilities = normalize_capability_ids(capability_ids)?;
    let mut digest = Sha256::new();
    digest.update(HARNESS_SCHEMA.as_bytes());
    digest.update([0]);
    digest.update(mode.as_bytes());
    digest.update([0]);
    digest.update(include_bytes!("../../agent/SYSTEM.md"));
    digest.update([0]);
    digest.update(extension_bundle_sha256().as_bytes());
    for skill in ALWAYS_ON_SKILLS {
        hash_skill(&mut digest, skill);
    }
    for id in &normalized_skills {
        hash_skill(&mut digest, bundled_skill(id)?);
    }
    for id in &normalized_capabilities {
        digest.update([0]);
        digest.update(id.as_bytes());
    }
    Ok(AgentHarnessSnapshot {
        schema: HARNESS_SCHEMA.to_owned(),
        mode: mode.to_owned(),
        skill_ids: normalized_skills,
        capability_ids: normalized_capabilities,
        digest: hex::encode(digest.finalize()),
    })
}

fn normalize_capability_ids(capability_ids: &[String]) -> Result<Vec<String>, AgentHarnessError> {
    let mut normalized = capability_ids.to_vec();
    normalized.sort();
    if normalized.windows(2).any(|pair| pair[0] == pair[1])
        || normalized.iter().any(|id| {
            id.is_empty()
                || id.len() > 96
                || !id.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-".contains(&byte)
                })
                || !id.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
                || !id.as_bytes().last().is_some_and(u8::is_ascii_alphanumeric)
        })
    {
        return Err(AgentHarnessError::Context);
    }
    Ok(normalized)
}

/// The host serializes one typed context after freezing the turn. Internal
/// seals stay here for validation; the extension exposes only actionable context
/// to the model. Skills never change the runtime's Memory permissions.
#[derive(Clone, Debug, Serialize)]
pub struct AgentHostContext {
    pub agent_harness: AgentHarnessSnapshot,
    pub agent_runtime: AgentRuntimeProfile,
    pub workspace: AgentWorkspaceSnapshot,
    pub preload_component_ids: Vec<String>,
    pub explicit_skill_names: Vec<String>,
    pub surface: crate::chat::surface::ChatSurface,
}

impl AgentHostContext {
    pub fn tool_policy(
        &self,
        loupist_id: String,
        session_id: String,
        as_of: Option<String>,
    ) -> crate::broker::ToolPolicy {
        let runtime = &self.agent_runtime;
        crate::broker::ToolPolicy {
            loupist_id,
            session_id,
            as_of,
            use_memory: runtime.memory_access_enabled,
            propose_memory_updates: runtime.memory_update_enabled,
            memory_proposal_budget: if runtime.memory_update_enabled {
                crate::broker::MAX_MEMORY_PROPOSALS
            } else {
                0
            },
        }
    }
}

/// Map composer `@entry_id` mentions onto runtime component ids that the agent
/// extension can preload. Unknown or disabled ids are ignored.
pub fn preload_component_ids_for_mentions(
    mentioned_entry_ids: &[String],
    capability_ids: &[String],
    profile: &AgentRuntimeProfile,
) -> Result<Vec<String>, AgentHarnessError> {
    if mentioned_entry_ids.is_empty() {
        return Ok(Vec::new());
    }
    let enabled = capability_ids
        .iter()
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    let catalog = crate::marketplace::bundled_catalog().map_err(|_| AgentHarnessError::Context)?;
    let entries_by_id = catalog
        .entries
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut preload = std::collections::BTreeSet::new();
    for entry_id in mentioned_entry_ids {
        if !enabled.contains(entry_id) {
            continue;
        }
        if crate::custom_connections::service::valid_id(entry_id)
            && profile.components.iter().any(|c| &c.id == entry_id)
        {
            preload.insert(entry_id.clone());
            continue;
        }
        let Some(entry) = entries_by_id.get(entry_id.as_str()) else {
            continue;
        };
        let component_id = entry.plugin.clone();
        if profile
            .components
            .iter()
            .any(|component| component.id == component_id)
        {
            preload.insert(component_id);
        }
    }
    Ok(preload.into_iter().collect())
}

/// A connection the user named for this turn. The extension preloads it; the
/// model hears about that in the turn envelope, not the cached system prefix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MentionedConnection {
    pub capability_id: String,
    pub name: String,
    pub provider_ids: Vec<String>,
}

pub fn mentioned_connections(
    mentioned_entry_ids: &[String],
    profile: &AgentRuntimeProfile,
) -> Vec<MentionedConnection> {
    let Ok(market) = crate::marketplace::bundled_plugins() else {
        return Vec::new();
    };
    let enabled = profile
        .capability_ids
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    let entries = market
        .entries
        .iter()
        .map(|entry| (entry.id.as_str(), entry))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut grouped =
        std::collections::BTreeMap::<String, (String, std::collections::BTreeSet<String>)>::new();
    for entry_id in mentioned_entry_ids {
        if !enabled.contains(entry_id) {
            continue;
        }
        if crate::custom_connections::service::valid_id(entry_id) {
            if let Some(component) = profile.components.iter().find(|c| &c.id == entry_id) {
                grouped
                    .entry(entry_id.clone())
                    .or_insert_with(|| (component.name.clone(), Default::default()));
            }
            continue;
        }
        let Some(entry) = entries.get(entry_id.as_str()) else {
            continue;
        };
        let Some(component) = profile
            .components
            .iter()
            .find(|component| component.id == entry.plugin)
        else {
            continue;
        };
        let slot = grouped
            .entry(component.id.clone())
            .or_insert_with(|| (component.name.clone(), std::collections::BTreeSet::new()));
        slot.1.extend(entry.runtime.provider_ids.iter().cloned());
    }
    grouped
        .into_iter()
        .map(
            |(capability_id, (name, provider_ids))| MentionedConnection {
                capability_id,
                name,
                provider_ids: provider_ids.into_iter().collect(),
            },
        )
        .collect()
}

/// The user's as-of date for this turn (finance-point-in-time.md §7): answer
/// only from what was published by the end of that day.
pub fn as_of_envelope(cutoff: &str) -> Value {
    json!({
        "schema": "loupist-as-of/1",
        "authority": "user_request",
        "cutoff": cutoff,
        "end_of_day": format!("{cutoff}T23:59:59 local"),
        "rule": format!("Answer only from what was published or known by the end of {cutoff}. Tools and Memory already keep to this date; do not add later facts, prices, filings or outcomes from your own knowledge. Say what could not be shown as known then. The user's principles apply as they are today."),
        "not_evidence": true
    })
}

pub fn attach_chat_surface(envelope: &str, surface: &crate::chat::surface::ChatSurface) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(envelope) else {
        return envelope.to_owned();
    };
    let Some(object) = value.as_object_mut() else {
        return envelope.to_owned();
    };
    object.insert("chat_surface".into(), surface.envelope_block());
    serde_json::to_string(&value).unwrap_or_else(|_| envelope.to_owned())
}

/// Adds the as-of envelope beside the live-time envelope.
pub fn attach_as_of(envelope: &str, cutoff: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(envelope) else {
        return envelope.to_owned();
    };
    let Some(object) = value.as_object_mut() else {
        return envelope.to_owned();
    };
    object.insert("as_of".into(), as_of_envelope(cutoff));
    serde_json::to_string(&value).unwrap_or_else(|_| envelope.to_owned())
}

pub fn live_time_envelope(current_utc: DateTime<Utc>) -> Value {
    json!({
        "schema": "loupist-live-time/1",
        "authority": "rust_host_wall_clock",
        "current_utc": current_utc.to_rfc3339_opts(SecondsFormat::Millis, true),
        "current_date_utc": current_utc.format("%Y-%m-%d").to_string(),
        "applies_to": "live_chat_only",
        "not_evidence": true
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LearnedMemoryIndexEntry {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub as_of: String,
    pub kind: String,
    #[serde(rename = "pageKind", skip_serializing_if = "Option::is_none")]
    pub lens_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sections: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
}

/// Discovery metadata only; never a read receipt or admitted context read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelevantMemoryPointer {
    #[serde(rename = "laterNotes", skip_serializing_if = "Vec::is_empty")]
    pub later_notes: Vec<crate::knowledge::LaterNote>,
    pub kind: String,
    pub title: String,
    pub as_of: String,
    pub snippet: String,
    pub tool: String,
    #[serde(rename = "readArgs")]
    pub read_args: Value,
}

pub fn attach_relevant_memory(envelope: &str, relevant: &[RelevantMemoryPointer]) -> String {
    if relevant.is_empty() {
        return envelope.to_owned();
    }
    let Ok(mut value) = serde_json::from_str::<Value>(envelope) else {
        return envelope.to_owned();
    };
    if let Some(protocol) = value
        .get_mut("memory_protocol")
        .and_then(Value::as_object_mut)
    {
        if protocol.get("active").and_then(Value::as_bool) == Some(true) {
            protocol.insert("relevant".into(), json!(relevant));
            protocol.insert("relevantHint".into(), json!("These are discovery pointers, not reads or receipts. Exact-read with the supplied tool and readArgs before relying on a match; say when a pointer does not apply. Pointers do not authorize Journals or citations."));
        }
    }
    value.to_string()
}

pub fn attach_world_kinds(envelope: &str, kinds: &Value) -> String {
    let Ok(mut value) = serde_json::from_str::<Value>(envelope) else {
        return envelope.to_owned();
    };
    if let Some(protocol) = value
        .get_mut("memory_protocol")
        .and_then(Value::as_object_mut)
    {
        if protocol.get("active").and_then(Value::as_bool) == Some(true) {
            protocol.insert("worldKinds".into(), kinds.clone());
        }
    }
    value.to_string()
}

#[allow(clippy::too_many_arguments)]
pub fn turn_envelope_block(
    current_utc: DateTime<Utc>,
    use_memory: bool,
    learned_index: &[LearnedMemoryIndexEntry],
    charter: Option<&str>,
    charter_read_ref: Option<&str>,
    lens_catalog: Option<(usize, usize)>,
    world_catalog: Option<(usize, usize)>,
    mentioned: &[MentionedConnection],
) -> Result<String, AgentHarnessError> {
    let mut envelope = json!({
        "live_time": live_time_envelope(current_utc)
    });
    if !mentioned.is_empty() {
        envelope["mentioned_connections"] = json!(mentioned);
    }
    if use_memory {
        let mut memory_protocol = json!({
            "active": true,
            "learned_index": learned_index
        });
        if let Some((shown, total)) = lens_catalog {
            memory_protocol["lensCatalog"] = json!({
                "shown": shown,
                "total": total,
                "truncated": shown < total
            });
        }
        if let Some((shown, total)) = world_catalog {
            memory_protocol["worldCatalog"] = json!({
                "shown": shown,
                "total": total,
                "truncated": shown < total,
                "hint": "Call world_search for World records that are not in learned_index."
            });
        }
        if let Some(body) = charter.filter(|body| !body.is_empty()) {
            memory_protocol["charter"] = json!(body);
            if let Some(read_ref) = charter_read_ref.filter(|value| !value.is_empty()) {
                memory_protocol["charterReadRef"] = json!(read_ref);
            }
        }
        envelope["memory_protocol"] = memory_protocol;
    }
    serde_json::to_string(&envelope).map_err(|_| AgentHarnessError::Context)
}

/// Profile instructions are definition, not Memory. Bound again at delivery so
/// imported/corrupt profiles cannot bypass the normal save-time limit.
pub(crate) fn loupist_instructions(instructions: &str) -> String {
    instructions.trim().chars().take(4_000).collect()
}

pub(crate) const FOLLOWED_COMPANIES_HINT: &str = "These are companies the user holds (holding: true) or watches (watching: true); a company can be both. Use them to make answers relevant; never infer position sizes or give personalized buy, sell, or hold advice.";

/// Only the identity projection can enter this function: financial position
/// fields cannot accidentally be serialized. One row per identity carries both
/// flags, because imported holdings are usually watched too; holdings fill the
/// 60-row cap first.
pub(crate) fn followed_companies(mut context: crate::store::user_context::MapContext) -> Value {
    let mut index = std::collections::BTreeMap::new();
    let mut rows: Vec<Value> = Vec::new();
    for (flag, items) in [
        ("holding", &mut context.holdings),
        ("watching", &mut context.watchlist),
    ] {
        items.sort_by(|a, b| a.symbol.cmp(&b.symbol).then(a.market.cmp(&b.market)));
        for item in items.iter() {
            let key = (item.symbol.clone(), item.market.clone());
            if let Some(&row) = index.get(&key) {
                let row: &mut Value = &mut rows[row];
                row[flag] = json!(true);
                continue;
            }
            if rows.len() == 60 {
                continue;
            }
            index.insert(key, rows.len());
            rows.push(json!({
                "symbol": item.symbol.chars().take(64).collect::<String>(),
                "market": item.market.chars().take(64).collect::<String>(),
                "name": item.display_name.as_deref().map(|name| name.chars().take(256).collect::<String>()),
                "holding": flag == "holding",
                "watching": flag == "watching",
            }));
        }
    }
    json!({"hint": FOLLOWED_COMPANIES_HINT, "symbols": rows})
}

pub(crate) const FOLLOWED_INTERESTS_HINT: &str = "These are companies the user watches. This answer looks back to an earlier date, so what the user holds today is left out. Use them only to make answers relevant; never give personalized buy, sell, or hold advice.";

/// Under an as-of date the user's interests stay but holdings never reach the
/// model (finance-point-in-time.md §3.2): watched companies only, by symbol,
/// without flags.
pub(crate) fn followed_interests(mut context: crate::store::user_context::MapContext) -> Value {
    context
        .watchlist
        .sort_by(|a, b| a.symbol.cmp(&b.symbol).then(a.market.cmp(&b.market)));
    context
        .watchlist
        .dedup_by(|a, b| a.symbol == b.symbol && a.market == b.market);
    let rows: Vec<Value> = context
        .watchlist
        .iter()
        .take(60)
        .map(|item| {
            json!({
                "symbol": item.symbol.chars().take(64).collect::<String>(),
                "market": item.market.chars().take(64).collect::<String>(),
                "name": item.display_name.as_deref().map(|name| name.chars().take(256).collect::<String>()),
            })
        })
        .collect();
    json!({"hint": FOLLOWED_INTERESTS_HINT, "symbols": rows})
}

/// Due checks are discovery, never automatic delivery of their frozen evidence.
/// In particular, earlier Journal inputs can contain recursively retained text.
pub(crate) fn journal_review_context(record: &Value) -> Value {
    let text = |key: &str, max: usize| {
        record[key]
            .as_str()
            .unwrap_or_default()
            .chars()
            .take(max)
            .collect::<String>()
    };
    let mut pointer = json!({
        "journalId": text("journalId", 128),
        "judgment": text("judgment", 1_000),
        "reasoning": text("reasoning", 2_000),
        "readArgs": {"action":"read", "journalId":text("journalId", 128)},
        "notEvidence": true,
    });
    if let Some(check) = record.get("check").filter(|check| check.is_object()) {
        let mut bounded = json!({});
        for (key, max) in [
            ("claim", 2_000),
            ("metric", 200),
            ("unit", 80),
            ("byDate", 10),
            ("direction", 5),
        ] {
            if let Some(text) = check[key].as_str() {
                bounded[key] = json!(text.chars().take(max).collect::<String>());
            }
        }
        for key in ["threshold", "confidence"] {
            if check[key].is_number() {
                bounded[key] = check[key].clone();
            }
        }
        pointer["check"] = bounded;
    }
    pointer
}

fn bound_automatic_memory(value: &mut Value) {
    if let Some(note) = value
        .pointer_mut("/memory_protocol/workingNote/current")
        .and_then(Value::as_object_mut)
    {
        // The note's 16 KB text and bounded identity/lineage metadata suffice;
        // evidence remains behind conversation_memory's exact-read boundary.
        note.remove("inputs");
    }
    if let Some(due) = value
        .pointer_mut("/memory_protocol/dueJournals")
        .and_then(Value::as_array_mut)
    {
        *due = due.iter().take(5).map(journal_review_context).collect();
    }
}

pub(crate) fn attach_loupist_context(
    envelope: &str,
    instructions: &str,
    tone: crate::domain::LoupistTone,
    followed: Option<&Value>,
) -> String {
    let mut value: Value = serde_json::from_str(envelope).expect("host turn envelope");
    bound_automatic_memory(&mut value);
    let instructions = loupist_instructions(instructions);
    // An empty current definition must also replace instructions from history.
    value["Instructions from the user for this Loupist"] = json!(instructions);
    value["Loupist tone"] = json!(tone);
    if value
        .pointer("/memory_protocol/active")
        .and_then(Value::as_bool)
        == Some(true)
    {
        if let Some(followed) = followed {
            value["What the user follows"] = followed.clone();
        }
    }
    value.to_string()
}

/// Render stable Loupist-level blocks before catalogs and volatile turn data.
/// Internal envelopes retain their original paths for digests/read accounting;
/// only the model-facing serialization is ordered here. JSON escaping keeps
/// user-authored text within its block, even when it contains markup.
pub(crate) fn render_turn_context(envelope: &str) -> String {
    let mut value: Value = serde_json::from_str(envelope).expect("host turn envelope");
    let mut blocks: Vec<(String, Value)> = Vec::new();
    if let Some(memory) = value
        .get_mut("memory_protocol")
        .and_then(Value::as_object_mut)
    {
        if let Some(charter) = memory.remove("charter") {
            blocks.push(("Standing perspective".into(), charter));
        }
    }
    let object = value.as_object_mut().expect("host turn envelope object");
    for key in [
        "User principles",
        "user_principle_revisions",
        "Instructions from the user for this Loupist",
        "What the user follows",
        "Loupist tone",
        "chat_surface",
        "memory_protocol",
        "mentioned_connections",
    ] {
        if let Some(block) = object.remove(key) {
            blocks.push((key.into(), block));
        }
    }
    let live_time = object.remove("live_time");
    blocks.extend(
        object
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    if let Some(time) = live_time {
        blocks.push(("live_time".into(), time));
    }
    format!(
        "{{{}}}",
        blocks
            .into_iter()
            .map(|(key, value)| format!("{}:{}", json!(key), value))
            .collect::<Vec<_>>()
            .join(",")
    )
}

/// Hash of the turn envelope without live time. Warm sessions resend this
/// digest instead of the full Memory body until compaction or a real change.
pub fn envelope_context_digest(envelope: &str) -> String {
    let mut value: Value = serde_json::from_str(envelope).unwrap_or_else(|_| json!({}));
    if let Some(object) = value.as_object_mut() {
        object.remove("live_time");
        if let Some(memory) = object
            .get_mut("memory_protocol")
            .and_then(Value::as_object_mut)
        {
            memory.remove("charterReadRef");
        }
    }
    let canonical = serde_json::to_string(&value).unwrap_or_default();
    crate::util::sha256(canonical.as_bytes())
}

pub fn maybe_abbreviate_envelope(
    envelope: &str,
    previous_digest: Option<&str>,
    resumed: bool,
) -> (String, String) {
    let digest = envelope_context_digest(envelope);
    if resumed && previous_digest == Some(digest.as_str()) {
        let parsed: Value = serde_json::from_str(envelope).unwrap_or_else(|_| json!({}));
        let mut short = json!({
            "live_time": parsed.get("live_time").cloned().unwrap_or_else(|| json!({})),
            "context": { "unchanged": true, "digest": digest }
        });
        for key in [
            "Instructions from the user for this Loupist",
            "What the user follows",
            "Loupist tone",
        ] {
            if let Some(value) = parsed.get(key) {
                short[key] = value.clone();
            }
        }
        for key in [
            "charterReadRef",
            "dueJournals",
            "record",
            "relevant",
            "relevantHint",
        ] {
            if let Some(value) = parsed.pointer(&format!("/memory_protocol/{key}")) {
                if short.get("memory_protocol").is_none() {
                    short["memory_protocol"] = json!({});
                }
                short["memory_protocol"][key] = value.clone();
            }
        }
        (
            serde_json::to_string(&short).unwrap_or_else(|_| envelope.to_owned()),
            digest,
        )
    } else {
        (envelope.to_owned(), digest)
    }
}

fn bundled_skill_frontmatter(skill: &BundledSkill) -> (String, String) {
    let markdown = std::str::from_utf8(skill.content).expect("bundled skill is utf-8");
    let mut lines = markdown.lines();
    assert_eq!(lines.next(), Some("---"), "bundled skill frontmatter");
    let mut name = None;
    let mut description = None;
    let mut closed = false;
    for line in lines.by_ref() {
        if line == "---" {
            closed = true;
            break;
        }
        if let Some(value) = line.strip_prefix("name: ") {
            assert!(
                name.replace(value.to_owned()).is_none(),
                "duplicate bundled skill name"
            );
        }
        if let Some(value) = line.strip_prefix("description: ") {
            assert!(
                description.replace(value.to_owned()).is_none(),
                "duplicate bundled skill description"
            );
        }
    }
    assert!(closed, "bundled skill frontmatter is not closed");
    let name = name.expect("bundled skill name");
    assert_eq!(name, skill.id, "bundled skill name differs from id");
    let description = description.expect("bundled skill description");
    assert!(
        !description.trim().is_empty(),
        "bundled skill description is empty"
    );
    (name, description)
}

fn selectable_skill(id: &str) -> Result<&'static BundledSkill, AgentHarnessError> {
    SELECTABLE_SKILLS
        .iter()
        .find(|skill| skill.id == id)
        .ok_or(AgentHarnessError::Skill)
}

fn bundled_skill(id: &str) -> Result<&'static BundledSkill, AgentHarnessError> {
    all_skills()
        .find(|skill| skill.id == id)
        .ok_or(AgentHarnessError::Skill)
}

fn historical_skill_id_is_valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && id.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        && id.as_bytes().last().is_some_and(u8::is_ascii_alphanumeric)
}

fn all_skills() -> impl Iterator<Item = &'static BundledSkill> {
    ALWAYS_ON_SKILLS.iter().chain(SELECTABLE_SKILLS.iter())
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_purpose_cut_ends_at_a_sentence_or_word() {
        let sentences = "Keep this Chat's progress. Retain delivered originals for later reading. File what a turn established into Lens pages and save the user's stated rules as principles today.";
        assert!(sentences.len() > 160);
        assert_eq!(
            super::connection_purpose(sentences),
            "Keep this Chat's progress. Retain delivered originals for later reading."
        );
        let words = "word ".repeat(40);
        let cut = super::connection_purpose(&words);
        assert!(cut.len() < 160 && cut.ends_with("word"), "{cut}");
        assert_eq!(super::connection_purpose("  short   text "), "short text");
    }

    use super::*;
    use chrono::TimeZone;

    #[test]
    fn scheduled_runtime_has_no_native_or_schedule_management_tools() {
        let interactive = AgentRuntimeProfile::new("chat", false, false, &[]).unwrap();
        assert!(interactive
            .core_tool_names
            .iter()
            .any(|name| name == "bash"));
        let scheduled = interactive.with_schedule_management(false);
        assert!(!scheduled.core_tool_names.iter().any(|name| name == "bash"));
        assert!(!scheduled
            .components
            .iter()
            .any(|component| component.id == "schedules"));
    }

    fn write_agent_files(root: &Path, files: &[(&str, &[u8])]) {
        for (name, content) in files {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
    }

    #[test]
    fn extension_bundles_bind_exactly_the_modules_each_entrypoint_loads() {
        let temporary = tempfile::tempdir().unwrap();
        write_agent_files(temporary.path(), EXTENSION_FILES);
        write_agent_files(temporary.path(), PROVIDER_EXTENSION_FILES);
        let chat = temporary.path().join(EXTENSION_ENTRYPOINT);
        let support = temporary.path().join(PROVIDER_EXTENSION_ENTRYPOINT);
        validate_extension_bundle(&chat).unwrap();
        validate_provider_extension_bundle(&support).unwrap();

        std::fs::write(temporary.path().join("tool-catalog.json"), b"{}").unwrap();
        assert!(validate_extension_bundle(&chat).is_err());
        validate_provider_extension_bundle(&support).unwrap();
        write_agent_files(temporary.path(), EXTENSION_FILES);

        // Native search belongs to the provider extension only.
        std::fs::write(
            temporary.path().join("native-search/common.mjs"),
            b"tampered",
        )
        .unwrap();
        validate_extension_bundle(&chat).unwrap();
        assert!(validate_provider_extension_bundle(&support).is_err());
        write_agent_files(temporary.path(), PROVIDER_EXTENSION_FILES);

        // Both entrypoints load the shared run controls.
        std::fs::write(temporary.path().join("model-run-controls.mjs"), b"tampered").unwrap();
        assert!(validate_extension_bundle(&chat).is_err());
        assert!(validate_provider_extension_bundle(&support).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn extension_bundles_reject_a_symlinked_module_directory() {
        use std::os::unix::fs::symlink;

        let temporary = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        write_agent_files(temporary.path(), PROVIDER_EXTENSION_FILES);
        write_agent_files(external.path(), PROVIDER_EXTENSION_FILES);
        std::fs::remove_dir_all(temporary.path().join("native-search")).unwrap();
        symlink(
            external.path().join("native-search"),
            temporary.path().join("native-search"),
        )
        .unwrap();
        assert!(validate_provider_extension_bundle(
            &temporary.path().join(PROVIDER_EXTENSION_ENTRYPOINT)
        )
        .is_err());
    }

    #[test]
    fn bundled_skill_references_are_required_and_integrity_checked() {
        let skill = SELECTABLE_SKILLS
            .iter()
            .find(|skill| !skill.support_files.is_empty())
            .unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let files = skill_files(skill).collect::<Vec<_>>();
        write_agent_files(temporary.path(), &files);
        let ids = vec![skill.id.to_owned()];
        resolve_skill_paths(temporary.path(), &ids).unwrap();

        let reference = temporary.path().join(skill.support_files[0].0);
        std::fs::write(&reference, b"changed instructions").unwrap();
        assert!(resolve_skill_paths(temporary.path(), &ids).is_err());
        write_agent_files(temporary.path(), &files);
        std::fs::remove_file(&reference).unwrap();
        assert!(resolve_skill_paths(temporary.path(), &ids).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn bundled_skills_reject_symlinked_reference_directories() {
        let skill = SELECTABLE_SKILLS
            .iter()
            .find(|skill| !skill.support_files.is_empty())
            .unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let files = skill_files(skill).collect::<Vec<_>>();
        write_agent_files(temporary.path(), &files);
        write_agent_files(external.path(), &files);
        let directory = Path::new(skill.support_files[0].0).parent().unwrap();
        std::fs::remove_dir_all(temporary.path().join(directory)).unwrap();
        std::os::unix::fs::symlink(
            external.path().join(directory),
            temporary.path().join(directory),
        )
        .unwrap();
        assert!(resolve_skill_paths(temporary.path(), &[skill.id.to_owned()]).is_err());
    }

    #[test]
    fn the_memory_envelope_carries_world_kinds_only_while_memory_is_active() {
        let kinds = json!({"core": {"types": {"Technology": "a science"}}, "version": 0});
        for memory in [false, true] {
            let base =
                turn_envelope_block(Utc::now(), memory, &[], None, None, None, None, &[]).unwrap();
            let value: Value = serde_json::from_str(&attach_world_kinds(&base, &kinds)).unwrap();
            assert_eq!(value.get("memory_protocol").is_some(), memory);
            if memory {
                assert_eq!(value["memory_protocol"]["worldKinds"], kinds);
            }
        }
    }

    #[test]
    fn turn_envelope_gates_memory_and_bounds_loupist_context() {
        let now = Utc.with_ymd_and_hms(2026, 8, 10, 1, 2, 3).unwrap();
        let followed = followed_companies(Default::default());
        let instructions = "한😀".repeat(2_500);
        let pointer = RelevantMemoryPointer {
            later_notes: Vec::new(),
            kind: "WorkingNote".into(),
            title: "Acme covenant".into(),
            as_of: "2026-08-09".into(),
            snippet: "User supplied covenant headroom.".into(),
            tool: "conversation_memory".into(),
            read_args: json!({"action":"read", "noteId":"note-a"}),
        };
        for memory in [false, true] {
            let base = turn_envelope_block(
                now,
                memory,
                &[],
                Some("charter-body"),
                Some("rdg_charter"),
                None,
                None,
                &[],
            )
            .unwrap();
            let full = attach_relevant_memory(
                &attach_loupist_context(
                    &base,
                    &instructions,
                    crate::domain::LoupistTone::Direct,
                    Some(&followed),
                ),
                std::slice::from_ref(&pointer),
            );
            let value: Value = serde_json::from_str(&full).unwrap();
            assert_eq!(value["live_time"]["current_date_utc"], "2026-08-10");
            assert_eq!(
                value["Instructions from the user for this Loupist"]
                    .as_str()
                    .unwrap()
                    .chars()
                    .count(),
                4_000
            );
            assert_eq!(value.get("memory_protocol").is_some(), memory);
            assert_eq!(value.get("What the user follows").is_some(), memory);
            assert_eq!(value["Loupist tone"], "direct");
            let careful = attach_loupist_context(
                &base,
                &instructions,
                crate::domain::LoupistTone::Careful,
                Some(&followed),
            );
            assert!(render_turn_context(&careful).contains("careful"));
            assert_ne!(
                envelope_context_digest(&full),
                envelope_context_digest(&careful)
            );
            assert_eq!(full.contains("charter-body"), memory);
            assert_eq!(full.contains("Acme covenant"), memory);
            let changed = attach_loupist_context(
                &base,
                "Changed instruction",
                crate::domain::LoupistTone::Direct,
                Some(&followed),
            );
            assert_ne!(
                envelope_context_digest(&full),
                envelope_context_digest(&changed)
            );
        }
    }

    #[test]
    fn warm_envelope_abbreviates_until_the_digest_changes_but_repeats_due_work() {
        let envelope = json!({"live_time": {}, "memory_protocol": {
            "active": true, "charter": "Prefer cash flow.", "charterReadRef": "read-a",
            "dueJournals": [{"journalId":"jnl_due"}],
            "record": {"cameTrue":1,"didNot":1,"unclear":0,"pending":1},
            "relevant": [{"title":"Acme covenant"}]
        }})
        .to_string();
        let digest = envelope_context_digest(&envelope);
        let (cold, _) = maybe_abbreviate_envelope(&envelope, Some(&digest), false);
        assert_eq!(cold, envelope);
        let (changed, _) = maybe_abbreviate_envelope(&envelope, Some("different"), true);
        assert_eq!(changed, envelope);

        // A fresh read receipt alone does not change the digest.
        let mut next: Value = serde_json::from_str(&envelope).unwrap();
        next["memory_protocol"]["charterReadRef"] = json!("read-b");
        let next = next.to_string();
        assert_eq!(digest, envelope_context_digest(&next));
        let (short, sent) = maybe_abbreviate_envelope(&next, Some(&digest), true);
        assert_eq!(sent, digest);
        let original: Value = serde_json::from_str(&next).unwrap();
        let short: Value = serde_json::from_str(&short).unwrap();
        assert_eq!(short["context"]["unchanged"], true);
        assert!(short["memory_protocol"].get("charter").is_none());
        for key in ["charterReadRef", "dueJournals", "record", "relevant"] {
            assert_eq!(
                short["memory_protocol"][key],
                original["memory_protocol"][key]
            );
        }
    }

    #[test]
    fn automatic_memory_omits_nested_evidence_and_bounds_due_reviews() {
        let record = json!({"journalId":"jnl_fixture", "judgment":"Review revenue", "inputs":{"earlier":[{"deliveredText":"private-evidence".repeat(10_000)}]}, "check":{"claim":"x".repeat(3_000),"metric":"revenue","byDate":"2026-10-28"}});
        let base = json!({"memory_protocol":{"active":true,"workingNote":{"current":{"researchId":"rsh_fixture","markdown":"Keep this note", "inputs":{"lenses":[{"markdown":"frozen-lens-evidence"}]}}},"dueJournals":vec![record; 20]}}).to_string();
        let result = attach_loupist_context(&base, "", crate::domain::LoupistTone::Balanced, None);
        assert!(!result.contains("private-evidence"));
        assert!(!result.contains("frozen-lens-evidence"));
        assert!(result.contains("Keep this note"));
        let value: Value = serde_json::from_str(&result).unwrap();
        let due = value["memory_protocol"]["dueJournals"].as_array().unwrap();
        assert_eq!(due.len(), 5);
        assert_eq!(due[0]["check"]["claim"].as_str().unwrap().len(), 2_000);
        assert!(result.len() < 15_000);
    }

    #[test]
    fn an_as_of_turn_sees_watched_names_and_never_holdings() {
        use crate::store::user_context::{MapContext, MapContextItem};
        let item = |symbol: &str| MapContextItem {
            symbol: symbol.into(),
            market: "KRX".into(),
            display_name: Some(format!("{symbol} name")),
        };
        let value = followed_interests(MapContext {
            watchlist: vec![item("990308"), item("990221")],
            holdings: vec![item("990221"), item("990777")],
            updated_at_ms: 0,
        });
        let rows = value["symbols"].as_array().unwrap();
        let symbols: Vec<_> = rows
            .iter()
            .map(|row| row["symbol"].as_str().unwrap())
            .collect();
        // A company held only is left out; a watched one carries no flag.
        assert_eq!(symbols, ["990221", "990308"]);
        assert!(rows
            .iter()
            .all(|row| row.get("holding").is_none() && row.get("watching").is_none()));
        assert!(!value.to_string().contains("990777"));
    }

    #[test]
    fn followed_companies_are_identity_only_ordered_and_capped_from_storage() {
        use crate::store::user_context::{HoldingsPosition, HoldingsSource};
        let root = tempfile::tempdir().unwrap();
        let state = crate::app::AppState::for_test(root.path().join("app"));
        for index in (0..59).rev() {
            state
                .store
                .watchlist_add(&format!("W{index:02}"), "NYSE", Some("Watch company"))
                .unwrap();
        }
        state
            .store
            .save_holdings_snapshot(
                HoldingsSource::Csv,
                ["ZZZ", "AAA", "W00"]
                    .into_iter()
                    .map(|symbol| HoldingsPosition {
                        symbol: symbol.into(),
                        market: "NYSE".into(),
                        display_name: Some("Holding company".into()),
                        quantity: "123456789".into(),
                        cost_basis: Some("998877".into()),
                        market_value: Some("11223344".into()),
                        currency: "USD".into(),
                    })
                    .collect(),
            )
            .unwrap();
        let value = followed_companies(state.store.map_context().unwrap());
        let rows = value["symbols"].as_array().unwrap();
        // Holdings fill the cap first; W00 is held and watched, so it is one row.
        let symbols = rows
            .iter()
            .map(|row| row["symbol"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(symbols.len(), 60);
        assert_eq!(symbols[..4], ["AAA", "W00", "ZZZ", "W01"]);
        assert_eq!(symbols[59], "W57");
        assert_eq!(
            (rows[1]["holding"].clone(), rows[1]["watching"].clone()),
            (json!(true), json!(true))
        );
        let base = turn_envelope_block(Utc::now(), true, &[], None, None, None, None, &[]).unwrap();
        let delivered = render_turn_context(&attach_loupist_context(
            &base,
            "Keep concise",
            crate::domain::LoupistTone::Balanced,
            Some(&value),
        ));
        for excluded in ["123456789", "998877", "11223344", "USD"] {
            assert!(!delivered.contains(excluded), "{excluded}");
        }
    }

    #[test]
    fn crypto_holdings_reach_the_turn_context_as_held_symbols_only() {
        let root = tempfile::tempdir().unwrap();
        let state = crate::app::AppState::for_test(root.path().join("app"));
        let preview = state
            .store
            .preview_crypto_bytes(b"asset,balance\nXXBT,0.4271\nSOL,41.0700001\n")
            .unwrap();
        state
            .store
            .holdings_commit_csv(&preview.preview_id)
            .unwrap();
        let address = "0xfB6916095ca1df60bB79Ce92cE3Ea74c37c5d359";
        let wallet = state
            .store
            .wallet_add(address, Some("Cold storage"))
            .unwrap();
        state
            .store
            .wallet_save_read(
                &wallet.id,
                crate::store::user_context::wallets::WalletRead {
                    balances: vec![("ETH".into(), "3.14159265".into())],
                    unlisted: 2,
                },
            )
            .unwrap();
        let value = followed_companies(state.store.map_context().unwrap());
        let held = value["symbols"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                (
                    row["symbol"].as_str().unwrap(),
                    row["market"].as_str().unwrap(),
                    row["holding"].as_bool().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            held,
            [
                ("BTC", "CRYPTO", true),
                ("ETH", "CRYPTO", true),
                ("SOL", "CRYPTO", true)
            ]
        );
        let base = turn_envelope_block(Utc::now(), true, &[], None, None, None, None, &[]).unwrap();
        let delivered = render_turn_context(&attach_loupist_context(
            &base,
            "Keep concise",
            crate::domain::LoupistTone::Balanced,
            Some(&value),
        ));
        assert!(delivered.contains("BTC"));
        for excluded in [
            "0.4271",
            "41.0700001",
            "3.14159265",
            address,
            "0xfB69",
            "Cold storage",
        ] {
            assert!(!delivered.contains(excluded), "{excluded}");
        }
    }

    #[test]
    fn stored_harness_locks_remain_readable_after_bundled_skills_are_removed() {
        let snapshot = AgentHarnessSnapshot {
            schema: HARNESS_SCHEMA.to_owned(),
            mode: "chat".into(),
            skill_ids: vec!["finance-research".into(), "investment-postmortem".into()],
            capability_ids: vec!["community.web-research".into()],
            digest: "a".repeat(64),
        };
        snapshot.validate().unwrap();
        let invalid = AgentHarnessSnapshot {
            skill_ids: vec!["Finance Research".into()],
            ..snapshot.clone()
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn skill_authoring_is_opt_in_and_method_skills_are_not_selectable() {
        for authoring in ["skill-creator", "skill-installer"] {
            assert!(!skill_default_enabled(authoring));
        }
        assert!(skill_default_enabled(RESEARCH_SKILL_ID));
        let always_on = always_on_skill_ids();
        assert!(!always_on.is_empty());
        for id in &always_on {
            assert!(normalize_selectable_skill_ids(std::slice::from_ref(id)).is_err());
            assert!(snapshot("chat", std::slice::from_ref(id), &[]).is_err());
        }
    }

    #[test]
    fn mentions_preload_their_runtime_components_and_keep_the_named_provider() {
        let capability_ids = vec![
            "openbb.platform".into(),
            "fred.macro".into(),
            "sec.edgar".into(),
            "opendart.disclosures".into(),
        ];
        let profile = AgentRuntimeProfile::new("chat", false, false, &capability_ids).unwrap();
        let preload = preload_component_ids_for_mentions(
            &[
                "sec.edgar".into(),
                "opendart.disclosures".into(),
                "missing".into(),
            ],
            &capability_ids,
            &profile,
        )
        .unwrap();
        assert_eq!(preload, ["openbb", "opendart"]);
        let mentioned = mentioned_connections(&["fred.macro".into(), "missing".into()], &profile);
        assert_eq!(mentioned.len(), 1);
        assert_eq!(mentioned[0].capability_id, "openbb");
        assert_eq!(mentioned[0].provider_ids, ["fred"]);
    }

    #[test]
    fn memory_write_tools_exist_only_when_learning_is_enabled() {
        const MEMORY_WRITES: [&str; 7] = [
            "source_save",
            "note_save",
            "journal_save",
            "journal_review",
            "principle_save",
            "world_schema_update",
            "loupist_setup",
        ];
        let tools = |profile: &AgentRuntimeProfile| {
            profile
                .core_tool_names
                .iter()
                .chain(profile.components.iter().flat_map(|c| &c.tool_names))
                .cloned()
                .collect::<Vec<_>>()
        };
        for (read, write) in [(false, false), (true, false), (true, true)] {
            let profile = AgentRuntimeProfile::new("chat", read, write, &[]).unwrap();
            assert_eq!(
                profile,
                AgentRuntimeProfile::new("chat", read, write, &[]).unwrap()
            );
            assert_eq!(
                (profile.memory_access_enabled, profile.memory_update_enabled),
                (read, write)
            );
            let available = tools(&profile);
            for mutation in MEMORY_WRITES {
                assert_eq!(
                    available.iter().any(|name| name == mutation),
                    write,
                    "{mutation}"
                );
                // Mutations are deferred behind capability_load, never core.
                assert!(!profile.core_tool_names.iter().any(|name| name == mutation));
            }
        }
    }

    #[test]
    fn selected_memory_skills_do_not_change_frozen_turn_authority() {
        let temporary = tempfile::tempdir().unwrap();
        let workspace = temporary.path().join("workspace");
        let app_data = temporary.path().join("app-data");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::create_dir_all(&app_data).unwrap();
        for (read, write) in [(false, false), (true, false), (true, true)] {
            let context = AgentHostContext {
                agent_harness: snapshot("chat", &[], &[]).unwrap(),
                agent_runtime: AgentRuntimeProfile::new("chat", read, write, &[]).unwrap(),
                workspace: workspace::workspace_snapshot_with_preferences(
                    &workspace,
                    &app_data,
                    &Default::default(),
                )
                .unwrap(),
                preload_component_ids: Vec::new(),
                explicit_skill_names: explicit_skill_names("$world $skill:lens"),
                surface: crate::chat::surface::ChatSurface::Session,
            };
            let policy = context.tool_policy("loupist-a".into(), "chat-a".into(), None);
            assert_eq!(
                (policy.use_memory, policy.propose_memory_updates),
                (read, write)
            );
            assert_eq!(policy.memory_proposal_budget > 0, write);
        }
    }
}
