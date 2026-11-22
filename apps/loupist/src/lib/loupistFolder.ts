import i18n from "@/i18n";
import { formatNumber } from "@/i18n/format";
import type { LoupistSummary } from "../types";

function folderParts(path: string) {
  const separator = /^(?:[a-z]:[/\\]|\\\\)/iu.test(path) ? /[/\\]/u : /\//u;
  const parts = path.split(separator);
  while (parts.length > 1 && !parts.at(-1)) parts.pop();
  return parts;
}

export function workspaceFolderName(path: string): string {
  return folderParts(path).at(-1) || path;
}

function shortFocus(loupist: LoupistSummary) {
  const chars = Array.from(loupist.philosophy.trim().replace(/\s+/gu, " "));
  return chars.length > 40 ? chars.slice(0, 39).join("").trimEnd() + "…" : chars.join("");
}

/** Duplicate names share the same human distinction wherever a profile is chosen. */
function loupistDistinction(loupist: LoupistSummary, loupists: readonly LoupistSummary[]): string {
  const peers = loupists.filter(other => other.name === loupist.name);
  if (peers.length < 2) return "";
  const focuses = peers.map(shortFocus);
  if (focuses.every(Boolean) && new Set(focuses).size === peers.length) return shortFocus(loupist);
  // Use the same ordinal across differently sorted lists. The stored ID stays private.
  const number = peers.filter(other => other.id < loupist.id).length + 1;
  return i18n.t("app:shell.profileNumber", { number: formatNumber(number) });
}

export function loupistIdentity(loupist: LoupistSummary, loupists: readonly LoupistSummary[]) {
  const distinction = loupistDistinction(loupist, loupists);
  return { label: distinction ? `${loupist.name} · ${distinction}` : loupist.name, distinction };
}

export function loupistOptionLabel(loupist: LoupistSummary, loupists: readonly LoupistSummary[]): string {
  return loupistIdentity(loupist, loupists).label;
}
