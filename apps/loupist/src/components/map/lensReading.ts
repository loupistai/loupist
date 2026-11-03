import type { KnowledgeMapEdge, KnowledgeMapNode, KnowledgeRecordSummary } from "../../contracts/knowledge";

/** Host seed in `ensure_standing_lens`. Display-only; stored titles stay as written. */
export const HOST_STANDING_TITLE = "Standing philosophy";
/** Empty philosophy body. It is a placeholder, not a learned insight. */
export const HOST_STANDING_PLACEHOLDER = "This Loupist has not recorded a standing philosophy.";

const LENS_KINDS = new Set(["lens_perspective", "lens_principle", "lens_method"]);

export function isLensNode(node: { kind: string }): boolean {
  return LENS_KINDS.has(node.kind);
}

type StandingCatalog = KnowledgeRecordSummary & { isStanding?: boolean };

function catalogLens(lenses: readonly KnowledgeRecordSummary[], node: KnowledgeMapNode) {
  return node.recordId ? lenses.find(lens => lens.recordId === node.recordId) : undefined;
}

function standingFlag(lens: KnowledgeRecordSummary | undefined): boolean | undefined {
  if (!lens || !("isStanding" in lens)) return undefined;
  const value = (lens as StandingCatalog).isStanding;
  return typeof value === "boolean" ? value : undefined;
}

/** Standing selection. Catalog `isStanding` wins; otherwise a perspective, or the exact host title when no row says otherwise. */
export function isBasePerspective(node: KnowledgeMapNode, lenses: readonly KnowledgeRecordSummary[]): boolean {
  if (node.kind !== "lens_perspective") return false;
  const catalog = catalogLens(lenses, node);
  const standing = standingFlag(catalog);
  if (standing === false) return false;
  if (catalog?.lensKind === "method" || catalog?.lensKind === "principle") return false;
  if (standing === true || catalog?.lensKind === "perspective") return true;
  return (catalog?.title ?? node.label) === HOST_STANDING_TITLE;
}

/** Localized default only for the untouched host title. A chosen title stays. */
export function lensDisplayTitle(node: KnowledgeMapNode, lenses: readonly KnowledgeRecordSummary[], defaultTitle: string): string {
  const catalog = catalogLens(lenses, node);
  const stored = catalog?.title ?? node.label;
  if (isBasePerspective(node, lenses) && stored === HOST_STANDING_TITLE && node.label === HOST_STANDING_TITLE) return defaultTitle;
  return isBasePerspective(node, lenses) && stored !== HOST_STANDING_TITLE ? stored : node.label;
}

export function isStandingPlaceholder(text: string | undefined): boolean {
  const trimmed = text?.replace(/^Use:\s*/i, "").trim() ?? "";
  return trimmed === HOST_STANDING_PLACEHOLDER;
}

/** Entity names an `applies_to` edge already links to this Lens. No inferred neighbors. */
export function lensAppliedEntities(nodes: readonly KnowledgeMapNode[], edges: readonly KnowledgeMapEdge[], lensId: string): KnowledgeMapNode[] {
  const byId = new Map(nodes.map(node => [node.id, node]));
  const seen = new Set<string>();
  const applied: KnowledgeMapNode[] = [];
  for (const edge of edges) {
    if (edge.kind !== "applies_to") continue;
    const otherId = edge.source === lensId ? edge.target : edge.target === lensId ? edge.source : undefined;
    if (!otherId || seen.has(otherId)) continue;
    const other = byId.get(otherId);
    if (other?.kind !== "entity") continue;
    seen.add(otherId);
    applied.push(other);
  }
  return applied;
}
