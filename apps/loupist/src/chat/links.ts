import type { ChatEvidenceRef, ChatMessage } from "../types";

/** How an answer link asks to be opened: in the panel's browser (default) or the system browser. */
export type OpenLinkOptions = { external?: boolean };

/** Compare pages, not fragments or a trailing slash. */
function pageKey(url: string) {
  try {
    const parsed = new URL(url);
    parsed.hash = "";
    return `${parsed.origin}${parsed.pathname.replace(/\/+$/u, "")}${parsed.search}`;
  } catch {
    return null;
  }
}

/**
 * The retained source in this chat for a link, if any (side-panel spec X4):
 * a link to a page the Loupist already kept opens that retained copy in Evidence.
 */
export function retainedEvidenceFor(messages: readonly ChatMessage[], url: string): ChatEvidenceRef | undefined {
  const key = pageKey(url);
  if (!key) return undefined;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const match = messages[index]!.evidence_refs?.find((ref) => ref.kind === "original" && ref.url && pageKey(ref.url) === key);
    if (match) return match;
  }
  return undefined;
}
