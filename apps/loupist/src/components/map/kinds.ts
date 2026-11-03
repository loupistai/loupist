import type { KnowledgeMapNode } from "../../contracts/knowledge";

export const entityCategories = ["company", "person", "product", "market", "event", "other"] as const;
export type EntityCategory = typeof entityCategories[number];
/** Core World kinds. Anything else, including place, policy and entity, stays visible as other. */
const categories: Record<string, EntityCategory> = {
  organization: "company",
  person: "person",
  product: "product",
  asset: "market",
  event: "event",
  metric: "event",
};
export function entityCategory(kind?: string): EntityCategory { return categories[kind?.trim().toLowerCase() ?? ""] ?? "other"; }
export function nodeCategory(node: KnowledgeMapNode) {
  if (node.kind === "journal") return "journal";
  if (node.kind === "entity") return node.categoryOverride ?? entityCategory(node.entityKind);
  return node.kind === "lens_method" ? "method" : node.kind === "lens_principle" ? "principle" : "perspective";
}
/** A core World Metric. An extension type is never a metric, even when its category is event. */
export function isMetricNode(node: Pick<KnowledgeMapNode, "kind" | "entityKind" | "categoryOverride" | "kindLabels">) {
  return node.kind === "entity" && !node.categoryOverride && !node.kindLabels && node.entityKind?.trim().toLowerCase() === "metric";
}
/** Canvas fill, and the swatch of a chip or palette row naming that node outside the Map, so the two match.
 *  The legend, filters and the Map's own lists keep using nodeCategory, so a metric stays in Event & metric. */
export function nodeColorKey(node: KnowledgeMapNode) {
  return isMetricNode(node) ? "metric" : nodeCategory(node);
}
/** Inspector kicker: the extension label in the UI language, then the category label. */
export function extensionKindLabel(labels: { en: string; ko?: string } | undefined, language: string, categoryLabel: string) {
  if (language.toLowerCase().startsWith("ko") && labels?.ko) return labels.ko;
  return labels?.en || categoryLabel;
}
export function nodeRadius(node: KnowledgeMapNode) {
  if (isMetricNode(node)) return 4;
  return node.kind === "journal" ? 6 : node.kind === "entity" ? Math.max(5, Math.min(16, 4 + 2 * Math.sqrt(node.degree))) : 8;
}
