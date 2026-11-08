use std::collections::{HashMap, HashSet};
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::DomainError;

const MAX_PROGRESS_TEXT_BYTES: usize = 512 * 1024;
const MAX_PROGRESS_LABEL_BYTES: usize = 512;
const MAX_PROGRESS_HREF_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatProgressStatus {
    Running,
    Succeeded,
    Failed,
    Stopped,
    /// A source that cannot show what was known on the turn's as-of date; not a failure.
    Skipped,
}

/// Why the host refused a failed step, when a security rule or the user's own
/// card answer did. The renderer explains it in the step and offers no override.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatStepRefusal {
    /// The turn read third-party text: a command on a host without a verified
    /// sandbox, an instruction write, a custom connection, a schedule change
    /// or a new site stays paused until the turn ends (broker code
    /// `untrusted_text_paused`).
    UntrustedTextPaused,
    /// The path is inside the app's private data, a credential store or a
    /// git directory's hooks or config (`protected_path`).
    ProtectedPath,
    /// The path is outside the Loupist folder (`outside_workspace`).
    OutsideWorkspace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatProgressCategory {
    Web,
    Memory,
    Capability,
    Finance,
    Files,
    Artifact,
    Compute,
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatProgressSourceKind {
    Web,
    Memory,
    Files,
    Finance,
    Connector,
    Artifact,
    Compute,
    Capability,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatProgressSource {
    pub kind: ChatProgressSourceKind,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatProgressOperation {
    Search,
    Read,
    Write,
    Edit,
    List,
    Calculate,
    Publish,
    Execute,
    Submit,
    Retry,
    Compact,
    Generic,
}

// A reply holds a few dozen steps at most; boxing the tool step would only add an
// indirection to every projection update.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChatProgressItem {
    Commentary {
        id: String,
        text: String,
    },
    Tool {
        id: String,
        /// The host's name for the tool this step called (`chart_publish`,
        /// `mcp__openbb__equity_price_quote`). The renderer recognizes and
        /// localizes steps by it; absent only on steps recorded before it existed.
        #[serde(default, rename = "toolName", skip_serializing_if = "Option::is_none")]
        tool_name: Option<String>,
        category: ChatProgressCategory,
        operation: ChatProgressOperation,
        action: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        running: Option<String>,
        #[serde(deserialize_with = "crate::domain::required_option")]
        target: Option<String>,
        #[serde(deserialize_with = "crate::domain::required_option")]
        href: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        source: Option<ChatProgressSource>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
        #[serde(default, rename = "resultIds", skip_serializing_if = "Vec::is_empty")]
        result_ids: Vec<String>,
        /// World names this step's acknowledged reads delivered. Host-attributed, never model input.
        #[serde(default, rename = "entityIds", skip_serializing_if = "Vec::is_empty")]
        entity_ids: Vec<String>,
        #[serde(
            default,
            rename = "outputRevision",
            skip_serializing_if = "Option::is_none"
        )]
        output_revision: Option<u64>,
        #[serde(default, rename = "detailId", skip_serializing_if = "Option::is_none")]
        detail_id: Option<String>,
        #[serde(default, rename = "filePath", skip_serializing_if = "Option::is_none")]
        file_path: Option<String>,
        status: ChatProgressStatus,
        /// Set on a failed step the host refused under a security rule.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        refusal: Option<ChatStepRefusal>,
        #[serde(default, rename = "startedAtMs")]
        started_at_ms: i64,
        #[serde(default, rename = "finishedAtMs")]
        #[serde(deserialize_with = "crate::domain::required_option")]
        finished_at_ms: Option<i64>,
    },
    System {
        id: String,
        category: ChatProgressCategory,
        operation: ChatProgressOperation,
        action: String,
        #[serde(deserialize_with = "crate::domain::required_option")]
        target: Option<String>,
        #[serde(deserialize_with = "crate::domain::required_option")]
        href: Option<String>,
        status: ChatProgressStatus,
        #[serde(default, rename = "startedAtMs")]
        started_at_ms: i64,
        #[serde(default, rename = "finishedAtMs")]
        #[serde(deserialize_with = "crate::domain::required_option")]
        finished_at_ms: Option<i64>,
    },
}

impl ChatProgressItem {
    fn id(&self) -> &str {
        match self {
            Self::Commentary { id, .. } | Self::Tool { id, .. } | Self::System { id, .. } => id,
        }
    }

    fn validate(&self, durable: bool) -> Result<(), DomainError> {
        validate_id(self.id())?;
        if let Self::Tool {
            tool_name,
            detail_id,
            file_path,
            running,
            summary,
            source,
            result_ids,
            entity_ids,
            ..
        } = self
        {
            if tool_name
                .as_deref()
                .is_some_and(|name| !valid_tool_name(name))
            {
                return Err(DomainError::Invalid("chat progress tool name is invalid"));
            }
            if detail_id.as_ref().is_some_and(|id| {
                id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit())
            }) {
                return Err(DomainError::Invalid("chat activity reference is invalid"));
            }
            if file_path.as_ref().is_some_and(|path| {
                crate::fs::workspace_files::relative_path(path)
                    .ok()
                    .as_deref()
                    != Some(path)
                    || path == "."
            }) {
                return Err(DomainError::Invalid("chat workspace path is invalid"));
            }
            if let Some(running) = running {
                validate_text(
                    running,
                    MAX_PROGRESS_LABEL_BYTES,
                    "chat progress action is invalid",
                )?;
            }
            if let Some(summary) = summary {
                validate_text(
                    summary,
                    MAX_PROGRESS_LABEL_BYTES,
                    "chat progress summary is invalid",
                )?;
            }
            if let Some(source) = source {
                validate_text(
                    &source.name,
                    MAX_PROGRESS_LABEL_BYTES,
                    "chat progress source is invalid",
                )?;
            }
            if result_ids.len() > 32
                || result_ids.iter().any(|id| {
                    id.len() > 128
                        || id.is_empty()
                        || !id
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
                })
            {
                return Err(DomainError::Invalid(
                    "chat progress result reference is invalid",
                ));
            }
            if entity_ids.len() > MAX_PROGRESS_ENTITY_IDS
                || entity_ids.iter().any(|id| !valid_entity_id(id))
            {
                return Err(DomainError::Invalid(
                    "chat progress entity reference is invalid",
                ));
            }
        }
        match self {
            Self::Commentary { text, .. } => validate_text(
                text,
                MAX_PROGRESS_TEXT_BYTES,
                "chat progress commentary is invalid",
            ),
            Self::Tool {
                category,
                operation,
                action,
                target,
                href,
                status,
                started_at_ms,
                finished_at_ms,
                ..
            }
            | Self::System {
                category,
                operation,
                action,
                target,
                href,
                status,
                started_at_ms,
                finished_at_ms,
                ..
            } => {
                validate_text(
                    action,
                    MAX_PROGRESS_LABEL_BYTES,
                    "chat progress action is invalid",
                )?;
                if let Some(target) = target {
                    validate_text(
                        target,
                        MAX_PROGRESS_LABEL_BYTES,
                        "chat progress target is invalid",
                    )?;
                    if target.contains('\0')
                        || target.lines().count() != 1
                        || looks_absolute_path(target)
                    {
                        return Err(DomainError::Invalid("chat progress target is unsafe"));
                    }
                }
                if let Some(href) = href {
                    validate_progress_href(href, *category, *operation, target.as_deref())?;
                }
                if matches!(self, Self::System { .. })
                    && (*category != ChatProgressCategory::System
                        || !matches!(
                            operation,
                            ChatProgressOperation::Retry
                                | ChatProgressOperation::Compact
                                | ChatProgressOperation::Generic
                        )
                        || href.is_some())
                {
                    return Err(DomainError::Invalid(
                        "chat progress system presentation is invalid",
                    ));
                }
                if *started_at_ms < 0
                    || finished_at_ms.is_some_and(|finished| finished < *started_at_ms)
                {
                    return Err(DomainError::Invalid(
                        "chat progress item timestamps are invalid",
                    ));
                }
                if durable && *status == ChatProgressStatus::Running {
                    return Err(DomainError::Invalid(
                        "durable chat progress contains a running item",
                    ));
                }
                if durable && finished_at_ms.is_none() {
                    return Err(DomainError::Invalid(
                        "durable chat progress is missing an item finish time",
                    ));
                }
                Ok(())
            }
        }
    }
}

/// A step names at most this many World entities; the mini map highlights them while the run lasts.
pub const MAX_PROGRESS_ENTITY_IDS: usize = 32;

/// Catalog and connection tool names: `chart_publish`, `mcp__server-id__tool_name`.
fn valid_tool_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

/// World node ids use the knowledge token alphabet (see `knowledge::model::validate_token`).
fn valid_entity_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.'))
}

fn validate_progress_href(
    href: &str,
    category: ChatProgressCategory,
    operation: ChatProgressOperation,
    target: Option<&str>,
) -> Result<(), DomainError> {
    if href.len() > MAX_PROGRESS_HREF_BYTES
        || category != ChatProgressCategory::Web
        || operation != ChatProgressOperation::Read
        || target.is_none()
    {
        return Err(DomainError::Invalid("chat progress link is invalid"));
    }
    let url = reqwest::Url::parse(href)
        .map_err(|_| DomainError::Invalid("chat progress link is invalid"))?;
    let port_is_allowed = matches!(
        (url.scheme(), url.port()),
        ("http", None | Some(80)) | ("https", None | Some(443))
    );
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || !port_is_allowed
    {
        return Err(DomainError::Invalid("chat progress link is unsafe"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatProgress {
    pub started_at_ms: i64,
    #[serde(deserialize_with = "crate::domain::required_option")]
    pub finished_at_ms: Option<i64>,
    pub items: Vec<ChatProgressItem>,
    /// The agent's short checklist for multi-step work (`plan_update`); absent for ordinary runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<ChatPlan>,
    /// Approvals this reply asked for (composer spec D3), in request order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub approvals: Vec<crate::chat::approvals::ChatApproval>,
    /// Questions this reply asked the user (`ask_user`), in request order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<crate::chat::questions::ChatQuestion>,
}

/// A plan has this many items at least and at most.
/// A reply asks for at most this many approvals; later requests are refused like before.
pub const MAX_PROGRESS_APPROVALS: usize = 20;
pub const MIN_PLAN_ITEMS: usize = 2;
pub const MAX_PLAN_ITEMS: usize = 6;
const MAX_PLAN_ID_CHARS: usize = 32;
const MAX_PLAN_LABEL_CHARS: usize = 120;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatPlanStatus {
    Pending,
    Active,
    Done,
    NotDone,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatPlanItem {
    pub id: String,
    pub label: String,
    pub status: ChatPlanStatus,
    /// When the item first became active; steps that start after it nest under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activated_at_ms: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatPlan {
    pub items: Vec<ChatPlanItem>,
}

impl ChatPlan {
    fn validate(&self, durable: bool) -> Result<(), DomainError> {
        if !(MIN_PLAN_ITEMS..=MAX_PLAN_ITEMS).contains(&self.items.len()) {
            return Err(DomainError::Invalid("chat plan size is invalid"));
        }
        let mut ids = std::collections::BTreeSet::new();
        for item in &self.items {
            if !valid_plan_id(&item.id) || !ids.insert(item.id.as_str()) {
                return Err(DomainError::Invalid("chat plan item id is invalid"));
            }
            if !valid_plan_label(&item.label) {
                return Err(DomainError::Invalid("chat plan item label is invalid"));
            }
            if item.activated_at_ms.is_some_and(|at| at < 0) {
                return Err(DomainError::Invalid("chat plan item time is invalid"));
            }
            if durable && item.status == ChatPlanStatus::Active {
                return Err(DomainError::Invalid("durable chat plan has an active item"));
            }
        }
        Ok(())
    }
}

fn valid_plan_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().count() <= MAX_PLAN_ID_CHARS
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_plan_label(label: &str) -> bool {
    let trimmed = label.trim();
    !trimmed.is_empty()
        && trimmed.len() == label.len()
        && label.chars().count() <= MAX_PLAN_LABEL_CHARS
        && !label.chars().any(char::is_control)
}

/// Reads `plan_update` arguments. Errors name the field and the expected shape for the model.
pub fn parse_plan_update(args: &Value) -> Result<Vec<ChatPlanItem>, (String, String)> {
    let invalid = |field: &str, expected: &str| (field.to_owned(), expected.to_owned());
    let object = args
        .as_object()
        .ok_or_else(|| invalid("arguments", "an object with an items array"))?;
    if let Some(extra) = object.keys().find(|key| key.as_str() != "items") {
        return Err(invalid(extra, "no other fields; send only items"));
    }
    let items = object
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("items", "an array of 2 to 6 plan items"))?;
    if !(MIN_PLAN_ITEMS..=MAX_PLAN_ITEMS).contains(&items.len()) {
        return Err(invalid("items", "2 to 6 plan items"));
    }
    let mut seen = HashSet::new();
    let mut active = 0;
    let mut parsed = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let field = |name: &str| format!("items[{index}].{name}");
        let object = item.as_object().ok_or_else(|| {
            (
                format!("items[{index}]"),
                "an object with id, label and status".to_owned(),
            )
        })?;
        if let Some(extra) = object
            .keys()
            .find(|key| !matches!(key.as_str(), "id" | "label" | "status"))
        {
            return Err((field(extra), "no other fields".to_owned()));
        }
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| valid_plan_id(id))
            .ok_or_else(|| (field("id"), "1 to 32 letters, digits, - or _".to_owned()))?;
        if !seen.insert(id.to_owned()) {
            return Err((field("id"), "a unique id within the plan".to_owned()));
        }
        let label = object
            .get("label")
            .and_then(Value::as_str)
            .filter(|label| valid_plan_label(label))
            .ok_or_else(|| {
                (
                    field("label"),
                    "one line of plain words, at most 120 characters".to_owned(),
                )
            })?;
        let status = object
            .get("status")
            .cloned()
            .and_then(|status| serde_json::from_value::<ChatPlanStatus>(status).ok())
            .ok_or_else(|| {
                (
                    field("status"),
                    "pending, active, done or not_done".to_owned(),
                )
            })?;
        if status == ChatPlanStatus::Active {
            active += 1;
        }
        parsed.push(ChatPlanItem {
            id: id.to_owned(),
            label: label.to_owned(),
            status,
            activated_at_ms: None,
        });
    }
    if active > 1 {
        return Err(invalid("items", "at most one active item"));
    }
    Ok(parsed)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatProgressPatch {
    pub sequence: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub upsert_items: Vec<ChatProgressItem>,
    /// Text appended to commentary this stream already published, in order. A
    /// commentary id is in `upsert_items` or here, never both: its first
    /// publication carries the whole item.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub append_text: Vec<CommentaryAppend>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub remove_item_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<i64>,
    /// The whole plan whenever it changed since the last patch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<ChatPlan>,
    /// Every approval whenever one changed since the last patch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approvals: Option<Vec<crate::chat::approvals::ChatApproval>>,
    /// Every question whenever one changed since the last patch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub questions: Option<Vec<crate::chat::questions::ChatQuestion>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentaryAppend {
    pub id: String,
    pub text: String,
}

impl ChatProgress {
    pub fn validate(&self, durable: bool) -> Result<(), DomainError> {
        if self.started_at_ms < 0
            || self
                .finished_at_ms
                .is_some_and(|finished| finished < self.started_at_ms)
            || (durable && self.finished_at_ms.is_none())
        {
            return Err(DomainError::Invalid("chat progress timestamps are invalid"));
        }
        let mut ids = std::collections::BTreeSet::new();
        for item in &self.items {
            item.validate(durable)?;
            if !ids.insert(item.id()) {
                return Err(DomainError::Invalid("chat progress item id is duplicated"));
            }
        }
        if let Some(plan) = &self.plan {
            plan.validate(durable)?;
        }
        if self.approvals.len() > MAX_PROGRESS_APPROVALS {
            return Err(DomainError::Invalid("chat progress has too many approvals"));
        }
        for approval in &self.approvals {
            approval.validate(durable)?;
        }
        if self.questions.len() > crate::chat::questions::MAX_PROGRESS_QUESTIONS {
            return Err(DomainError::Invalid("chat progress has too many questions"));
        }
        for question in &self.questions {
            question.validate(durable)?;
        }
        Ok(())
    }
}

fn validate_id(value: &str) -> Result<(), DomainError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(DomainError::Invalid("chat progress item id is unsafe"));
    }
    Ok(())
}

fn validate_text(value: &str, max: usize, message: &'static str) -> Result<(), DomainError> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(DomainError::Invalid(message));
    }
    Ok(())
}

#[derive(Debug)]
pub struct ChatProgressProjection {
    progress: ChatProgress,
    next_item: usize,
    next_patch_sequence: u64,
    active_commentary: Option<String>,
    tool_items: HashMap<String, String>,
    pending_file_exports: HashMap<String, String>,
    system_items: HashMap<String, String>,
    item_indices: HashMap<String, usize>,
    dirty_item_ids: Vec<String>,
    dirty_item_set: HashSet<String>,
    published_item_ids: HashSet<String>,
    /// The text of each published commentary as the stream has seen it, so a
    /// patch carries only what was appended since, and the whole item again
    /// should the text ever change under that prefix.
    published_text: HashMap<String, String>,
    pending_remove_item_ids: Vec<String>,
    pending_finished_at_ms: Option<i64>,
    plan_dirty: bool,
    approvals_dirty: bool,
    questions_dirty: bool,
    asset_names: AssetNames,
}

/// Arguments that name an asset by its symbol.
const SYMBOL_ARGS: &[&str] = &["symbol", "symbols", "ticker", "tickers", "company"];
/// Symbols one argument may list ("AAPL,MSFT,NVDA") and still be named.
const MAX_NAMED_SYMBOLS: usize = 12;

/// The names of the assets a Loupist has pages for, by symbol, so a step reads
/// "Reading the asset page · Solsup Foods" instead of its tool's `990221`.
#[derive(Debug, Default, Clone)]
pub struct AssetNames(HashMap<String, Vec<(String, String)>>);

impl AssetNames {
    /// `(symbol, market, name)` for each page.
    pub fn new(pages: impl IntoIterator<Item = (String, String, String)>) -> Self {
        let mut names: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for (symbol, market, name) in pages {
            let symbol = symbol.trim().to_uppercase();
            if symbol.is_empty() || name.trim().is_empty() {
                continue;
            }
            names
                .entry(symbol)
                .or_default()
                .push((market.trim().to_uppercase(), name.trim().to_owned()));
        }
        Self(names)
    }

    /// One symbol's name: the page in `market` when given, else the only name
    /// its pages share.
    fn name(&self, symbol: &str, market: Option<&str>) -> Option<&str> {
        let pages = self.0.get(&symbol.trim().to_uppercase())?;
        if let Some(market) = market {
            if let Some((_, name)) = pages.iter().find(|(m, _)| m.eq_ignore_ascii_case(market)) {
                return Some(name);
            }
        }
        let first = &pages.first()?.1;
        pages
            .iter()
            .all(|(_, name)| name == first)
            .then_some(first.as_str())
    }

    /// A symbol argument ("AAPL" or "AAPL,MSFT") with every known symbol
    /// named; None when it names none.
    fn named(&self, value: &str, market: Option<&str>) -> Option<String> {
        let parts = value
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        if parts.is_empty() || parts.len() > MAX_NAMED_SYMBOLS {
            return None;
        }
        let mut found = false;
        let named = parts
            .iter()
            .map(|part| match self.name(part, market) {
                Some(name) => {
                    found = true;
                    name.to_owned()
                }
                None => (*part).to_owned(),
            })
            .collect::<Vec<_>>();
        found.then(|| named.join(", "))
    }

    /// Tool arguments with each symbol argument replaced by its asset's name,
    /// at the top level and inside an MCP call's `params`.
    fn rename(&self, args: &Value) -> Value {
        fn apply(names: &AssetNames, object: &mut serde_json::Map<String, Value>) {
            let market = ["market", "exchange"]
                .iter()
                .find_map(|key| object.get(*key).and_then(Value::as_str))
                .map(str::to_owned);
            for key in SYMBOL_ARGS {
                if let Some(Value::String(value)) = object.get(*key) {
                    if let Some(named) = names.named(value, market.as_deref()) {
                        object.insert((*key).to_owned(), Value::String(named));
                    }
                }
            }
        }
        let mut renamed = args.clone();
        if self.0.is_empty() {
            return renamed;
        }
        if let Some(object) = renamed.as_object_mut() {
            apply(self, object);
            if let Some(Value::Object(params)) = object.get_mut("params") {
                apply(self, params);
            }
        }
        renamed
    }
}

fn truncate_utf8(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ToolPresentation {
    category: ChatProgressCategory,
    operation: ChatProgressOperation,
    action: String,
    running: Option<String>,
    target: Option<String>,
    href: Option<String>,
    source: Option<ChatProgressSource>,
}

impl ChatProgressProjection {
    /// Whether any tool call started in this turn.
    pub fn has_tool_activity(&self) -> bool {
        !self.tool_items.is_empty()
    }

    pub fn new(started_at_ms: i64) -> Self {
        Self {
            progress: ChatProgress {
                started_at_ms,
                finished_at_ms: None,
                items: Vec::new(),
                plan: None,
                approvals: Vec::new(),
                questions: Vec::new(),
            },
            next_item: 1,
            next_patch_sequence: 1,
            active_commentary: None,
            tool_items: HashMap::new(),
            pending_file_exports: HashMap::new(),
            system_items: HashMap::new(),
            item_indices: HashMap::new(),
            dirty_item_ids: Vec::new(),
            dirty_item_set: HashSet::new(),
            published_item_ids: HashSet::new(),
            published_text: HashMap::new(),
            pending_remove_item_ids: Vec::new(),
            pending_finished_at_ms: None,
            plan_dirty: false,
            approvals_dirty: false,
            questions_dirty: false,
            asset_names: AssetNames::default(),
        }
    }

    /// The Loupist's asset names, read once when the turn starts streaming.
    pub fn set_asset_names(&mut self, names: AssetNames) {
        self.asset_names = names;
    }

    fn close_commentary(&mut self) {
        self.active_commentary = None;
    }

    /// The commentary still being written, which a settling answer replaces.
    pub fn live_commentary_id(&self) -> Option<&str> {
        self.active_commentary.as_deref()
    }

    pub fn start_assistant_turn(&mut self) {
        self.close_commentary();
    }

    pub fn append_commentary(&mut self, delta: &str) {
        if delta.is_empty() {
            return;
        }
        let id = match self.active_commentary.clone() {
            Some(id) => id,
            None => {
                let index = self.progress.items.len();
                let id = self.next_id("commentary");
                self.progress.items.push(ChatProgressItem::Commentary {
                    id: id.clone(),
                    text: String::new(),
                });
                self.item_indices.insert(id.clone(), index);
                self.active_commentary = Some(id.clone());
                id
            }
        };
        let index = self.item_indices[&id];
        if let ChatProgressItem::Commentary { text, .. } = &mut self.progress.items[index] {
            text.push_str(delta);
        }
        self.mark_dirty(&id);
    }

    pub fn finish_assistant_turn(&mut self, is_final: bool) {
        if is_final {
            if let Some(id) = self.active_commentary.take() {
                self.remove_item(&id);
            }
        } else {
            self.close_commentary();
        }
    }

    pub fn start_tool(
        &mut self,
        tool_call_id: &str,
        tool_name: &str,
        args: &Value,
        web_source: Option<&crate::web::WebSource>,
        at_ms: i64,
    ) {
        self.close_commentary();
        if tool_name == "plan_update" {
            // The checklist is the presentation; an invalid call shows nothing and the
            // executor returns the field error to the agent.
            if let Ok(items) = parse_plan_update(args) {
                self.update_plan(items, at_ms);
            }
            return;
        }
        // Steps name assets as the Loupist knows them; the call keeps its symbols.
        let presentation = summarize_tool(tool_name, &self.asset_names.rename(args), web_source);
        if tool_name == "dataset_export" {
            if let Some(path) = args
                .get("path")
                .and_then(Value::as_str)
                .and_then(|path| crate::fs::workspace_files::relative_path(path).ok())
                .filter(|path| path != ".")
            {
                self.pending_file_exports
                    .insert(tool_call_id.to_owned(), path);
            }
        }
        let id = self.next_id("tool");
        let index = self.progress.items.len();
        self.progress.items.push(ChatProgressItem::Tool {
            id: id.clone(),
            tool_name: valid_tool_name(tool_name).then(|| tool_name.to_owned()),
            category: presentation.category,
            operation: presentation.operation,
            action: presentation.action,
            running: presentation.running,
            target: presentation.target,
            href: presentation.href,
            source: presentation.source,
            summary: None,
            result_ids: Vec::new(),
            entity_ids: Vec::new(),
            output_revision: None,
            detail_id: None,
            file_path: None,
            status: ChatProgressStatus::Running,
            refusal: None,
            started_at_ms: at_ms.max(0),
            finished_at_ms: None,
        });
        self.item_indices.insert(id.clone(), index);
        self.tool_items.insert(tool_call_id.to_owned(), id.clone());
        self.mark_dirty(&id);
    }

    /// A failed step the host refused under a security rule says which rule,
    /// so the Chat can explain it in the step instead of only the run log.
    pub fn refuse_tool(&mut self, tool_call_id: &str, reason: ChatStepRefusal) {
        let Some(id) = self.tool_items.get(tool_call_id).cloned() else {
            return;
        };
        let Some(&index) = self.item_indices.get(&id) else {
            return;
        };
        if let Some(ChatProgressItem::Tool {
            status, refusal, ..
        }) = self.progress.items.get_mut(index)
        {
            if *status == ChatProgressStatus::Failed {
                *refusal = Some(reason);
                self.mark_dirty(&id);
            }
        }
    }

    /// A finished step whose source refused the turn's as-of date reads as
    /// skipped, never as a failure (finance-point-in-time.md §2.3).
    pub fn skip_tool(&mut self, tool_call_id: &str) {
        let Some(id) = self.tool_items.get(tool_call_id).cloned() else {
            return;
        };
        let Some(&index) = self.item_indices.get(&id) else {
            return;
        };
        if let Some(ChatProgressItem::Tool { status, .. }) = self.progress.items.get_mut(index) {
            if *status == ChatProgressStatus::Failed {
                *status = ChatProgressStatus::Skipped;
                self.mark_dirty(&id);
            }
        }
    }

    pub fn finish_tool(
        &mut self,
        tool_call_id: &str,
        failed: bool,
        at_ms: i64,
        summary: Option<String>,
    ) {
        let exported_path = self.pending_file_exports.remove(tool_call_id);
        let Some(id) = self.tool_items.get(tool_call_id).cloned() else {
            return;
        };
        let Some(&index) = self.item_indices.get(&id) else {
            return;
        };
        if let Some(ChatProgressItem::Tool {
            status,
            file_path,
            summary: item_summary,
            started_at_ms,
            finished_at_ms,
            ..
        }) = self.progress.items.get_mut(index)
        {
            if !failed && exported_path.is_some() {
                *file_path = exported_path;
            }
            if item_summary.is_none() {
                *item_summary = summary.filter(|value| safe_value(value).as_deref() == Some(value));
            }
            *status = if failed {
                ChatProgressStatus::Failed
            } else {
                ChatProgressStatus::Succeeded
            };
            *finished_at_ms = Some(at_ms.max(*started_at_ms));
        }
        self.mark_dirty(&id);
    }

    /// Links a host-issued result to the tool call that produced it.
    pub fn attach_result(&mut self, tool_call_id: &str, evidence_id: &str) {
        let Some(id) = self.tool_items.get(tool_call_id).cloned() else {
            return;
        };
        let Some(&index) = self.item_indices.get(&id) else {
            return;
        };
        if let Some(ChatProgressItem::Tool { result_ids, .. }) = self.progress.items.get_mut(index)
        {
            if result_ids.len() < 32 && !result_ids.iter().any(|existing| existing == evidence_id) {
                result_ids.push(evidence_id.to_owned());
                self.mark_dirty(&id);
            }
        }
    }

    /// Links the World names a tool call's acknowledged read delivered to that step.
    pub fn attach_entities(&mut self, tool_call_id: &str, ids: &[String]) {
        let Some(id) = self.tool_items.get(tool_call_id).cloned() else {
            return;
        };
        let Some(&index) = self.item_indices.get(&id) else {
            return;
        };
        if let Some(ChatProgressItem::Tool { entity_ids, .. }) = self.progress.items.get_mut(index)
        {
            let before = entity_ids.len();
            for candidate in ids {
                if entity_ids.len() >= MAX_PROGRESS_ENTITY_IDS {
                    break;
                }
                if valid_entity_id(candidate)
                    && !entity_ids.iter().any(|existing| existing == candidate)
                {
                    entity_ids.push(candidate.clone());
                }
            }
            if entity_ids.len() != before {
                self.mark_dirty(&id);
            }
        }
    }

    /// Bumps the activity output revision after a flushed live update.
    pub fn note_output(&mut self, tool_call_id: &str) {
        let Some(id) = self.tool_items.get(tool_call_id).cloned() else {
            return;
        };
        let Some(&index) = self.item_indices.get(&id) else {
            return;
        };
        if let Some(ChatProgressItem::Tool {
            output_revision, ..
        }) = self.progress.items.get_mut(index)
        {
            *output_revision = Some(output_revision.unwrap_or(0).saturating_add(1));
            self.mark_dirty(&id);
        }
    }

    pub(crate) fn attach_activity(
        &mut self,
        tool_call_id: &str,
        reference: crate::chat::activity::ActivityReference,
    ) {
        let Some(id) = self.tool_items.get(tool_call_id).cloned() else {
            return;
        };
        let Some(&index) = self.item_indices.get(&id) else {
            return;
        };
        if let Some(ChatProgressItem::Tool {
            detail_id,
            file_path,
            target,
            ..
        }) = self.progress.items.get_mut(index)
        {
            *detail_id = Some(reference.detail_id);
            *file_path = reference.file_path;
            if target.is_none() {
                *target = file_path.as_deref().and_then(safe_relative_path);
            }
        }
        self.mark_dirty(&id);
    }

    pub fn start_system(
        &mut self,
        key: &str,
        operation: ChatProgressOperation,
        action: &str,
        target: Option<String>,
        at_ms: i64,
    ) {
        self.close_commentary();
        let id = self.next_id("system");
        let index = self.progress.items.len();
        self.progress.items.push(ChatProgressItem::System {
            id: id.clone(),
            category: ChatProgressCategory::System,
            operation,
            action: action.to_owned(),
            target,
            href: None,
            status: ChatProgressStatus::Running,
            started_at_ms: at_ms.max(0),
            finished_at_ms: None,
        });
        self.item_indices.insert(id.clone(), index);
        self.system_items.insert(key.to_owned(), id.clone());
        self.mark_dirty(&id);
    }

    pub fn finish_system(&mut self, key: &str, failed: bool, at_ms: i64) {
        let Some(id) = self.system_items.remove(key) else {
            return;
        };
        let Some(&index) = self.item_indices.get(&id) else {
            return;
        };
        if let Some(ChatProgressItem::System {
            status,
            started_at_ms,
            finished_at_ms,
            ..
        }) = self.progress.items.get_mut(index)
        {
            *status = if failed {
                ChatProgressStatus::Failed
            } else {
                ChatProgressStatus::Succeeded
            };
            *finished_at_ms = Some(at_ms.max(*started_at_ms));
        }
        self.mark_dirty(&id);
    }

    /// Replaces the plan, keeping each item's first activation time so steps stay nested.
    fn update_plan(&mut self, mut items: Vec<ChatPlanItem>, at_ms: i64) {
        let previous = self.progress.plan.take();
        for item in &mut items {
            let earlier = previous
                .as_ref()
                .and_then(|plan| plan.items.iter().find(|old| old.id == item.id))
                .and_then(|old| old.activated_at_ms);
            item.activated_at_ms = earlier
                .or_else(|| matches!(item.status, ChatPlanStatus::Active).then_some(at_ms.max(0)));
        }
        self.progress.plan = Some(ChatPlan { items });
        self.plan_dirty = true;
    }

    /// A failed run did not finish its active step.
    pub fn fail_plan(&mut self) {
        self.close_plan(ChatPlanStatus::NotDone);
    }

    /// Unfinished items cannot stay active or pending once the run ends.
    fn close_plan(&mut self, active_becomes: ChatPlanStatus) {
        let Some(plan) = self.progress.plan.as_mut() else {
            return;
        };
        let mut changed = false;
        for item in &mut plan.items {
            let next = match item.status {
                ChatPlanStatus::Active => active_becomes,
                ChatPlanStatus::Pending => ChatPlanStatus::NotDone,
                status => status,
            };
            if next != item.status {
                item.status = next;
                changed = true;
            }
        }
        self.plan_dirty |= changed;
    }

    /// Adds or updates an approval card, attached to the step of `tool_call_id` when known.
    /// Returns false when the reply already holds its maximum of approvals.
    pub fn upsert_approval(
        &mut self,
        tool_call_id: Option<&str>,
        mut approval: crate::chat::approvals::ChatApproval,
    ) -> bool {
        approval.item_id = tool_call_id.and_then(|call| self.tool_items.get(call).cloned());
        if let Some(existing) = self
            .progress
            .approvals
            .iter_mut()
            .find(|existing| existing.request_id == approval.request_id)
        {
            if approval.item_id.is_none() {
                approval.item_id = existing.item_id.clone();
            }
            *existing = approval;
        } else if self.progress.approvals.len() >= MAX_PROGRESS_APPROVALS {
            return false;
        } else {
            self.progress.approvals.push(approval);
        }
        self.approvals_dirty = true;
        true
    }

    /// Adds or updates a question card, attached to the step of `tool_call_id` when known.
    /// Returns false when the reply already holds its maximum of questions.
    pub fn upsert_question(
        &mut self,
        tool_call_id: Option<&str>,
        mut question: crate::chat::questions::ChatQuestion,
    ) -> bool {
        question.item_id = tool_call_id.and_then(|call| self.tool_items.get(call).cloned());
        if let Some(existing) = self
            .progress
            .questions
            .iter_mut()
            .find(|existing| existing.request_id == question.request_id)
        {
            if question.item_id.is_none() {
                question.item_id = existing.item_id.clone();
            }
            *existing = question;
        } else if self.progress.questions.len() >= crate::chat::questions::MAX_PROGRESS_QUESTIONS {
            return false;
        } else {
            self.progress.questions.push(question);
        }
        self.questions_dirty = true;
        true
    }

    /// A question still waiting when the run ends can no longer be answered.
    fn expire_waiting_questions(&mut self) {
        for question in &mut self.progress.questions {
            if question.state == crate::chat::questions::QuestionState::Waiting {
                question.state = crate::chat::questions::QuestionState::Expired;
                self.questions_dirty = true;
            }
        }
    }

    /// A card still waiting when the run ends can no longer be answered.
    fn expire_waiting_approvals(&mut self) {
        for approval in &mut self.progress.approvals {
            if approval.state == crate::chat::approvals::ApprovalState::Waiting {
                approval.state = crate::chat::approvals::ApprovalState::Expired;
                self.approvals_dirty = true;
            }
        }
    }

    pub fn snapshot(&self) -> ChatProgress {
        self.progress.clone()
    }

    pub fn latest_patch_sequence(&self) -> u64 {
        self.next_patch_sequence.saturating_sub(1)
    }

    pub fn take_patch(&mut self) -> Option<ChatProgressPatch> {
        let dirty_item_ids = std::mem::take(&mut self.dirty_item_ids);
        let mut upsert_items = Vec::with_capacity(dirty_item_ids.len());
        let mut append_text = Vec::new();
        for id in dirty_item_ids {
            self.dirty_item_set.remove(&id);
            let Some(item) = self
                .item_indices
                .get(&id)
                .and_then(|index| self.progress.items.get(*index))
            else {
                continue;
            };
            if let ChatProgressItem::Commentary { text, .. } = item {
                match self.published_text.get_mut(&id) {
                    Some(published) if text.starts_with(published.as_str()) => {
                        if text.len() > published.len() {
                            let appended = &text[published.len()..];
                            append_text.push(CommentaryAppend {
                                id: id.clone(),
                                text: appended.to_owned(),
                            });
                            published.push_str(appended);
                        }
                        continue;
                    }
                    // First publish, or text that changed under the published prefix:
                    // the renderer needs the whole item.
                    _ => {
                        self.published_text.insert(id.clone(), text.clone());
                    }
                }
            }
            self.published_item_ids.insert(id);
            upsert_items.push(item.clone());
        }
        let remove_item_ids = std::mem::take(&mut self.pending_remove_item_ids);
        let finished_at_ms = self.pending_finished_at_ms.take();
        let plan = std::mem::take(&mut self.plan_dirty)
            .then(|| self.progress.plan.clone())
            .flatten();
        let approvals =
            std::mem::take(&mut self.approvals_dirty).then(|| self.progress.approvals.clone());
        let questions =
            std::mem::take(&mut self.questions_dirty).then(|| self.progress.questions.clone());
        if upsert_items.is_empty()
            && append_text.is_empty()
            && remove_item_ids.is_empty()
            && finished_at_ms.is_none()
            && plan.is_none()
            && approvals.is_none()
            && questions.is_none()
        {
            return None;
        }
        let sequence = self.next_patch_sequence;
        self.next_patch_sequence = self.next_patch_sequence.saturating_add(1);
        Some(ChatProgressPatch {
            sequence,
            upsert_items,
            append_text,
            remove_item_ids,
            finished_at_ms,
            plan,
            approvals,
            questions,
        })
    }

    pub fn finish(&mut self, finished_at_ms: i64, stopped: bool) -> Option<ChatProgress> {
        self.close_commentary();
        self.expire_waiting_approvals();
        self.expire_waiting_questions();
        let closed_at = finished_at_ms.max(self.progress.started_at_ms);
        self.progress.finished_at_ms = Some(closed_at);
        let mut changed_item_ids = Vec::new();
        for item in &mut self.progress.items {
            match item {
                ChatProgressItem::Tool {
                    id,
                    status,
                    started_at_ms,
                    finished_at_ms,
                    ..
                }
                | ChatProgressItem::System {
                    id,
                    status,
                    started_at_ms,
                    finished_at_ms,
                    ..
                } => {
                    let mut changed = false;
                    if *status == ChatProgressStatus::Running {
                        *status = if stopped {
                            ChatProgressStatus::Stopped
                        } else {
                            ChatProgressStatus::Failed
                        };
                        changed = true;
                    }
                    if finished_at_ms.is_none() {
                        *finished_at_ms = Some(closed_at.max(*started_at_ms));
                        changed = true;
                    }
                    if changed {
                        changed_item_ids.push(id.clone());
                    }
                }
                _ => {}
            }
        }
        for id in changed_item_ids {
            self.mark_dirty(&id);
        }
        // A completed run finished the step it was on; a stopped one did not.
        self.close_plan(if stopped {
            ChatPlanStatus::NotDone
        } else {
            ChatPlanStatus::Done
        });
        self.pending_finished_at_ms = Some(closed_at);
        (!self.progress.items.is_empty() || self.progress.plan.is_some())
            .then(|| self.progress.clone())
    }

    fn mark_dirty(&mut self, id: &str) {
        if self.dirty_item_set.insert(id.to_owned()) {
            self.dirty_item_ids.push(id.to_owned());
        }
    }

    fn remove_item(&mut self, id: &str) {
        let Some(index) = self.item_indices.remove(id) else {
            return;
        };
        self.progress.items.remove(index);
        for (item_index, item) in self.progress.items.iter().enumerate().skip(index) {
            self.item_indices.insert(item.id().to_owned(), item_index);
        }
        self.dirty_item_set.remove(id);
        self.dirty_item_ids.retain(|dirty_id| dirty_id != id);
        self.published_text.remove(id);
        if self.published_item_ids.remove(id) {
            self.pending_remove_item_ids.push(id.to_owned());
        }
    }

    fn next_id(&mut self, prefix: &str) -> String {
        let id = format!("{prefix}-{}", self.next_item);
        self.next_item += 1;
        id
    }
}

fn summarize_tool(
    tool_name: &str,
    args: &Value,
    web_source: Option<&crate::web::WebSource>,
) -> ToolPresentation {
    let (servers, providers) = crate::marketplace::progress_names();
    if let Some(rest) = tool_name.strip_prefix("mcp__") {
        return mcp_presentation(rest, args, &servers, &providers);
    }
    if tool_name == "capability_load" {
        return capability_load_presentation(args);
    }
    if tool_name == "connection_setup" {
        return connection_setup_presentation(args);
    }
    let spec = crate::broker::tools::TOOL_CATALOG
        .tools
        .iter()
        .find(|tool| tool.name == tool_name)
        .and_then(|tool| tool.progress.as_ref());
    let Some(spec) = spec else {
        return presentation(
            ChatProgressCategory::Capability,
            ChatProgressOperation::Generic,
            "Used a research tool",
            None,
            None,
            None,
            None,
        );
    };
    let category = parse_category(&spec.category);
    let operation = parse_operation(&spec.operation);
    let flat = flatten_params(args);
    let target_args = spec
        .target_args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let (mut target, href) = match spec.target.as_deref() {
        Some("path") => (
            path_target(args).or_else(|| first_string(args, &target_args)),
            None,
        ),
        Some("command") => (command_target(args), None),
        Some("join") => (named_join(args, &target_args, &providers), None),
        Some("join_flat") => (named_join(&flat, &target_args, &providers), None),
        Some("count") => (
            args.get("calls")
                .and_then(Value::as_array)
                .map(|calls| format!("{} requests", calls.len())),
            None,
        ),
        Some("web") => web_source
            .and_then(safe_web_source)
            .map_or((None, None), |(target, href)| (Some(target), Some(href))),
        _ => (first_string(args, &target_args), None),
    };
    let mut source = progress_source(spec.source.as_deref(), args, &flat, &providers);
    if let Some(label) = connection_label(tool_name) {
        target = Some(match target {
            Some(detail) => format!("{label} · {detail}"),
            None => label.clone(),
        });
        source = Some(ChatProgressSource {
            kind: ChatProgressSourceKind::Connector,
            name: label,
        });
    }
    presentation(
        category,
        operation,
        &spec.done,
        Some(spec.running.clone()),
        target,
        href,
        source,
    )
}

fn plugin_label(plugin: &str) -> Option<String> {
    crate::marketplace::bundled_plugins()
        .ok()?
        .plugins
        .into_iter()
        .find(|item| item.name == plugin)
        .map(|item| item.display_name)
}

fn connection_label(tool_name: &str) -> Option<String> {
    let catalog = &crate::broker::tools::TOOL_CATALOG;
    let component_id = catalog
        .tools
        .iter()
        .find(|tool| tool.name == tool_name)?
        .component
        .as_deref()?;
    let connection = catalog
        .components
        .iter()
        .find(|component| component.id == component_id)?
        .connection
        .as_deref()?;
    plugin_label(connection)
}

/// The step's target is the resolved catalog entry id, which the renderer
/// reads back to draw the setup card; the connector's name labels the step.
fn connection_setup_presentation(args: &Value) -> ToolPresentation {
    let wording = crate::broker::tools::TOOL_CATALOG
        .tools
        .iter()
        .find(|tool| tool.name == "connection_setup")
        .and_then(|tool| tool.progress.as_ref());
    let entry = args
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| crate::marketplace::entry_lookup::resolve_setup_entry(id).ok());
    let (target, source) = match entry {
        Some(entry) => (
            Some(entry.id.clone()),
            Some(ChatProgressSource {
                kind: ChatProgressSourceKind::Connector,
                name: entry.name.clone(),
            }),
        ),
        // An unresolved id may be a pasted key, so it never becomes a visible target.
        None => (None, None),
    };
    presentation(
        ChatProgressCategory::Capability,
        ChatProgressOperation::Generic,
        wording.map_or("Prepared a connection", |spec| spec.done.as_str()),
        wording.map(|spec| spec.running.clone()),
        target,
        None,
        source,
    )
}

fn capability_load_presentation(args: &Value) -> ToolPresentation {
    let id = args.get("id").and_then(Value::as_str).unwrap_or("");
    let catalog = &crate::broker::tools::TOOL_CATALOG;
    let component = catalog
        .components
        .iter()
        .find(|component| component.id == id);
    let (label, category) = if let Some(component) = component {
        let category = catalog
            .tools
            .iter()
            .find(|tool| tool.component.as_deref() == Some(component.id.as_str()))
            .and_then(|tool| tool.progress.as_ref())
            .map(|spec| parse_category(&spec.category))
            .unwrap_or(ChatProgressCategory::Capability);
        let Some(connection) = component.connection.as_deref() else {
            // A built-in group's name is internal; the catalog wording is what
            // the interface translates.
            let wording = catalog
                .tools
                .iter()
                .find(|tool| tool.name == "capability_load")
                .and_then(|tool| tool.progress.as_ref());
            return presentation(
                category,
                ChatProgressOperation::Read,
                wording.map_or("Prepared a research tool", |spec| spec.done.as_str()),
                wording.map(|spec| spec.running.clone()),
                None,
                None,
                None,
            );
        };
        let label = plugin_label(connection).unwrap_or_else(|| component.name.clone());
        (label, category)
    } else if let Some(label) = plugin_label(id) {
        (label, ChatProgressCategory::Finance)
    } else {
        return presentation(
            ChatProgressCategory::Capability,
            ChatProgressOperation::Read,
            "Opened a connection",
            Some("Opening a connection".into()),
            None,
            None,
            None,
        );
    };
    presentation(
        category,
        ChatProgressOperation::Read,
        &format!("Opened {label}"),
        Some(format!("Opening {label}")),
        None,
        None,
        None,
    )
}

fn presentation(
    category: ChatProgressCategory,
    operation: ChatProgressOperation,
    action: &str,
    running: Option<String>,
    target: Option<String>,
    href: Option<String>,
    source: Option<ChatProgressSource>,
) -> ToolPresentation {
    ToolPresentation {
        category,
        operation,
        action: action.into(),
        running,
        target,
        href,
        source,
    }
}

fn mcp_presentation(
    rest: &str,
    args: &Value,
    servers: &std::collections::BTreeMap<String, String>,
    providers: &std::collections::BTreeMap<String, String>,
) -> ToolPresentation {
    let (server, tool) = rest.split_once("__").unwrap_or((rest, rest));
    let human = humanize(tool);
    let flat = flatten_params(args);
    let plugin = servers.get(server).cloned();
    let provider_id = flat
        .get("provider")
        .and_then(Value::as_str)
        .filter(|value| safe_value(value).is_some());
    let connector = provider_id.and_then(|id| providers.get(id).cloned());
    let source_name = connector.clone().or(plugin.clone());
    let source = source_name.map(|name| ChatProgressSource {
        kind: if connector.is_some() {
            ChatProgressSourceKind::Connector
        } else {
            ChatProgressSourceKind::Finance
        },
        name,
    });
    let target = named_join(
        &flat,
        &["symbol", "ticker", "query", "cik", "series", "start", "end"],
        providers,
    );
    presentation(
        if plugin.is_some() {
            ChatProgressCategory::Finance
        } else {
            ChatProgressCategory::Capability
        },
        ChatProgressOperation::Read,
        &format!("Read {human}"),
        Some(format!("Reading {human}")),
        target,
        None,
        source,
    )
}

fn progress_source(
    kind: Option<&str>,
    args: &Value,
    flat: &Value,
    providers: &std::collections::BTreeMap<String, String>,
) -> Option<ChatProgressSource> {
    let kind = kind?;
    let provider = flat
        .get("provider")
        .or_else(|| args.get("provider"))
        .and_then(Value::as_str)
        .filter(|value| safe_value(value).is_some());
    if kind == "finance" {
        if let Some(name) = provider.and_then(|id| providers.get(id)) {
            return Some(ChatProgressSource {
                kind: ChatProgressSourceKind::Connector,
                name: name.clone(),
            });
        }
    }
    let parsed = match kind {
        "web" => ChatProgressSourceKind::Web,
        "memory" => ChatProgressSourceKind::Memory,
        "files" => ChatProgressSourceKind::Files,
        "finance" => ChatProgressSourceKind::Finance,
        "artifact" => ChatProgressSourceKind::Artifact,
        "compute" => ChatProgressSourceKind::Compute,
        "capability" => ChatProgressSourceKind::Capability,
        _ => return None,
    };
    let name = match parsed {
        ChatProgressSourceKind::Web => "Web",
        ChatProgressSourceKind::Memory => "Memory",
        ChatProgressSourceKind::Files => "Files",
        ChatProgressSourceKind::Finance => provider.unwrap_or("Finance"),
        ChatProgressSourceKind::Artifact => "Documents",
        ChatProgressSourceKind::Compute => "Computation",
        ChatProgressSourceKind::Capability => "Tools",
        ChatProgressSourceKind::Connector => "Connector",
    };
    Some(ChatProgressSource {
        kind: parsed,
        name: name.to_owned(),
    })
}

fn parse_category(value: &str) -> ChatProgressCategory {
    match value {
        "web" => ChatProgressCategory::Web,
        "memory" => ChatProgressCategory::Memory,
        "capability" => ChatProgressCategory::Capability,
        "finance" => ChatProgressCategory::Finance,
        "files" => ChatProgressCategory::Files,
        "artifact" => ChatProgressCategory::Artifact,
        "compute" => ChatProgressCategory::Compute,
        "system" => ChatProgressCategory::System,
        _ => ChatProgressCategory::Capability,
    }
}

fn parse_operation(value: &str) -> ChatProgressOperation {
    match value {
        "search" => ChatProgressOperation::Search,
        "read" => ChatProgressOperation::Read,
        "write" => ChatProgressOperation::Write,
        "edit" => ChatProgressOperation::Edit,
        "list" => ChatProgressOperation::List,
        "calculate" => ChatProgressOperation::Calculate,
        "publish" => ChatProgressOperation::Publish,
        "execute" => ChatProgressOperation::Execute,
        "submit" => ChatProgressOperation::Submit,
        "retry" => ChatProgressOperation::Retry,
        "compact" => ChatProgressOperation::Compact,
        _ => ChatProgressOperation::Generic,
    }
}

fn humanize(value: &str) -> String {
    let mut words = Vec::new();
    for word in value.split(['_', '-']) {
        if word.is_empty() {
            continue;
        }
        let mut chars = word.chars();
        let Some(first) = chars.next() else { continue };
        words.push(format!(
            "{}{}",
            first.to_uppercase(),
            chars.as_str().to_lowercase()
        ));
    }
    if words.is_empty() {
        "a connector tool".into()
    } else {
        let mut label = words.remove(0);
        if !words.is_empty() {
            label.push(' ');
            label.push_str(&words.join(" ").to_lowercase());
        }
        label
    }
}

fn first_string(args: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| args.get(*key).and_then(Value::as_str))
        .and_then(safe_value)
}

fn path_target(args: &Value) -> Option<String> {
    ["path", "file", "directory"]
        .iter()
        .find_map(|key| args.get(*key).and_then(Value::as_str))
        .and_then(safe_relative_path)
}

fn command_target(args: &Value) -> Option<String> {
    let first = args
        .get("command")
        .and_then(Value::as_str)?
        .lines()
        .next()?
        .trim();
    safe_value(first).map(|value| value.chars().take(120).collect())
}

fn named_join(
    args: &Value,
    keys: &[&str],
    providers: &std::collections::BTreeMap<String, String>,
) -> Option<String> {
    let values = keys
        .iter()
        .filter_map(|key| args.get(*key))
        .filter_map(|value| match value {
            Value::String(value) => {
                let resolved = providers
                    .get(value)
                    .cloned()
                    .or_else(|| safe_value(value))?;
                Some(resolved)
            }
            Value::Number(value) => Some(value.to_string()),
            _ => None,
        })
        .collect::<Vec<_>>();
    (!values.is_empty()).then(|| truncate_utf8(&values.join(" · "), 160).to_owned())
}

/// Whether a failed tool result is the host refusing a read under the turn's
/// as-of date (`as_of_unsupported`).
pub(crate) fn as_of_refusal(result: &Value) -> bool {
    let text = result
        .pointer("/content/0/text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    // The extension surfaces the broker's message ("tool execution failed:
    // as_of_unsupported: ..."); a structured result may carry the code itself.
    let parsed = serde_json::from_str::<Value>(text).ok();
    let code = |value: &Value| {
        value
            .get("code")
            .or_else(|| value.pointer("/error/code"))
            .and_then(Value::as_str)
            == Some("as_of_unsupported")
    };
    text.contains("as_of_unsupported:") || parsed.as_ref().is_some_and(code) || code(result)
}

pub(crate) fn summarize_tool_result(result: &Value) -> Option<String> {
    let parsed = result
        .pointer("/content/0/text")
        .and_then(Value::as_str)
        .and_then(|text| serde_json::from_str::<Value>(text).ok());
    let payload = parsed.as_ref().unwrap_or(result);
    let counted = ["rows", "data", "matches", "results", "items", "hits"]
        .iter()
        .find_map(|key| {
            payload
                .get(*key)
                .and_then(Value::as_array)
                .map(|items| (*key, items.len()))
        });
    if let Some((key, len)) = counted {
        let noun = match key {
            "rows" | "data" => "rows",
            "matches" => "matches",
            _ => "results",
        };
        return Some(format!("{len} {noun}"));
    }
    let exit = payload
        .get("exitCode")
        .or_else(|| payload.get("exit_code"))
        .or_else(|| result.pointer("/details/exitCode"))
        .and_then(Value::as_i64);
    exit.map(|code| format!("exit {code}"))
}

fn safe_web_source(source: &crate::web::WebSource) -> Option<(String, String)> {
    let url = reqwest::Url::parse(&source.url).ok()?;
    let host = url.host_str()?;
    validate_progress_href(
        &source.url,
        ChatProgressCategory::Web,
        ChatProgressOperation::Read,
        Some("source"),
    )
    .ok()?;
    let title = safe_web_title(&source.title).unwrap_or_else(|| host.to_owned());
    Some((format!("{title} · {host}"), source.url.clone()))
}

fn safe_web_title(value: &str) -> Option<String> {
    let cleaned = value
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>();
    safe_value(&cleaned)
}

fn safe_value(value: &str) -> Option<String> {
    let value = value.trim();
    let lowercase = value.to_ascii_lowercase();
    if value.is_empty()
        || value.contains(['\0', '\n', '\r'])
        || looks_absolute_path(value)
        || [
            "api_key",
            "apikey",
            "authorization",
            "bearer ",
            "credential",
            "password",
            "secret",
            "token=",
        ]
        .iter()
        .any(|marker| lowercase.contains(marker))
    {
        return None;
    }
    Some(truncate_utf8(value, 160).to_owned())
}

fn safe_relative_path(value: &str) -> Option<String> {
    let value = value.trim();
    let path = Path::new(value);
    if path.as_os_str().is_empty()
        || looks_absolute_path(value)
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return None;
    }
    safe_value(value)
}

fn looks_absolute_path(value: &str) -> bool {
    let value = value.trim();
    Path::new(value).is_absolute()
        || value.starts_with(['~', '\\'])
        || value.as_bytes().get(1) == Some(&b':')
}

fn flatten_params(args: &Value) -> Value {
    let mut combined = args.clone();
    let Some(params) = args.get("params").and_then(Value::as_object) else {
        return combined;
    };
    if let Some(object) = combined.as_object_mut() {
        for (key, value) in params {
            object.entry(key.clone()).or_insert(value.clone());
        }
    }
    combined
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_read_refused_under_the_as_of_date_is_skipped_not_failed() {
        let refusal = serde_json::json!({"content": [{"type": "text", "text":
            "{\"code\":\"as_of_unsupported\",\"message\":\"tool execution failed: as_of_unsupported: kis_read\"}"}]});
        assert!(as_of_refusal(&refusal));
        let other = serde_json::json!({"content": [{"type": "text", "text": "{\"code\":\"execution_failed\"}"}]});
        assert!(!as_of_refusal(&other));
        let message = serde_json::json!({"content": [{"type": "text", "text":
            "tool execution failed: as_of_unsupported: this KIS read endpoint cannot verify publication at a historical as-of cutoff"}]});
        assert!(as_of_refusal(&message));
        let mut projection = ChatProgressProjection::new(10);
        projection.start_tool("call-quote", "kis_read", &serde_json::json!({}), None, 11);
        projection.finish_tool("call-quote", true, 12, None);
        projection.skip_tool("call-quote");
        let Some(ChatProgressItem::Tool { status, .. }) =
            projection.snapshot().items.first().cloned()
        else {
            panic!("the step is listed");
        };
        assert_eq!(status, ChatProgressStatus::Skipped);
    }

    #[test]
    fn a_step_refused_after_third_party_text_carries_its_rule_to_the_renderer() {
        let mut projection = ChatProgressProjection::new(10);
        projection.start_tool(
            "call-bash",
            "bash",
            &json!({"command": "python3 model.py"}),
            None,
            11,
        );
        projection.start_tool("call-read", "read", &json!({"path": "notes.md"}), None, 11);
        // Only a failed step can carry a refusal.
        projection.refuse_tool("call-read", ChatStepRefusal::UntrustedTextPaused);
        projection.finish_tool("call-bash", true, 12, None);
        projection.finish_tool("call-read", false, 12, None);
        projection.refuse_tool("call-bash", ChatStepRefusal::UntrustedTextPaused);
        projection.refuse_tool("call-read", ChatStepRefusal::UntrustedTextPaused);
        let snapshot = projection.snapshot();
        let encoded = serde_json::to_value(&snapshot.items).unwrap();
        assert_eq!(encoded[0]["status"], "failed");
        assert_eq!(encoded[0]["refusal"], "untrusted_text_paused");
        assert!(encoded[1].get("refusal").is_none());
        let decoded: Vec<ChatProgressItem> = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, snapshot.items);
        assert!(decoded[0].validate(true).is_ok());
    }

    #[test]
    fn preserves_parallel_tool_order_and_updates_by_call_id() {
        let mut projection = ChatProgressProjection::new(10);
        projection.start_tool("call-b", "web_search", &json!({"query":"rates"}), None, 10);
        projection.start_tool(
            "call-a",
            "world_read",
            &json!({"scope":"lens","recordId":"module-rates"}),
            None,
            11,
        );
        projection.finish_tool("call-a", false, 15, None);
        projection.finish_tool("call-b", true, 16, None);
        // A finished tool still accepts its result and output revision.
        projection.attach_result("call-b", "evidence-1");
        projection.note_output("call-b");
        let snapshot = projection.finish(20, false).unwrap();
        assert!(matches!(
            &snapshot.items[0],
            ChatProgressItem::Tool {
                status: ChatProgressStatus::Failed,
                result_ids,
                output_revision: Some(1),
                ..
            } if result_ids == &["evidence-1".to_owned()]
        ));
        assert!(matches!(
            snapshot.items[1],
            ChatProgressItem::Tool {
                status: ChatProgressStatus::Succeeded,
                ..
            }
        ));
    }

    #[test]
    fn an_approval_card_sits_at_its_step_and_cannot_stay_waiting_after_the_run() {
        use crate::chat::approvals::{ApprovalState, ChatApproval};
        let mut projection = ChatProgressProjection::new(10);
        projection.start_tool("call-c", "mcp_call", &json!({}), None, 11);
        let card = ChatApproval {
            request_id: "req-1".into(),
            item_id: None,
            connection_id: "custom-1".into(),
            connection_name: "Company research".into(),
            tool_name: "read_company".into(),
            target: Some("Hanul Precision".into()),
            state: ApprovalState::Waiting,
            requested_at_ms: 12,
        };
        assert!(projection.upsert_approval(Some("call-c"), card.clone()));
        let patch = projection.take_patch().unwrap();
        let approvals = patch.approvals.unwrap();
        assert_eq!(approvals.len(), 1);
        assert!(
            approvals[0].item_id.is_some(),
            "the card is attached to its step"
        );
        assert!(
            projection.take_patch().is_none(),
            "an unchanged card is not resent"
        );
        // A second card for the same request replaces the first; the run ending expires it.
        assert!(projection.upsert_approval(None, card));
        projection.finish_tool("call-c", false, 13, None);
        let snapshot = projection.finish(20, true).unwrap();
        assert_eq!(snapshot.approvals.len(), 1);
        assert_eq!(snapshot.approvals[0].state, ApprovalState::Expired);
        assert!(snapshot.approvals[0].item_id.is_some());
        assert!(snapshot.validate(true).is_ok());
    }

    #[test]
    fn a_question_card_sits_at_its_step_is_capped_and_cannot_stay_waiting_after_the_run() {
        use crate::chat::questions::{
            ChatQuestion, ChatQuestionOption, QuestionAnswer, QuestionState, MAX_PROGRESS_QUESTIONS,
        };
        let card = |request_id: &str| ChatQuestion {
            request_id: request_id.into(),
            item_id: None,
            question: "Which period?".into(),
            options: ["Quarterly", "Annual"]
                .map(|label| ChatQuestionOption {
                    label: label.into(),
                    description: None,
                })
                .to_vec(),
            recommended: Some(0),
            state: QuestionState::Waiting,
            answer: None,
            requested_at_ms: 12,
        };
        let mut projection = ChatProgressProjection::new(10);
        projection.start_tool("call-q", "ask_user", &json!({}), None, 11);
        assert!(projection.upsert_question(Some("call-q"), card("req-1")));
        let questions = projection.take_patch().unwrap().questions.unwrap();
        assert!(
            questions[0].item_id.is_some(),
            "the card is attached to its step"
        );
        assert!(projection.take_patch().is_none());
        // The answer replaces the waiting card and keeps its step.
        let mut answered = card("req-1");
        answered.state = QuestionState::Answered;
        answered.answer = Some(QuestionAnswer::Choice(1));
        assert!(projection.upsert_question(None, answered));
        let questions = projection.take_patch().unwrap().questions.unwrap();
        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].answer, Some(QuestionAnswer::Choice(1)));
        assert!(questions[0].item_id.is_some());
        for index in 2..=MAX_PROGRESS_QUESTIONS {
            assert!(projection.upsert_question(None, card(&format!("req-{index}"))));
        }
        assert!(!projection.upsert_question(None, card("req-over")));
        projection.finish_tool("call-q", false, 13, None);
        let snapshot = projection.finish(20, true).unwrap();
        assert_eq!(snapshot.questions[0].state, QuestionState::Answered);
        assert_eq!(snapshot.questions[1].state, QuestionState::Expired);
        assert!(snapshot.validate(true).is_ok());
        let mut waiting = snapshot;
        waiting.questions[1].state = QuestionState::Waiting;
        assert!(waiting.validate(true).is_err());
    }

    #[test]
    fn each_tool_step_names_its_tool_so_two_tools_with_one_label_stay_apart() {
        let mut projection = ChatProgressProjection::new(10);
        projection.start_tool(
            "call-chart",
            "chart_publish",
            &json!({"title":"Margin"}),
            None,
            11,
        );
        projection.start_tool(
            "call-quote",
            "mcp__openbb__equity_price_quote",
            &json!({"symbol":"AAPL"}),
            None,
            12,
        );
        projection.start_tool("call-odd", "bad name/../x", &json!({}), None, 13);
        let patch = projection.take_patch().unwrap();
        let encoded = serde_json::to_value(&patch.upsert_items).unwrap();
        assert_eq!(encoded[0]["toolName"], "chart_publish");
        assert_eq!(encoded[1]["toolName"], "mcp__openbb__equity_price_quote");
        // A name outside the tool alphabet is left out rather than shown.
        assert!(encoded[2].get("toolName").is_none());
        for item in &patch.upsert_items {
            assert!(item.validate(false).is_ok());
        }
        let mut forged = encoded[0].clone();
        forged["toolName"] = json!("chart publish");
        let forged: ChatProgressItem = serde_json::from_value(forged).unwrap();
        assert!(forged.validate(false).is_err());
        // Steps recorded before the field existed still load, without a name.
        let mut earlier = encoded[0].clone();
        earlier.as_object_mut().unwrap().remove("toolName");
        let earlier: ChatProgressItem = serde_json::from_value(earlier).unwrap();
        assert!(matches!(
            earlier,
            ChatProgressItem::Tool {
                tool_name: None,
                ..
            }
        ));
    }

    #[test]
    fn attaches_bounded_unique_world_names_to_the_reading_step() {
        let mut projection = ChatProgressProjection::new(10);
        projection.start_tool(
            "call-w",
            "world_read",
            &json!({"scope":"world","recordId":"world-1"}),
            None,
            11,
        );
        let mut ids: Vec<String> = (0..40).map(|index| format!("node-{index}")).collect();
        ids.insert(1, "node-0".into());
        ids.insert(2, "bad id with spaces".into());
        projection.attach_entities("call-w", &ids);
        // Unknown calls are ignored rather than attributed to another step.
        projection.attach_entities("call-unknown", &["node-x".to_owned()]);
        projection.finish_tool("call-w", false, 12, None);
        let snapshot = projection.finish(20, false).unwrap();
        let ChatProgressItem::Tool { entity_ids, .. } = &snapshot.items[0] else {
            panic!("expected a tool step");
        };
        assert_eq!(entity_ids.len(), MAX_PROGRESS_ENTITY_IDS);
        assert_eq!(entity_ids[0], "node-0");
        assert_eq!(entity_ids[1], "node-1");
        assert!(!entity_ids.iter().any(|id| id.contains(' ')));
        let encoded = serde_json::to_value(&snapshot.items[0]).unwrap();
        assert_eq!(encoded["entityIds"][0], "node-0");
        assert!(snapshot.items[0].validate(true).is_ok());
    }

    #[test]
    fn rejects_entity_references_beyond_the_bound_or_alphabet() {
        let item = |ids: Vec<String>| ChatProgressItem::Tool {
            id: "tool-1".into(),
            tool_name: Some("world_read".into()),
            category: ChatProgressCategory::Memory,
            operation: ChatProgressOperation::Read,
            action: "Read the map".into(),
            running: None,
            target: None,
            href: None,
            source: None,
            summary: None,
            result_ids: Vec::new(),
            entity_ids: ids,
            output_revision: None,
            detail_id: None,
            file_path: None,
            status: ChatProgressStatus::Succeeded,
            refusal: None,
            started_at_ms: 1,
            finished_at_ms: Some(2),
        };
        assert!(item(vec!["ent:a.b-c_d".into()]).validate(true).is_ok());
        assert!(item(vec!["bad/id".into()]).validate(true).is_err());
        assert!(item(vec![String::new()]).validate(true).is_err());
        assert!(item((0..33).map(|index| format!("n{index}")).collect())
            .validate(true)
            .is_err());
    }

    #[test]
    fn patches_coalesce_commentary_and_only_upsert_changed_items() {
        let mut projection = ChatProgressProjection::new(10);
        projection.start_assistant_turn();
        projection.append_commentary("I will ");
        projection.append_commentary("inspect this.");

        let commentary = projection.take_patch().unwrap();
        assert_eq!(commentary.sequence, 1);
        assert_eq!(commentary.upsert_items.len(), 1);
        assert!(matches!(
            &commentary.upsert_items[0],
            ChatProgressItem::Commentary { text, .. } if text == "I will inspect this."
        ));
        assert!(commentary.append_text.is_empty());
        assert!(projection.take_patch().is_none());

        // Once published, a commentary travels as the text appended since.
        projection.append_commentary(" Then ");
        projection.append_commentary("the filing.");
        let appended = projection.take_patch().unwrap();
        assert!(appended.upsert_items.is_empty());
        assert_eq!(
            appended.append_text,
            vec![CommentaryAppend {
                id: "commentary-1".into(),
                text: " Then the filing.".into()
            }]
        );
        assert!(projection.take_patch().is_none());

        projection.start_tool("call-1", "web_search", &json!({"query": "btc"}), None, 11);
        let started = projection.take_patch().unwrap();
        assert_eq!(started.sequence, 3);
        assert_eq!(started.upsert_items.len(), 1);
        assert!(matches!(
            started.upsert_items[0],
            ChatProgressItem::Tool {
                status: ChatProgressStatus::Running,
                ..
            }
        ));

        projection.finish_tool("call-1", false, 12, None);
        let finished = projection.take_patch().unwrap();
        assert_eq!(finished.sequence, 4);
        assert_eq!(finished.upsert_items.len(), 1);
        assert!(matches!(
            finished.upsert_items[0],
            ChatProgressItem::Tool {
                status: ChatProgressStatus::Succeeded,
                ..
            }
        ));
    }

    #[test]
    fn patches_remove_published_final_commentary_and_mark_terminal_finish() {
        let mut projection = ChatProgressProjection::new(10);
        projection.start_assistant_turn();
        projection.append_commentary("Final answer draft");
        let published = projection.take_patch().unwrap();
        let commentary_id = published.upsert_items[0].id().to_owned();

        projection.append_commentary(" and more");
        assert_eq!(
            projection.live_commentary_id(),
            Some(commentary_id.as_str())
        );
        projection.finish_assistant_turn(true);
        assert_eq!(projection.live_commentary_id(), None);
        // Text appended after the last patch leaves with the item: no append for a removed id.
        let removed = projection.take_patch().unwrap();
        assert_eq!(removed.sequence, 2);
        assert!(removed.upsert_items.is_empty());
        assert!(removed.append_text.is_empty());
        assert_eq!(removed.remove_item_ids, vec![commentary_id]);

        assert!(projection.finish(20, false).is_none());
        let terminal = projection.take_patch().unwrap();
        assert_eq!(terminal.sequence, 3);
        assert_eq!(terminal.finished_at_ms, Some(20));
        assert!(terminal.upsert_items.is_empty());
        assert_eq!(projection.latest_patch_sequence(), 3);
        assert_eq!(projection.snapshot().finished_at_ms, Some(20));
    }

    #[test]
    fn tool_use_commentary_interleaves_with_tools_and_final_commentary_is_removed() {
        let mut projection = ChatProgressProjection::new(10);
        projection.start_assistant_turn();
        projection.append_commentary("I will search first.");
        projection.start_tool("call-1", "web_search", &json!({"query": "btc"}), None, 11);
        projection.finish_tool("call-1", false, 12, None);
        projection.append_commentary("The source is enough to continue.");
        projection.start_tool(
            "call-2",
            "world_read",
            &json!({"scope":"lens","recordId":"module-rates"}),
            None,
            13,
        );
        projection.finish_assistant_turn(false);
        // A final answer turn's commentary is removed; tool-use commentary stays.
        projection.start_assistant_turn();
        projection.append_commentary("The change is complete.");
        projection.finish_assistant_turn(true);
        let snapshot = projection.snapshot();
        assert_eq!(snapshot.items.len(), 4);
        assert!(
            matches!(&snapshot.items[0], ChatProgressItem::Commentary { text, .. } if text == "I will search first.")
        );
        assert!(matches!(&snapshot.items[1], ChatProgressItem::Tool { .. }));
        assert!(
            matches!(&snapshot.items[2], ChatProgressItem::Commentary { text, .. } if text == "The source is enough to continue.")
        );
        assert!(matches!(&snapshot.items[3], ChatProgressItem::Tool { .. }));
    }

    #[test]
    fn a_patch_never_carries_one_commentary_as_both_an_upsert_and_an_append() {
        let mut projection = ChatProgressProjection::new(10);
        projection.start_assistant_turn();
        projection.append_commentary("First");
        projection.take_patch().unwrap();
        projection.append_commentary(" and second");
        projection.finish_assistant_turn(false);
        projection.start_assistant_turn();
        projection.append_commentary("A new note");
        let patch = projection.take_patch().unwrap();
        let upserted: Vec<&str> = patch
            .upsert_items
            .iter()
            .map(ChatProgressItem::id)
            .collect();
        let appended: Vec<&str> = patch
            .append_text
            .iter()
            .map(|append| append.id.as_str())
            .collect();
        assert_eq!(upserted, ["commentary-2"]);
        assert_eq!(appended, ["commentary-1"]);
        assert!(upserted.iter().all(|id| !appended.contains(id)));
        assert_eq!(patch.append_text[0].text, " and second");
    }

    #[test]
    fn a_commentary_rewritten_under_its_published_prefix_is_published_whole_again() {
        let mut projection = ChatProgressProjection::new(10);
        projection.start_assistant_turn();
        projection.append_commentary("Revenue rose 8%");
        let first = projection.take_patch().unwrap();
        let id = first.upsert_items[0].id().to_owned();

        // The same length under a different prefix: an append could carry nothing.
        let index = projection.item_indices[&id];
        if let ChatProgressItem::Commentary { text, .. } = &mut projection.progress.items[index] {
            *text = "Revenue fell 8%".to_owned();
        }
        projection.mark_dirty(&id);
        let rewritten = projection
            .take_patch()
            .expect("a rewritten commentary is republished");
        assert!(rewritten.append_text.is_empty());
        assert!(matches!(
            &rewritten.upsert_items[0],
            ChatProgressItem::Commentary { text, .. } if text == "Revenue fell 8%"
        ));

        // Growth after the rewrite travels as an append again.
        projection.append_commentary(" in Q3.");
        let grown = projection.take_patch().unwrap();
        assert!(grown.upsert_items.is_empty());
        assert_eq!(
            grown.append_text,
            vec![CommentaryAppend {
                id,
                text: " in Q3.".into()
            }]
        );
    }

    #[test]
    fn summaries_drop_absolute_paths_private_arguments_and_unsafe_links() {
        for (tool, arguments) in [
            ("read", json!({"path":"/Users/me/secret.txt"})),
            ("read", json!({"path":"C:\\Users\\me\\secret.txt"})),
            (
                "world_read",
                json!({"scope":"lens","recordId":"module-rates"}),
            ),
            ("note_save", json!({"journal":{"thesis":"private thesis"}})),
            (
                "principle_save",
                json!({"statement":"private rule", "why":"private reason"}),
            ),
            (
                "capability_load",
                json!({"capability_id":"world-bank.indicators"}),
            ),
            ("capability_secret", json!({"token":"credential"})),
        ] {
            assert!(
                summarize_tool(tool, &arguments, None).target.is_none(),
                "{tool}"
            );
        }
        let query = summarize_tool(
            "web_search",
            &json!({"query":"authorization: Bearer credential"}),
            None,
        );
        assert!(!query.target.unwrap_or_default().contains("credential"));
        // A built-in tool group shows the catalog's wording the interface
        // translates, never its internal name; a connection keeps its name.
        let built_in = summarize_tool("capability_load", &json!({"id": "memory-save"}), None);
        assert_eq!(
            (built_in.action.as_str(), built_in.running.as_deref()),
            (
                "Prepared a research tool",
                Some("Preparing a research tool")
            )
        );
        assert_eq!(
            summarize_tool("capability_load", &json!({"id": "opendart"}), None).action,
            "Opened OpenDART"
        );
        let openbb = summarize_tool(
            "mcp__openbb__equity_price_historical",
            &json!({"symbol":"AAPL","provider":"yfinance"}),
            None,
        );
        assert_eq!(openbb.target.as_deref(), Some("AAPL"));
        // A setup step's target is the resolved entry id the card is drawn
        // from, and the connection's name labels the step.
        let setup = summarize_tool("connection_setup", &json!({"id": "FRED"}), None);
        assert_eq!(setup.target.as_deref(), Some("fred.macro"));
        assert_eq!(
            setup.source.map(|source| source.name).as_deref(),
            Some("FRED")
        );
        assert_eq!(setup.action, "Prepared a connection");
        let unresolved = summarize_tool("connection_setup", &json!({"id": "Bloomberg"}), None);
        assert!(unresolved.target.is_none());
        assert!(unresolved.source.is_none());

        let source = |title: &str, url: &str| crate::web::WebSource {
            source_id: "web:f6495eccf2c0cca04e4c6a03".into(),
            title: title.into(),
            url: url.into(),
            snippet: "raw provider text".into(),
            published_at: None,
        };
        let safe = source("Quarterly\u{0001} report", "https://example.com/reports/q1");
        let mut projection = ChatProgressProjection::new(10);
        projection.start_tool(
            "call",
            "web_fetch",
            &json!({"source_id": safe.source_id}),
            Some(&safe),
            10,
        );
        projection.finish_tool("call", false, 18, None);
        let serialized = serde_json::to_string(&projection.finish(20, false).unwrap()).unwrap();
        assert!(serialized.contains("Quarterly report · example.com"));
        assert!(serialized.contains("https://example.com/reports/q1"));
        assert!(!serialized.contains("web:f6495ecc"));
        assert!(!serialized.contains("raw provider text"));
        let credentialed = source("Private source", "https://user:password@example.com/report");
        let presentation = summarize_tool("web_fetch", &json!({}), Some(&credentialed));
        assert!(!presentation.target.unwrap_or_default().contains("Private"));
        assert!(presentation.href.is_none());
    }

    #[test]
    fn stop_finalizes_running_items_without_raw_results() {
        let mut projection = ChatProgressProjection::new(10);
        projection.start_tool(
            "call",
            "web_fetch",
            &json!({"source_id":"source-1"}),
            None,
            10,
        );
        let snapshot = projection.finish(20, true).unwrap();
        assert!(matches!(
            snapshot.items[0],
            ChatProgressItem::Tool {
                status: ChatProgressStatus::Stopped,
                ..
            }
        ));
        assert!(!serde_json::to_string(&snapshot)
            .unwrap()
            .contains("source-1"));
    }
}

#[cfg(test)]
mod dataset_export_progress_tests {
    use super::*;
    #[test]
    fn dataset_exports_link_the_exact_file_only_after_success() {
        let mut projection = ChatProgressProjection::new(1);
        let path = format!("reports/{}/prices.csv", "a".repeat(180));
        projection.start_tool(
            "export",
            "dataset_export",
            &serde_json::json!({"path":path}),
            None,
            2,
        );
        let start = projection.take_patch().unwrap();
        assert!(matches!(
            &start.upsert_items[0],
            ChatProgressItem::Tool {
                file_path: None,
                category: ChatProgressCategory::Files,
                operation: ChatProgressOperation::Write,
                ..
            }
        ));
        projection.finish_tool("export", false, 3, None);
        let done = projection.take_patch().unwrap();
        assert!(
            matches!(&done.upsert_items[0], ChatProgressItem::Tool { file_path: Some(actual), status: ChatProgressStatus::Succeeded, .. } if actual == &path)
        );
        projection.start_tool(
            "failed",
            "dataset_export",
            &serde_json::json!({"path":"failed.csv"}),
            None,
            4,
        );
        projection.take_patch();
        projection.finish_tool("failed", true, 5, None);
        assert!(matches!(
            &projection.take_patch().unwrap().upsert_items[0],
            ChatProgressItem::Tool {
                file_path: None,
                status: ChatProgressStatus::Failed,
                ..
            }
        ));
    }
}

#[cfg(test)]
mod plan_tests {
    use super::*;
    use serde_json::json;

    fn plan_args(statuses: &[(&str, &str)]) -> Value {
        json!({"items": statuses.iter().map(|(id, status)| json!({
            "id": id, "label": format!("Check {id}"), "status": status,
        })).collect::<Vec<_>>()})
    }

    fn plan_statuses(plan: &ChatPlan) -> Vec<ChatPlanStatus> {
        plan.items.iter().map(|item| item.status).collect()
    }

    #[test]
    fn plan_updates_become_one_checklist_not_step_rows() {
        let mut projection = ChatProgressProjection::new(10);
        projection.start_tool(
            "plan-1",
            "plan_update",
            &plan_args(&[("filings", "active"), ("prices", "pending")]),
            None,
            11,
        );
        projection.finish_tool("plan-1", false, 12, None);
        let first = projection.take_patch().expect("plan patch");
        assert!(first.upsert_items.is_empty());
        let plan = first.plan.expect("plan in patch");
        assert_eq!(plan.items[0].activated_at_ms, Some(11));
        assert_eq!(plan.items[1].activated_at_ms, None);
        let encoded = serde_json::to_value(&plan).unwrap();
        assert_eq!(encoded["items"][0]["activatedAtMs"], 11);
        assert_eq!(encoded["items"][1]["status"], "pending");

        projection.start_tool("read", "web_search", &json!({"query":"filing"}), None, 13);
        projection.start_tool(
            "plan-2",
            "plan_update",
            &plan_args(&[("filings", "done"), ("prices", "active")]),
            None,
            20,
        );
        let second = projection.take_patch().expect("second patch");
        let plan = second.plan.expect("changed plan");
        // An item keeps the time it first became active.
        assert_eq!(plan.items[0].activated_at_ms, Some(11));
        assert_eq!(plan.items[1].activated_at_ms, Some(20));
        assert_eq!(projection.snapshot().items.len(), 1);
        // Nothing changed, so no patch and no plan.
        assert!(projection.take_patch().is_none());
    }

    #[test]
    fn invalid_plan_calls_show_nothing() {
        let mut projection = ChatProgressProjection::new(10);
        for args in [
            plan_args(&[("only", "active")]),
            plan_args(&[("a", "active"), ("b", "active")]),
            plan_args(&[("a", "pending"), ("a", "pending")]),
            plan_args(&[("bad id", "pending"), ("b", "pending")]),
            plan_args(&[("a", "started"), ("b", "pending")]),
            json!({"items": [{"id":"a","label":"Line\nbreak","status":"pending"},{"id":"b","label":"B","status":"pending"}]}),
            json!({"items": [{"id":"a","label":"x".repeat(121),"status":"pending"},{"id":"b","label":"B","status":"pending"}]}),
            json!({"items": [{"id":"a","label":"A","status":"pending","note":"x"},{"id":"b","label":"B","status":"pending"}]}),
        ] {
            projection.start_tool("plan", "plan_update", &args, None, 11);
        }
        assert!(projection.take_patch().is_none());
        assert!(projection.snapshot().plan.is_none());
        assert!(projection.finish(20, false).is_none());
    }

    #[test]
    fn finishing_closes_unfinished_plan_items() {
        let statuses = [("a", "done"), ("b", "active"), ("c", "pending")];
        let mut completed = ChatProgressProjection::new(10);
        completed.start_tool("p", "plan_update", &plan_args(&statuses), None, 11);
        let snapshot = completed
            .finish(20, false)
            .expect("plan keeps the progress");
        assert_eq!(
            plan_statuses(snapshot.plan.as_ref().unwrap()),
            [
                ChatPlanStatus::Done,
                ChatPlanStatus::Done,
                ChatPlanStatus::NotDone
            ]
        );
        assert!(snapshot.validate(true).is_ok());
        assert!(completed.take_patch().unwrap().plan.is_some());

        let mut stopped = ChatProgressProjection::new(10);
        stopped.start_tool("p", "plan_update", &plan_args(&statuses), None, 11);
        let snapshot = stopped.finish(20, true).unwrap();
        assert_eq!(
            plan_statuses(snapshot.plan.as_ref().unwrap()),
            [
                ChatPlanStatus::Done,
                ChatPlanStatus::NotDone,
                ChatPlanStatus::NotDone
            ]
        );

        let mut failed = ChatProgressProjection::new(10);
        failed.start_tool("p", "plan_update", &plan_args(&statuses), None, 11);
        failed.fail_plan();
        let snapshot = failed.finish(20, false).unwrap();
        assert_eq!(
            plan_statuses(snapshot.plan.as_ref().unwrap())[1],
            ChatPlanStatus::NotDone
        );
    }

    #[test]
    fn durable_progress_rejects_an_active_or_oversized_plan() {
        let item = |id: &str, status| ChatPlanItem {
            id: id.into(),
            label: "Check".into(),
            status,
            activated_at_ms: None,
        };
        let progress = |items| ChatProgress {
            started_at_ms: 1,
            finished_at_ms: Some(2),
            items: vec![],
            plan: Some(ChatPlan { items }),
            approvals: Vec::new(),
            questions: Vec::new(),
        };
        let running = progress(vec![
            item("a", ChatPlanStatus::Active),
            item("b", ChatPlanStatus::Pending),
        ]);
        assert!(running.validate(false).is_ok());
        assert!(running.validate(true).is_err());
        let oversized = progress(
            (0..7)
                .map(|index| item(&format!("i{index}"), ChatPlanStatus::Done))
                .collect(),
        );
        assert!(oversized.validate(false).is_err());
        let decoded: ChatProgress = serde_json::from_value(json!({
            "startedAtMs": 1, "finishedAtMs": null, "items": [],
        }))
        .unwrap();
        assert!(decoded.plan.is_none());
    }

    /// Steps behind a grid read assets by symbol; the timeline names them as
    /// the Loupist's asset pages do, and keeps symbols it has no page for (B99).
    #[test]
    fn steps_name_assets_the_loupist_has_pages_for() {
        let mut projection = ChatProgressProjection::new(10);
        projection.set_asset_names(AssetNames::new([
            (
                "990221".to_owned(),
                "KRX".to_owned(),
                "Solsup Foods".to_owned(),
            ),
            ("AAPL".to_owned(), "NASDAQ".to_owned(), "Apple".to_owned()),
            (
                "MSFT".to_owned(),
                "NASDAQ".to_owned(),
                "Microsoft".to_owned(),
            ),
            // One symbol on two markets under two names is named only with its market.
            ("BTC".to_owned(), "CRYPTO".to_owned(), "Bitcoin".to_owned()),
            (
                "BTC".to_owned(),
                "CME".to_owned(),
                "Bitcoin futures".to_owned(),
            ),
        ]));
        projection.start_tool(
            "a",
            "asset_page_read",
            &json!({"symbol": "990221", "market": "KRX"}),
            None,
            11,
        );
        projection.start_tool(
            "b",
            "krx_daily_quote",
            &json!({"symbol": "990221", "date": "2026-03-18"}),
            None,
            12,
        );
        projection.start_tool(
            "c",
            "mcp__openbb__equity_fundamental_income",
            &json!({"params": {"symbol": "aapl,MSFT,ZZZZ", "provider": "yfinance"}}),
            None,
            13,
        );
        projection.start_tool("d", "asset_page_read", &json!({"symbol": "BTC"}), None, 14);
        projection.start_tool(
            "e",
            "asset_page_read",
            &json!({"symbol": "BTC", "market": "CME"}),
            None,
            15,
        );
        let targets = projection
            .snapshot()
            .items
            .iter()
            .map(|item| match item {
                ChatProgressItem::Tool { target, .. } => target.clone().unwrap_or_default(),
                _ => String::new(),
            })
            .collect::<Vec<_>>();
        // A connection's name may lead the target ("KRX · …").
        assert!(targets[0].ends_with("Solsup Foods"), "{targets:?}");
        assert!(
            targets[1].ends_with("Solsup Foods · 2026-03-18"),
            "{targets:?}"
        );
        assert!(targets[2].contains("Apple, Microsoft, ZZZZ"), "{targets:?}");
        assert!(targets[3].ends_with("BTC"), "{targets:?}");
        assert!(targets[4].ends_with("Bitcoin futures"), "{targets:?}");
        // Without names, steps show the arguments as before.
        let mut plain = ChatProgressProjection::new(10);
        plain.start_tool(
            "a",
            "asset_page_read",
            &json!({"symbol": "990221"}),
            None,
            11,
        );
        assert!(matches!(
            &plain.snapshot().items[0],
            ChatProgressItem::Tool { target: Some(target), .. } if target.ends_with("990221")
        ));
    }
}
