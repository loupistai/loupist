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

/** Show location only where identical folder names would make selection ambiguous. */
export function loupistParentPath(loupist: LoupistSummary, loupists: LoupistSummary[]): string | undefined {
  if (!loupists.some((other) => other.id !== loupist.id && other.name === loupist.name)) return undefined;
  const parent = folderParts(loupist.workspace_path).slice(0, -1).join("/");
  return parent || "/";
}

export function loupistOptionLabel(loupist: LoupistSummary, loupists: LoupistSummary[]): string {
  const parent = loupistParentPath(loupist, loupists);
  if (!parent) return loupist.name;
  const sameParent = loupists.some(other => other.id !== loupist.id && other.name === loupist.name && loupistParentPath(other, loupists) === parent);
  if (!sameParent) return `${loupist.name} · ${parent}`;
  const samePath = loupists.some(other => other.id !== loupist.id && other.name === loupist.name && other.workspace_path === loupist.workspace_path);
  return `${loupist.name} · ${loupist.workspace_path}${samePath ? ` · ${loupist.id}` : ""}`;
}
