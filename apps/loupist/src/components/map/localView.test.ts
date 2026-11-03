import type { KnowledgeMapEdge, KnowledgeMapNode } from "../../contracts/knowledge";
import { defaultFocal, followedNames, localSet, neighborOrder } from "./localView";

const node = (id: string, degree = 1, extra: Partial<KnowledgeMapNode> = {}): KnowledgeMapNode => ({ id, label: id, kind: "entity", entityKind: "Organization", degree, watched: false, held: false, ...extra });
const claim = (source: string, target: string): KnowledgeMapEdge => ({ id: `${source}-${target}`, kind: "claim", source, target, status: "active" });

describe("localSet", () => {
  const nodes = [node("focal", 4), node("a", 3), node("b", 2), node("a1", 1), node("a2", 5), node("b1", 1), node("far", 1), node("lens", 0, { kind: "lens_method", entityKind: undefined }), node("lens-far", 0, { kind: "lens_method", entityKind: undefined })];
  const edges = [claim("focal", "a"), claim("b", "focal"), claim("a", "a1"), claim("a", "a2"), claim("b", "b1"), claim("b1", "far"),
    { id: "applies", kind: "applies_to" as const, source: "lens", target: "a" }, { id: "applies-far", kind: "applies_to" as const, source: "lens-far", target: "far" }];

  it("follows relationships one or two hops and attaches Lens items to drawn names only", () => {
    const one = localSet(nodes, edges, "focal", 1);
    expect([...one.ids].sort()).toEqual(["a", "b", "focal", "lens"]);
    expect(one).toMatchObject({ total: 3, shown: 3, capped: false, neighbors: 2 });
    const two = localSet(nodes, edges, "focal", 2);
    expect([...two.ids].sort()).toEqual(["a", "a1", "a2", "b", "b1", "focal", "lens"]);
    expect(two.ids.has("far")).toBe(false);
    expect(two.ids.has("lens-far")).toBe(false);
  });

  it("keeps the focal and its first ring when capped, then the best-connected second ring", () => {
    const capped = localSet(nodes, edges, "focal", 2, 4);
    expect(capped).toMatchObject({ total: 6, shown: 4, capped: true });
    expect([...capped.ids].filter((id) => id !== "lens").sort()).toEqual(["a", "a2", "b", "focal"]);
  });

  it("reports a name with no relationships", () => {
    expect(localSet([...nodes, node("alone")], edges, "alone", 1)).toMatchObject({ total: 1, neighbors: 0 });
  });
});

describe("defaultFocal", () => {
  it("prefers the remembered name, then a watched company, then a held one, then the best-connected company", () => {
    const nodes = [node("big", 9), node("watched-small", 1, { watched: true }), node("held", 5, { held: true }), node("person", 20, { entityKind: "Person" })];
    expect(defaultFocal(nodes, "held")).toBe("held");
    expect(defaultFocal(nodes, "missing")).toBe("watched-small");
    expect(defaultFocal(nodes.filter((item) => !item.watched))).toBe("held");
    expect(defaultFocal(nodes.filter((item) => !item.watched && !item.held))).toBe("big");
    expect(defaultFocal([node("person", 3, { entityKind: "Person" })])).toBeUndefined();
  });

  it("lists watched names before held ones", () => {
    expect(followedNames([node("h", 9, { held: true }), node("w", 1, { watched: true }), node("x")]).map((item) => item.id)).toEqual(["w", "h"]);
  });
});

it("orders neighbors clockwise from twelve o'clock", () => {
  const at = (id: string, x: number, y: number) => ({ ...node(id), x, y });
  const positioned = [at("focal", 0, 0), at("east", 10, 0), at("north", 0, -10), at("south", 0, 10), at("west", -10, 0), at("loose", 50, 50)];
  const edges = ["east", "north", "south", "west"].map((id) => claim("focal", id));
  expect(neighborOrder("focal", positioned, edges)).toEqual(["north", "east", "south", "west"]);
});
