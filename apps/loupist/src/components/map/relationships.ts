import type { KnowledgeMap, KnowledgeMapEdge, KnowledgeMapNode } from "../../contracts/knowledge";
import { nodeCategory } from "./kinds";

/** Groups of the inspector's Relationships view (map spec §2.3), in display order. */
export type RelationshipGroupId = "peers" | "suppliers" | "customers" | "ownership" | "other";
/** `lens`: a Lens entry the focal name is seen through (an `applies_to` edge), drawn dotted. */
export type RelationshipRow = { edge: KnowledgeMapEdge; node: KnowledgeMapNode; relation: string; lens: boolean };
export type RelationshipGroup = { id: RelationshipGroupId; rows: RelationshipRow[] };

const OWNERSHIP = new Set(["OWNS", "SUBSIDIARY_OF", "ACQUIRED", "INVESTED_IN", "PARTNERS_WITH"]);
const ORDER: RelationshipGroupId[] = ["peers", "suppliers", "customers", "ownership", "other"];

/** Which group a claim lands in, seen from the focal node. */
export function relationshipGroup(edge: KnowledgeMapEdge, focalId: string): RelationshipGroupId {
  const relation = (edge.relation ?? "").toUpperCase();
  const outgoing = edge.source === focalId;
  if (relation === "COMPETES_WITH") return "peers";
  if ((relation === "SUPPLIES" && !outgoing) || (relation === "DEPENDS_ON" && outgoing)) return "suppliers";
  if ((relation === "SUPPLIES" && outgoing) || (relation === "CUSTOMER_OF" && !outgoing)) return "customers";
  if (OWNERSHIP.has(relation)) return "ownership";
  return "other";
}

/** Only companies and markets get the diagram; other kinds keep Facts only. */
export const hasRelationshipsView = (node: KnowledgeMapNode) => node.kind === "entity" && ["company", "market"].includes(nodeCategory(node));

/**
 * The focal node's claim relationships grouped by role: weight first when the
 * edge carries one, then the counterpart's degree, then its name. Empty groups
 * are left out. Lens entries applied to the focal name (`applies_to`) close
 * Other, after its claims; unlinked edges are not relationships.
 */
export function relationshipGroups(map: KnowledgeMap, focalId: string): RelationshipGroup[] {
  const nodes = new Map(map.nodes.map((node) => [node.id, node]));
  const groups = new Map<RelationshipGroupId, RelationshipRow[]>();
  const seen = new Set<string>();
  for (const edge of map.edges) {
    const lens = edge.kind === "applies_to" && edge.target === focalId && edge.source !== focalId;
    if (!lens && (edge.kind !== "claim" || (edge.source !== focalId && edge.target !== focalId) || edge.source === edge.target)) continue;
    const node = nodes.get(edge.source === focalId ? edge.target : edge.source);
    if (!node) continue;
    const group = lens ? "other" : relationshipGroup(edge, focalId);
    // One row per counterpart and role; the strongest claim represents it.
    const key = `${lens ? "lens" : group}:${node.id}`;
    if (seen.has(key)) continue;
    seen.add(key);
    groups.set(group, [...(groups.get(group) ?? []), { edge, node, relation: lens ? "" : (edge.relation ?? "").toUpperCase(), lens }]);
  }
  return ORDER.flatMap((id) => {
    const rows = groups.get(id);
    if (!rows?.length) return [];
    rows.sort((a, b) => Number(a.lens) - Number(b.lens) || (b.edge.weight ?? 0) - (a.edge.weight ?? 0) || b.node.degree - a.node.degree || a.node.label.localeCompare(b.node.label));
    return [{ id, rows }];
  });
}

/** Stroke width 1.5–3 px by normalized weight, only when weights exist in the group. */
export function strokeWidth(rows: readonly RelationshipRow[], row: RelationshipRow) {
  const weights = rows.map((item) => item.edge.weight).filter((weight): weight is number => typeof weight === "number");
  if (!weights.length || row.edge.weight === undefined) return 1.5;
  const max = Math.max(...weights), min = Math.min(...weights);
  return max === min ? 1.5 : 1.5 + 1.5 * ((row.edge.weight - min) / (max - min));
}
