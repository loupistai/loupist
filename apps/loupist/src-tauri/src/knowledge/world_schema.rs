//! Per-Loupist World extension: extra entity and relation kinds on top of the core schema.
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

use super::error::{KnowledgeError, KnowledgeResult};
use super::model::{mint_id, validate_loupist_id};
use super::world_extract::ontology::{is_core_relation, is_core_type};

const PROTECTED_TYPES: &[&str] = &["Entity", "Episodic", "Community", "Saga"];
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionLabels {
    pub en: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub ko: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionField {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtensionRelation {
    pub name: String,
    pub description: String,
    pub signatures: Vec<(String, String)>,
    pub labels: ExtensionLabels,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorldExtension {
    pub version: u32,
    pub types: Vec<ExtensionType>,
    pub relations: Vec<ExtensionRelation>,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Producer {
    Preset,
    Chat,
    Review,
    Undo,
}

impl Producer {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Preset => "preset",
            Self::Chat => "chat",
            Self::Review => "review",
            Self::Undo => "undo",
        }
    }

    fn parse(value: &str) -> KnowledgeResult<Self> {
        match value {
            "preset" => Ok(Self::Preset),
            "chat" => Ok(Self::Chat),
            "review" => Ok(Self::Review),
            "undo" => Ok(Self::Undo),
            _ => Err(KnowledgeError::Invalid("schema change producer is invalid")),
        }
    }

    fn actor(self) -> &'static str {
        match self {
            Self::Review => "processing",
            _ => "user",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SchemaChange {
    pub change_id: String,
    pub producer: Producer,
    pub before: WorldExtension,
    pub after: WorldExtension,
    pub evidence: serde_json::Value,
    pub origin: Option<(String, String)>,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct KindLabel {
    pub name: String,
    pub labels: ExtensionLabels,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SchemaChangeView {
    pub change_id: String,
    pub producer: String,
    pub summary: String,
    pub added_types: Vec<String>,
    pub added_relations: Vec<String>,
    pub removed_types: Vec<String>,
    pub removed_relations: Vec<String>,
    pub kind_labels: Vec<KindLabel>,
    pub created_at_ms: i64,
    pub undoable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct WorldSchemaView {
    pub extension: WorldExtension,
    pub changes: Vec<SchemaChangeView>,
}

const VIEW_CHANGES: usize = 20;

struct Delta {
    added_types: Vec<ExtensionType>,
    removed_types: Vec<ExtensionType>,
    added_relations: Vec<ExtensionRelation>,
    removed_relations: Vec<ExtensionRelation>,
}

impl Delta {
    fn between(before: &WorldExtension, after: &WorldExtension) -> Self {
        let minus = |left: &WorldExtension, right: &WorldExtension| {
            (
                left.types
                    .iter()
                    .filter(|item| !right.types.iter().any(|other| other.name == item.name))
                    .cloned()
                    .collect::<Vec<_>>(),
                left.relations
                    .iter()
                    .filter(|item| !right.relations.iter().any(|other| other.name == item.name))
                    .cloned()
                    .collect::<Vec<_>>(),
            )
        };
        let (added_types, added_relations) = minus(after, before);
        let (removed_types, removed_relations) = minus(before, after);
        Self {
            added_types,
            removed_types,
            added_relations,
            removed_relations,
        }
    }

    fn is_empty(&self) -> bool {
        self.added_types.is_empty()
            && self.removed_types.is_empty()
            && self.added_relations.is_empty()
            && self.removed_relations.is_empty()
    }

    fn untouched_by(mut self, later: &Delta) -> Self {
        let types: Vec<&str> = later
            .added_types
            .iter()
            .chain(&later.removed_types)
            .map(|item| item.name.as_str())
            .collect();
        let relations: Vec<&str> = later
            .added_relations
            .iter()
            .chain(&later.removed_relations)
            .map(|item| item.name.as_str())
            .collect();
        self.added_types
            .retain(|item| !types.contains(&item.name.as_str()));
        self.removed_types
            .retain(|item| !types.contains(&item.name.as_str()));
        self.added_relations
            .retain(|item| !relations.contains(&item.name.as_str()));
        self.removed_relations
            .retain(|item| !relations.contains(&item.name.as_str()));
        self
    }
}

fn names<T>(items: &[T], name: impl Fn(&T) -> &str) -> Vec<String> {
    items.iter().map(|item| name(item).to_owned()).collect()
}

pub(crate) fn live_extension(
    connection: &rusqlite::Connection,
    loupist_id: &str,
) -> KnowledgeResult<WorldExtension> {
    validate_loupist_id(loupist_id)?;
    let json: Option<String> = connection
        .query_row(
            "SELECT after_json FROM knowledge_schema_changes WHERE loupist_id = ?1
             ORDER BY rowid DESC LIMIT 1",
            [loupist_id],
            |row| row.get(0),
        )
        .optional()?;
    json.map_or_else(
        || Ok(WorldExtension::default()),
        |json| serde_json::from_str(&json).map_err(KnowledgeError::from),
    )
}

fn without_type(extension: &mut WorldExtension, name: &str) {
    extension.types.retain(|item| item.name != name);
    for relation in &mut extension.relations {
        relation
            .signatures
            .retain(|(source, target)| source != name && target != name);
    }
    extension
        .relations
        .retain(|relation| !relation.signatures.is_empty());
}

fn reversed(
    current: &WorldExtension,
    change: &SchemaChange,
    later: &[SchemaChange],
) -> Option<WorldExtension> {
    let delta = later.iter().fold(
        Delta::between(&change.before, &change.after),
        |delta, other| delta.untouched_by(&Delta::between(&other.before, &other.after)),
    );
    let mut next = current.clone();
    for item in &delta.added_relations {
        next.relations.retain(|other| other.name != item.name);
    }
    for item in &delta.added_types {
        without_type(&mut next, &item.name);
    }
    next.types.extend(delta.removed_types);
    for mut item in delta.removed_relations {
        let type_names: Vec<String> = next.types.iter().map(|other| other.name.clone()).collect();
        item.signatures.retain(|(source, target)| {
            endpoint_allowed(source, &type_names) && endpoint_allowed(target, &type_names)
        });
        if !item.signatures.is_empty() {
            next.relations.push(item);
        }
    }
    let unchanged = next.types == current.types && next.relations == current.relations;
    (!unchanged).then_some(next)
}

fn change_and_later(
    connection: &rusqlite::Connection,
    loupist_id: &str,
    change_id: &str,
) -> KnowledgeResult<(SchemaChange, Vec<SchemaChange>)> {
    validate_loupist_id(loupist_id)?;
    let mut changes = read_changes(
        connection,
        "WHERE loupist_id = ?1 AND rowid >= (
             SELECT rowid FROM knowledge_schema_changes WHERE loupist_id = ?1 AND change_id = ?2
         )",
        params![loupist_id, change_id],
    )?;
    let change = changes.pop().ok_or(KnowledgeError::NotFound)?;
    Ok((change, changes))
}

pub(crate) fn recent_changes(
    connection: &rusqlite::Connection,
    loupist_id: &str,
    limit: usize,
) -> KnowledgeResult<Vec<SchemaChange>> {
    validate_loupist_id(loupist_id)?;
    let mut rows = read_changes(connection, "WHERE loupist_id = ?1", params![loupist_id])?;
    rows.truncate(limit);
    Ok(rows)
}

fn read_changes(
    connection: &rusqlite::Connection,
    filter: &str,
    parameters: impl rusqlite::Params,
) -> KnowledgeResult<Vec<SchemaChange>> {
    let mut statement = connection.prepare(&format!(
        "SELECT change_id, producer, before_json, after_json, evidence_json,
                origin_chat_id, origin_message_id, created_at_ms
         FROM knowledge_schema_changes {filter} ORDER BY rowid DESC"
    ))?;
    let rows = statement
        .query_map(parameters, |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|row| {
            Ok(SchemaChange {
                change_id: row.0,
                producer: Producer::parse(&row.1)?,
                before: serde_json::from_str(&row.2)?,
                after: serde_json::from_str(&row.3)?,
                evidence: serde_json::from_str(&row.4)?,
                origin: row.5.zip(row.6),
                created_at_ms: row.7,
            })
        })
        .collect()
}

fn change_view(
    change: &SchemaChange,
    later: &[SchemaChange],
    current: &WorldExtension,
) -> SchemaChangeView {
    let delta = Delta::between(&change.before, &change.after);
    let labelled = |label: &dyn Fn(&str) -> Option<ExtensionLabels>, names: &[String]| {
        names
            .iter()
            .filter_map(|name| {
                label(name).map(|labels| KindLabel {
                    name: name.clone(),
                    labels,
                })
            })
            .collect::<Vec<_>>()
    };
    let added_types = names(&delta.added_types, |item| &item.name);
    let added_relations = names(&delta.added_relations, |item| &item.name);
    let removed_types = names(&delta.removed_types, |item| &item.name);
    let removed_relations = names(&delta.removed_relations, |item| &item.name);
    let find = |name: &str| {
        let types = delta.added_types.iter().chain(&delta.removed_types);
        let relations = delta.added_relations.iter().chain(&delta.removed_relations);
        types
            .map(|item| (&item.name, &item.labels))
            .chain(relations.map(|item| (&item.name, &item.labels)))
            .find(|(other, _)| other.as_str() == name)
            .map(|(_, labels)| labels.clone())
    };
    let mut kind_labels = labelled(&find, &added_types);
    kind_labels.extend(labelled(&find, &added_relations));
    kind_labels.extend(labelled(&find, &removed_types));
    kind_labels.extend(labelled(&find, &removed_relations));
    let list = |kinds: &[KindLabel], names: &[String], other: &[String]| {
        kinds
            .iter()
            .filter(|kind| names.contains(&kind.name) || other.contains(&kind.name))
            .map(|kind| kind.labels.en.clone())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut parts = Vec::new();
    let added = list(&kind_labels, &added_types, &added_relations);
    if !added.is_empty() {
        parts.push(format!("Added {added}"));
    }
    let removed = list(&kind_labels, &removed_types, &removed_relations);
    if !removed.is_empty() {
        parts.push(format!("Removed {removed}"));
    }
    SchemaChangeView {
        change_id: change.change_id.clone(),
        producer: change.producer.as_str().into(),
        summary: parts.join(". "),
        added_types,
        added_relations,
        removed_types,
        removed_relations,
        kind_labels,
        created_at_ms: change.created_at_ms,
        undoable: reversed(current, change, later).is_some(),
    }
}

pub(crate) fn schema_view(
    connection: &rusqlite::Connection,
    loupist_id: &str,
) -> KnowledgeResult<WorldSchemaView> {
    let extension = live_extension(connection, loupist_id)?;
    let recent = recent_changes(connection, loupist_id, VIEW_CHANGES)?;
    let changes = recent
        .iter()
        .enumerate()
        .map(|(index, change)| change_view(change, &recent[..index], &extension))
        .collect();
    Ok(WorldSchemaView { extension, changes })
}

const WORLD_KINDS_MAX_BYTES: usize = 1536;

fn means(description: &str, max: usize) -> String {
    let first = description
        .split(['.', '\n'])
        .next()
        .unwrap_or(description)
        .trim();
    let mut text: String = first.chars().take(max).collect();
    if first.chars().count() > max {
        if let Some(cut) = text.rfind(' ') {
            text.truncate(cut);
        }
    }
    text
}

pub(crate) fn world_kinds_block(extension: &WorldExtension) -> serde_json::Value {
    let core_types = || {
        super::world_extract::ontology::core_entity_types()
            .into_iter()
            .map(|item| (item.name, serde_json::Value::from(means(&item.doc, 36))))
            .collect::<serde_json::Map<_, _>>()
    };
    let relations = super::world_extract::ontology::core_edge_types()
        .into_iter()
        .map(|item| item.name)
        .collect::<Vec<_>>()
        .join(", ");
    let mut width = 48;
    loop {
        let describe = |items: Vec<(&String, &String)>| {
            items
                .into_iter()
                .map(|(name, description)| {
                    (
                        name.clone(),
                        serde_json::Value::from(means(description, width)),
                    )
                })
                .collect::<serde_json::Map<_, _>>()
        };
        let block = serde_json::json!({
            "core": {
                "types": core_types(),
                "relations": relations,
            },
            "added": {
                "types": describe(extension.types.iter().map(|item| (&item.name, &item.description)).collect()),
                "relations": describe(extension.relations.iter().map(|item| (&item.name, &item.description)).collect()),
            },
            "version": extension.version,
            "hint": "To track a kind of thing or link not listed, use world_schema_update; never reuse a listed name.",
        });
        if width == 0 || block.to_string().len() <= WORLD_KINDS_MAX_BYTES {
            return block;
        }
        width = width.saturating_sub(16);
    }
}

pub(crate) fn visible_extension(
    connection: &rusqlite::Connection,
    loupist_id: &str,
    visible: &dyn Fn(i64) -> bool,
) -> KnowledgeResult<WorldExtension> {
    Ok(recent_changes(connection, loupist_id, usize::MAX)?
        .into_iter()
        .find(|change| visible(change.created_at_ms))
        .map(|change| change.after)
        .unwrap_or_default())
}

pub(crate) fn added_labels(change: &SchemaChange) -> Vec<KindLabel> {
    let delta = Delta::between(&change.before, &change.after);
    let types = delta
        .added_types
        .iter()
        .map(|item| (&item.name, &item.labels));
    let relations = delta
        .added_relations
        .iter()
        .map(|item| (&item.name, &item.labels));
    types
        .chain(relations)
        .map(|(name, labels)| KindLabel {
            name: name.clone(),
            labels: labels.clone(),
        })
        .collect()
}

pub(crate) struct Provenance<'a> {
    pub producer: Producer,
    pub evidence: serde_json::Value,
    pub origin: Option<(&'a str, &'a str)>,
}

impl<'a> Provenance<'a> {
    pub(crate) fn new(producer: Producer, evidence: serde_json::Value) -> Self {
        Self {
            producer,
            evidence,
            origin: None,
        }
    }
}

pub(crate) fn record_change(
    tx: &Transaction<'_>,
    loupist_id: &str,
    mut next: WorldExtension,
    provenance: Provenance<'_>,
    now_ms: i64,
) -> KnowledgeResult<SchemaChange> {
    let Provenance {
        producer,
        evidence,
        origin,
    } = provenance;
    let before = live_extension(tx, loupist_id)?;
    let delta = Delta::between(&before, &next);
    if delta.is_empty() {
        return Err(KnowledgeError::Invalid("schema change alters nothing"));
    }
    next.version = before
        .version
        .checked_add(1)
        .ok_or(KnowledgeError::Invalid("schema version overflow"))?;
    next.updated_at_ms = now_ms;
    validate_extension(&next)?;
    let change = SchemaChange {
        change_id: mint_id("wsc"),
        producer,
        before,
        after: next,
        evidence,
        origin: origin.map(|(chat, message)| (chat.to_owned(), message.to_owned())),
        created_at_ms: now_ms,
    };
    tx.execute(
        "INSERT INTO knowledge_schema_changes
         (change_id, loupist_id, before_json, after_json, producer, evidence_json,
          origin_chat_id, origin_message_id, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            change.change_id,
            loupist_id,
            serde_json::to_string(&change.before)?,
            serde_json::to_string(&change.after)?,
            producer.as_str(),
            serde_json::to_string(&change.evidence)?,
            change.origin.as_ref().map(|origin| origin.0.as_str()),
            change.origin.as_ref().map(|origin| origin.1.as_str()),
            now_ms,
        ],
    )?;
    let growth = |kind: &str, name: &str, labels: &ExtensionLabels| {
        append_growth(tx, loupist_id, &change, kind, name, labels)
    };
    for item in &delta.added_types {
        growth("schema_extended", &item.name, &item.labels)?;
    }
    for item in &delta.added_relations {
        growth("schema_extended", &item.name, &item.labels)?;
    }
    for item in &delta.removed_types {
        growth("schema_reduced", &item.name, &item.labels)?;
    }
    for item in &delta.removed_relations {
        growth("schema_reduced", &item.name, &item.labels)?;
    }
    Ok(change)
}

pub(crate) fn add_kinds(
    tx: &Transaction<'_>,
    loupist_id: &str,
    types: Vec<ExtensionType>,
    relations: Vec<ExtensionRelation>,
    provenance: Provenance<'_>,
    now_ms: i64,
) -> KnowledgeResult<SchemaChange> {
    let mut next = live_extension(tx, loupist_id)?;
    if types
        .iter()
        .any(|item| type_already_known(&item.name, &next))
        || relations
            .iter()
            .any(|item| relation_already_known(&item.name, &next))
    {
        return Err(KnowledgeError::KindAlreadyKnown);
    }
    next.types.extend(types);
    next.relations.extend(relations);
    record_change(tx, loupist_id, next, provenance, now_ms)
}

pub(crate) fn undo_change(
    tx: &Transaction<'_>,
    loupist_id: &str,
    change_id: &str,
    now_ms: i64,
) -> KnowledgeResult<SchemaChange> {
    let (change, later) = change_and_later(tx, loupist_id, change_id)?;
    let current = live_extension(tx, loupist_id)?;
    let next = reversed(&current, &change, &later).ok_or(KnowledgeError::Invalid(
        "schema change has nothing left to undo",
    ))?;
    record_change(
        tx,
        loupist_id,
        next,
        Provenance::new(Producer::Undo, serde_json::json!({ "undoes": change_id })),
        now_ms,
    )
}

pub(crate) fn remove_schema_item(
    tx: &Transaction<'_>,
    loupist_id: &str,
    kind: &str,
    name: &str,
    now_ms: i64,
) -> KnowledgeResult<SchemaChange> {
    let mut next = live_extension(tx, loupist_id)?;
    match kind {
        "type" => {
            if !next.types.iter().any(|item| item.name == name) {
                return Err(KnowledgeError::NotFound);
            }
            without_type(&mut next, name);
        }
        "relation" => {
            if !next.relations.iter().any(|item| item.name == name) {
                return Err(KnowledgeError::NotFound);
            }
            next.relations.retain(|item| item.name != name);
        }
        _ => return Err(KnowledgeError::Invalid("schema remove kind is invalid")),
    }
    record_change(
        tx,
        loupist_id,
        next,
        Provenance::new(Producer::Undo, serde_json::json!({ "removed": name })),
        now_ms,
    )
}

pub fn curated_seed(template: Option<&str>, now_ms: i64) -> Option<WorldExtension> {
    let mut extension = match template? {
        "semiconductors" => semiconductors(),
        "dividends" => dividends(),
        "macro" => macro_rates(),
        _ => return None,
    };
    extension.version = 1;
    extension.updated_at_ms = now_ms;
    Some(extension)
}

pub(crate) fn missing_preset_kinds(
    tx: &Transaction<'_>,
    loupist_id: &str,
    template: &str,
) -> KnowledgeResult<(Vec<ExtensionType>, Vec<ExtensionRelation>)> {
    let Some(seed) = curated_seed(Some(template), 0) else {
        return Ok((Vec::new(), Vec::new()));
    };
    let live = live_extension(tx, loupist_id)?;
    Ok((
        seed.types
            .into_iter()
            .filter(|item| !type_already_known(&item.name, &live))
            .collect(),
        seed.relations
            .into_iter()
            .filter(|item| !relation_already_known(&item.name, &live))
            .collect(),
    ))
}

/// Adds the template's missing curated kinds as one preset change; `None`
/// when the template has no curated kinds or the map tracks them all.
pub(crate) fn apply_preset(
    tx: &Transaction<'_>,
    loupist_id: &str,
    template: &str,
    origin: Option<(&str, &str)>,
    now_ms: i64,
) -> KnowledgeResult<Option<SchemaChange>> {
    let (types, relations) = missing_preset_kinds(tx, loupist_id, template)?;
    if types.is_empty() && relations.is_empty() {
        return Ok(None);
    }
    let mut provenance = Provenance::new(
        Producer::Preset,
        serde_json::json!({ "template": template }),
    );
    provenance.origin = origin;
    add_kinds(tx, loupist_id, types, relations, provenance, now_ms).map(Some)
}

pub(super) const MAX_EXTENSION_TYPES: usize = 4;
pub(super) const MAX_EXTENSION_RELATIONS: usize = 6;

pub fn validate_extension(extension: &WorldExtension) -> KnowledgeResult<()> {
    if extension.updated_at_ms < 0 {
        return Err(KnowledgeError::Invalid("extension version time is invalid"));
    }
    if extension.types.len() > MAX_EXTENSION_TYPES {
        return Err(KnowledgeError::Invalid("extension has too many types"));
    }
    if extension.relations.len() > MAX_EXTENSION_RELATIONS {
        return Err(KnowledgeError::Invalid("extension has too many relations"));
    }
    let mut type_names = Vec::with_capacity(extension.types.len());
    for item in &extension.types {
        if !is_type_name(&item.name) {
            return Err(KnowledgeError::Invalid("extension type name is invalid"));
        }
        if is_core_type(&item.name) || PROTECTED_TYPES.contains(&item.name.as_str()) {
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
        if is_core_relation(&relation.name) || relation.name == "RELATES_TO" {
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
    name == "Entity" || is_core_type(name) || extension_types.iter().any(|item| item == name)
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
            description: "A company's stated policy for returning cash to shareholders: a payout target, a progressive dividend commitment or a buyback authorization. A single dividend declaration is an Event, and a paid amount is a claim on the company.".into(),
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
            "A published economic data series such as CPI, payrolls or GDP. One release of it is an Event, and one value is a claim on the series.",
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

pub(crate) fn type_already_known(name: &str, extension: &WorldExtension) -> bool {
    is_core_type(name)
        || PROTECTED_TYPES.contains(&name)
        || extension.types.iter().any(|item| item.name == name)
}

pub(crate) fn relation_already_known(name: &str, extension: &WorldExtension) -> bool {
    is_core_relation(name)
        || name == "RELATES_TO"
        || extension.relations.iter().any(|item| item.name == name)
}

pub(crate) fn listed_relation(name: &str, extension: &WorldExtension) -> bool {
    is_core_relation(name) || extension.relations.iter().any(|item| item.name == name)
}

fn append_growth(
    tx: &Transaction<'_>,
    loupist_id: &str,
    change: &SchemaChange,
    kind: &str,
    name: &str,
    labels: &ExtensionLabels,
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
        at_ms: change.created_at_ms,
        kind: kind.into(),
        record_id,
        subject_id: Some(name.into()),
        origin_chat_id: change.origin.as_ref().map(|origin| origin.0.clone()),
        origin_message_id: change.origin.as_ref().map(|origin| origin.1.clone()),
        actor: change.producer.actor().into(),
        origin: Some(serde_json::json!({
            "labels": labels,
            "producer": change.producer.as_str(),
            "changeId": change.change_id,
        })),
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
    use crate::knowledge::embedded_ddl;
    use rusqlite::Connection;

    fn kind(name: &str) -> ExtensionType {
        ExtensionType {
            name: name.into(),
            description: "A Loupist-specific kind kept distinct from the shared core types.".into(),
            category: ExtensionCategory::Product,
            labels: labels(name, name),
            blurbs: labels("A kind of thing", "사물의 한 종류"),
            fields: Vec::new(),
        }
    }

    fn link(name: &str, source: &str, target: &str) -> ExtensionRelation {
        ExtensionRelation {
            name: name.into(),
            description: "The source is connected to the target in the stated way.".into(),
            signatures: vec![(source.into(), target.into())],
            labels: labels(name, name),
        }
    }

    fn memory() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(&embedded_ddl()).unwrap();
        db
    }

    fn add(
        tx: &Transaction<'_>,
        producer: Producer,
        types: Vec<ExtensionType>,
        relations: Vec<ExtensionRelation>,
        at: i64,
    ) -> SchemaChange {
        add_kinds(
            tx,
            "loupist-a",
            types,
            relations,
            Provenance::new(producer, serde_json::json!({})),
            at,
        )
        .unwrap()
    }

    fn type_names(connection: &Connection) -> Vec<String> {
        live_extension(connection, "loupist-a")
            .unwrap()
            .types
            .into_iter()
            .map(|item| item.name)
            .collect()
    }

    #[test]
    fn extension_validation_rejects_reserved_malformed_and_oversized_items() {
        let ok = WorldExtension {
            version: 1,
            types: vec![kind("ProcessNode")],
            relations: vec![link("MADE_ON", "Product", "ProcessNode")],
            updated_at_ms: 10,
        };
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
            ("new core type", |item| {
                item.types[0].name = "Technology".into()
            }),
            ("graph label", |item| item.types[0].name = "Episodic".into()),
            ("duplicate type", |item| {
                item.types.push(kind("ProcessNode"))
            }),
            ("short description", |item| {
                item.types[0].description = "too short".into()
            }),
            ("blank label", |item| item.types[0].labels.en = " ".into()),
            ("too many types", |item| {
                item.types = ["One", "Two", "Three", "Four", "Five"].map(kind).to_vec()
            }),
            ("core relation", |item| {
                item.relations[0].name = "SUPPLIES".into()
            }),
            ("new core relation", |item| {
                item.relations[0].name = "PART_OF".into()
            }),
            ("lowercase relation", |item| {
                item.relations[0].name = "made_on".into()
            }),
            ("unknown endpoint", |item| {
                item.relations[0].signatures = vec![("Widget".into(), "Entity".into())]
            }),
            ("no signatures", |item| item.relations[0].signatures.clear()),
            ("negative time", |item| item.updated_at_ms = -1),
        ];
        for (name, edit) in cases {
            let mut bad = ok.clone();
            edit(&mut bad);
            assert!(validate_extension(&bad).is_err(), "{name}");
        }
    }

    #[test]
    fn core_names_come_from_the_ontology() {
        for name in ["Organization", "Technology", "Segment"] {
            assert!(
                type_already_known(name, &WorldExtension::default()),
                "{name}"
            );
        }
        for name in [
            "SUPPLIES",
            "PART_OF",
            "REQUIRES",
            "OPERATES_IN",
            "ISSUES",
            "OWNS",
        ] {
            assert!(
                relation_already_known(name, &WorldExtension::default()),
                "{name}"
            );
        }
    }

    #[test]
    fn a_template_gets_its_curated_kinds_once_whatever_the_focus() {
        for template in ["semiconductors", "dividends", "macro"] {
            let seed = curated_seed(Some(template), 5).expect(template);
            validate_extension(&seed).unwrap();
            assert!(
                seed.types.len() <= 4 && seed.relations.len() <= 6,
                "{template}"
            );
            assert!(
                seed.types.iter().all(|item| item.labels.ko.is_some()),
                "{template}"
            );
            let mut db = memory();
            let tx = db.transaction().unwrap();
            assert!(apply_preset(&tx, "loupist-a", template, None, 7)
                .unwrap()
                .is_some());
            assert!(apply_preset(&tx, "loupist-a", template, None, 8)
                .unwrap()
                .is_none());
            let rows: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM knowledge_schema_changes WHERE producer = 'preset'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(rows, 1, "{template}");
        }
        for template in [None, Some("portfolio"), Some("crypto"), Some("custom")] {
            assert!(curated_seed(template, 5).is_none(), "{template:?}");
        }
    }

    #[test]
    fn undo_removes_only_its_own_kinds_even_after_a_newer_change() {
        let mut db = memory();
        let tx = db.transaction().unwrap();
        let first = add(
            &tx,
            Producer::Chat,
            vec![kind("Alpha")],
            vec![link("USES_ALPHA", "Organization", "Alpha")],
            10,
        );
        let second = add(&tx, Producer::Review, vec![kind("Beta")], Vec::new(), 20);
        let view = schema_view(&tx, "loupist-a").unwrap();
        assert_eq!(view.changes.len(), 2);
        assert!(view.changes.iter().all(|change| change.undoable));
        assert_eq!(view.changes[0].added_types, ["Beta"]);
        undo_change(&tx, "loupist-a", &first.change_id, 30).unwrap();
        let live = live_extension(&tx, "loupist-a").unwrap();
        assert_eq!(type_names(&tx), ["Beta"]);
        assert!(
            live.relations.is_empty(),
            "the link that needed Alpha goes with it"
        );
        assert_eq!(live.version, 3);
        let view = schema_view(&tx, "loupist-a").unwrap();
        assert_eq!(view.changes[0].producer, "undo");
        assert_eq!(view.changes[0].removed_types, ["Alpha"]);
        assert!(
            !view
                .changes
                .iter()
                .find(|change| change.change_id == first.change_id)
                .unwrap()
                .undoable
        );
        let reduced: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM knowledge_growth_events WHERE kind = 'schema_reduced'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(reduced, 2);
        assert!(undo_change(&tx, "loupist-a", &first.change_id, 40).is_err());
        undo_change(&tx, "loupist-a", &second.change_id, 50).unwrap();
        assert!(type_names(&tx).is_empty());
    }

    #[test]
    fn undo_leaves_every_kind_a_later_change_added_or_removed() {
        let mut db = memory();
        let tx = db.transaction().unwrap();
        let first = add(
            &tx,
            Producer::Chat,
            vec![kind("Alpha"), kind("Gamma")],
            Vec::new(),
            10,
        );
        let removed = remove_schema_item(&tx, "loupist-a", "type", "Alpha", 20).unwrap();
        let mut again = kind("Alpha");
        again.description = "The same name added again later with another meaning.".into();
        let third = add(&tx, Producer::Chat, vec![again.clone()], Vec::new(), 30);
        let undoable = |change_id: &str| {
            schema_view(&tx, "loupist-a")
                .unwrap()
                .changes
                .into_iter()
                .find(|change| change.change_id == change_id)
                .unwrap()
                .undoable
        };
        assert!(undoable(&first.change_id), "Gamma is still its own");
        assert!(
            !undoable(&removed.change_id),
            "Alpha came back in a later change"
        );
        assert!(undoable(&third.change_id));
        undo_change(&tx, "loupist-a", &first.change_id, 40).unwrap();
        assert_eq!(
            live_extension(&tx, "loupist-a").unwrap().types,
            [again.clone()],
            "only Gamma goes; the Alpha added later stays"
        );
        assert!(!undoable(&first.change_id));
        assert!(undo_change(&tx, "loupist-a", &first.change_id, 50).is_err());
        assert_eq!(live_extension(&tx, "loupist-a").unwrap().types, [again]);
    }

    #[test]
    fn undoing_the_latest_change_restores_the_previous_extension_and_undo_can_be_undone() {
        let mut db = memory();
        let tx = db.transaction().unwrap();
        add(&tx, Producer::Chat, vec![kind("Alpha")], Vec::new(), 10);
        let before = live_extension(&tx, "loupist-a").unwrap();
        let second = add(&tx, Producer::Chat, vec![kind("Beta")], Vec::new(), 20);
        let undone = undo_change(&tx, "loupist-a", &second.change_id, 30).unwrap();
        let live = live_extension(&tx, "loupist-a").unwrap();
        assert_eq!(
            (&live.types, &live.relations),
            (&before.types, &before.relations)
        );
        undo_change(&tx, "loupist-a", &undone.change_id, 40).unwrap();
        assert_eq!(type_names(&tx), ["Alpha", "Beta"]);
    }

    #[test]
    fn a_name_the_world_already_has_is_refused() {
        let mut db = memory();
        let tx = db.transaction().unwrap();
        add(&tx, Producer::Chat, vec![kind("Alpha")], Vec::new(), 10);
        for types in [vec![kind("Alpha")], vec![kind("Technology")]] {
            assert!(add_kinds(
                &tx,
                "loupist-a",
                types,
                Vec::new(),
                Provenance::new(Producer::Chat, serde_json::json!({})),
                20
            )
            .is_err());
        }
        assert_eq!(schema_view(&tx, "loupist-a").unwrap().changes.len(), 1);
    }

    #[test]
    fn removing_a_type_removes_the_links_that_end_at_it() {
        let mut db = memory();
        let tx = db.transaction().unwrap();
        add(
            &tx,
            Producer::Chat,
            vec![kind("Alpha")],
            vec![link("USES_ALPHA", "Organization", "Alpha")],
            10,
        );
        remove_schema_item(&tx, "loupist-a", "type", "Alpha", 20).unwrap();
        let live = live_extension(&tx, "loupist-a").unwrap();
        assert!(live.types.is_empty() && live.relations.is_empty());
        assert!(remove_schema_item(&tx, "loupist-a", "type", "Alpha", 30).is_err());
    }

    #[test]
    fn world_kinds_lists_core_and_added_names_within_the_bound() {
        let mut db = memory();
        let tx = db.transaction().unwrap();
        let empty = world_kinds_block(&WorldExtension::default());
        assert!(empty["core"]["types"]["Technology"].is_string());
        assert!(empty["core"]["relations"]
            .as_str()
            .unwrap()
            .contains("PART_OF"));
        let types: Vec<_> = ["Alpha", "Beta", "Gamma", "Delta"].map(kind).to_vec();
        let relations: Vec<_> = (0..6)
            .map(|index| link(&format!("LINK_{index}"), "Entity", "Entity"))
            .collect();
        add(&tx, Producer::Chat, types, relations, 10);
        let full = world_kinds_block(&live_extension(&tx, "loupist-a").unwrap());
        assert!(
            full.to_string().len() <= WORLD_KINDS_MAX_BYTES,
            "{}",
            full.to_string().len()
        );
        assert_eq!(full["added"]["types"].as_object().unwrap().len(), 4);
        assert_eq!(full["version"], 1);
        let before = visible_extension(&tx, "loupist-a", &|at| at < 10).unwrap();
        assert!(before.types.is_empty());
        let after = visible_extension(&tx, "loupist-a", &|at| at <= 10).unwrap();
        assert_eq!(after.types.len(), 4);
    }

    #[test]
    fn the_live_extension_in_settings_is_the_latest_change() {
        let mut db = memory();
        db.execute_batch(
            "CREATE TABLE app_settings (id TEXT PRIMARY KEY NOT NULL, data_json TEXT NOT NULL) STRICT;",
        )
        .unwrap();
        let tx = db.transaction().unwrap();
        add(&tx, Producer::Chat, vec![kind("Alpha")], Vec::new(), 10);
        let settings = crate::knowledge::load_settings(&tx, "loupist-a").unwrap();
        assert_eq!(settings.extension.types[0].name, "Alpha");
        crate::knowledge::store_settings(&tx, "loupist-a", &settings).unwrap();
        let stored: String = tx
            .query_row("SELECT data_json FROM app_settings", [], |row| row.get(0))
            .unwrap();
        assert!(
            !stored.contains("Alpha"),
            "the settings row does not store the vocabulary"
        );
    }
}
