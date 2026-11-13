import { useState } from "react";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { KnowledgeMap, KnowledgeMapEdge, KnowledgeMapNode } from "../../contracts/knowledge";
import { MapInspector } from "./MapInspector";
import { hasRelationshipsView, relationshipGroups, strokeWidth } from "./relationships";

const company = (id: string, label: string, degree = 1): KnowledgeMapNode => ({ id, label, kind: "entity", entityKind: "Organization", degree, watched: false, held: false });
const claim = (id: string, source: string, target: string, relation: string, extra: Partial<KnowledgeMapEdge> = {}): KnowledgeMapEdge => ({ id, kind: "claim", source, target, relation, ...extra });

const nodes: KnowledgeMapNode[] = [
  company("tsmc", "TSMC", 9), company("asml", "ASML", 4), company("amat", "Applied Materials", 3), company("apple", "Apple", 6),
  company("nvidia", "NVIDIA", 5), company("samsung", "Samsung Foundry", 2), company("visera", "VisEra"), company("hsinchu", "Hsinchu"),
  { id: "lens-1", recordId: "rec-cash", label: "Cash first", kind: "lens_principle", degree: 3, watched: false, held: false },
  { id: "lens-2", recordId: "rec-moat", label: "Moat check", kind: "lens_method", degree: 1, watched: false, held: false },
];
const map = {
  nodes,
  edges: [
    claim("c1", "asml", "tsmc", "SUPPLIES", { weight: 1 }),
    claim("c2", "tsmc", "amat", "DEPENDS_ON", { weight: 3 }),
    claim("c3", "tsmc", "apple", "SUPPLIES"),
    claim("c4", "nvidia", "tsmc", "CUSTOMER_OF", { status: "disputed" }),
    claim("c5", "tsmc", "samsung", "COMPETES_WITH"),
    claim("c6", "tsmc", "visera", "OWNS"),
    claim("c7", "tsmc", "hsinchu", "LOCATED_IN"),
    { id: "lens", kind: "applies_to", source: "lens-1", target: "tsmc", weight: 9 },
    { id: "lens-moat", kind: "applies_to", source: "lens-2", target: "tsmc" },
    // A Lens entry applied to another name is not TSMC's.
    { id: "lens-apple", kind: "applies_to", source: "lens-1", target: "apple" },
  ],
  worldRevisionId: "rev",
} as KnowledgeMap;

describe("relationship groups", () => {
  it("places counterparts by the direction of dependency and closes Other with the Lens entries applied to the name", () => {
    const groups = relationshipGroups(map, "tsmc");
    expect(groups.map((group) => [group.id, group.rows.map((row) => row.node.id)])).toEqual([
      ["peers", ["samsung"]],
      // Weight first: Applied Materials (3) before ASML (1).
      ["suppliers", ["amat", "asml"]],
      // No weights: the better-connected counterpart first.
      ["customers", ["apple", "nvidia"]],
      ["ownership", ["visera"]],
      // Claims first, even against a heavier Lens link.
      ["other", ["hsinchu", "lens-1", "lens-2"]],
    ]);
    expect(groups.at(-1)!.rows.map((row) => row.lens)).toEqual([false, true, true]);
    expect(relationshipGroups(map, "apple").find((group) => group.id === "other")?.rows.map((row) => row.node.id)).toEqual(["lens-1"]);
    const suppliers = groups.find((group) => group.id === "suppliers")!;
    expect(strokeWidth(suppliers.rows, suppliers.rows[0]!)).toBe(3);
    expect(strokeWidth(suppliers.rows, suppliers.rows[1]!)).toBe(1.5);
  });

  it("is offered for companies and markets only", () => {
    expect(hasRelationshipsView(company("x", "X"))).toBe(true);
    expect(hasRelationshipsView({ ...company("p", "Person"), entityKind: "Person" })).toBe(false);
  });
});

function Inspector({ bridge }: { bridge: MockLoupistBridge }) {
  const [node, setNode] = useState(nodes[0]!);
  return <>
    <button type="button" onClick={() => setNode(nodes[4]!)}>Select NVIDIA on the canvas</button>
    <MapInspector node={node} map={map} bridge={bridge} location={{ loupistId: "loupist", surface: "world" }} providedGraph={{ nodes: [], edges: [] } as never}
      onClose={vi.fn()} onSelect={setNode} onOpen={vi.fn()} onAsk={vi.fn()} />
  </>;
}

it("walks from name to name through the Relationships view and back, and a selection elsewhere starts over", async () => {
  const user = userEvent.setup();
  render(<Inspector bridge={new MockLoupistBridge({ delay_ms: 0 })} />);
  await user.click(screen.getByRole("button", { name: "Relationships" }));
  const suppliers = screen.getByRole("region", { name: "Suppliers · Customers" });
  expect(within(suppliers).getByText("TSMC")).toBeVisible();
  expect(screen.getByRole("region", { name: "Peers" })).toHaveTextContent("Samsung Foundry");
  await user.click(within(suppliers).getByRole("button", { name: "Apple" }));
  expect(screen.getByRole("heading", { level: 2, name: "Apple" })).toBeVisible();
  // The view stays on Relationships while walking.
  expect(screen.getByRole("button", { name: "Relationships" })).toHaveAttribute("aria-pressed", "true");
  await user.click(screen.getByRole("button", { name: "Back" }));
  expect(screen.getByRole("heading", { level: 2, name: "TSMC" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "Back" })).not.toBeInTheDocument();
  await user.click(within(screen.getByRole("region", { name: "Suppliers · Customers" })).getByRole("button", { name: "ASML" }));
  await user.click(screen.getByRole("button", { name: "Select NVIDIA on the canvas" }));
  expect(screen.queryByRole("button", { name: "Back" })).not.toBeInTheDocument();
});

it("names a counterpart in full on its chip and leaves truncation to the chip's width", async () => {
  const { MapRelationships } = await import("./MapRelationships");
  const long = { ...map, nodes: [...nodes, company("shin", "Shin-Etsu Chemical Co.")], edges: [...map.edges, claim("c8", "shin", "tsmc", "SUPPLIES")] } as KnowledgeMap;
  render(<MapRelationships map={long} node={nodes[0]!} onSelect={vi.fn()} />);
  expect(screen.getByRole("button", { name: "Shin-Etsu Chemical Co." })).toHaveTextContent(/^Shin-Etsu Chemical Co\.$/);
});

it("points the canvas at a relationship while a chip is hovered or focused", async () => {
  const user = userEvent.setup();
  const onPoint = vi.fn();
  const { MapRelationships } = await import("./MapRelationships");
  const view = render(<MapRelationships map={map} node={nodes[0]!} onSelect={vi.fn()} onPoint={onPoint} />);
  await user.hover(screen.getByRole("button", { name: "ASML" }));
  expect(onPoint).toHaveBeenLastCalledWith("c1");
  await user.unhover(screen.getByRole("button", { name: "ASML" }));
  expect(onPoint).toHaveBeenLastCalledWith();
  screen.getByRole("button", { name: "Apple" }).focus();
  expect(onPoint).toHaveBeenLastCalledWith("c3");
  // A Lens entry reads as one the name is seen through, and points at its dotted line.
  const other = screen.getByRole("region", { name: "Other" });
  // The Lens entries share one line after the claims.
  expect(within(other).getAllByRole("listitem").map((item) => item.textContent)).toEqual(["located inHsinchu", "Lenses appliedCash firstMoat check"]);
  await user.hover(within(other).getByRole("button", { name: "Cash first" }));
  expect(onPoint).toHaveBeenLastCalledWith("lens");
  // Leaving the view clears the line it pointed at.
  view.unmount();
  expect(onPoint).toHaveBeenLastCalledWith();
});
