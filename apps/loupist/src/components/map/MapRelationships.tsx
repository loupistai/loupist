import { useEffect, useRef, useState } from "react";
import { useTranslation } from "../../i18n";
import { formatNumber } from "../../i18n/format";
import type { KnowledgeMap, KnowledgeMapNode } from "../../contracts/knowledge";
import { nodeCategory } from "../../knowledge/mapKinds";
import { entityLabel } from "../../knowledge/mapItems";
import { relationName } from "./relations";
import { relationshipGroups, strokeWidth, type RelationshipGroup, type RelationshipRow } from "./relationships";

const PER_GROUP = 6;
const CHIP_CHARS = 16;

/**
 * A counterpart chip: the category swatch and a name cut at 16 characters (full
 * name in the tooltip). Its left rule carries the claim's weight (1.5–3 px) and
 * turns dashed when the claim is disputed and dotted for a Lens entry, standing
 * in for the line on the canvas.
 */
function Chip({ row, group, onSelect, onPoint }: { row: RelationshipRow; group?: RelationshipGroup; onSelect: (node: KnowledgeMapNode) => void; onPoint?: (edgeId?: string) => void }) {
  const { i18n } = useTranslation("knowledge");
  const name = entityLabel(row.node, i18n.language);
  const short = [...name].length > CHIP_CHARS ? `${[...name].slice(0, CHIP_CHARS).join("")}…` : name;
  return <button type="button" className="map-relationship-chip" title={name} data-status={row.edge.status === "disputed" ? "disputed" : undefined} data-lens={row.lens || undefined}
    style={group ? { borderLeftWidth: `${strokeWidth(group.rows, row)}px` } : undefined} onClick={() => onSelect(row.node)}
    onPointerEnter={() => onPoint?.(row.edge.id)} onPointerLeave={() => onPoint?.()} onFocus={() => onPoint?.(row.edge.id)} onBlur={() => onPoint?.()}>
    <i className={`map-swatch is-${nodeCategory(row.node)}`} aria-hidden="true" /><span>{short}</span>
  </button>;
}

/** The arrow between a block of counterparts and the focal box, pointing the way the goods or services flow. */
function FlowArrow() {
  return <svg className="map-relationship-arrow" viewBox="0 0 12 22" width="12" height="22" aria-hidden="true">
    <line x1="6" y1="0" x2="6" y2="16" />
    <path d="M1 15 L6 22 L11 15 Z" />
  </svg>;
}

function useFold(group?: RelationshipGroup) {
  const [open, setOpen] = useState(false);
  const rows = group ? (open ? group.rows : group.rows.slice(0, PER_GROUP)) : [];
  const more = group ? group.rows.length - rows.length : 0;
  return { rows, more, expand: () => setOpen(true) };
}

function More({ count, onClick }: { count: number; onClick: () => void }) {
  const { t } = useTranslation("knowledge");
  return count > 0 ? <button type="button" className="map-relationship-more" onClick={onClick}>{t("map.relationshipsView.more", { formattedCount: formatNumber(count) })}</button> : null;
}

/**
 * The inspector's Relationships view (map spec §2.3, B3): suppliers flow into
 * the focal name and on to customers, peers sit above without arrows, and
 * ownership and everything else are listed. A chip makes that name the subject;
 * pointing at one (hover or focus) widens its line on the canvas.
 */
export function MapRelationships({ map, node, onSelect, onPoint }: { map: KnowledgeMap; node: KnowledgeMapNode; onSelect: (node: KnowledgeMapNode) => void; onPoint?: (edgeId?: string) => void }) {
  const { t, i18n } = useTranslation("knowledge");
  // A chip that disappears under the pointer (Facts view, another subject) leaves no line widened.
  const point = useRef(onPoint);
  point.current = onPoint;
  useEffect(() => () => point.current?.(), []);
  const groups = relationshipGroups(map, node.id);
  const byId = (id: RelationshipGroup["id"]) => groups.find((group) => group.id === id);
  const peers = useFold(byId("peers"));
  const suppliers = useFold(byId("suppliers"));
  const customers = useFold(byId("customers"));
  const ownership = useFold(byId("ownership"));
  const other = useFold(byId("other"));
  if (!groups.length) return <p className="map-muted">{t("map.relationshipsView.none")}</p>;
  const flow = byId("suppliers") || byId("customers");
  const listed = (id: "ownership" | "other", fold: ReturnType<typeof useFold>) => {
    const group = byId(id);
    if (!group) return null;
    return <section className="map-relationship-group" aria-label={t(`map.relationshipsView.${id}`)}>
      <h4>{t(`map.relationshipsView.${id}`)}</h4>
      <ul>{fold.rows.filter((row) => !row.lens).map((row) => <li key={row.edge.id}><span className="map-relationship-relation">{relationName(row.relation, t)}</span><Chip row={row} onSelect={onSelect} onPoint={onPoint} /></li>)}
        {/* The Lens entries the name is seen through share one line. */}
        {fold.rows.some((row) => row.lens) ? <li className="map-relationship-lenses"><span className="map-relationship-relation">{t("map.seenThrough")}</span>
          {fold.rows.filter((row) => row.lens).map((row) => <Chip key={row.edge.id} row={row} onSelect={onSelect} onPoint={onPoint} />)}</li> : null}</ul>
      <More count={fold.more} onClick={fold.expand} />
    </section>;
  };
  return <div className="map-relationships">
    {byId("peers") ? <section className="map-relationship-group" aria-label={t("map.relationshipsView.peers")}>
      <h4>{t("map.relationshipsView.peers")}</h4>
      <div className="map-relationship-chips">{peers.rows.map((row) => <Chip key={row.edge.id} row={row} onSelect={onSelect} onPoint={onPoint} />)}<More count={peers.more} onClick={peers.expand} /></div>
    </section> : null}
    {/* Suppliers flow down into the focal box and on to customers: one column fits a narrow inspector,
        and the arrows always meet the box. */}
    {flow ? <section className="map-relationship-flow" aria-label={`${t("map.relationshipsView.suppliers")} · ${t("map.relationshipsView.customers")}`}>
      {byId("suppliers") ? <div className="map-relationship-side">
        <h4>{t("map.relationshipsView.suppliersOf", { name: entityLabel(node, i18n.language) })}</h4>
        <ul>{suppliers.rows.map((row) => <li key={row.edge.id}><Chip row={row} group={byId("suppliers")} onSelect={onSelect} onPoint={onPoint} /></li>)}</ul>
        <More count={suppliers.more} onClick={suppliers.expand} />
        <FlowArrow />
      </div> : null}
      <div className="map-relationship-focal" aria-current="true" title={entityLabel(node, i18n.language)}>{entityLabel(node, i18n.language)}</div>
      {byId("customers") ? <div className="map-relationship-side is-customers">
        <FlowArrow />
        <h4>{t("map.relationshipsView.customersOf", { name: entityLabel(node, i18n.language) })}</h4>
        <ul>{customers.rows.map((row) => <li key={row.edge.id}><Chip row={row} group={byId("customers")} onSelect={onSelect} onPoint={onPoint} /></li>)}</ul>
        <More count={customers.more} onClick={customers.expand} />
      </div> : null}
    </section> : null}
    {listed("ownership", ownership)}
    {listed("other", other)}
  </div>;
}
