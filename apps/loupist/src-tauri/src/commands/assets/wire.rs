//! Asset catalog, saved price and renderer wire types.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    datasets::DatasetRef,
    knowledge::assets::{LineView, MapCounts, MoveView, ReadView, ReasonView},
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AssetReferenceOrigin {
    User,
    Agent,
    Openfigi,
    Imported,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Default)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetReferenceFields {
    pub figi: Option<String>,
    pub ticker: Option<String>,
    pub exchange: Option<String>,
    #[serde(default)]
    pub exchange_name: Option<String>,
    pub security_type: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AssetReference {
    #[serde(flatten)]
    pub fields: AssetReferenceFields,
    pub origin: AssetReferenceOrigin,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AssetRow {
    pub asset_id: String,
    pub name: String,
    pub reference: Option<AssetReference>,
    /// When the requesting Loupist followed the asset; null when it does not follow it.
    pub followed_at_ms: Option<i64>,
    pub created_at_ms: i64,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetsListRequest {
    pub loupist_id: String,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetFollowNameRequest {
    pub loupist_id: String,
    pub name: String,
    pub reference: Option<AssetReferenceFields>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetIdRequest {
    pub loupist_id: String,
    pub asset_id: String,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetOpenRequest {
    pub loupist_id: String,
    pub asset_id: Option<String>,
    pub ticker: Option<String>,
    pub entity_id: Option<String>,
    pub stored_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetPriceSeriesRequest {
    pub loupist_id: String,
    pub asset_id: String,
    #[cfg_attr(test, ts(optional))]
    pub stored_only: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetLookupRequest {
    pub loupist_id: String,
    pub query: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AssetLookupCandidate {
    pub figi: String,
    pub name: String,
    pub ticker: Option<String>,
    pub exchange: Option<String>,
    #[cfg_attr(test, ts(optional = nullable))]
    pub exchange_name: Option<String>,
    pub security_type: Option<String>,
    pub market_sector: Option<String>,
}

/// One listing search. The key is optional: a saved key that could not be
/// read leaves the public route, and `saved_key_unread` says so.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AssetLookupResult {
    pub candidates: Vec<AssetLookupCandidate>,
    pub saved_key_unread: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum PriceReadState {
    None,
    Ready,
    Failed,
    NeedsConnection,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PriceFields {
    pub value: String,
    pub date: String,
    pub currency: Option<String>,
    pub rows: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RollingDates {
    pub from_arg: String,
    pub to_arg: String,
    pub days: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SavedReadCall {
    pub tool: String,
    pub server: Option<String>,
    #[cfg_attr(test, ts(type = "Record<string, unknown>"))]
    pub arguments: Value,
    pub fields: PriceFields,
    pub rolling: Option<RollingDates>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SavedPriceRead {
    pub state: PriceReadState,
    pub current: SavedReadCall,
    pub history: Option<SavedReadCall>,
    /// The currency a quote for the same symbol stated in the reply that saved
    /// this read, kept for repeats of a read whose own result states none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub quoted_currency: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AssetQuoteKind {
    Open,
    High,
    Low,
    Close,
    AdjustedClose,
    Last,
    Bid,
    Ask,
}

/// Where a stored price's currency came from: the provider's result (the read
/// itself, or a quote for the same symbol in the reply that saved it), the
/// listing the asset is bound to, or nowhere, when the currency is unknown.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum CurrencySource {
    Result,
    Listing,
    #[default]
    None,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AssetStoredPrice {
    #[serde(default)]
    #[cfg_attr(test, ts(optional = nullable))]
    pub kind: Option<AssetQuoteKind>,
    pub value: f64,
    pub date: String,
    pub currency: Option<String>,
    #[serde(default)]
    pub currency_source: CurrencySource,
    pub provider: String,
    pub source_url: Option<String>,
    pub dataset: DatasetRef,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AssetPricePoint {
    pub value: f64,
    pub date: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct StoredAssetQuote {
    pub asset_id: String,
    pub price: AssetStoredPrice,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ThinAssetPageView {
    pub asset: AssetRow,
    pub price_setup_running: bool,
    pub current_price: Option<AssetStoredPrice>,
    pub saved_read: Option<SavedPriceRead>,
    pub entity_id: Option<String>,
    pub reasons: Vec<ReasonView>,
    pub lines: Vec<LineView>,
    pub read: Option<ReadView>,
    pub history: Vec<MoveView>,
    pub map: Option<MapCounts>,
    pub revision: i64,
    /// Changes about this asset since its latest move. Omitted under an as-of date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub since_read: Option<SinceReadBlock>,
}

/// Counts and at most eight rows, the same feed as `world_changes` about this asset.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SinceReadBlock {
    /// Counts for the whole window, not only the rows shown.
    pub counts: BTreeMap<String, u32>,
    #[cfg_attr(test, ts(type = "unknown[]"))]
    pub changes: Vec<Value>,
    #[cfg_attr(test, ts(type = "unknown[]"))]
    pub lens: Vec<Value>,
    pub pending: u32,
    pub failed: u32,
    pub capped: bool,
    pub next: String,
}
