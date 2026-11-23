/** Room kept between a revealed cell and the edge it scrolls to. */
const MARGIN_PX = 12;

/** A saved table has row labels when its first column names its rows: some body cell there holds a letter. */
export const namesRows = (firstColumn: readonly string[], columns: number) =>
  columns > 1 && firstColumn.some(text => /\p{L}/u.test(text));

/**
 * Scroll the saved table holding `target` (a cited mark or its cell) sideways so
 * the cell is in view. A sticky row-label column stays visible: the cell lands
 * beside it, never under it. Vertical position is the caller's.
 */
export function revealTableCell(target: Element) {
  const scroller = target.closest<HTMLElement>(".saved-passage-table");
  const cell = target.closest<HTMLElement>("td, th");
  if (!scroller || !cell || !scroller.contains(cell) || scroller.scrollWidth <= scroller.clientWidth) return;
  const box = scroller.getBoundingClientRect();
  const label = cell.parentElement?.firstElementChild;
  const start = label instanceof HTMLElement && label !== cell && getComputedStyle(label).position === "sticky"
    ? label.getBoundingClientRect().right : box.left;
  const end = box.left + scroller.clientWidth;
  const rect = cell.getBoundingClientRect();
  let delta = Math.max(0, rect.right + MARGIN_PX - end);
  // A cell wider than the room left beside the labels shows its start.
  if (rect.left - delta < start) delta = rect.left - start;
  if (delta) scroller.scrollLeft = Math.max(0, scroller.scrollLeft + delta);
}
