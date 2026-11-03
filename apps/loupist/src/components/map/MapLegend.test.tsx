import { render, screen, within } from "@testing-library/react";
import type { KnowledgeMapNode } from "../../contracts/knowledge";
import { LEGEND_MAX_ROWS, MapLegend, legendRows } from "./MapLegend";

let serial = 0;
const node = (kind: KnowledgeMapNode["kind"], entityKind?: string, extra: Partial<KnowledgeMapNode> = {}): KnowledgeMapNode => ({ id: `${kind}-${entityKind ?? "none"}-${serial++}`, label: kind, kind, entityKind, degree: 1, watched: false, held: false, ...extra });
const times = (count: number, make: () => KnowledgeMapNode) => Array.from({ length: count }, make);

it("lists only the marks in the drawn view, most drawn colour first, then Lens, watchlist, holdings and disagreeing sources", () => {
  const view = render(<MapLegend open nodes={[node("entity", "Product"), node("entity", "Product"), node("entity", "Organization"), node("lens_method")]} onOpenChange={() => {}} />);
  const items = within(screen.getByRole("group")).getAllByRole("listitem").map((item) => item.textContent);
  expect(items).toEqual(["Product & technology", "Company"]);
  // The legend names only the rings that are drawn: the dashed watchlist ring, the solid holdings ring, or both.
  view.rerender(<MapLegend open nodes={[node("entity", "Organization", { watched: true })]} onOpenChange={() => {}} />);
  expect(screen.getByText("On your watchlist")).toBeVisible();
  expect(screen.queryByText("In your holdings")).toBeNull();
  expect(screen.queryByText(/Lens:/)).toBeNull();
  expect(screen.queryByText("Sources disagree")).toBeNull();
  // The conflict line and dot on the canvas are explained only when a drawn relationship carries them.
  view.rerender(<MapLegend open conflicts nodes={[node("entity", "Organization", { watched: true }), node("entity", "Organization", { held: true })]} onOpenChange={() => {}} />);
  expect(screen.getByText("Dashed ring: watchlist · solid ring: holdings")).toBeVisible();
  expect(screen.getByText("Sources disagree").closest("li")?.querySelector(".is-conflict-line")).not.toBeNull();
  // Highlighted names are explained while a Lens is applied, without a Lens-shape row.
  view.rerender(<MapLegend open lensLines nodes={[node("entity", "Organization"), node("lens_method")]} onOpenChange={() => {}} />);
  expect(screen.getByText("Highlighted names: where this Lens was applied")).toBeVisible();
  expect(screen.queryByText(/Lens:/)).toBeNull();
  expect(screen.queryByText(/Solid line/)).toBeNull();
});

it("never shows more than six rows and explains drawn colours before folding marks into More", () => {
  const nodes = [
    ...times(5, () => node("entity", "Place")),
    ...times(2, () => node("entity", "Organization")), node("entity", "Organization", { watched: true }),
    ...times(2, () => node("entity", "Product")),
    node("entity", "Person"), node("entity", "Asset"), node("entity", "Event"), node("journal"), node("lens_method"),
  ];
  const { rows, more } = legendRows(nodes);
  expect(rows.length + (more.length ? 1 : 0)).toBeLessThanOrEqual(LEGEND_MAX_ROWS);
  // The drawn kinds come first, most drawn first; the least drawn colours and the other marks fold away.
  expect(rows.map((row) => row.id)).toEqual(["other", "company", "product", "person", "event"]);
  expect(more.map((row) => row.id)).toEqual(["market", "journal", "personal"]);
  // Disagreeing sources are explained whenever they are drawn, even when the card is full.
  const disputed = legendRows(nodes, true, true);
  expect(disputed.rows.map((row) => row.id)).toEqual(["other", "company", "product", "person", "conflict"]);
  expect(disputed.rows.length + 1).toBeLessThanOrEqual(LEGEND_MAX_ROWS);
  render(<MapLegend open nodes={nodes} onOpenChange={() => {}} />);
  expect(screen.getByText("More…")).toBeInTheDocument();
  expect(screen.getByText("Journal")).toBeInTheDocument();
});

it("keeps a small view whole", () => {
  const { rows, more } = legendRows([node("entity", "Organization"), node("entity", "Person"), node("lens_perspective")]);
  expect(rows.map((row) => row.id)).toEqual(["company", "person"]);
  expect(more).toEqual([]);
});
