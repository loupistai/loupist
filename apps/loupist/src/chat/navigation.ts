import type { AssetPageTarget } from "../contracts/assets";
import type { ChatArtifactRef, ChatEvidenceRef, KnowledgeLocation, MemoryRef } from "../types";
import { parseCredentialFreeHttpUrl } from "../lib/credentialFreeUrl";
import { isMacPlatform } from "../lib/platform";
import { knowledgeLocationFromMemoryRef } from "../knowledge/location";
import { citationPageUrl } from "./evidence";
import { artifactReadTarget } from "./artifactReadTarget";
import { isInlineWorkEvidence, memoryReadForEvidence } from "./memoryActivity";
import type { EvidenceCell } from "./numberChecks";
import { quoteEvidence } from "./quoteEvidence";
import type { EvidenceStepLink } from "./workspace";

/** Everything chat can open, as one value. Each provider layer adds its own policy before App routes it. */
export type ChatDestination =
  | {
      kind: "evidence";
      evidence: ChatEvidenceRef;
      /** A data number opens its reader at the matched cell. */
      cell?: EvidenceCell;
      /** The step that produced the evidence; the turn fills it in. */
      step?: EvidenceStepLink;
      /** Where a target this layer cannot open goes instead (a host post's asset page). */
      fallback?: ChatDestination;
    }
  | { kind: "link"; url: string }
  | { kind: "memory"; location: KnowledgeLocation; title: string }
  /** A Lens record of the Loupist on screen. It always opens in the Evidence reader. */
  | { kind: "lens"; recordId: string }
  | { kind: "page"; location: KnowledgeLocation }
  | { kind: "artifact"; artifact: ChatArtifactRef; threadId?: string }
  /** Resolve a retained read receipt to the latest saved work through the host. */
  | { kind: "artifact-link"; artifactId: string }
  /** A workspace file by its path, or the Files tab at the workspace root without one. */
  | { kind: "file"; path?: string }
  | { kind: "asset"; target: AssetPageTarget }
  | { kind: "schedule"; taskId: string }
  | { kind: "chat"; threadId: string }
  /**
   * A scheduled run's own session. A run's session goes with the run, so once it is
   * gone this opens the run's task in Schedule, and once the task is gone too the
   * open rejects with {@link RunRecordRemoved}.
   */
  | { kind: "run"; threadId: string | null; taskId: string }
  | { kind: "briefing"; runId: string }
  | { kind: "settings"; section: "connections" }
  /** The Following page (what you follow, shared by every Loupist). */
  | { kind: "following" };

/** A scheduled run whose session and task are both gone: there is nothing left to open. */
export class RunRecordRemoved extends Error {
  constructor() { super("run_record_removed"); this.name = "RunRecordRemoved"; }
}

/** How a click asked to open: in the system browser, and from which control (a popover anchors to it). */
export type OpenHow = { external?: boolean; anchor?: HTMLElement };
/** Rejects only on a real failure. A target nothing can open resolves with nothing opened. */
export type Open = (to: ChatDestination, how?: OpenHow) => Promise<void>;

/** Where a citation goes. Every evidence ref has one, so a numbered citation is never a no-op. */
export type EvidenceTarget =
  | { kind: "artifact"; artifactId: string }
  | { kind: "memory"; location: KnowledgeLocation; title: string }
  | { kind: "asset"; target: AssetPageTarget }
  | { kind: "step" }
  | { kind: "quote" }
  | { kind: "browser"; url: string }
  | { kind: "reader" };

type PointerLike = { button?: number; metaKey: boolean; ctrlKey: boolean };


/** Cmd-click on macOS, Ctrl-click elsewhere, and middle-click ask for the system browser. */
export function wantsExternal(event: PointerLike, mac = isMacPlatform()): boolean {
  return event.button === 1 || (mac ? event.metaKey : event.ctrlKey);
}

/** A top-level string field of the host's pretty-printed JSON (two-space indent), so a nested one never stands in. */
const topLevel = (excerpt: string, field: string) =>
  new RegExp(`^ {2}"${field}"\\s*:\\s*"([^"\\\\]{1,200})"`, "mu").exec(excerpt)?.[1];
const ASSET_READ_FIELDS = ["assetId", "ticker", "symbol", "name"] as const;

/**
 * The asset an `asset_page_read` receipt read: its id, its reference ticker (an older
 * receipt's `symbol`) and its name, as the stored excerpt kept them. A whole excerpt is
 * parsed; a bounded one gives what its top-level lines still hold. Without an id or a
 * ticker it names no asset.
 */
function assetReadTarget(evidence: ChatEvidenceRef, loupistId: string): AssetPageTarget | null {
  if (evidence.kind !== "saved_context" || evidence.tool_name !== "asset_page_read") return null;
  let parsed: Record<string, unknown> | null = null;
  try {
    const value: unknown = JSON.parse(evidence.excerpt);
    if (value && typeof value === "object" && !Array.isArray(value)) parsed = value as Record<string, unknown>;
  } catch { /* bounded: read its top-level lines */ }
  const text = (value: unknown) => typeof value === "string" && value.trim() && value.length <= 200 ? value : undefined;
  const field = (key: typeof ASSET_READ_FIELDS[number]) => text(parsed ? parsed[key] : topLevel(evidence.excerpt, key));
  // A whole excerpt may name the asset as the page does: `asset: {assetId, name, reference: {ticker}}`.
  const asset = parsed?.asset && typeof parsed.asset === "object" ? parsed.asset as Record<string, unknown> : null;
  const reference = asset?.reference && typeof asset.reference === "object" ? asset.reference as Record<string, unknown> : null;
  const [assetId, ticker, symbol, name] = ASSET_READ_FIELDS.map(field);
  return savedAssetTarget(loupistId, { assetId: assetId ?? text(asset?.assetId), ticker: ticker ?? symbol ?? text(reference?.ticker), name: name ?? text(asset?.name) });
}

export function evidenceTarget(evidence: ChatEvidenceRef, memoryRefs: readonly MemoryRef[] | undefined, loupistId: string): EvidenceTarget {
  const artifactId = artifactReadTarget(evidence);
  if (artifactId) return { kind: "artifact", artifactId };
  const read = memoryReadForEvidence(evidence, memoryRefs);
  const location = read ? knowledgeLocationFromMemoryRef(loupistId, read) : null;
  if (read && location) return { kind: "memory", location, title: read.title };
  if (isInlineWorkEvidence(evidence, memoryRefs)) return { kind: "step" };
  if (quoteEvidence(evidence)) return { kind: "quote" };
  const asset = assetReadTarget(evidence, loupistId);
  if (asset) return { kind: "asset", target: asset };
  const page = citationPageUrl(evidence);
  return page ? { kind: "browser", url: page } : { kind: "reader" };
}

/** A host post's source: its page through the evidence path, else the asset page the post is about. */
export function hostSourceDestination(evidence: ChatEvidenceRef, asset: AssetPageTarget | null): ChatDestination {
  // A saved post without a usable instant has no asset page to fall back to; its evidence still opens.
  if (!asset) return { kind: "evidence", evidence };
  const page: ChatDestination = { kind: "asset", target: asset };
  return evidence.url ? { kind: "evidence", evidence, fallback: page } : page;
}

/**
 * The page a saved reference opens: by its asset id, else by the exact reference ticker it
 * kept; the host says when the asset is no longer tracked. Without either there is no target.
 */
export function savedAssetTarget(
  loupistId: string,
  fields: { assetId?: string | null; ticker?: string | null; name?: string | null },
  place: Pick<AssetPageTarget, "section"> = {},
): AssetPageTarget | null {
  const field = (value: string | null | undefined) => value?.trim() ? value : undefined;
  const assetId = field(fields.assetId), ticker = field(fields.ticker), name = field(fields.name);
  if (!assetId && !ticker) return null;
  return {
    loupistId, ...(assetId ? { assetId } : { ticker: ticker! }), ...(name ? { name } : {}),
    ...(place.section ? { section: place.section } : {}),
  };
}

const httpUrl = (url: string | undefined) => (url ? parseCredentialFreeHttpUrl(url)?.href ?? null : null);

/** The address the system browser can open for this destination, if any. */
export function externalUrl(to: ChatDestination): string | null {
  if (to.kind === "link") return httpUrl(to.url);
  if (to.kind === "evidence") return httpUrl(to.evidence.url) ?? (to.fallback ? externalUrl(to.fallback) : null);
  return null;
}

/**
 * PanelRightOpen opens something in the side panel; ExternalLink is only the system browser.
 * Destinations that replace the view or stay in the message carry no icon. Prose links never
 * show one: the underline already says it is a link.
 */
export function destinationIcon(to: ChatDestination, how: OpenHow = {}): "panel" | "external" | null {
  if (how.external) return externalUrl(to) ? "external" : null;
  switch (to.kind) {
    case "link":
    case "memory":
    case "lens":
    case "artifact":
    case "artifact-link":
    case "file":
      return "panel";
    case "evidence": {
      // Without the answer's Memory reads a citation is never a Memory target here.
      const target = evidenceTarget(to.evidence, undefined, "").kind;
      return target === "browser" || target === "reader" || target === "artifact" ? "panel" : null;
    }
    default:
      return null;
  }
}
