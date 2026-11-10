import { useLayoutEffect, useState, type RefObject } from "react";
import { flushSync } from "react-dom";

/**
 * What gives way when the Map toolbar cannot hold everything on one row, lowest
 * priority first: the spec's toolbar order read from the right. Actions fold into
 * "More"; last, the scope segment turns into its compact Scope menu button, as it
 * does on a narrow Map.
 */
export const MAP_OVERFLOW_ORDER = ["list", "export", "fit", "replay", "library", "scope"] as const;
export type MapOverflowItem = (typeof MAP_OVERFLOW_ORDER)[number];

/**
 * Marks a foldable toolbar action. While folded it stays mounted (so a running
 * export keeps its state) but leaves the row, the tab order and the accessibility tree.
 */
export function foldProps(name: MapOverflowItem, folded: boolean) {
  return folded ? { "data-overflow-item": name, "data-overflowed": true, inert: true } : { "data-overflow-item": name };
}

/**
 * Widths in px: the room inside the toolbar, the gap between items, the items that
 * always stay, the foldable ones and the More button (an icon button, as wide as
 * the Scope menu button that replaces a folded scope segment).
 */
export type ToolbarWidths = { available: number; gap: number; fixed: readonly number[]; items: Partial<Record<MapOverflowItem, number>>; more: number };

/**
 * The fewest items, taken in MAP_OVERFLOW_ORDER, that must give way for the rest
 * to fit. The scope segment folds only when folding every action is not enough;
 * the room it frees brings back as many actions as fit.
 */
export function foldedActions({ available, gap, fixed, items, more }: ToolbarWidths): MapOverflowItem[] {
  const actions = MAP_OVERFLOW_ORDER.filter((name) => name !== "scope" && items[name] !== undefined);
  const scope = items.scope;
  for (const scopeFolded of scope === undefined ? [false] : [false, true]) {
    for (let count = 0; count <= actions.length; count++) {
      const widths = [...fixed, ...actions.slice(count).map((name) => items[name]!), ...(count ? [more] : []), ...(scope === undefined ? [] : [scopeFolded ? more : scope])];
      const total = widths.reduce((sum, width) => sum + width, 0) + gap * Math.max(0, widths.length - 1);
      if (total <= available + 0.5) return [...actions.slice(0, count), ...(scopeFolded ? ["scope" as const] : [])];
    }
  }
  return [...actions, ...(scope === undefined ? [] : ["scope" as const])];
}

const px = (value: string) => Number.parseFloat(value) || 0;

/**
 * Reads the toolbar's direct children. Foldable actions stay mounted while folded
 * (hidden and out of the row), so their width is known before they come back.
 */
export function measureToolbar(bar: HTMLElement): ToolbarWidths | undefined {
  if (!bar.clientWidth) return undefined;
  const style = getComputedStyle(bar);
  const fixed: number[] = [];
  const items: ToolbarWidths["items"] = {};
  let more = 0;
  for (const child of Array.from(bar.children) as HTMLElement[]) {
    const width = child.getBoundingClientRect().width;
    const childStyle = getComputedStyle(child);
    const name = child.dataset.overflowItem as MapOverflowItem | undefined;
    // A narrow Map hides the scope segment itself; then it is not in play.
    if (name && (MAP_OVERFLOW_ORDER as readonly string[]).includes(name)) { if (childStyle.display !== "none") items[name] = width; continue; }
    if (child.hasAttribute("data-map-more")) { more = width; continue; }
    // While the segment is in play, its menu button is what a folded segment becomes, counted as `more`.
    if (child.hasAttribute("data-map-scope-menu") && items.scope !== undefined) continue;
    if (childStyle.display === "none" || childStyle.position === "absolute" || childStyle.position === "fixed") continue;
    // The search box gives way down to its minimum width; the spacer only adds a gap.
    fixed.push(child.classList.contains("map-search") ? px(childStyle.minWidth) : child.classList.contains("map-toolbar-spacer") ? 0 : width);
  }
  return { available: bar.clientWidth - px(style.paddingLeft) - px(style.paddingRight), gap: px(style.columnGap), fixed, items, more };
}

const same = (a: readonly MapOverflowItem[], b: readonly MapOverflowItem[]) => a.length === b.length && a.every((name, index) => name === b[index]);

/** Which actions are folded into "More" right now; rechecked after every render, on resize and once fonts load. */
export function useToolbarOverflow(ref: RefObject<HTMLElement | null>): readonly MapOverflowItem[] {
  const [folded, setFolded] = useState<readonly MapOverflowItem[]>([]);
  const next = () => {
    const widths = ref.current ? measureToolbar(ref.current) : undefined;
    return widths ? foldedActions(widths) : [];
  };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- measures after every render; the update bails out when nothing folds differently.
  useLayoutEffect(() => {
    const value = next();
    setFolded((current) => same(current, value) ? current : value);
  });
  useLayoutEffect(() => {
    const bar = ref.current;
    if (!bar) return;
    let live = true;
    // Resizing settles before paint, so the row never shows clipped actions.
    const update = () => { if (!live) return; const value = next(); flushSync(() => setFolded((current) => same(current, value) ? current : value)); };
    const observer = typeof ResizeObserver === "undefined" ? undefined : new ResizeObserver(update);
    observer?.observe(bar);
    void document.fonts?.ready.then(update, () => {});
    return () => { live = false; observer?.disconnect(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `next` reads only the ref.
  }, [ref]);
  return folded;
}
