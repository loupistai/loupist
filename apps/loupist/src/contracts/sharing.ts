import type { StreamObserver } from "./runtime";
import type { LoupistTone } from "./loupist";
import type * as Wire from "./generated";

/**
 * Sharing (W19): packs, GitHub publishing, import review and suggestions.
 * Rust owns every pack byte, scan, network call, credential and transaction;
 * the renderer receives counts, findings (never the matched value), validated
 * item text and the GitHub login only.
 */

export type PackLicense = "CC-BY-4.0" | "CC-BY-SA-4.0" | "CC0-1.0" | "CC-BY-NC-4.0" | "CC-BY-NC-SA-4.0" | "LicenseRef-Loupist-Personal";
export type ShareLicense = Wire.ShareLicense;
export interface LicenseConflict {
  code: "license_conflict";
  items: { key: string; name: string; license: PackLicense }[];
}
export type ShareSectionId = "profile" | "perspective" | "methods" | "principles" | "map";
export type ShareSections = Record<ShareSectionId, boolean>;

/** A whole Loupist, or one method or principle ("Share this method"). */
export type ShareScope = { kind: "loupist" } | { kind: "item"; recordId: string };

/**
 * credential and account block until left out; holding blocks; personal warns
 * and allows "It's fine".
 */
export type ShareFindingClass = "credential" | "account" | "holding" | "personal";
export type ShareItemKind = "profile" | "perspective" | "method" | "principle" | "entity" | "claim" | "source";

export interface ShareFinding {
  /** Stable for this summary; pass back in acknowledgedFindingIds ("It's fine"). */
  id: string;
  class: ShareFindingClass;
  /** Pass in excludedItemKeys to leave the item out ("Leave it out"). */
  itemKey: string;
  itemKind: ShareItemKind;
  /** The record's own label (a method title, a principle statement, a name). Never the matched text. */
  itemLabel: string;
  /** `holding` only: the held symbol that appears near its quantity, value or cost. */
  symbol?: string;
  blocking: boolean;
}

export interface ShareSummary {
  loupistName: string;
  scope: ShareScope;
  /** Present for an item scope: the item's title row. */
  item: { kind: "method" | "principle"; title: string } | null;
  counts: {
    perspective: number;
    methods: number;
    principles: number;
    names: number;
    links: number;
    sources: number;
    /** Relationships supported only by private or restricted-provider Sources. */
    leftOut: number;
    /** Items and dependent relationships whose stored rights prohibit sharing. */
    leftOutLicense: number;
  };
  /** The profile line exists whenever the Loupist has a name. */
  findings: ShareFinding[];
  /** Recomputed in Rust for the selected sections and exclusions. */
  allowedLicenses: ShareLicense[];
  /** NC-SA for a new share; otherwise the first compatible choice. */
  defaultLicense: PackLicense;
  licenseConflict: LicenseConflict | null;
  /** The last recorded byline, if any. */
  byline: string | null;
  /** The repository this Loupist was published to before, if any. */
  repo: { fullName: string; url: string; version: number } | null;
  /** The pack version the next export or publish will carry. */
  nextVersion: number;
}

export interface ShareRequest {
  loupistId: string;
  scope: ShareScope;
  sections: ShareSections;
  excludedItemKeys: string[];
  acknowledgedFindingIds: string[];
  /** Omit to use Rust's compatible default. */
  license?: PackLicense;
  byline?: string;
}

export interface ShareExportRequest extends ShareRequest {
  /** Rust opens the native folder or save dialog itself. */
  destination: "folder" | "zip";
}

export type SharePublishTarget = { repo: { name: string } } | { update: true } | { gist: "public" | "secret" };
export interface SharePublishRequest extends ShareRequest {
  target: SharePublishTarget;
}
export type SharePublishEvent = { kind: "creating" } | { kind: "uploading"; files: number };
export interface SharePublishResult {
  url: string;
  /** owner/name for a repository; null for a gist. */
  fullName: string | null;
  version: number;
  /** The commit the publish made; null for a gist. */
  commit: string | null;
}

export type GithubAccount = Wire.GithubAccount;
export type GithubSignInFailure = "expired" | "denied" | "not_configured" | "offline";
export type GithubSignInEvent =
  | { kind: "code"; userCode: string; expiresInSec: number }
  | { kind: "waiting" }
  | { kind: "signed_in"; login: string }
  | { kind: "failed"; reason: GithubSignInFailure };

export type PackFlag = "instruction" | "tool" | "secret" | "link" | "hidden";
export interface PackItemFinding {
  flag: PackFlag;
  /** 1-based lines of `markdown` to highlight. */
  lines: number[];
}
export interface PackItemView {
  key: string;
  kind: "perspective" | "method" | "principle";
  title: string;
  /** Validated Markdown, hidden characters removed; render without HTML or images, links as text. */
  markdown: string;
  findings: PackItemFinding[];
}

/** One map record (a name, relationship or Source) whose text was flagged. */
export interface PackMapFinding {
  kind: "entity" | "claim" | "source";
  /** The record's own name, relationship or title, hidden characters removed. */
  label: string;
  flags: PackFlag[];
}

export type PackRejection = "notPack" | "newer" | "tooLarge" | "unsafe" | "damaged" | "spoof";
export type PackBlockedReason = "reported" | "unsafe" | "author";

export interface PackManifestView {
  id: string;
  kind: "loupist" | "method" | "principle";
  name: string;
  version: number;
  createdAt: string;
  language: string;
  license: PackLicense;
  licenseUrl: string;
  redistributable: boolean;
  author: string | null;
  /** Ancestors, nearest first. */
  origin: { id: string; name: string; version: number; url: string | null }[];
  /** Attribution for included material, as supplied by the pack manifest. */
  credits: { name: string; author: string | null; license: PackLicense | null; url: string | null; version: number }[];
  assetClasses: string[];
}

export interface PackPreview {
  stagingId: string;
  manifest: PackManifestView;
  /** "github.com/owner/repo" for a link, null for a local ZIP or folder. */
  sourceLabel: string | null;
  sourceUrl: string | null;
  profile: { name: string; focus: string; tone: LoupistTone; description: string };
  perspective: PackItemView | null;
  methods: PackItemView[];
  principles: PackItemView[];
  world: {
    names: number;
    links: number;
    sources: number;
    /** Source hosts with counts, most first, at most 6 (the rest summed in `moreHosts`). */
    sourceHosts: { host: string; count: number }[];
    moreHosts: number;
    /** Entity kinds and relation names the map tracks, most used first. */
    kinds: string[];
    relations: string[];
    /** World extension types/relations the pack adds; the preview counts them; a merge does not change what the map tracks. */
    extension: { types: number; relations: number };
    /** Flagged map records, at most 50 listed. */
    findings: PackMapFinding[];
    /** Every flagged map record, listed or not. */
    flagged: number;
  };
  /** Flags on the pack's own words: its name, author, origin and profile. */
  profileFindings: PackFlag[];
  /** Items, map records and the pack's own words with at least one finding. */
  flaggedCount: number;
  /** The community index lists this pack as removed: Add is refused. */
  blocked: PackBlockedReason | null;
}
export type PackOpenResult =
  | { kind: "preview"; preview: PackPreview }
  | { kind: "rejected"; reason: PackRejection; sourceLabel: string | null };

/**
 * `pending` opens the link a deep link left with Rust; Rust shows the chooser for zip and folder;
 * `gallery` names a Gallery card (Rust resolves its repository, path and commit from the index).
 */
export type PackSource = { url: string } | { zip: true } | { folder: true } | { pending: true } | { gallery: string };
/** Link errors (CommandError.code): link_invalid, link_not_found, offline. */
export type PackLinkErrorCode = "link_invalid" | "link_not_found" | "offline";

export type PackAddMode = { new: { name: string; tone: LoupistTone } } | { merge: { loupistId: string } };
export type PackAddRequest = Wire.PackAddRequest;
export type PackAddEvent = { done: number; total: number };
export interface PackAddResult { loupistId: string; importId: string }

export type PackItemState = "pending" | "accepted" | "skipped" | "removed";
export interface PackSuggestion {
  key: string;
  kind: "perspective" | "method" | "principle";
  title: string;
  flags: PackFlag[];
  state: PackItemState;
  /** The author's newer text for an item already accepted (W19.5); Accept takes it, Skip keeps yours. */
  update: boolean;
}
export interface PackSuggestionGroup {
  importId: string;
  packName: string;
  items: PackSuggestion[];
}

export interface PackImportView {
  importId: string;
  packId: string;
  name: string;
  author: string | null;
  version: number;
  license: PackLicense;
  sourceUrl: string | null;
  addedAtMs: number;
  mode: "new" | "merge";
  blockedReason: PackBlockedReason | null;
  /** What Remove would take away: names and relationships only this pack supports, accepted Lens items. */
  removable: { names: number; links: number; accepted: number };
  /** When "Check for updates" last ran; null before the first check. */
  checkedAtMs: number | null;
}

/** W19.5: what a newer version of an added pack would change here. */
export interface PackUpdateNewItem {
  key: string;
  kind: PackItemView["kind"];
  title: string;
  flags: PackFlag[];
  item: PackItemView;
}
export interface PackUpdateChangedItem {
  key: string;
  kind: PackItemView["kind"];
  title: string;
  /** Still a suggestion here, or accepted into this Loupist. */
  state: "pending" | "accepted";
  /** Accepted and then changed here: theirs is shown beside yours. */
  edited: boolean;
  flags: PackFlag[];
  /** The text as it stands here. */
  then: string;
  /** The author's newer text, validated and scanned. */
  now: PackItemView;
}
export interface PackUpdateRemovedItem {
  key: string;
  kind: PackItemView["kind"];
  title: string;
  /** The accepted record, kept here; Archive uses the ordinary Lens or principle archive. */
  lensRecordId: string;
}
export interface PackUpdate {
  stagingId: string;
  importId: string;
  fromVersion: number;
  toVersion: number;
  createdAt: string;
  newItems: PackUpdateNewItem[];
  changed: PackUpdateChangedItem[];
  removed: PackUpdateRemovedItem[];
  /** Null when the map does not change; `added` is false when this import's map was never added. */
  map: { names: number; links: number; withdrawn: number; added: boolean } | null;
  blocked: PackBlockedReason | null;
}
export type PackUpdateCheck =
  | { kind: "upToDate" }
  /** An automatic check already ran today. */
  | { kind: "notChecked" }
  /** Added from a ZIP or folder: choose a newer copy. */
  | { kind: "local" }
  | { kind: "originGone" }
  | { kind: "offline" }
  | { kind: "differentPack" }
  | { kind: "rejected"; reason: PackRejection }
  | { kind: "available"; update: PackUpdate };
export type PackApplyUpdateRequest = Wire.PackApplyUpdateRequest;

/** W19.6: one Loupist listed in the community index. */
export type PackAssetClass = "stocks" | "etfs" | "bonds" | "crypto" | "economy";
export interface GalleryEntry {
  id: string;
  /** "github.com/owner/repo" */
  source: string;
  name: string;
  focus: string;
  author: string | null;
  language: string;
  license: PackLicense;
  assetClasses: PackAssetClass[];
  counts: { methods: number; principles: number; names: number; links: number };
  stars: number;
  forks: number;
  updatedAt: string;
  hasPreview: boolean;
}
export interface GalleryIndex {
  /** `offline`: the list kept from `fetchedAtMs`; `unavailable`: none kept; `notConfigured`: no index in this build. */
  status: "ready" | "offline" | "unavailable" | "notConfigured";
  fetchedAtMs: number | null;
  entries: GalleryEntry[];
}
/** Explicit acceptance of the exact listing terms URL this build names; Rust refuses anything else. */
export interface GalleryListingRequest { loupistId: string; acceptedTerms: true; termsUrl: string }
/** The index lists this Loupist at its last published commit, at an earlier one, or not at all. */
export type GalleryListingStatus = "listed" | "outdated" | null;

export interface PackOrigin {
  importId: string;
  packName: string;
  author: string | null;
}

export type DeepLinkEvent = Wire.DeepLinkEvent;

export interface SharingBridge {
  shareSummary(request: { loupistId: string; scope: ShareScope; sections?: ShareSections; excludedItemKeys?: string[] }): Promise<ShareSummary>;
  /** Null when the native dialog was cancelled. Rust re-runs the scan and refuses unresolved blocking findings. */
  shareExport(request: ShareExportRequest): Promise<{ path: string } | null>;
  /** Markdown of one method or principle (item scope only). */
  shareCopyText(request: ShareRequest): Promise<{ text: string }>;
  /** Shows the last folder or ZIP this Loupist was saved to in Finder. */
  shareReveal(request: { loupistId: string }): Promise<void>;
  sharePublish(request: SharePublishRequest, observer: StreamObserver<SharePublishEvent>): Promise<SharePublishResult>;

  githubAccount(): Promise<GithubAccount>;
  githubSignIn(observer: StreamObserver<GithubSignInEvent>): Promise<GithubAccount>;
  /** Opens GitHub's device verification page (Rust holds the address). */
  githubSignInOpen(): Promise<void>;
  githubSignInCancel(): Promise<void>;
  githubSignOut(): Promise<GithubAccount>;

  /** Null when the native chooser was cancelled. */
  packOpen(request: { source: PackSource }): Promise<PackOpenResult | null>;
  packAdd(request: PackAddRequest, observer: StreamObserver<PackAddEvent>): Promise<PackAddResult>;
  packList(request: { loupistId: string }): Promise<PackImportView[]>;
  packSuggestions(request: { loupistId: string }): Promise<PackSuggestionGroup[]>;
  packItemRead(request: { loupistId: string; importId: string; key: string }): Promise<PackItemView>;
  /** Flagged items are accepted one at a time; a batch with a flagged item is refused. */
  packDecide(request: { loupistId: string; importId: string; itemKeys: string[]; decision: "accept" | "skip" }): Promise<PackSuggestionGroup[]>;
  packRemove(request: { loupistId: string; importId: string; archiveAccepted: boolean }): Promise<PackImportView[]>;
  /** Rust opens the community report form with only the public pack URL filled in. */
  packReport(request: { loupistId: string; importId: string }): Promise<void>;
  /** "Shared · {pack} by {author}" for a Lens record or a Map name; null when not shared. */
  packOrigin(request: { loupistId: string; subjectId: string }): Promise<PackOrigin | null>;
  /** Manual, or `automatic` when the section opens (Rust runs it at most once a day). Never in the background. */
  packCheckUpdate(request: { loupistId: string; importId: string; automatic?: boolean }): Promise<PackUpdateCheck>;
  /** A newer ZIP or folder of a pack added from one (same pack id required); null when the chooser was cancelled. */
  packChooseNewer(request: { loupistId: string; importId: string; source: { zip: true } | { folder: true } }): Promise<PackUpdateCheck | null>;
  packApplyUpdate(request: PackApplyUpdateRequest): Promise<PackImportView[]>;

  /** The community index, kept in app data for offline use and read again after 6 h or on refresh. */
  galleryIndex(request: { refresh: boolean }): Promise<GalleryIndex>;
  /** A card's Map image as a data URL (PNG, at most 1 MiB, cached by commit); null when it has none. */
  galleryImage(request: { entryId: string }): Promise<string | null>;
  galleryListingStatus(request: { loupistId: string }): Promise<GalleryListingStatus>;
  /**
   * Rust forks the index, adds this Loupist's entry file and opens a pull request; its address.
   * Only after the user explicitly accepted the listing terms at `termsUrl` (GithubAccount.galleryTermsUrl).
   */
  galleryListingRequest(request: GalleryListingRequest): Promise<{ url: string }>;

  /** A link Rust is holding (for example from a cold start), once onboarding is done. */
  deepLinkPending(): Promise<DeepLinkEvent | null>;
  deepLinkDismiss(): Promise<void>;
  onDeepLinkImport(handler: (event: DeepLinkEvent) => void): () => void;
}
