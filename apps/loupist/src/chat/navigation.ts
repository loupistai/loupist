import type { AssetPageTarget } from "../contracts/assets";
import type { ChatArtifactRef, ChatEvidenceRef, KnowledgeLocation, MemoryRef } from "../types";
import { parseCredentialFreeHttpUrl } from "../lib/credentialFreeUrl";
import { knowledgeLocationFromMemoryRef } from "../knowledge/location";
import { citationPageUrl } from "./evidence";
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
  /** A workspace file by its path, or the Files tab at the workspace root without one. */
  | { kind: "file"; path?: string }
  | { kind: "asset"; target: AssetPageTarget }
  | { kind: "schedule"; taskId: string }
  | { kind: "chat"; threadId: string }
  | { kind: "briefing"; runId: string }
  | { kind: "settings"; section: "connections" };

/** How a click asked to open: in the system browser, and from which control (a popover anchors to it). */
export type OpenHow = { external?: boolean; anchor?: HTMLElement };
/** Rejects only on a real failure. A target nothing can open resolves with nothing opened. */
export type Open = (to: ChatDestination, how?: OpenHow) => Promise<void>;

/** Where a citation goes. Every evidence ref has one, so a numbered citation is never a no-op. */
export type EvidenceTarget =
  | { kind: "memory"; location: KnowledgeLocation; title: string }
  | { kind: "asset"; target: AssetPageTarget }
  | { kind: "step" }
  | { kind: "quote" }
  | { kind: "browser"; url: string }
  | { kind: "reader" };

type PointerLike = { button?: number; metaKey: boolean; ctrlKey: boolean };

const isMac = () => typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

/** Cmd-click on macOS, Ctrl-click elsewhere, and middle-click ask for the system browser. */
export function wantsExternal(event: PointerLike, mac = isMac()): boolean {
  return event.button === 1 || (mac ? event.metaKey : event.ctrlKey);
}

const ASSET_ID = /"assetId"\s*:\s*"([^"\\]{1,200})"/u;

/** The asset an `asset_page_read` receipt read. The id is the first field of the host's JSON. */
function assetReadTarget(evidence: ChatEvidenceRef, loupistId: string): AssetPageTarget | null {
  if (evidence.kind !== "saved_context" || evidence.tool_name !== "asset_page_read") return null;
  const assetId = ASSET_ID.exec(evidence.excerpt)?.[1];
  return assetId ? { loupistId, assetId } : null;
}

export function evidenceTarget(evidence: ChatEvidenceRef, memoryRefs: readonly MemoryRef[] | undefined, loupistId: string): EvidenceTarget {
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
export function hostSourceDestination(evidence: ChatEvidenceRef, asset: AssetPageTarget): ChatDestination {
  const page: ChatDestination = { kind: "asset", target: asset };
  return evidence.url ? { kind: "evidence", evidence, fallback: page } : page;
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
    case "file":
      return "panel";
    case "evidence": {
      // Without the answer's Memory reads a citation is never a Memory target here.
      const target = evidenceTarget(to.evidence, undefined, "").kind;
      return target === "browser" || target === "reader" ? "panel" : null;
    }
    default:
      return null;
  }
}
