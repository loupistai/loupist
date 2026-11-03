//! Per-Loupist World extension: extra entity and relation kinds on top of the core schema.
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use super::error::{KnowledgeError, KnowledgeResult};
use super::model::{mint_id, validate_loupist_id};

const CORE_TYPES: &[&str] = &[
    "Organization",
    "Person",
    "Asset",
    "Product",
    "Place",
    "Policy",
    "Event",
    "Metric",
];
const PROTECTED_TYPES: &[&str] = &["Entity", "Episodic", "Community", "Saga"];
const CORE_RELATIONS: &[&str] = &[
    "SUPPLIES",
    "COMPETES_WITH",
    "PARTNERS_WITH",
    "SUBSIDIARY_OF",
    "ACQUIRED",
    "INVESTED_IN",
    "LEADS",
    "OFFERS",
    "USES",
    "REPORTS",
    "LOCATED_IN",
    "AFFECTS",
];
const PROTECTED_FIELDS: &[&str] = &[
    "original_name",
    "attributes",
    "created_at",
    "group_id",
    "labels",
    "name",
    "name_embedding",
    "summary",
    "uuid",
];

const SEMICONDUCTOR_FOCUS: &[&str] = &[
    "Chips, memory, foundries and equipment",
    "칩, 메모리, 파운드리, 장비",
];
const DIVIDEND_FOCUS: &[&str] = &[
    "Dividend payers and payout safety",
    "배당 기업과 배당 지속성",
];
const MACRO_FOCUS: &[&str] = &[
    "Interest rates, inflation, currencies and central banks",
    "금리, 물가, 환율과 중앙은행",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionLabels {
    pub en: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ko: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionCategory {
    Company,
    Person,
    Product,
    Market,
    Event,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionField {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionType {
    pub name: String,
    pub description: String,
    pub category: ExtensionCategory,
    pub labels: ExtensionLabels,
    /// Short user-facing line. `description` stays the English model-facing definition.
    pub blurbs: ExtensionLabels,
    pub fields: Vec<ExtensionField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionRelation {
    pub name: String,
    pub description: String,
    pub signatures: Vec<(String, String)>,
    pub labels: ExtensionLabels,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldExtension {
    pub version: u32,
    pub types: Vec<ExtensionType>,
    pub relations: Vec<ExtensionRelation>,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProposalKind {
    Seed,
    Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProposalStatus {
    Preparing,
    Pending,
    Applied,
    Dismissed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SchemaProposal {
    pub proposal_id: String,
    pub kind: ProposalKind,
    pub status: ProposalStatus,
    pub types: Vec<ExtensionType>,
    pub relations: Vec<ExtensionRelation>,
    pub created_at_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decided_at_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorldSchemaView {
    pub extension: WorldExtension,
    pub proposal: Option<SchemaProposal>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProposalBody {
    types: Vec<ExtensionType>,
    relations: Vec<ExtensionRelation>,
}

impl ProposalKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Seed => "seed",
            Self::Review => "review",
        }
    }

    fn parse(value: &str) -> KnowledgeResult<Self> {
        match value {
            "seed" => Ok(Self::Seed),
            "review" => Ok(Self::Review),
            _ => Err(KnowledgeError::Invalid("schema proposal is invalid")),
        }
    }
}

impl ProposalStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::Pending => "pending",
            Self::Applied => "applied",
            Self::Dismissed => "dismissed",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> KnowledgeResult<Self> {
        match value {
            "preparing" => Ok(Self::Preparing),
            "pending" => Ok(Self::Pending),
            "applied" => Ok(Self::Applied),
            "dismissed" => Ok(Self::Dismissed),
            "failed" => Ok(Self::Failed),
            _ => Err(KnowledgeError::Invalid("schema proposal is invalid")),
        }
    }
}

pub fn validate_extension(extension: &WorldExtension) -> KnowledgeResult<()> {
    if extension.updated_at_ms < 0 {
        return Err(KnowledgeError::Invalid("extension version time is invalid"));
    }
    if extension.types.len() > 4 {
        return Err(KnowledgeError::Invalid("extension has too many types"));
    }
    if extension.relations.len() > 6 {
        return Err(KnowledgeError::Invalid("extension has too many relations"));
    }
    let mut type_names = Vec::with_capacity(extension.types.len());
    for item in &extension.types {
        if !is_type_name(&item.name) {
            return Err(KnowledgeError::Invalid("extension type name is invalid"));
        }
        if CORE_TYPES.contains(&item.name.as_str()) || PROTECTED_TYPES.contains(&item.name.as_str())
        {
            return Err(KnowledgeError::Invalid("extension type name is reserved"));
        }
        if type_names.iter().any(|name| name == &item.name) {
            return Err(KnowledgeError::Invalid("extension type name is duplicated"));
        }
        check_description(&item.description)?;
        check_labels(&item.labels)?;
        check_blurbs(&item.blurbs)?;
        if item.fields.len() > 3 {
            return Err(KnowledgeError::Invalid(
                "extension type has too many fields",
            ));
        }
        let mut fields = Vec::with_capacity(item.fields.len());
        for field in &item.fields {
            if !is_field_name(&field.name) {
                return Err(KnowledgeError::Invalid("extension field name is invalid"));
            }
            if PROTECTED_FIELDS.contains(&field.name.as_str()) {
                return Err(KnowledgeError::Invalid("extension field name is reserved"));
            }
            if fields.iter().any(|name| name == &field.name) {
                return Err(KnowledgeError::Invalid(
                    "extension field name is duplicated",
                ));
            }
            let length = field.description.chars().count();
            if field.description.contains('\0') || length == 0 || length > 200 {
                return Err(KnowledgeError::Invalid(
                    "extension field description is invalid",
                ));
            }
            fields.push(field.name.clone());
        }
        type_names.push(item.name.clone());
    }
    let mut relation_names = Vec::with_capacity(extension.relations.len());
    for relation in &extension.relations {
        if !is_relation_name(&relation.name) {
            return Err(KnowledgeError::Invalid(
                "extension relation name is invalid",
            ));
        }
        if CORE_RELATIONS.contains(&relation.name.as_str()) || relation.name == "RELATES_TO" {
            return Err(KnowledgeError::Invalid(
                "extension relation name is reserved",
            ));
        }
        if relation_names.iter().any(|name| name == &relation.name) {
            return Err(KnowledgeError::Invalid(
                "extension relation name is duplicated",
            ));
        }
        check_description(&relation.description)?;
        check_labels(&relation.labels)?;
        if relation.signatures.is_empty() {
            return Err(KnowledgeError::Invalid(
                "extension relation has no signatures",
            ));
        }
        if relation.signatures.len() > 6 {
            return Err(KnowledgeError::Invalid(
                "extension relation has too many signatures",
            ));
        }
        for (source, target) in &relation.signatures {
            if !endpoint_allowed(source, &type_names) || !endpoint_allowed(target, &type_names) {
                return Err(KnowledgeError::Invalid(
                    "extension signature endpoint is invalid",
                ));
            }
        }
        relation_names.push(relation.name.clone());
    }
    Ok(())
}

fn check_description(text: &str) -> KnowledgeResult<()> {
    let length = text.chars().count();
    if text.contains('\0') || !(20..=500).contains(&length) {
        Err(KnowledgeError::Invalid("extension description is invalid"))
    } else {
        Ok(())
    }
}

fn check_labels(labels: &ExtensionLabels) -> KnowledgeResult<()> {
    if !label_ok(&labels.en) {
        return Err(KnowledgeError::Invalid("extension label is invalid"));
    }
    if labels.ko.as_deref().is_some_and(|value| !label_ok(value)) {
        return Err(KnowledgeError::Invalid("extension label is invalid"));
    }
    Ok(())
}

fn label_ok(text: &str) -> bool {
    bounded_text(text, 40)
}

fn check_blurbs(blurbs: &ExtensionLabels) -> KnowledgeResult<()> {
    if !blurb_ok(&blurbs.en) || blurbs.ko.as_deref().is_some_and(|value| !blurb_ok(value)) {
        Err(KnowledgeError::Invalid("extension blurb is invalid"))
    } else {
        Ok(())
    }
}

fn blurb_ok(text: &str) -> bool {
    bounded_text(text, 120)
}

fn bounded_text(text: &str, max: usize) -> bool {
    let length = text.chars().count();
    !text.trim().is_empty()
        && (1..=max).contains(&length)
        && !text
            .chars()
            .any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
}

fn endpoint_allowed(name: &str, extension_types: &[String]) -> bool {
    name == "Entity"
        || CORE_TYPES.contains(&name)
        || extension_types.iter().any(|item| item == name)
}

fn is_type_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_uppercase() {
        return false;
    }
    let rest = chars.count();
    (1..=31).contains(&rest) && name.chars().skip(1).all(|ch| ch.is_ascii_alphanumeric())
}

fn is_relation_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (2..=32).contains(&bytes.len())
        && bytes[0].is_ascii_uppercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn is_field_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    (2..=32).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

fn bump(extension: &mut WorldExtension, now_ms: i64) -> KnowledgeResult<()> {
    extension.version = extension
        .version
        .checked_add(1)
        .ok_or(KnowledgeError::Invalid("schema version overflow"))?;
    extension.updated_at_ms = now_ms;
    Ok(())
}

pub(crate) fn schema_view(
    tx: &Transaction<'_>,
    loupist_id: &str,
    extension: &WorldExtension,
) -> KnowledgeResult<WorldSchemaView> {
    Ok(WorldSchemaView {
        extension: extension.clone(),
        proposal: open_proposal(tx, loupist_id)?,
    })
}

pub(crate) fn open_proposal(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<Option<SchemaProposal>> {
    validate_loupist_id(loupist_id)?;
    let row = tx
        .query_row(
            "SELECT proposal_id, kind, status, body_json, created_at_ms, decided_at_ms
             FROM knowledge_schema_proposals
             WHERE loupist_id = ?1 AND status IN ('preparing', 'pending')
             ORDER BY created_at_ms DESC, proposal_id DESC LIMIT 1",
            [loupist_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                ))
            },
        )
        .optional()?;
    row.map(|row| decode_proposal(row.0, row.1, row.2, row.3, row.4, row.5))
        .transpose()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_proposal(
    tx: &Transaction<'_>,
    loupist_id: &str,
    kind: ProposalKind,
    status: ProposalStatus,
    types: Vec<ExtensionType>,
    relations: Vec<ExtensionRelation>,
    created_at_ms: i64,
    decided_at_ms: Option<i64>,
) -> KnowledgeResult<String> {
    validate_loupist_id(loupist_id)?;
    if created_at_ms < 0 || decided_at_ms.is_some_and(|value| value < 0) {
        return Err(KnowledgeError::Invalid("extension version time is invalid"));
    }
    let proposal_id = mint_id("wsp");
    let body = serde_json::to_string(&ProposalBody { types, relations })?;
    tx.execute(
        "INSERT INTO knowledge_schema_proposals
         (proposal_id, loupist_id, kind, status, body_json, created_at_ms, decided_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            proposal_id,
            loupist_id,
            kind.as_str(),
            status.as_str(),
            body,
            created_at_ms,
            decided_at_ms,
        ],
    )?;
    Ok(proposal_id)
}

pub(crate) fn apply_proposal(
    tx: &Transaction<'_>,
    loupist_id: &str,
    extension: &mut WorldExtension,
    proposal_id: &str,
    now_ms: i64,
) -> KnowledgeResult<()> {
    let proposal = load_proposal(tx, loupist_id, proposal_id)?;
    if proposal.status != ProposalStatus::Pending {
        return Err(KnowledgeError::Invalid("schema proposal is not pending"));
    }
    let mut merged = extension.clone();
    merged.types.extend(proposal.types.clone());
    merged.relations.extend(proposal.relations.clone());
    bump(&mut merged, now_ms)?;
    validate_extension(&merged)?;
    *extension = merged;
    decide(tx, loupist_id, proposal_id, ProposalStatus::Applied, now_ms)?;
    for item in &proposal.types {
        append_growth(
            tx,
            loupist_id,
            now_ms,
            "schema_extended",
            &item.name,
            &item.labels,
            "user",
        )?;
    }
    for item in &proposal.relations {
        append_growth(
            tx,
            loupist_id,
            now_ms,
            "schema_extended",
            &item.name,
            &item.labels,
            "user",
        )?;
    }
    Ok(())
}

pub(crate) fn dismiss_proposal(
    tx: &Transaction<'_>,
    loupist_id: &str,
    proposal_id: &str,
    now_ms: i64,
) -> KnowledgeResult<()> {
    let proposal = load_proposal(tx, loupist_id, proposal_id)?;
    if !matches!(
        proposal.status,
        ProposalStatus::Pending | ProposalStatus::Preparing
    ) {
        return Err(KnowledgeError::Invalid(
            "schema proposal cannot be dismissed",
        ));
    }
    decide(
        tx,
        loupist_id,
        proposal_id,
        ProposalStatus::Dismissed,
        now_ms,
    )
}

pub(crate) fn remove_schema_item(
    tx: &Transaction<'_>,
    loupist_id: &str,
    extension: &mut WorldExtension,
    kind: &str,
    name: &str,
    now_ms: i64,
) -> KnowledgeResult<()> {
    let mut next = extension.clone();
    let mut reduced = Vec::new();
    match kind {
        "type" => {
            let index = next
                .types
                .iter()
                .position(|item| item.name == name)
                .ok_or(KnowledgeError::NotFound)?;
            let removed = next.types.remove(index);
            reduced.push((removed.name, removed.labels));
            let mut kept = Vec::new();
            for mut relation in next.relations.drain(..) {
                relation
                    .signatures
                    .retain(|(source, target)| source != name && target != name);
                if relation.signatures.is_empty() {
                    reduced.push((relation.name, relation.labels));
                } else {
                    kept.push(relation);
                }
            }
            next.relations = kept;
        }
        "relation" => {
            let index = next
                .relations
                .iter()
                .position(|item| item.name == name)
                .ok_or(KnowledgeError::NotFound)?;
            let removed = next.relations.remove(index);
            reduced.push((removed.name, removed.labels));
        }
        _ => return Err(KnowledgeError::Invalid("schema remove kind is invalid")),
    }
    bump(&mut next, now_ms)?;
    validate_extension(&next)?;
    *extension = next;
    for (removed_name, labels) in &reduced {
        append_growth(
            tx,
            loupist_id,
            now_ms,
            "schema_reduced",
            removed_name,
            labels,
            "user",
        )?;
    }
    Ok(())
}

/// Curated extension for a new Loupist whose focus is still the template default.
pub fn curated_seed(template: Option<&str>, focus: &str, now_ms: i64) -> Option<WorldExtension> {
    let focus = focus.trim();
    let mut extension = match template? {
        "semiconductors" if SEMICONDUCTOR_FOCUS.contains(&focus) => semiconductors(),
        "dividends" if DIVIDEND_FOCUS.contains(&focus) => dividends(),
        "macro" if MACRO_FOCUS.contains(&focus) => macro_rates(),
        _ => return None,
    };
    extension.version = 1;
    extension.updated_at_ms = now_ms;
    Some(extension)
}

pub(crate) fn record_applied_seed(
    tx: &Transaction<'_>,
    loupist_id: &str,
    extension: &WorldExtension,
) -> KnowledgeResult<()> {
    if extension.types.is_empty() && extension.relations.is_empty() {
        return Ok(());
    }
    insert_proposal(
        tx,
        loupist_id,
        ProposalKind::Seed,
        ProposalStatus::Applied,
        extension.types.clone(),
        extension.relations.clone(),
        extension.updated_at_ms,
        Some(extension.updated_at_ms),
    )?;
    for item in &extension.types {
        append_growth(
            tx,
            loupist_id,
            extension.updated_at_ms,
            "schema_extended",
            &item.name,
            &item.labels,
            "user",
        )?;
    }
    for item in &extension.relations {
        append_growth(
            tx,
            loupist_id,
            extension.updated_at_ms,
            "schema_extended",
            &item.name,
            &item.labels,
            "user",
        )?;
    }
    Ok(())
}

fn semiconductors() -> WorldExtension {
    WorldExtension {
        version: 0,
        updated_at_ms: 0,
        types: vec![ExtensionType {
            name: "ProcessNode".into(),
            description: "A named semiconductor manufacturing process generation, such as N3, 18A or 1b DRAM. Not the fab that runs it (Place) and not the chip made on it (Product).".into(),
            category: ExtensionCategory::Product,
            labels: labels("Process node", "공정 노드"),
            blurbs: labels(
                "Process generations such as N3 or 18A",
                "N3, 18A 같은 반도체 공정 세대",
            ),
            fields: Vec::new(),
        }],
        relations: vec![
            ExtensionRelation {
                name: "QUALIFIED_BY".into(),
                description: "The target customer has qualified or certified the source product for use. Keep the date and stage in the fact.".into(),
                signatures: vec![("Product".into(), "Organization".into())],
                labels: labels("qualified by", "인증"),
            },
            ExtensionRelation {
                name: "MADE_ON".into(),
                description: "The source product is manufactured on the target process node.".into(),
                signatures: vec![("Product".into(), "ProcessNode".into())],
                labels: labels("made on", "제조 공정"),
            },
        ],
    }
}

fn dividends() -> WorldExtension {
    WorldExtension {
        version: 0,
        updated_at_ms: 0,
        types: vec![ExtensionType {
            name: "CapitalReturnPolicy".into(),
            description: "A company's stated policy for returning cash to shareholders: a payout target, a progressive dividend commitment or a buyback authorization. A single dividend declaration is an Event, and a paid amount is a Metric.".into(),
            category: ExtensionCategory::Event,
            labels: labels("Capital return policy", "주주환원 정책"),
            blurbs: labels(
                "Dividend and buyback policies",
                "배당·자사주 매입 같은 주주환원 정책",
            ),
            fields: vec![ExtensionField {
                name: "payout_target".into(),
                description: "The stated payout ratio or dividend target, as written.".into(),
            }],
        }],
        relations: vec![ExtensionRelation {
            name: "HAS_RETURN_POLICY".into(),
            description: "The organization has adopted or announced the target capital return policy.".into(),
            signatures: vec![("Organization".into(), "CapitalReturnPolicy".into())],
            labels: labels("has return policy", "환원 정책"),
        }],
    }
}

fn kind(
    name: &str,
    description: &str,
    category: ExtensionCategory,
    label: (&str, &str),
    blurb: (&str, &str),
) -> ExtensionType {
    ExtensionType {
        name: name.into(),
        description: description.into(),
        category,
        labels: labels(label.0, label.1),
        blurbs: labels(blurb.0, blurb.1),
        fields: Vec::new(),
    }
}

fn relation(
    name: &str,
    description: &str,
    signatures: &[(&str, &str)],
    label: (&str, &str),
) -> ExtensionRelation {
    ExtensionRelation {
        name: name.into(),
        description: description.into(),
        signatures: signatures
            .iter()
            .map(|(source, target)| ((*source).into(), (*target).into()))
            .collect(),
        labels: labels(label.0, label.1),
    }
}

fn seed(types: Vec<ExtensionType>, relations: Vec<ExtensionRelation>) -> WorldExtension {
    WorldExtension {
        version: 0,
        updated_at_ms: 0,
        types,
        relations,
    }
}

fn macro_rates() -> WorldExtension {
    seed(
        vec![kind(
            "EconomicIndicator",
            "A published economic data series such as CPI, payrolls or GDP. One release of it is an Event, and one value is a Metric.",
            ExtensionCategory::Event,
            ("Economic indicator", "경제 지표"),
            ("Published series such as CPI or payrolls", "소비자물가, 고용 같은 발표 지표"),
        )],
        vec![
            relation("SETS_POLICY_FOR", "The source central bank or authority sets monetary or fiscal policy for the target economy.", &[("Organization", "Place")], ("sets policy for", "정책 결정")),
            relation("PUBLISHES", "The source agency or central bank publishes the target economic indicator.", &[("Organization", "EconomicIndicator")], ("publishes", "발표")),
            relation("MEASURES", "The source indicator measures the target economy or market.", &[("EconomicIndicator", "Place"), ("EconomicIndicator", "Asset")], ("measures", "측정 대상")),
            relation("MOVES_WITH", "The source asset has moved with the target indicator or asset, only as a source states it. Keep the direction and the period in the fact.", &[("Asset", "EconomicIndicator"), ("Asset", "Asset")], ("moves with", "연동")),
        ],
    )
}

fn labels(en: &str, ko: &str) -> ExtensionLabels {
    ExtensionLabels {
        en: en.into(),
        ko: Some(ko.into()),
    }
}

fn load_proposal(
    tx: &Transaction<'_>,
    loupist_id: &str,
    proposal_id: &str,
) -> KnowledgeResult<SchemaProposal> {
    validate_loupist_id(loupist_id)?;
    let row = tx
        .query_row(
            "SELECT proposal_id, kind, status, body_json, created_at_ms, decided_at_ms
             FROM knowledge_schema_proposals WHERE proposal_id = ?1 AND loupist_id = ?2",
            params![proposal_id, loupist_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                ))
            },
        )
        .optional()?
        .ok_or(KnowledgeError::NotFound)?;
    decode_proposal(row.0, row.1, row.2, row.3, row.4, row.5)
}

fn decode_proposal(
    proposal_id: String,
    kind: String,
    status: String,
    body_json: String,
    created_at_ms: i64,
    decided_at_ms: Option<i64>,
) -> KnowledgeResult<SchemaProposal> {
    let body: ProposalBody = serde_json::from_str(&body_json)?;
    Ok(SchemaProposal {
        proposal_id,
        kind: ProposalKind::parse(&kind)?,
        status: ProposalStatus::parse(&status)?,
        types: body.types,
        relations: body.relations,
        created_at_ms,
        decided_at_ms,
    })
}

fn decide(
    tx: &Transaction<'_>,
    loupist_id: &str,
    proposal_id: &str,
    status: ProposalStatus,
    now_ms: i64,
) -> KnowledgeResult<()> {
    tx.execute(
        "UPDATE knowledge_schema_proposals SET status = ?1, decided_at_ms = ?2
         WHERE proposal_id = ?3 AND loupist_id = ?4",
        params![status.as_str(), now_ms, proposal_id, loupist_id],
    )?;
    Ok(())
}

/// A model result is settled only while the proposal is still `preparing`.
/// A valid result stays pending until the user applies it. Pending keeps
/// `decided_at_ms` empty so the user can still apply or dismiss it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn settle_model_proposal(
    tx: &Transaction<'_>,
    loupist_id: &str,
    proposal_id: &str,
    types: Vec<ExtensionType>,
    relations: Vec<ExtensionRelation>,
    outcome: ModelOutcome,
    now_ms: i64,
) -> KnowledgeResult<()> {
    let proposal = load_proposal(tx, loupist_id, proposal_id)?;
    if proposal.status != ProposalStatus::Preparing {
        return Ok(());
    }
    let body = serde_json::to_string(&ProposalBody { types, relations })?;
    let updated = tx.execute(
        "UPDATE knowledge_schema_proposals SET body_json = ?1
         WHERE proposal_id = ?2 AND loupist_id = ?3 AND status = 'preparing'",
        params![body, proposal_id, loupist_id],
    )?;
    if updated != 1 {
        return Ok(());
    }
    match outcome {
        ModelOutcome::Pending => {
            tx.execute(
                "UPDATE knowledge_schema_proposals SET status = 'pending'
                 WHERE proposal_id = ?1 AND loupist_id = ?2 AND status = 'preparing'",
                params![proposal_id, loupist_id],
            )?;
            Ok(())
        }
        ModelOutcome::Dismiss => decide(
            tx,
            loupist_id,
            proposal_id,
            ProposalStatus::Dismissed,
            now_ms,
        ),
        ModelOutcome::Fail => decide(tx, loupist_id, proposal_id, ProposalStatus::Failed, now_ms),
    }
}

pub(crate) enum ModelOutcome {
    Pending,
    Dismiss,
    Fail,
}

pub(crate) fn type_already_known(name: &str, extension: &WorldExtension) -> bool {
    CORE_TYPES.contains(&name)
        || PROTECTED_TYPES.contains(&name)
        || extension.types.iter().any(|item| item.name == name)
}

pub(crate) fn relation_already_known(name: &str, extension: &WorldExtension) -> bool {
    CORE_RELATIONS.contains(&name)
        || name == "RELATES_TO"
        || extension.relations.iter().any(|item| item.name == name)
}

pub(crate) fn listed_relation(name: &str, extension: &WorldExtension) -> bool {
    CORE_RELATIONS.contains(&name) || extension.relations.iter().any(|item| item.name == name)
}

fn append_growth(
    tx: &Transaction<'_>,
    loupist_id: &str,
    at_ms: i64,
    kind: &str,
    name: &str,
    labels: &ExtensionLabels,
    actor: &str,
) -> KnowledgeResult<()> {
    let record_id: Option<String> = tx
        .query_row(
            "SELECT record_id FROM knowledge_records
             WHERE owner_loupist_id = ?1 AND kind = 'world' ORDER BY record_id LIMIT 1",
            [loupist_id],
            |row| row.get(0),
        )
        .optional()?;
    super::growth::GrowthEvent {
        event_id: mint_id("gev"),
        loupist_id: loupist_id.into(),
        at_ms,
        kind: kind.into(),
        record_id,
        subject_id: Some(name.into()),
        origin_chat_id: None,
        origin_message_id: None,
        actor: actor.into(),
        origin: Some(serde_json::json!({ "labels": labels })),
        rationale: None,
        summary: super::growth::one_line(&labels.en),
        before_summary: None,
        after_summary: None,
        title_key: None,
    }
    .insert(tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::{embedded_ddl, store_settings, GraphProcessingSettings};
    use rusqlite::Connection;

    fn field(name: &str) -> ExtensionField {
        ExtensionField {
            name: name.into(),
            description: "The stated value, as written.".into(),
        }
    }

    fn kind(name: &str) -> ExtensionType {
        ExtensionType {
            name: name.into(),
            description: "A Loupist-specific kind kept distinct from the shared core types.".into(),
            category: ExtensionCategory::Product,
            labels: labels("Process node", "공정 노드"),
            blurbs: labels(
                "Process generations such as N3 or 18A",
                "N3, 18A 같은 반도체 공정 세대",
            ),
            fields: vec![field("payout_target")],
        }
    }

    fn relation(name: &str, source: &str, target: &str) -> ExtensionRelation {
        ExtensionRelation {
            name: name.into(),
            description: "The source is connected to the target in the stated way.".into(),
            signatures: vec![(source.into(), target.into())],
            labels: labels("made on", "공정"),
        }
    }

    fn extension(types: Vec<ExtensionType>, relations: Vec<ExtensionRelation>) -> WorldExtension {
        WorldExtension {
            version: 1,
            types,
            relations,
            updated_at_ms: 10,
        }
    }

    fn memory() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(&embedded_ddl()).unwrap();
        db.execute_batch(
            "CREATE TABLE app_settings (
                id TEXT PRIMARY KEY NOT NULL,
                data_json TEXT NOT NULL,
                CHECK (id IN ('model', 'model_visibility', 'update')
                    OR (substr(id, 1, 17) = 'world_processing:' AND length(id) > 17))
            ) STRICT;",
        )
        .unwrap();
        db
    }

    fn save(tx: &Transaction<'_>, loupist: &str, extension: &WorldExtension) {
        let settings = GraphProcessingSettings {
            extension: extension.clone(),
            ..GraphProcessingSettings::default()
        };
        store_settings(tx, loupist, &settings).unwrap();
    }

    #[test]
    fn extension_validation_rejects_reserved_malformed_and_oversized_items() {
        let ok = extension(
            vec![kind("ProcessNode")],
            vec![relation("MADE_ON", "Product", "ProcessNode")],
        );
        validate_extension(&ok).unwrap();
        validate_extension(&WorldExtension::default()).unwrap();
        type Edit = fn(&mut WorldExtension);
        let cases: &[(&str, Edit)] = &[
            ("lowercase type", |item| {
                item.types[0].name = "processNode".into()
            }),
            ("core type", |item| {
                item.types[0].name = "Organization".into()
            }),
            ("graph label", |item| item.types[0].name = "Episodic".into()),
            ("duplicate type", |item| {
                item.types.push(kind("ProcessNode"))
            }),
            ("short description", |item| {
                item.types[0].description = "too short".into()
            }),
            ("control character", |item| {
                item.types[0].description = "A useful description!!\0".into()
            }),
            ("blank label", |item| item.types[0].labels.en = " ".into()),
            ("multiline blurb", |item| {
                item.types[0].blurbs.ko = Some("line\nbreak".into())
            }),
            ("too many fields", |item| {
                item.types[0].fields = ["one", "two", "three", "four"].map(field).to_vec()
            }),
            ("reserved field", |item| {
                item.types[0].fields = vec![field("uuid")]
            }),
            ("too many types", |item| {
                item.types = ["One", "Two", "Three", "Four", "Five"].map(kind).to_vec()
            }),
            ("core relation", |item| {
                item.relations[0].name = "SUPPLIES".into()
            }),
            ("lowercase relation", |item| {
                item.relations[0].name = "made_on".into()
            }),
            ("unknown endpoint", |item| {
                item.relations[0].signatures = vec![("Widget".into(), "Entity".into())]
            }),
            ("no signatures", |item| item.relations[0].signatures.clear()),
            ("too many relations", |item| {
                item.relations = (0..7)
                    .map(|index| relation(&format!("REL_{index}"), "Entity", "Entity"))
                    .collect()
            }),
            ("negative time", |item| item.updated_at_ms = -1),
        ];
        for (name, edit) in cases {
            let mut bad = ok.clone();
            edit(&mut bad);
            assert!(validate_extension(&bad).is_err(), "{name}");
        }
        // Model output cannot smuggle extra fields or unknown categories.
        assert!(serde_json::from_str::<WorldExtension>(
            r#"{"version":0,"types":[],"relations":[],"updatedAtMs":0,"extra":true}"#
        )
        .is_err());
        assert!(serde_json::from_str::<ExtensionType>(
            r#"{"name":"ProcessNode","description":"A Loupist-specific kind kept distinct from the shared core types.","category":"factory","labels":{"en":"Process node"},"fields":[]}"#
        )
        .is_err());
    }

    #[test]
    fn curated_seeds_match_only_the_template_default_focus() {
        for (template, focus) in [
            ("semiconductors", "Chips, memory, foundries and equipment"),
            ("semiconductors", "칩, 메모리, 파운드리, 장비"),
            ("dividends", "배당 기업과 배당 지속성"),
            (
                "macro",
                "Interest rates, inflation, currencies and central banks",
            ),
            ("macro", "금리, 물가, 환율과 중앙은행"),
        ] {
            let seed = curated_seed(Some(template), focus, 5).expect(focus);
            validate_extension(&seed).unwrap();
            assert!(!seed.types.is_empty() && !seed.relations.is_empty());
            // Every class seed is shown in Korean too (W14 limits: ≤ 4 types, ≤ 6 relations).
            assert!(
                seed.types.len() <= 4 && seed.relations.len() <= 6,
                "{template}"
            );
            assert!(
                seed.types
                    .iter()
                    .all(|item| item.labels.ko.is_some() && item.blurbs.ko.is_some()),
                "{template}"
            );
            assert!(
                seed.relations.iter().all(|item| item.labels.ko.is_some()),
                "{template}"
            );
        }
        assert_eq!(
            curated_seed(Some("macro"), MACRO_FOCUS[0], 5)
                .unwrap()
                .types[0]
                .name,
            "EconomicIndicator"
        );
        for (template, focus) in [
            (Some("semiconductors"), "Edited focus"),
            (Some("portfolio"), "Chips, memory, foundries and equipment"),
            (Some("custom"), "칩, 메모리, 파운드리, 장비"),
            (None, "Dividend payers and payout safety"),
            (Some("unknown"), "Dividend payers and payout safety"),
            (Some("crypto"), "Only Bitcoin miners"),
            (
                Some("realAssets"),
                "Property, infrastructure and land: owners, tenants and financing",
            ),
            (
                Some("realAssets"),
                "부동산·인프라·토지의 소유자, 임차인, 자금 조달",
            ),
            (
                Some("crypto"),
                "Crypto networks, tokens and the companies around them",
            ),
            (Some("crypto"), "블록체인 네트워크, 토큰과 관련 기업"),
            (
                Some("commodities"),
                "Energy, metals and farm goods: supply, demand and positioning",
            ),
            (Some("commodities"), "에너지·금속·농산물의 수급과 포지션"),
            (
                Some("macro"),
                "Crypto networks, tokens and the companies around them",
            ),
        ] {
            assert!(curated_seed(template, focus, 5).is_none(), "{focus}");
        }
    }

    #[test]
    fn apply_dismiss_and_remove_cascade_signatures() {
        let mut db = memory();
        let tx = db.transaction().unwrap();
        tx.execute(
            "INSERT INTO knowledge_records VALUES ('world','world','loupist-a','World',1)",
            [],
        )
        .unwrap();
        let mut current = extension(vec![kind("ProcessNode")], vec![]);
        save(&tx, "loupist-a", &current);
        let pending = insert_proposal(
            &tx,
            "loupist-a",
            ProposalKind::Review,
            ProposalStatus::Pending,
            vec![kind("CapacityPlan")],
            vec![
                relation("MADE_ON", "Product", "ProcessNode"),
                relation("EXPANDS", "CapacityPlan", "ProcessNode"),
            ],
            11,
            None,
        )
        .unwrap();
        let preparing = insert_proposal(
            &tx,
            "loupist-b",
            ProposalKind::Review,
            ProposalStatus::Preparing,
            vec![],
            vec![],
            11,
            None,
        )
        .unwrap();
        assert!(apply_proposal(&tx, "loupist-b", &mut current, &preparing, 12).is_err());
        apply_proposal(&tx, "loupist-a", &mut current, &pending, 20).unwrap();
        assert_eq!(current.version, 2);
        assert_eq!(current.types.len(), 2);
        assert_eq!(current.relations.len(), 2);
        let events: Vec<(String, String)> = tx
            .prepare("SELECT kind, subject_id FROM knowledge_growth_events WHERE loupist_id = 'loupist-a' ORDER BY rowid")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            events,
            vec![
                ("schema_extended".into(), "CapacityPlan".into()),
                ("schema_extended".into(), "MADE_ON".into()),
                ("schema_extended".into(), "EXPANDS".into()),
            ]
        );
        assert!(open_proposal(&tx, "loupist-a").unwrap().is_none());
        assert!(apply_proposal(&tx, "loupist-a", &mut current, &pending, 21).is_err());

        dismiss_proposal(&tx, "loupist-b", &preparing, 22).unwrap();
        assert!(open_proposal(&tx, "loupist-b").unwrap().is_none());
        assert!(dismiss_proposal(&tx, "loupist-b", &preparing, 23).is_err());

        let kept = relation("ALSO_USES", "Product", "Organization");
        current.relations.push(kept);
        current.relations[1]
            .signatures
            .push(("Organization".into(), "Entity".into()));
        validate_extension(&current).unwrap();
        remove_schema_item(&tx, "loupist-a", &mut current, "type", "ProcessNode", 30).unwrap();
        assert_eq!(current.version, 3);
        assert!(current.types.iter().all(|item| item.name != "ProcessNode"));
        assert!(current.relations.iter().all(|item| item.name != "MADE_ON"));
        let expands = current
            .relations
            .iter()
            .find(|item| item.name == "EXPANDS")
            .unwrap();
        assert_eq!(
            expands.signatures,
            vec![("Organization".into(), "Entity".into())]
        );
        assert!(current
            .relations
            .iter()
            .any(|item| item.name == "ALSO_USES"));
        let reduced: Vec<String> = tx
            .prepare("SELECT subject_id FROM knowledge_growth_events WHERE kind = 'schema_reduced' ORDER BY rowid")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            reduced,
            vec!["ProcessNode".to_string(), "MADE_ON".to_string()]
        );
        remove_schema_item(&tx, "loupist-a", &mut current, "relation", "ALSO_USES", 40).unwrap();
        assert!(current
            .relations
            .iter()
            .all(|item| item.name != "ALSO_USES"));
        assert_eq!(current.version, 4);
        assert!(remove_schema_item(&tx, "loupist-a", &mut current, "type", "Missing", 41).is_err());
        tx.commit().unwrap();
    }
}
