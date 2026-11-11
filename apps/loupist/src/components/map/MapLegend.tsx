import { useEffect, useRef, useState } from "react";
import type { KnowledgeMapNode } from "../../contracts/knowledge";
import { useTranslation } from "../../i18n";
import { entityCategories, nodeCategory } from "../../knowledge/mapKinds";

/** Rows the legend card shows at most; the rest fold into "More…". */
export const LEGEND_MAX_ROWS = 6;
const categoryOrder = ["company", "product", "technology", "industry", "person", "event", "market", "other"] as const satisfies readonly (typeof entityCategories)[number][];

/** `personal` says which rings are drawn: the dashed watchlist ring, the solid holdings ring, or both. */
export type LegendRow = { id: string; kind: "category" | "lens" | "lensLine" | "personal" | "conflict"; rings?: "watch" | "hold" | "both" };

/**
 * Rows for every mark in the drawn view. Colour rows (entity categories and
 * Journal) come first, most drawn first, so the colour a reader sees most is
 * always explained; then, while a Lens highlight is on, the highlighted names,
 * one row naming the dashed watchlist ring and the solid holdings ring and, when a drawn relationship has
 * sources that disagree, the red dashed line and dot. Past six rows, the red line
 * stays, then the drawn kinds' colours, most drawn first; the other marks and the
 * least drawn colours fold into a "More…" disclosure.
 */
export function legendRows(nodes: readonly KnowledgeMapNode[], conflicts = false, lensLines = false): { rows: LegendRow[]; more: LegendRow[] } {
  const counts = new Map<string, number>();
  for (const node of nodes) {
    const category = nodeCategory(node);
    if ((categoryOrder as readonly string[]).includes(category) || category === "journal") counts.set(category, (counts.get(category) ?? 0) + 1);
  }
  const order = [...categoryOrder, "journal"];
  const colours = [...counts.keys()].sort((a, b) => counts.get(b)! - counts.get(a)! || order.indexOf(a) - order.indexOf(b))
    .map((id): LegendRow => ({ id, kind: "category" }));
  // A held name draws the solid ring even when it is also watched.
  const watch = nodes.some((node) => node.watched && !node.held);
  const hold = nodes.some((node) => node.held);
  const marks: LegendRow[] = [
    ...(lensLines ? [{ id: "lensLine", kind: "lensLine" as const }] : []),
    ...(watch || hold ? [{ id: "personal", kind: "personal" as const, rings: watch && hold ? "both" as const : watch ? "watch" as const : "hold" as const }] : []),
    ...(conflicts ? [{ id: "conflict", kind: "conflict" as const }] : []),
  ];
  if (colours.length + marks.length <= LEGEND_MAX_ROWS) return { rows: [...colours, ...marks], more: [] };
  // One slot goes to the "More…" row itself. Disagreeing sources are always explained
  // when drawn; then every drawn colour, most drawn first, so no dot on the canvas goes
  // unexplained before a line or ring does; the other marks fill what is left.
  const ranked = [...marks.filter((row) => row.kind === "conflict"), ...colours, ...marks.filter((row) => row.kind !== "conflict")];
  const shown = new Set(ranked.slice(0, LEGEND_MAX_ROWS - 1));
  return { rows: [...colours, ...marks].filter((row) => shown.has(row)), more: [...colours, ...marks].filter((row) => !shown.has(row)) };
}

export function MapLegend({ open, nodes, conflicts = false, lensLines = false, onOpenChange }: {
  open: boolean;
  nodes: KnowledgeMapNode[];
  /** A drawn relationship has sources that disagree. */
  conflicts?: boolean;
  /** Highlighted names are where the selected Lens was applied. */
  lensLines?: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { t } = useTranslation("knowledge");
  const { rows, more } = legendRows(nodes, conflicts, lensLines);
  const ref = useRef<HTMLDetailsElement>(null);
  const [overflowing, setOverflowing] = useState(false);
  useEffect(() => {
    const element = ref.current;
    if (!element || !open) { setOverflowing(false); return; }
    const update = () => setOverflowing(element.scrollHeight - element.scrollTop - element.clientHeight > 1);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    element.addEventListener("scroll", update);
    return () => { observer.disconnect(); element.removeEventListener("scroll", update); };
  }, [open, nodes, conflicts, lensLines]);
  const row = (item: LegendRow) => {
    if (item.kind === "lens") return <li key={item.id}><span className="map-legend-glyphs" aria-hidden="true"><i className="map-swatch is-perspective" /><i className="map-swatch is-method" /><i className="map-swatch is-principle" /></span>{t("map.legendLens")}</li>;
    if (item.kind === "lensLine") return <li key={item.id}><span className="map-legend-glyphs" aria-hidden="true"><i className="map-swatch is-lens-line" /></span>{t("map.legendLensLine")}</li>;
    if (item.kind === "conflict") return <li key={item.id}><span className="map-legend-glyphs" aria-hidden="true"><i className="map-swatch is-conflict-line" /><i className="map-swatch is-conflict" /></span>{t("map.legendConflict")}</li>;
    if (item.kind === "personal") return <li key={item.id}><span className="map-legend-glyphs" aria-hidden="true">
      {item.rings !== "hold" ? <i className="map-swatch is-watching" /> : null}{item.rings !== "watch" ? <i className="map-swatch is-holding" /> : null}
    </span>{t(item.rings === "both" ? "map.legendWatchHold" : item.rings === "watch" ? "map.legendWatch" : "map.legendHold")}</li>;
    return <li key={item.id}><span className="map-legend-glyphs" aria-hidden="true"><i className={`map-swatch is-${item.id}`} /></span>{t(`map.kind.${item.id as (typeof categoryOrder)[number] | "journal"}`)}</li>;
  };
  return <details ref={ref} className={`map-legend${overflowing ? " is-overflowing" : ""}`} open={open} onToggle={(event) => {
    const next = event.currentTarget.open;
    if (next !== open) onOpenChange(next);
  }}><summary>{t("map.legend")}</summary><ul>
    {rows.map(row)}
    {more.length ? <li className="map-legend-more"><details><summary>{t("map.legendMore")}</summary><ul>{more.map(row)}</ul></details></li> : null}
  </ul></details>;
}
