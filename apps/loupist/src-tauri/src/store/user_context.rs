//! The asset catalog every Loupist shares (an ID, a name and optional reference
//! fields) and each Loupist's own followed list over it.
use super::{SqliteStore, StoreError, StoreResult};
use crate::commands::assets::wire::{
    AssetReference, AssetReferenceFields, AssetReferenceOrigin, AssetRow,
};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

pub(super) const DDL: &str = r#"
CREATE TABLE assets (
    asset_id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    reference_json TEXT,
    created_at_ms INTEGER NOT NULL
) STRICT;
"#;

/// Which catalog assets each Loupist follows, and since when. The catalog
/// creates it when it opens, outside the ladder. Removing a Loupist's
/// registration removes its follows.
pub(super) const FOLLOWS_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS asset_follows (
    loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE CASCADE,
    asset_id TEXT NOT NULL REFERENCES assets(asset_id) ON DELETE CASCADE,
    followed_at_ms INTEGER NOT NULL,
    PRIMARY KEY (loupist_id, asset_id)
) STRICT;
"#;

fn invalid(message: &str) -> StoreError {
    StoreError::Invalid(message.into())
}

pub(crate) fn text(value: &str, maximum: usize) -> StoreResult<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > maximum || value.chars().any(char::is_control) {
        return Err(invalid(
            "text is empty, too long, or contains control characters",
        ));
    }
    Ok(value.to_owned())
}

pub(crate) fn identity(value: &str) -> StoreResult<String> {
    let value = text(value, 64)?.to_uppercase();
    if !value.is_ascii() || value.chars().any(char::is_whitespace) {
        return Err(invalid("expected an ASCII identifier without spaces"));
    }
    Ok(value)
}

pub(crate) fn validate_reference(fields: &AssetReferenceFields) -> StoreResult<()> {
    for value in [
        &fields.figi,
        &fields.ticker,
        &fields.exchange,
        &fields.security_type,
    ]
    .into_iter()
    .flatten()
    {
        text(value, 64)?;
    }
    if let Some(name) = &fields.exchange_name {
        text(name, 128)?;
    }
    Ok(())
}

fn read_asset(row: &rusqlite::Row<'_>) -> rusqlite::Result<AssetRow> {
    let reference: Option<String> = row.get(2)?;
    Ok(AssetRow {
        asset_id: row.get(0)?,
        name: row.get(1)?,
        reference: reference
            .map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?,
        followed_at_ms: row.get(3)?,
        created_at_ms: row.get(4)?,
    })
}

/// Catalog columns with the follow time of the Loupist bound to `?1`.
const ASSET_COLUMNS: &str =
    "SELECT a.asset_id,a.name,a.reference_json,f.followed_at_ms,a.created_at_ms FROM assets a
     LEFT JOIN asset_follows f ON f.asset_id=a.asset_id AND f.loupist_id=?1";

/// The whole catalog; `followed_at_ms` is when this Loupist followed each asset.
pub(crate) fn rows(db: &rusqlite::Connection, loupist_id: &str) -> StoreResult<Vec<AssetRow>> {
    let mut statement = db.prepare(&format!(
        "{ASSET_COLUMNS} ORDER BY a.name COLLATE NOCASE,a.asset_id"
    ))?;
    let rows = statement.query_map([loupist_id], read_asset)?;
    Ok(super::collect_readable_rows(rows, "assets")?)
}

pub(crate) fn by_id(
    db: &rusqlite::Connection,
    loupist_id: &str,
    asset_id: &str,
) -> StoreResult<Option<AssetRow>> {
    Ok(db
        .query_row(
            &format!("{ASSET_COLUMNS} WHERE a.asset_id=?2"),
            [loupist_id, asset_id],
            read_asset,
        )
        .optional()?)
}

pub(crate) fn matching<'a>(rows: &'a [AssetRow], query: &str) -> Vec<&'a AssetRow> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    let lower = query.to_lowercase();
    for matches in [
        rows.iter()
            .filter(|row| row.asset_id == query)
            .collect::<Vec<_>>(),
        by_ticker(rows, query),
        rows.iter()
            .filter(|row| row.name.to_lowercase() == lower)
            .collect(),
        rows.iter()
            .filter(|row| row.name.to_lowercase().contains(&lower))
            .collect(),
    ] {
        if !matches.is_empty() {
            return matches;
        }
    }
    Vec::new()
}

pub(crate) fn by_ticker<'a>(rows: &'a [AssetRow], ticker: &str) -> Vec<&'a AssetRow> {
    rows.iter()
        .filter(|row| {
            row.reference
                .as_ref()
                .and_then(|reference| reference.fields.ticker.as_deref())
                .is_some_and(|value| value.eq_ignore_ascii_case(ticker.trim()))
        })
        .collect()
}

fn same_reference(stored: &AssetReferenceFields, requested: &AssetReferenceFields) -> bool {
    if let Some(figi) = &requested.figi {
        return stored.figi.as_ref() == Some(figi);
    }
    // Agent and legacy references can identify a listing by ticker and exchange.
    requested.ticker.is_some()
        && stored.ticker == requested.ticker
        && stored.exchange == requested.exchange
}

fn follow_match<'a>(
    assets: &'a [AssetRow],
    name: &str,
    fields: Option<&AssetReferenceFields>,
) -> Option<&'a AssetRow> {
    let name = name.trim().to_lowercase();
    assets
        .iter()
        .filter_map(|asset| {
            let stored = asset.reference.as_ref().map(|reference| &reference.fields);
            let priority = match (fields, stored) {
                (Some(requested), Some(stored)) if same_reference(stored, requested) => 0,
                (None, _) | (_, None) if asset.name.trim().to_lowercase() == name => 1,
                _ => return None,
            };
            Some((priority, asset))
        })
        // Several listings can have the same name. Prefer one this Loupist follows.
        .min_by_key(|(priority, asset)| {
            (*priority, asset.followed_at_ms.is_none(), &asset.asset_id)
        })
        .map(|(_, asset)| asset)
}

/// The catalog asset a follow by `name` (and a reference) lands on, or a new
/// one. `assets` is the catalog as one Loupist reads it.
pub(crate) fn plan_follow(
    assets: &[AssetRow],
    name: &str,
    fields: Option<AssetReferenceFields>,
    origin: AssetReferenceOrigin,
    at_ms: i64,
) -> StoreResult<AssetRow> {
    let name = text(name, 256)?;
    if let Some(fields) = &fields {
        validate_reference(fields)?;
    }
    let existing = follow_match(assets, &name, fields.as_ref());
    let mut row = existing.cloned().unwrap_or_else(|| AssetRow {
        asset_id: format!("ast_{}", &uuid::Uuid::new_v4().simple().to_string()[..16]),
        name,
        reference: None,
        followed_at_ms: None,
        created_at_ms: at_ms,
    });
    add_reference(
        &mut row.reference,
        fields.map(|fields| AssetReference { fields, origin }),
    );
    Ok(row)
}

fn add_reference(stored: &mut Option<AssetReference>, requested: Option<AssetReference>) -> bool {
    let Some(requested) = requested else {
        return false;
    };
    match stored {
        None => *stored = Some(requested),
        Some(reference)
            if reference.fields.exchange_name.is_none()
                && requested.fields.exchange_name.is_some()
                && same_reference(&reference.fields, &requested.fields)
                && reference.fields.exchange == requested.fields.exchange =>
        {
            reference.fields.exchange_name = requested.fields.exchange_name;
        }
        _ => return false,
    }
    true
}

/// Saves the asset in the catalog (its reference, when it gained one) and this
/// Loupist's follow of it, keeping an earlier follow time.
fn write_follow(
    tx: &rusqlite::Transaction<'_>,
    loupist_id: &str,
    row: &mut AssetRow,
    at_ms: i64,
) -> StoreResult<()> {
    let at = *row.followed_at_ms.get_or_insert(at_ms);
    tx.execute(
        "INSERT INTO assets(asset_id,name,reference_json,created_at_ms)
         VALUES(?1,?2,?3,?4) ON CONFLICT(asset_id) DO UPDATE SET
           reference_json=excluded.reference_json",
        params![
            row.asset_id,
            row.name,
            row.reference
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            row.created_at_ms,
        ],
    )?;
    tx.execute(
        "INSERT INTO asset_follows(loupist_id,asset_id,followed_at_ms) VALUES(?1,?2,?3)
         ON CONFLICT(loupist_id,asset_id) DO NOTHING",
        params![loupist_id, row.asset_id, at],
    )?;
    Ok(())
}

impl SqliteStore {
    /// The catalog with this Loupist's follow times; `followed_only` keeps
    /// what it follows.
    pub fn assets_list(&self, loupist_id: &str, followed_only: bool) -> StoreResult<Vec<AssetRow>> {
        let db = self.lock()?;
        let mut assets = rows(&db, loupist_id)?;
        if followed_only {
            assets.retain(|row| row.followed_at_ms.is_some());
        }
        Ok(assets)
    }

    pub fn asset_by_id(&self, loupist_id: &str, asset_id: &str) -> StoreResult<Option<AssetRow>> {
        let db = self.lock()?;
        by_id(&db, loupist_id, asset_id)
    }

    pub fn asset_follow_name(
        &self,
        loupist_id: &str,
        name: &str,
        fields: Option<AssetReferenceFields>,
        origin: AssetReferenceOrigin,
        at_ms: i64,
    ) -> StoreResult<AssetRow> {
        let mut db = self.lock()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut row = plan_follow(&rows(&tx, loupist_id)?, name, fields, origin, at_ms)?;
        write_follow(&tx, loupist_id, &mut row, at_ms)?;
        tx.commit()?;
        Ok(row)
    }

    /// Applies a follow `plan_follow` made earlier in the turn, unless the
    /// catalog changed under it.
    pub(crate) fn apply_asset_follow(
        &self,
        loupist_id: &str,
        row: &AssetRow,
        at_ms: i64,
    ) -> StoreResult<Option<AssetRow>> {
        let mut db = self.lock()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let assets = rows(&tx, loupist_id)?;
        let existing = follow_match(
            &assets,
            &row.name,
            row.reference.as_ref().map(|reference| &reference.fields),
        );
        let mut row = match existing {
            Some(asset) if asset.asset_id == row.asset_id => {
                let mut asset = asset.clone();
                let added = add_reference(&mut asset.reference, row.reference.clone());
                if asset.followed_at_ms.is_some() && !added {
                    return Ok(None);
                }
                asset
            }
            None if !assets.iter().any(|asset| asset.asset_id == row.asset_id) => row.clone(),
            _ => {
                return Err(StoreError::Conflict(
                    "This asset was followed during the reply",
                ))
            }
        };
        write_follow(&tx, loupist_id, &mut row, at_ms)?;
        tx.commit()?;
        Ok(Some(row))
    }

    /// Follows (`Some`, keeping an earlier follow time) or unfollows (`None`) a
    /// catalog asset for this Loupist. `None` when the catalog has no such asset.
    pub fn set_asset_followed(
        &self,
        loupist_id: &str,
        asset_id: &str,
        at_ms: Option<i64>,
    ) -> StoreResult<Option<AssetRow>> {
        if !crate::knowledge::assets::valid_asset_id(asset_id) {
            return Err(invalid("Invalid asset ID."));
        }
        let mut db = self.lock()?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        match at_ms {
            Some(at_ms) => tx.execute(
                "INSERT INTO asset_follows(loupist_id,asset_id,followed_at_ms)
                 SELECT ?1,asset_id,?3 FROM assets WHERE asset_id=?2
                 ON CONFLICT(loupist_id,asset_id) DO NOTHING",
                params![loupist_id, asset_id, at_ms],
            )?,
            None => tx.execute(
                "DELETE FROM asset_follows WHERE loupist_id=?1 AND asset_id=?2",
                params![loupist_id, asset_id],
            )?,
        };
        let row = by_id(&tx, loupist_id, asset_id)?;
        tx.commit()?;
        Ok(row)
    }

    /// What this Loupist follows, as the Map, World extraction and briefings read it.
    pub fn map_context(&self, loupist_id: &str) -> StoreResult<MapContext> {
        let assets = self.assets_list(loupist_id, true)?;
        let updated_at_ms = assets
            .iter()
            .filter_map(|asset| asset.followed_at_ms)
            .max()
            .unwrap_or_default();
        Ok(MapContext {
            watchlist: assets.iter().map(MapContextItem::from).collect(),
            updated_at_ms,
        })
    }
}

/// A name and listing labels projected for the Map's entity matcher.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct MapContextItem {
    pub asset_id: String,
    pub symbol: String,
    pub market: String,
    pub display_name: Option<String>,
}

impl From<&AssetRow> for MapContextItem {
    fn from(asset: &AssetRow) -> Self {
        let fields = asset.reference.as_ref().map(|reference| &reference.fields);
        Self {
            asset_id: asset.asset_id.clone(),
            symbol: fields
                .and_then(|fields| fields.ticker.clone())
                .unwrap_or_default(),
            market: fields
                .and_then(|fields| fields.exchange.clone())
                .unwrap_or_default(),
            display_name: Some(asset.name.clone()),
        }
    }
}

#[derive(Default)]
pub struct MapContext {
    pub watchlist: Vec<MapContextItem>,
    pub updated_at_ms: i64,
}

// Tests, the e2e fixture and the eval harness seed one Loupist's followed listings.
#[cfg(any(test, feature = "e2e"))]
impl SqliteStore {
    pub(crate) fn seed_followed_listing(
        &self,
        loupist_id: &str,
        ticker: &str,
        exchange: &str,
        name: Option<&str>,
    ) -> StoreResult<AssetRow> {
        self.asset_follow_name(
            loupist_id,
            name.unwrap_or(ticker),
            Some(AssetReferenceFields {
                ticker: Some(ticker.to_owned()),
                exchange: (!exchange.is_empty() && exchange != "UNKNOWN")
                    .then(|| exchange.to_owned()),
                ..Default::default()
            }),
            AssetReferenceOrigin::User,
            crate::commands::now_ms(),
        )
    }
}

#[cfg(test)]
mod tests;
