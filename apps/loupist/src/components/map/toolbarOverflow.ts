import { useLayoutEffect, useState, type RefObject } from "react";
import { flushSync } from "react-dom";

/** A folded Scope segment stays measurable but hidden and inert while the Around menu carries its choices. */
export function foldProps(name: "scope", folded: boolean) {
  return folded ? { "data-overflow-item": name, "data-overflowed": true, inert: true } : { "data-overflow-item": name };
}

const px = (value: string) => Number.parseFloat(value) || 0;

/** Only Scope can collapse: Export and More always occupy their actual row width. */
function scopeNeedsMenu(bar: HTMLElement): boolean {
  if (!bar.clientWidth) return false;
  const style = getComputedStyle(bar);
  const fixed: number[] = [];
  let scope: number | undefined;
  for (const child of Array.from(bar.children) as HTMLElement[]) {
    const childStyle = getComputedStyle(child);
    if (child.dataset.overflowItem === "scope") {
      if (childStyle.display !== "none") scope = child.getBoundingClientRect().width;
      continue;
    }
    // Folded, Around shows only the focal name; measure it at the width it has beside the segment, so the fold does not flip back and forth.
    if (child.hasAttribute("data-map-around")) {
      const width = child.getBoundingClientRect().width;
      if (!child.hasAttribute("data-scope-folded")) child.dataset.unfoldedWidth = String(width);
      fixed.push(Math.max(width, px(child.dataset.unfoldedWidth ?? "")));
      continue;
    }
    if (child.hasAttribute("data-overflowed") || childStyle.display === "none" || childStyle.position === "absolute" || childStyle.position === "fixed") continue;
    fixed.push(child.classList.contains("map-search") ? px(childStyle.minWidth) : child.classList.contains("map-toolbar-spacer") ? 0 : child.getBoundingClientRect().width);
  }
  if (scope === undefined) return false;
  const total = fixed.reduce((sum, width) => sum + width, scope) + px(style.columnGap) * fixed.length;
  return total > bar.clientWidth - px(style.paddingLeft) - px(style.paddingRight) + 0.5;
}

/** Remeasure Scope after content, pane width or font changes. */
export function useScopeOverflow(ref: RefObject<HTMLElement | null>): boolean {
  const [folded, setFolded] = useState(false);
  const next = () => ref.current ? scopeNeedsMenu(ref.current) : false;
  // eslint-disable-next-line react-hooks/exhaustive-deps -- measures current row content; unchanged measurements bail out.
  useLayoutEffect(() => { setFolded(next()); });
  useLayoutEffect(() => {
    const bar = ref.current;
    if (!bar) return;
    let live = true;
    const update = () => { if (live) flushSync(() => setFolded(next())); };
    const observer = typeof ResizeObserver === "undefined" ? undefined : new ResizeObserver(update);
    observer?.observe(bar);
    void document.fonts?.ready.then(update, () => {});
    return () => { live = false; observer?.disconnect(); };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `next` reads only the ref.
  }, [ref]);
  return folded;
}
