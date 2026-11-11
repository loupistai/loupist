//! Updates to shared Loupists already added (W19.5). A check stages the
//! author's newer version through the same validation and scans as a first
//! import and compares it with the version added: the retained copy under
//! `.loupist/packs/<import>/<version>/`, or what the import recorded when that
//! copy is gone. Nothing changes until the person applies a selection: new
//! items arrive as suggestions, a changed item they pending stays a suggestion
//! with the newer text, a changed accepted item takes the author's text only
//! when chosen (one with a scan finding waits in the suggestions to be
//! accepted on its own), and the map gains the new relationships while the
//! ones the author withdrew are marked withdrawn. Packs never grant anything.
use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Transaction;
use serde::Serialize;

use super::import::{self, ItemView, Staged, StoredItem};
use super::pack::{self, Files};
use super::scan::Flag;
use super::store::{self, ImportMode, ImportRow, ItemRow};
use crate::app::CommandError;
use crate::knowledge::shared_pack::{self, ImportRefs, WorldImport};
use crate::knowledge::{BlobStore, KnowledgeError};

fn internal(error: impl std::fmt::Display) -> CommandError {
    CommandError::internal(error.to_string())
}

fn knowledge_error(error: KnowledgeError) -> CommandError {
    match error {
        KnowledgeError::Invalid("world processing is busy") => CommandError::new(
            "world_busy",
            "The map is being updated. Try again in a minute.",
        ),
        other => CommandError::invalid(other.to_string()),
    }
}

/// What the version already added held: each item's hash and the map's keys.
#[derive(Debug, Clone, Default)]
pub(crate) struct Base {
    pub items: BTreeMap<String, String>,
    pub entities: BTreeSet<String>,
    pub claims: BTreeSet<String>,
    /// The import brought map content (its Map part was on at Add).
    pub map_added: bool,
}

/// The base from the retained copy of the version added, if it still validates.
pub(crate) fn base_from_files(files: &Files) -> Option<Base> {
    let parsed = pack::validate(files).ok()?;
    Some(Base {
        items: import::items_of(&parsed)
            .iter()
            .map(|(key, _, item)| (key.clone(), import::body_of(item).1))
            .collect(),
        entities: parsed.entities.iter().map(|e| e.key.clone()).collect(),
        claims: parsed.claims.iter().map(|c| c.key.clone()).collect(),
        map_added: false,
    })
}

/// Without the retained copy: the items and map subjects the import recorded.
pub(crate) fn base_from_store(items: &[ItemRow], refs: &ImportRefs, import_id: &str) -> Base {
    let prefix = shared_pack::shared_id(import_id, "");
    let keys = |ids: &BTreeSet<String>| -> BTreeSet<String> {
        ids.iter()
            .filter_map(|id| id.strip_prefix(&prefix).map(str::to_owned))
            .collect()
    };
    Base {
        items: items
            .iter()
            .map(|item| (item.key.clone(), item.body_hash.clone()))
            .collect(),
        entities: keys(&refs.nodes),
        claims: keys(&refs.claims),
        map_added: false,
    }
}

pub(crate) fn map_added(refs: &ImportRefs) -> bool {
    !refs.nodes.is_empty() || !refs.claims.is_empty() || !refs.spans.is_empty()
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct NewItem {
    pub key: String,
    pub kind: &'static str,
    pub title: String,
    pub flags: Vec<Flag>,
    pub item: ItemView,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChangedItem {
    pub key: String,
    pub kind: &'static str,
    pub title: String,
    /// `pending` (still a suggestion) or `accepted`.
    pub state: String,
    /// Accepted, then changed here: theirs is shown beside yours.
    pub edited: bool,
    pub flags: Vec<Flag>,
    /// The text as it stands here (yours).
    pub then: String,
    /// The author's newer text, validated and scanned.
    pub now: ItemView,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemovedItem {
    pub key: String,
    pub kind: String,
    pub title: String,
    /// The accepted record it became: kept here, and the person may archive it.
    pub lens_record_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct MapChanges {
    pub names: u32,
    pub links: u32,
    pub withdrawn: u32,
    /// The import's map is already here (otherwise applying adds the whole map).
    pub added: bool,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct PackUpdate {
    pub staging_id: String,
    pub import_id: String,
    pub from_version: u32,
    pub to_version: u32,
    pub created_at: String,
    pub new_items: Vec<NewItem>,
    pub changed: Vec<ChangedItem>,
    pub removed: Vec<RemovedItem>,
    pub map: Option<MapChanges>,
    pub blocked: Option<&'static str>,
}

struct Changed {
    current: ItemRow,
    kind: &'static str,
    item: StoredItem,
    edited: bool,
    then: String,
}

#[derive(Default)]
struct MapPlan {
    entities: BTreeSet<String>,
    claims: BTreeSet<String>,
    /// World claim ids of this import's own relationships the author withdrew.
    withdraw: BTreeSet<String>,
    withdrawn_keys: usize,
    full: bool,
}

impl MapPlan {
    fn is_empty(&self) -> bool {
        self.claims.is_empty() && self.withdraw.is_empty()
    }
}

struct Plan {
    new: Vec<(String, &'static str, StoredItem)>,
    changed: Vec<Changed>,
    removed_pending: Vec<String>,
    removed_accepted: Vec<ItemRow>,
    map: MapPlan,
}

fn flags(item: &StoredItem) -> Vec<Flag> {
    let mut flags: Vec<Flag> = item.findings.iter().map(|finding| finding.flag).collect();
    flags.sort();
    flags.dedup();
    flags
}

/// The stored `flags` column for an item: its scan findings' classes.
pub(crate) fn flags_json(item: &StoredItem) -> String {
    serde_json::to_string(&flags(item)).unwrap_or_else(|_| "[]".into())
}

fn same_text(left: Option<&str>, right: Option<&str>) -> bool {
    left.unwrap_or("").trim() == right.unwrap_or("").trim()
}

/// An accepted item as it stands here: its Markdown and whether the person
/// changed it after accepting. None once it was archived or deleted here.
fn standing(
    tx: &Transaction<'_>,
    loupist_id: &str,
    current: &ItemRow,
) -> Result<Option<(String, bool)>, CommandError> {
    let Some(record) = current.lens_record_id.as_deref() else {
        return Ok(None);
    };
    let accepted: StoredItem = serde_json::from_str(&current.body_json).map_err(internal)?;
    if current.kind == "principle" {
        let Some(principle) =
            shared_pack::shared_principle_text(tx, loupist_id, record).map_err(knowledge_error)?
        else {
            return Ok(None);
        };
        let edited = !same_text(
            Some(&principle.statement),
            accepted.statement.as_deref().or(Some(&accepted.title)),
        ) || !same_text(principle.why.as_deref(), accepted.why.as_deref())
            || !same_text(principle.example.as_deref(), accepted.example.as_deref());
        return Ok(Some((
            import::principle_markdown(
                &principle.statement,
                principle.why.as_deref(),
                principle.example.as_deref(),
            ),
            edited,
        )));
    }
    let Some(document) =
        shared_pack::shared_lens_document(tx, loupist_id, record).map_err(knowledge_error)?
    else {
        return Ok(None);
    };
    let edited = accepted.document.as_ref() != Some(&document);
    Ok(Some((import::method_markdown(&document), edited)))
}

fn plan(
    tx: &Transaction<'_>,
    loupist_id: &str,
    row: &ImportRow,
    base: &Base,
    staged: &Staged,
) -> Result<Plan, CommandError> {
    let current: BTreeMap<String, ItemRow> = store::items(tx, loupist_id, &row.import_id)
        .map_err(internal)?
        .into_iter()
        .map(|item| (item.key.clone(), item))
        .collect();
    let mut plan = Plan {
        new: Vec::new(),
        changed: Vec::new(),
        removed_pending: Vec::new(),
        removed_accepted: Vec::new(),
        map: MapPlan::default(),
    };
    let mut offered = BTreeSet::new();
    for (key, kind, item) in &staged.items {
        offered.insert(key.clone());
        if *kind == "perspective" && row.mode == ImportMode::Merge {
            continue;
        }
        let hash = import::body_of(item).1;
        match base.items.get(key) {
            None if !current.contains_key(key) => {
                plan.new.push((key.clone(), kind, item.clone()));
            }
            Some(before) if *before != hash => {
                let Some(here) = current.get(key) else {
                    continue;
                };
                let waiting = here
                    .update_json
                    .as_deref()
                    .is_some_and(|json| pack::sha256_hex(json.as_bytes()) == hash);
                if here.body_hash == hash || waiting {
                    continue;
                }
                let (then, edited) = match here.state.as_str() {
                    "pending" => {
                        let stored: StoredItem =
                            serde_json::from_str(&here.body_json).map_err(internal)?;
                        (stored.markdown, false)
                    }
                    "accepted" => match standing(tx, loupist_id, here)? {
                        Some(standing) => standing,
                        None => continue,
                    },
                    _ => continue,
                };
                plan.changed.push(Changed {
                    current: here.clone(),
                    kind,
                    item: item.clone(),
                    edited,
                    then,
                });
            }
            _ => {}
        }
    }
    for key in base.items.keys().filter(|key| !offered.contains(*key)) {
        let Some(here) = current.get(key) else {
            continue;
        };
        match here.state.as_str() {
            "pending" => plan.removed_pending.push(key.clone()),
            "accepted" if standing(tx, loupist_id, here)?.is_some() => {
                plan.removed_accepted.push(here.clone());
            }
            _ => {}
        }
    }
    let parsed = &staged.parsed;
    let claims: BTreeSet<String> = parsed.claims.iter().map(|c| c.key.clone()).collect();
    let entities: BTreeSet<String> = parsed.entities.iter().map(|e| e.key.clone()).collect();
    if base.map_added {
        let refs = store::world_refs(tx, loupist_id, &row.import_id).map_err(internal)?;
        let withdrawn: Vec<&String> = base.claims.difference(&claims).collect();
        plan.map = MapPlan {
            entities: entities.difference(&base.entities).cloned().collect(),
            claims: claims.difference(&base.claims).cloned().collect(),
            withdraw: withdrawn
                .iter()
                .map(|key| shared_pack::shared_id(&row.import_id, key))
                .filter(|id| refs.claims.contains(id))
                .collect(),
            withdrawn_keys: withdrawn.len(),
            full: false,
        };
    } else {
        plan.map = MapPlan {
            entities,
            claims,
            withdraw: BTreeSet::new(),
            withdrawn_keys: 0,
            full: true,
        };
    }
    Ok(plan)
}

/// The update diff shown under the pack's row.
pub(crate) fn diff(
    tx: &Transaction<'_>,
    loupist_id: &str,
    row: &ImportRow,
    base: &Base,
    staged: &Staged,
    staging_id: &str,
) -> Result<PackUpdate, CommandError> {
    let plan = plan(tx, loupist_id, row, base, staged)?;
    let manifest = &staged.parsed.manifest;
    Ok(PackUpdate {
        staging_id: staging_id.to_owned(),
        import_id: row.import_id.clone(),
        from_version: row.version,
        to_version: manifest.version,
        created_at: manifest.created_at.clone(),
        new_items: plan
            .new
            .iter()
            .map(|(key, kind, item)| NewItem {
                key: key.clone(),
                kind,
                title: item.title.clone(),
                flags: flags(item),
                item: import::item_view(key, kind, item),
            })
            .collect(),
        changed: plan
            .changed
            .iter()
            .map(|changed| ChangedItem {
                key: changed.current.key.clone(),
                kind: changed.kind,
                title: changed.item.title.clone(),
                state: changed.current.state.clone(),
                edited: changed.edited,
                flags: flags(&changed.item),
                then: changed.then.clone(),
                now: import::item_view(&changed.current.key, changed.kind, &changed.item),
            })
            .collect(),
        removed: plan
            .removed_accepted
            .iter()
            .map(|item| RemovedItem {
                key: item.key.clone(),
                kind: item.kind.clone(),
                title: item.title.clone(),
                lens_record_id: item.lens_record_id.clone().unwrap_or_default(),
            })
            .collect(),
        map: (!plan.map.is_empty()).then(|| MapChanges {
            names: plan.map.entities.len() as u32,
            links: plan.map.claims.len() as u32,
            withdrawn: plan.map.withdraw.len().min(plan.map.withdrawn_keys) as u32,
            added: !plan.map.full,
        }),
        blocked: staged.blocked,
    })
}

/// The import as it stands after an applied update.
pub(crate) fn advanced(row: &ImportRow, staged: &Staged) -> ImportRow {
    let manifest = &staged.parsed.manifest;
    let mut origin = store::manifest_origins(manifest);
    origin.extend(row.origin.clone());
    origin.push(pack::OriginEntry {
        id: row.pack_id.clone(),
        name: row.name.clone(),
        author: row.author.clone(),
        license: Some(row.license),
        version: row.version,
        url: row.source_url.clone(),
        commit: row.commit.clone(),
    });
    store::dedup_origins(&mut origin);
    ImportRow {
        name: manifest.name.clone(),
        author: manifest
            .author
            .as_ref()
            .map(|author| author.display.clone()),
        version: manifest.version,
        license: if row.license.redistributable() {
            manifest.license
        } else {
            row.license
        },
        commit: staged.fetched.commit.clone().or_else(|| row.commit.clone()),
        origin,
        ..row.clone()
    }
}

fn stored_row(row: &ImportRow, key: &str, kind: &str, item: &StoredItem) -> ItemRow {
    let (body_json, body_hash) = import::body_of(item);
    ItemRow {
        import_id: row.import_id.clone(),
        key: key.to_owned(),
        kind: kind.to_owned(),
        title: item.title.clone(),
        body_json,
        body_hash,
        flags: flags_json(item),
        state: "pending".into(),
        lens_record_id: None,
        update_json: None,
    }
}

/// Puts the author's newer text into an accepted Lens item.
pub(crate) fn take_theirs(
    tx: &Transaction<'_>,
    loupist_id: &str,
    row: &ImportRow,
    current: &ItemRow,
    item: &StoredItem,
    at_ms: i64,
) -> Result<bool, CommandError> {
    let record = current
        .lens_record_id
        .as_deref()
        .ok_or_else(|| CommandError::not_found("accepted item"))?;
    if current.kind == "principle" {
        return shared_pack::revise_shared_principle(
            tx,
            loupist_id,
            record,
            item.statement.as_deref().unwrap_or(&item.title),
            item.why.as_deref(),
            item.example.as_deref(),
            at_ms,
        )
        .map_err(knowledge_error);
    }
    let document = item
        .document
        .as_ref()
        .ok_or_else(|| CommandError::internal("method body"))?;
    shared_pack::revise_shared_lens(
        tx,
        loupist_id,
        record,
        document,
        import::lens_origin(row),
        at_ms,
    )
    .map_err(knowledge_error)?;
    Ok(true)
}

/// Applies the chosen part of an update in the caller's transaction, then
/// records the import at the newer version. Keys must come from this diff.
#[allow(clippy::too_many_arguments)]
pub(crate) fn apply(
    tx: &Transaction<'_>,
    blobs: &BlobStore,
    loupist_id: &str,
    row: &ImportRow,
    base: &Base,
    staged: &Staged,
    keys: &BTreeSet<String>,
    apply_map: bool,
    at_ms: i64,
) -> Result<(), CommandError> {
    let plan = plan(tx, loupist_id, row, base, staged)?;
    let offered: BTreeSet<&String> = plan
        .new
        .iter()
        .map(|(key, _, _)| key)
        .chain(plan.changed.iter().map(|changed| &changed.current.key))
        .collect();
    if keys.iter().any(|key| !offered.contains(key)) {
        return Err(CommandError::new(
            "update_changed",
            "This update changed. Check for updates again.",
        ));
    }
    let next = advanced(row, staged);
    for (key, kind, item) in plan.new.iter().filter(|(key, _, _)| keys.contains(key)) {
        store::insert_item(tx, loupist_id, &stored_row(&next, key, kind, item))
            .map_err(internal)?;
    }
    for changed in plan
        .changed
        .iter()
        .filter(|changed| keys.contains(&changed.current.key))
    {
        let fresh = stored_row(&next, &changed.current.key, changed.kind, &changed.item);
        let updated = if changed.current.state == "pending" {
            fresh
        } else if !changed.item.findings.is_empty() {
            // Flagged text is accepted on its own, from the suggestions.
            ItemRow {
                update_json: Some(fresh.body_json),
                ..changed.current.clone()
            }
        } else if take_theirs(
            tx,
            loupist_id,
            &next,
            &changed.current,
            &changed.item,
            at_ms,
        )? {
            ItemRow {
                state: "accepted".into(),
                lens_record_id: changed.current.lens_record_id.clone(),
                ..fresh
            }
        } else {
            // The Loupist already holds that wording as another principle.
            changed.current.clone()
        };
        store::set_item_body(tx, loupist_id, &updated).map_err(internal)?;
    }
    for key in &plan.removed_pending {
        store::set_item_state(tx, loupist_id, &row.import_id, key, "removed", None)
            .map_err(internal)?;
    }
    if apply_map && !plan.map.is_empty() {
        let parsed = &staged.parsed;
        let claims: Vec<pack::ClaimRecord> = parsed
            .claims
            .iter()
            .filter(|claim| plan.map.claims.contains(&claim.key))
            .cloned()
            .collect();
        let ends: BTreeSet<&str> = claims
            .iter()
            .flat_map(|claim| claim.endpoints.iter().map(|end| end.entity.as_str()))
            .collect();
        let cited: BTreeSet<&str> = claims
            .iter()
            .flat_map(|claim| claim.sources.iter().map(String::as_str))
            .collect();
        let entities: Vec<pack::EntityRecord> = parsed
            .entities
            .iter()
            .filter(|entity| {
                plan.map.full
                    || plan.map.entities.contains(&entity.key)
                    || ends.contains(entity.key.as_str())
            })
            .cloned()
            .collect();
        let sources: Vec<pack::SourceRecord> = parsed
            .sources
            .iter()
            .filter(|source| plan.map.full || cited.contains(source.key.as_str()))
            .cloned()
            .collect();
        let known =
            shared_pack::import_spans(tx, loupist_id, &row.import_id).map_err(knowledge_error)?;
        let imported = shared_pack::import_world(
            tx,
            blobs,
            loupist_id,
            &WorldImport {
                import_id: &row.import_id,
                pack_id: &row.pack_id,
                pack_name: &next.name,
                author: next.author.as_deref(),
                license: next.license.id(),
                entities: &entities,
                claims: &claims,
                sources: &sources,
                at_ms,
                known_spans: &known,
                withdraw: &plan.map.withdraw,
                version: Some(next.version),
            },
        )
        .map_err(knowledge_error)?;
        for (kind, subjects) in [
            ("node", &imported.nodes_added),
            ("claim", &imported.claims_added),
            ("source", &imported.source_records),
            ("span", &imported.spans),
        ] {
            for subject in subjects {
                store::insert_world_ref(tx, loupist_id, &row.import_id, kind, subject)
                    .map_err(internal)?;
            }
        }
    }
    store::advance_import(tx, loupist_id, &next).map_err(internal)?;
    Ok(())
}

/// Whether the plan's map part would change anything (the add path waits for a World job first).
pub(crate) fn touches_map(
    tx: &Transaction<'_>,
    loupist_id: &str,
    row: &ImportRow,
    base: &Base,
    staged: &Staged,
) -> Result<bool, CommandError> {
    Ok(!plan(tx, loupist_id, row, base, staged)?.map.is_empty())
}
