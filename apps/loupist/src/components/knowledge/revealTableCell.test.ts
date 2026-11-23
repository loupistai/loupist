import { namesRows, revealTableCell } from "./revealTableCell";

/** A 300 px wide saved table: a 120 px row-label column, then figures 100 px apart, scrolled by `scrollLeft`. */
function table({ sticky = true, columns = 6, width = 100 } = {}) {
  const scroller = document.createElement("div");
  scroller.className = "saved-passage-table";
  scroller.innerHTML = `<table data-row-labels><tbody><tr><td>Income from operations</td>${Array.from({ length: columns }, (_, index) => `<td><mark>${index}</mark></td>`).join("")}</tr></tbody></table>`;
  document.body.append(scroller);
  Object.defineProperty(scroller, "clientWidth", { value: 300 });
  Object.defineProperty(scroller, "scrollWidth", { value: 120 + columns * width });
  scroller.getBoundingClientRect = () => new DOMRect(0, 0, 300, 40);
  const cells = [...scroller.querySelectorAll("td")];
  if (sticky) cells[0]!.style.position = "sticky";
  cells.forEach((cell, index) => {
    cell.getBoundingClientRect = () => {
      const left = index === 0 ? (sticky ? 0 : -scroller.scrollLeft) : 120 + (index - 1) * width - scroller.scrollLeft;
      return new DOMRect(left, 0, index === 0 ? 120 : width, 40);
    };
  });
  return { scroller, marks: [...scroller.querySelectorAll("mark")] };
}

afterEach(() => { document.body.innerHTML = ""; });

it("brings a cited figure past the right edge into view beside the row labels", () => {
  const { scroller, marks } = table();
  revealTableCell(marks[3]!);
  // Figure 3 spans 420–520: it ends 12 px inside the 300 px edge.
  expect(scroller.scrollLeft).toBe(232);
  expect(marks[3]!.closest("td")!.getBoundingClientRect().left).toBeGreaterThanOrEqual(120);
});

it("moves a figure hidden under the sticky labels back beside them", () => {
  const { scroller, marks } = table();
  scroller.scrollLeft = 300;
  revealTableCell(marks[1]!);
  expect(scroller.scrollLeft).toBe(100);
  expect(marks[1]!.closest("td")!.getBoundingClientRect().left).toBe(120);
});

it("shows the start of a figure wider than the room beside the labels", () => {
  const { marks } = table({ width: 400 });
  revealTableCell(marks[2]!);
  expect(marks[2]!.closest("td")!.getBoundingClientRect().left).toBe(120);
});

it("scrolls under no labels when the first column is not sticky, and leaves a table that fits", () => {
  const plain = table({ sticky: false });
  revealTableCell(plain.marks[3]!);
  expect(plain.scroller.scrollLeft).toBe(232);
  document.body.innerHTML = "";
  const fits = table({ columns: 1 });
  revealTableCell(fits.marks[0]!);
  expect(fits.scroller.scrollLeft).toBe(0);
});

it("finds row labels only in a first column that names its rows", () => {
  expect(namesRows(["Income from operations", "Operating margin"], 6)).toBe(true);
  expect(namesRows(["영업이익", "1,409"], 3)).toBe(true);
  expect(namesRows(["2025", "2024"], 3)).toBe(false);
  expect(namesRows(["Revenue"], 1)).toBe(false);
});
