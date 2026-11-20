import type { KnowledgeMapEdge, KnowledgeMapNode } from "../contracts/knowledge";

/** Language tag → the script its Map names prefer. Another locale adds a row here. */
const localScripts: Record<string, RegExp> = {
  ko: /[\u1100-\u11FF\u3130-\u318F\uAC00-\uD7A3]/u,
};
/** The name to draw. Korean uses the first Hangul alias the host selected; every other language uses the canonical name. */
export function entityLabel(node: Pick<KnowledgeMapNode, "label" | "localNames">, language: string) {
  const script = Object.entries(localScripts).find(([tag]) => language.toLowerCase() === tag || language.toLowerCase().startsWith(`${tag}-`))?.[1];
  return script ? node.localNames?.find(name => script.test(name)) ?? node.label : node.label;
}
/** Map search matches the canonical name and every host-selected local name. */
export function matchesMapQuery(node: Pick<KnowledgeMapNode, "label" | "localNames">, query: string) {
  const needle = query.toLocaleLowerCase();
  return [node.label, ...(node.localNames ?? [])].some(name => name.toLocaleLowerCase().includes(needle));
}
/**
 * Whether two ends are one thing: the same node, or names that share a spelling (label, local
 * name or displayed label). The Map carries no listing on a node, so a shared name counts as the
 * same asset. A claim between them is no relationship: a reported value names its subject twice.
 */
export function oneName(a: Pick<KnowledgeMapNode, "id" | "label" | "localNames">, b: Pick<KnowledgeMapNode, "id" | "label" | "localNames">, language: string) {
  if (a.id === b.id) return true;
  const names = (node: Pick<KnowledgeMapNode, "label" | "localNames">) => [node.label, ...(node.localNames ?? []), entityLabel(node, language)].map(name => name.trim().toLocaleLowerCase());
  const first = new Set(names(a));
  return names(b).some(name => first.has(name));
}
/** The World claim a drawn edge projects; the Map suffixes a claim's id with its target. */
/**
 * A relation the Map draws and counts: its two ends are different names. A value reported
 * about one name (`REPORTS`) links it to itself; that is no relationship, and the layout,
 * the canvas and every relationship count leave it out.
 */
export const isDrawnRelation = (edge: Pick<KnowledgeMapEdge, "source" | "target">) => edge.source !== edge.target;

export function claimIdForEdge(edge: KnowledgeMapEdge) {
  const projectedSuffix = `:${JSON.stringify(edge.target)}`;
  return edge.id.endsWith(projectedSuffix) ? edge.id.slice(0, -projectedSuffix.length) : edge.id;
}
