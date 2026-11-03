import { relationName } from "./relations";
import type { KnowledgeMapEdge, KnowledgeMapNode } from "../../contracts/knowledge";
import { useTranslation } from "../../i18n";
import { formatNumber } from "../../i18n/format";
import { Button } from "../ui/button";
import { nodeCategory } from "./kinds";
import { entityLabel } from "./labels";
export function claimIdForEdge(edge: KnowledgeMapEdge) {
  const projectedSuffix = `:${JSON.stringify(edge.target)}`;
  return edge.id.endsWith(projectedSuffix) ? edge.id.slice(0, -projectedSuffix.length) : edge.id;
}
export function MapList({ nodes, edges, onSelect, onClaim }: { nodes: KnowledgeMapNode[]; edges: KnowledgeMapEdge[]; onSelect: (node: KnowledgeMapNode) => void; onClaim: (id: string) => void }) {
  const { t, i18n } = useTranslation("knowledge");
  const lookup = new Map(nodes.map((n) => [n.id, n]));
  const name = (node: KnowledgeMapNode) => entityLabel(node, i18n.language);
  return <div className="map-list"><table><caption>{t("map.listView")}</caption><thead><tr><th scope="col">{t("map.name")}</th><th scope="col">{t("map.kindLabel")}</th><th scope="col">{t("map.connectionsLabel")}</th></tr></thead><tbody>{nodes.map((node) => <tr key={node.id}><td><Button variant="link" onClick={() => onSelect(node)}>{name(node)}</Button></td><td>{t(`map.kind.${nodeCategory(node)}`)}</td><td>{formatNumber(node.degree)}</td></tr>)}</tbody></table>
    <table><caption>{t("map.relationshipsList")}</caption><thead><tr><th scope="col">{t("map.name")}</th><th scope="col">{t("map.connectionsLabel")}</th><th scope="col">{t("map.evidence")}</th></tr></thead><tbody>{edges.map((edge) => <tr key={edge.id}><td>{[edge.source, edge.target].map((id, index) => { const node = lookup.get(id); return node ? <span key={`${id}:${index}`}>{index ? " → " : ""}<Button variant="link" onClick={() => onSelect(node)}>{name(node)}</Button></span> : null; })}</td><td>{(edge.relation ? relationName(edge.relation, t) : undefined) ?? t(edge.kind === "applies_to" ? "map.seenThrough" : "map.kind.lens")}{edge.status === "disputed" ? <small>{t("map.kind.disputed")}</small> : null}</td><td>{edge.kind === "claim" ? <Button variant="link" onClick={() => onClaim(claimIdForEdge(edge))}>{t("map.keyFacts")}</Button> : null}</td></tr>)}</tbody></table>
  </div>;
}
