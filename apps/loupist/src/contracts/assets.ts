/**
 * Assets and their pages (Rust `commands/assets.rs`, `commands/assets/wire.rs`).
 * An asset is a name with optional reference fields; its page holds the thesis,
 * the read and its moves, and the one stored price a saved read replays. Rust
 * owns the catalog, the replay and the thesis; React renders what it returns and
 * submits typed intent.
 *
 * The payload types are generated from Rust.
 */
import type * as Wire from "./generated";

/** "Holding up" is stored as `holding`; it never reads as ownership. */
export type Verdict = Wire.Verdict;

/** Where a row came from: a retained dataset row, an original URL, a Source, a Map claim or a cited turn result. */
export type AssetEvidence = Wire.AssetEvidence;

export type AssetReferenceOrigin = Wire.AssetReferenceOrigin;
export type AssetReferenceFields = Wire.AssetReferenceFields;
export type AssetRow = Wire.AssetRow;
export type AssetLookupCandidate = Wire.AssetLookupCandidate;
export type PriceReadState = Wire.PriceReadState;
export type SavedPriceRead = Wire.SavedPriceRead;
export type AssetStoredPrice = Wire.AssetStoredPrice;
export type AssetPricePoint = Wire.AssetPricePoint;
export type StoredAssetQuote = Wire.StoredAssetQuote;
export type LineCrossing = Wire.LineCrossing;
export type AssetMove = Wire.MoveView;
export type AssetReason = Wire.ReasonView;
export type AssetLine = Wire.LineView;
export type AssetRead = Wire.ReadView;
export type AssetMapCounts = Wire.MapCounts;
export type AssetPage = Wire.ThinAssetPageView;
export type AssetsListRequest = Wire.AssetsListRequest;
export type AssetIdRequest = Wire.AssetIdRequest;
export type AssetLookupRequest = Wire.AssetLookupRequest;
export type AssetFollowNameRequest = Wire.AssetFollowNameRequest;
export type AssetOpenRequest = Wire.AssetOpenRequest;
export type AssetPriceSeriesRequest = Wire.AssetPriceSeriesRequest;
export type AssetEntitySetRequest = Wire.AssetEntitySetRequest;
export type AssetVerdictUndoRequest = Wire.AssetVerdictUndoRequest;

export interface AssetBridge {
  /** Followed assets only. */
  assetsList(request: AssetsListRequest): Promise<AssetRow[]>;
  /** One OpenFIGI request per submitted search; never called while typing. */
  assetLookup(request: AssetLookupRequest): Promise<AssetLookupCandidate[]>;
  /** Follows by name, with a picked listing's fields as reference; Rust starts one delegated price setup per burst. */
  assetFollowName(request: AssetFollowNameRequest): Promise<AssetRow>;
  /** Follows an asset the catalog already has. */
  assetFollow(request: AssetIdRequest): Promise<AssetRow>;
  assetUnfollow(request: AssetIdRequest): Promise<void>;
  /** Opening a page may replay its saved read, reusing a result under 15 minutes old. */
  assetPageGet(request: AssetOpenRequest): Promise<AssetPage>;
  /** Check now: replays without reuse, or starts the delegated price setup when there is no usable saved read. */
  assetPriceCheck(request: AssetIdRequest): Promise<AssetPage>;
  /** The chart: the optional history read with reuse, else the points replays gathered. */
  assetPriceSeries(request: AssetPriceSeriesRequest): Promise<AssetPricePoint[]>;
  /** The stored price of every followed asset that has one. */
  watchlistQuotes(request: AssetsListRequest): Promise<StoredAssetQuote[]>;
  assetPageSeen(request: AssetIdRequest): Promise<void>;
  /** Unlinks the page from its Map entity. */
  assetEntitySet(request: AssetEntitySetRequest): Promise<AssetPage>;
  assetVerdictUndo(request: AssetVerdictUndoRequest): Promise<AssetPage>;
  /** `assets-changed`: reload followed and price views; never start another check from it. */
  onAssetsChanged(handler: () => void): () => void;
}

/** Where the asset page opens: by id, or a saved link's ticker; `section` scrolls to the thesis history. */
export type AssetPageTarget = { loupistId: string; assetId?: string; ticker?: string; name?: string; section?: "history" };

/** The ticker an asset shows, if its reference has one. */
export const assetTicker = (asset: Pick<AssetRow, "reference">) => asset.reference?.ticker?.trim() || null;

/** The venue label shown beside a ticker. The original code still identifies the listing. */
export const assetExchangeLabel = (reference?: AssetReferenceFields | null) =>
  reference?.exchangeName?.trim() || reference?.exchange?.trim() || null;
