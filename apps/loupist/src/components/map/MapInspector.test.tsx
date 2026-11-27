import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PreviewBridge } from "../../preview/fixtures";
import { prepareConflictsFixture } from "../../preview/fixtures/conflicts";
import { hanulFixture } from "../../preview/assets";
import { AssetPageOpenerProvider } from "../assets/assetOpener";
import type { GraphClaim, KnowledgeMapNode } from "../../contracts/knowledge";
import type { LoupistBridge } from "../../types";
import { MapClaimInspector } from "./MapClaimInspector";
import { MapInspector } from "./MapInspector";
import { LearnRequestProvider } from "../briefings/LearnRequest";
it("shows both attributed conflicting reports and opens each exact source", async () => {
  const bridge = new PreviewBridge({delay_ms:0}); await prepareConflictsFixture(bridge);
  const catalog = await bridge.knowledgeCatalog({loupistId:"loupist-quality"});
  const map = await bridge.knowledgeMap({loupistId:"loupist-quality"});
  const location = {loupistId:"loupist-quality",surface:"world" as const,recordId:"preview-world",revisionId:"preview-revision"};
  const onOpen = vi.fn();
  render(<MapInspector node={map.nodes[0]} map={map} catalog={catalog} location={location} bridge={bridge} onClose={vi.fn()} onSelect={vi.fn()} onOpen={onOpen} onAsk={vi.fn()} />);
  const conflicts = await screen.findByRole("region",{name:"Sources disagree"});
  expect(within(conflicts).getByText(/expanded supplier capacity by 18%/)).toBeVisible();
  expect(within(conflicts).getByText(/reports unchanged capacity/)).toBeVisible();
  expect(within(conflicts).getByText(/Oct 27, 2026/)).toBeVisible();
  expect(within(conflicts).getByText(/Oct 26, 2026/)).toBeVisible();
  const user = userEvent.setup(); const links = within(conflicts).getAllByRole("button",{name:"Open source"});
  expect(links).toHaveLength(2);
  await user.click(links[1]);
  expect(onOpen).toHaveBeenCalledWith(expect.objectContaining({surface:"source",objectId:"preview-source-1",selection:expect.objectContaining({spanId:"conflict-span-1"})}));
});

it("lists an entity's History in its inspector and opens a row's claim", async () => {
  const bridge = new PreviewBridge({ delay_ms: 0 });
  const catalog = await bridge.knowledgeCatalog({ loupistId: "loupist-quality" });
  const map = await bridge.knowledgeMap({ loupistId: "loupist-quality" });
  const node = map.nodes.find(item => item.kind === "entity")!;
  const location = { loupistId: "loupist-quality", surface: "world" as const, recordId: "preview-world", revisionId: "preview-revision" };
  const onOpen = vi.fn();
  const read = vi.spyOn(bridge, "knowledgeWorldChanges");
  render(<MapInspector node={node} map={map} catalog={catalog} location={location} bridge={bridge} onClose={vi.fn()} onSelect={vi.fn()} onOpen={onOpen} onAsk={vi.fn()} />);
  expect(await screen.findByRole("region", { name: "History" })).toBeVisible();
  expect(read).toHaveBeenCalledWith(expect.objectContaining({ axis: "published", about: [{ node: node.id }] }));
});

it("shows current claims before ended ones, then the rest", async () => {
  const node = { id: "n", label: "Hanul", kind: "entity" as const, degree: 1, watched: false };
  const fact = (id: string, proposition: string, recordedAtMs: number, invalidAt: GraphClaim["invalidAt"]): GraphClaim => ({
    id, proposition, attribution: "", relation: "REPORTS", endpoints: [{ nodeId: "n", role: "subject" }], qualifiers: [], evidence: [],
    status: "active", recordedAtMs, publicationTime: null, effectiveTime: null, validAt: { precision: "date", value: "2020-01-01" }, invalidAt, expiredAt: null,
  });
  const edge = (item: GraphClaim) => ({ id: item.id, relation: "REPORTS", endpoints: item.endpoints, status: item.status, claim: item });
  const edges = [fact("ended", "Old margin", 1, { precision: "date", value: "2024-01-01" }), ...Array.from({ length: 7 }, (_, index) => fact(`c${index}`, `Current fact ${index}`, 10 + index, null))].map(edge);
  const map = { nodes: [node], edges: [], counts: {}, unmatchedWatchlist: [], worldRevisionId: "rev", generatedAtMs: 1 };
  const bridge = { knowledgeLensPagesForNode: async () => [], knowledgeWorldChanges: async () => ({ changes: [], next: null }), packOrigin: async () => null, assetPageGet: async () => null, onAssetsChanged: () => () => {} } as unknown as LoupistBridge;
  render(<MapInspector node={node as KnowledgeMapNode} map={map as never} bridge={bridge} location={{ loupistId: "loupist", surface: "world" }}
    providedGraph={{ edges, nodes: [], sourceTargets: {} } as never} onClose={vi.fn()} onSelect={vi.fn()} onOpen={vi.fn()} onAsk={vi.fn()} />);
  const facts = screen.getByRole("list", { name: "Key facts" });
  expect(within(facts).getAllByRole("listitem")).toHaveLength(6);
  expect(within(facts).queryByText("Old margin")).toBeNull();
  expect(within(facts).getByText("Current fact 6")).toBeVisible();
  await userEvent.click(screen.getByRole("button", { name: "Show all" }));
  expect(within(facts).getAllByRole("listitem")).toHaveLength(8);
  expect(within(facts).getByText("Old margin")).toBeVisible();
});

it("states when a fact was held, including an approximate end", async () => {
  const claim: GraphClaim = {
    id: "margin", proposition: "Margin was 14%", attribution: "Filing", relation: "REPORTS", endpoints: [], qualifiers: [], evidence: [],
    status: "active", recordedAtMs: Date.parse("2026-03-02T00:00:00Z"), publicationTime: { precision: "date", value: "2026-03-01" },
    effectiveTime: null, validAt: { precision: "date", value: "2024-01-01" }, invalidAt: { precision: "date", value: "2025-12-31", approximate: true }, expiredAt: null,
  };
  const bridge = { knowledgeWorldQuery: vi.fn(async () => ({ recordId: "world", revisionId: "rev", edges: [{ id: "margin", claim, endpoints: [], relation: "REPORTS", status: "active" }], nodes: [], sourceTargets: {} })) } as unknown as LoupistBridge;
  render(<MapClaimInspector bridge={bridge} location={{ loupistId: "loupist", surface: "world", recordId: "world", revisionId: "rev" }} claimId="margin" onClose={vi.fn()} onOpen={vi.fn()} onAsk={vi.fn()} />);
  const inspector = await screen.findByRole("complementary", { name: "Margin was 14%" });
  expect(within(inspector).getByText("Ended")).toBeVisible();
  expect(within(inspector).getByText(/Held since .+ · ended no later than /)).toBeVisible();
  expect(within(inspector).getByText(/^Published /)).toBeVisible();
  expect(within(inspector).getByText(/^Learned /)).toBeVisible();
});

it("names the entity before its kind, so the sheet's first line and a screen reader start with the name", async () => {
  const bridge = new PreviewBridge({ delay_ms: 0 }); await prepareConflictsFixture(bridge);
  const map = await bridge.knowledgeMap({ loupistId: "loupist-quality" });
  const node = map.nodes[0]!;
  render(<MapInspector node={node} map={map} location={{ loupistId: "loupist-quality", surface: "world" }} bridge={bridge} onClose={vi.fn()} onSelect={vi.fn()} onOpen={vi.fn()} onAsk={vi.fn()} />);
  const inspector = screen.getByRole("complementary", { name: node.label });
  const heading = within(inspector).getByRole("heading", { level: 2, name: node.label });
  expect(heading.parentElement?.firstElementChild).toBe(heading);
  expect(heading.nextElementSibling).toHaveTextContent("Company");
});

it("reads an entity's thesis in the inspector and opens its asset page on request", async () => {
  const bridge = new PreviewBridge({ delay_ms: 0 });
  await prepareConflictsFixture(bridge);
  const loupistId = "loupist-quality";
  const map = await bridge.knowledgeMap({ loupistId });
  const node = map.nodes[0]!;
  const fixture = hanulFixture();
  const assetId = fixture.asset.assetId;
  bridge.assets.installAsset(fixture.asset);
  bridge.assets.install(loupistId, assetId, { ...fixture.page, entityId: node.id });
  const get = vi.spyOn(bridge, "assetPageGet");
  const check = vi.spyOn(bridge, "assetPriceCheck");
  const series = vi.spyOn(bridge, "assetPriceSeries");
  const open = vi.fn();
  render(<AssetPageOpenerProvider value={open}><MapInspector bridge={bridge} map={map} node={node}
    location={{ loupistId, surface: "world" }} onClose={vi.fn()} onSelect={vi.fn()} onOpen={vi.fn()} onAsk={vi.fn()} /></AssetPageOpenerProvider>);
  const thesis = await screen.findByRole("region", { name: "Your thesis" });
  expect(thesis).toHaveTextContent("Weakening");
  expect(get).toHaveBeenCalledWith({ loupistId, entityId: node.id });
  expect(check).not.toHaveBeenCalled();
  expect(series).not.toHaveBeenCalled();
  await userEvent.setup().click(within(thesis).getByRole("button", { name: "Open asset page" }));
  expect(open).toHaveBeenCalledWith({ loupistId, assetId, name: fixture.asset.name });
});

it.each(["active", "disputed", "withdrawn"] as const)("offers Tell Loupist this is wrong for a %s claim and hands its draft up unsent", async (status) => {
  const claim: GraphClaim = {
    id: "capacity", proposition: "Capacité portée à 40 000 t", attribution: "Communiqué", relation: "REPORTS", endpoints: [], qualifiers: [],
    evidence: [{ spanId: "s", role: "supports", attribution: "Communiqué du 3 mars" }],
    status, recordedAtMs: null, publicationTime: null, effectiveTime: null, validAt: null, invalidAt: null, expiredAt: null,
  };
  const bridge = { knowledgeWorldQuery: vi.fn(async () => ({ recordId: "world", revisionId: "rev", edges: [{ id: "capacity", claim, endpoints: [], relation: "REPORTS", status }], nodes: [], sourceTargets: {} })) } as unknown as LoupistBridge;
  const onCorrect = vi.fn();
  render(<MapClaimInspector bridge={bridge} location={{ loupistId: "loupist", surface: "world", recordId: "world", revisionId: "rev" }} claimId="capacity" onClose={vi.fn()} onOpen={vi.fn()} onAsk={vi.fn()} onCorrect={onCorrect} />);
  await userEvent.click(await screen.findByRole("button", { name: "Tell Loupist this is wrong" }));
  expect(onCorrect).toHaveBeenCalledOnce();
  const text = onCorrect.mock.calls[0]![0] as string;
  expect(text).toContain("> Capacité portée à 40 000 t");
  expect(text).toContain("> Sources: Communiqué du 3 mars");
  expect(text.endsWith("This is wrong because ")).toBe(true);
});

it("offers Learn about this for an entity and sends Bot a request naming it (plan 4.4)", async () => {
  const bridge = new PreviewBridge({ delay_ms: 0 }); await prepareConflictsFixture(bridge);
  const map = await bridge.knowledgeMap({ loupistId: "loupist-quality" });
  const node = map.nodes[0]!;
  const ask = vi.fn();
  render(<LearnRequestProvider value={{ block: null, ask }}><MapInspector node={node} map={map} location={{ loupistId: "loupist-quality", surface: "world" }} bridge={bridge} onClose={vi.fn()} onSelect={vi.fn()} onOpen={vi.fn()} onAsk={vi.fn()} /></LearnRequestProvider>);
  await userEvent.click(screen.getByRole("button", { name: "Learn about this" }));
  expect(ask).toHaveBeenCalledExactlyOnceWith(`Propose a learning program about ${node.label} that keeps what you know about it current.`);
});

it("says when a cited passage's figure was not found, and opens the Source at the passage when the range is known (plan 4.1)", async () => {
  const claim: GraphClaim = {
    id: "growth", proposition: "Ingresos crecieron 18% interanual", attribution: "Informe", relation: "REPORTS", endpoints: [], qualifiers: [],
    evidence: [
      { spanId: "derived", role: "supports", attribution: "Informe trimestral", figureCheck: "not_found", passage: { chunkIndex: 2, charStart: 400, charEnd: 820 } },
      { spanId: "plain", role: "supports", attribution: "Comunicado", figureCheck: "matched" },
    ],
    status: "active", recordedAtMs: null, publicationTime: null, effectiveTime: null, validAt: null, invalidAt: null, expiredAt: null,
  };
  const target = (spanId: string) => ({ recordId: "sources", revisionId: "r1", sourceId: `source-${spanId}`, spanId, worldRecordId: "world", worldRevisionId: "rev" });
  const bridge = { knowledgeWorldQuery: vi.fn(async () => ({ recordId: "world", revisionId: "rev", edges: [{ id: "growth", claim, endpoints: [], relation: "REPORTS", status: "active" }], nodes: [], sourceTargets: { derived: target("derived"), plain: target("plain") } })) } as unknown as LoupistBridge;
  const onOpen = vi.fn();
  render(<MapClaimInspector bridge={bridge} location={{ loupistId: "loupist", surface: "world", recordId: "world", revisionId: "rev" }} claimId="growth" onClose={vi.fn()} onOpen={onOpen} onAsk={vi.fn()} />);
  const rows = await screen.findAllByRole("listitem");
  const derived = rows.find(row => row.textContent?.includes("Informe trimestral"))!;
  const plain = rows.find(row => row.textContent?.includes("Comunicado"))!;
  expect(within(derived).getByText("Figure not found in the cited passage")).toBeVisible();
  expect(within(plain).queryByText("Figure not found in the cited passage")).toBeNull();
  await userEvent.click(within(derived).getByRole("button", { name: "Open source" }));
  expect(onOpen).toHaveBeenLastCalledWith(expect.objectContaining({ surface: "source", selection: expect.objectContaining({ spanId: "derived", passage: { chunkIndex: 2, charStart: 400, charEnd: 820 } }) }));
  await userEvent.click(within(plain).getByRole("button", { name: "Open source" }));
  expect(onOpen.mock.lastCall![0].selection).not.toHaveProperty("passage");
});
