/** Preview scenarios, in the capture order `src/preview/scenarios.ts` assembles. */
import { prepareFixture, PreviewBridge } from "../fixtures";
import { rememberScope } from "../../components/map/mapVisit";
import type { GrowthEvent, KnowledgeMapNode } from "../../contracts/knowledge";
import { workspaceOverlayMedia } from "../../components/workspace/useWorkspacePanelResize";
import { MIN_VISIT_MS, recordVisit } from "../../app/loupistVisit";
import { base, click, clickMapAction, enterMap, hasLayout, localViewMap, mapCanvas, navigate, pressMenu, rememberPreviewMap, select, until, visible, type PreviewScenario } from "./kit";

/** Last visit two days ago and five changes since: two names, a relationship, conflicting evidence and a learned method. */
function whatsNewFixture(bridge: PreviewBridge, withChanges: boolean) {
  const now = Date.now(), hour = 3_600_000;
  const lastSeenAtMs = now - (withChanges ? 50 : 24) * hour;
  recordVisit("loupist-quality", lastSeenAtMs - MIN_VISIT_MS, lastSeenAtMs);
  const event = (index: number, kind: GrowthEvent["kind"], subjectId: string | null, atMs: number, summary: string, extra: Partial<GrowthEvent> = {}): GrowthEvent => ({ eventId: `whats-new-${index}`, loupistId: "loupist-quality", atMs, kind, subjectId, recordId: "map-world", originChatId: null, originMessageId: null, actor: "loupist", summary, beforeSummary: null, afterSummary: null, ...extra });
  const events = withChanges ? [
    event(0, "lens_created", null, now - 27 * hour, "Compare competitive advantages", { recordId: "map-lens-1", origin: { kind: "learning_review", reviewId: "preview-review" } }),
    event(1, "entity_added", "map-entity-09", now - 26 * hour, "SK hynix"),
    event(2, "source_retained", null, now - hour, "Q3 operating review", { originMessageId: "preview-answer" }),
    event(3, "entity_added", "map-entity-03", now - hour, "NVIDIA", { originMessageId: "preview-answer" }),
    event(4, "claim_added", "map-claim-01", now - 50 * 60_000, "Apple depends on TSMC for its most advanced chips", { originMessageId: "preview-answer" }),
    event(5, "claim_disputed", "map-claim-08", now - 20 * 60_000, "TSMC packaging capacity reserved for SK hynix HBM (54% vs 48%)"),
  ] : [];
  bridge.knowledgeGrowthEvents = async request => ({ events: events.filter(item => request.sinceMs === undefined || item.atMs >= request.sinceMs), nextCursor: null });
}

/** On a first visit the legend opens on a wide Map and starts collapsed on a narrow one. */
function legendAtFirstVisit() {
  const legend = select("details.map-legend");
  const width = select(".map-view")?.clientWidth ?? 0;
  return legend instanceof HTMLDetailsElement && legend.open === (!hasLayout() || width >= 900);
}

/** The Map: overview, selection, local views, what's new, sources and Lens pages. */
export const mapScenarios: PreviewScenario[] = [
  ...["map-overview", "map-1000", "map-empty", "map-selected", "map-filtered", "map-lens-overlay", "map-lens-on-world", "map-library-sources", "map-replay-paused", "map-export-menu"].map((id): PreviewScenario => ({
    ...base(id, "Research map", () => {
      if (id === "map-export-menu") return select(".map-export-menu, .map-more-menu");
      const map = select(".map-view");
      return map?.querySelector("canvas, .map-empty, .map-list") ?? (map?.clientWidth === 0 ? map : null);
    }, async (root) => {
      await navigate(root, "map");
      const map = await until(() => select(".map-view"), "Map did not load");
      if (id === "map-selected") {
        const input = await until(() => map.querySelector<HTMLInputElement>('[role="combobox"]'), "Map search did not load");
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "Apple");
        input.dispatchEvent(new Event("input", { bubbles: true }));
        await click('[role="option"]', map);
        await until(() => select(".map-inspector"), "Inspector did not open");
      }
      if (id === "map-replay-paused") {
        await clickMapAction("[data-map-replay]", map);
        const scrubber = await until(() => map.querySelector<HTMLInputElement>(".map-replay-scrubber"), "Replay did not open");
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(scrubber, String((Number(scrubber.min) + Number(scrubber.max)) / 2));
        scrubber.dispatchEvent(new Event("input", { bubbles: true }));
        scrubber.dispatchEvent(new Event("change", { bubbles: true }));
      }
      if (id === "map-library-sources") await clickMapAction("[data-map-library]", map);
      if (id === "map-lens-overlay") {
        await click("[data-map-filter]", map);
        await click(".map-lens-filter summary", document.body);
        await click('[data-lens-id="map-lens-0"]', document.body);
        await until(() => select(".map-lens-banner"), "Lens overlay did not open");
      }
      if (id === "map-lens-on-world") {
        await click("[data-map-filter]", map);
        await click(".map-lens-filter summary", document.body);
        await click('[data-lens-id="map-lens-1"]', document.body);
        await until(() => select('[data-lens-applied="map-lens-1"]'), "Selected Lens did not mark the World");
      }
      if (id === "map-filtered") {
        await click("[data-map-filter]", map);
        const product = await until(() => document.querySelector<HTMLInputElement>('[data-map-filter-kind="product"]'), "Product filter did not open");
        product.click();
        product.focus();
        product.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      }
      if (map.clientWidth > 0 && id !== "map-empty") {
        await until(() => map.querySelector("canvas"), "Map canvas did not mount");
        for (let frame = 0; frame < 3; frame++) await new Promise(requestAnimationFrame);
        if (id !== "map-replay-paused" && id !== "map-selected") await clickMapAction("[data-map-fit]", map);
      }
      if (id === "map-export-menu") {
        const trigger = map.querySelector("[data-map-export]");
        if (visible(trigger)) await click("[data-map-export]", map);
        else pressMenu(await until(() => select("[data-map-more]"), "More actions did not load"));
        await until(() => select(".map-export-menu, .map-more-menu"), "Export choices did not open");
      }
    }),
    setup: async (bridge) => {
      await prepareFixture(bridge);
      // The draw budget is measured on all 1,000 names, not the local view a large Map opens on.
      if (id === "map-1000") rememberScope("loupist-quality", "all");
      if (id === "map-export-menu") bridge.exportSave = async request => {
        window.dispatchEvent(new CustomEvent("map-preview-export", { detail: request }));
        return `/preview/${request.suggestedName}`;
      };
      const original = bridge.knowledgeMap.bind(bridge);
      bridge.knowledgeMap = async (request) => {
        const map = await original(request);
        if (id === "map-replay-paused") {
          const start = Date.now() - 28 * 86_400_000;
          map.nodes.forEach((node, index) => { node.firstSeenAtMs = start + index / Math.max(1, map.nodes.length - 1) * 28 * 86_400_000; });
          for (const edge of map.edges) edge.firstSeenAtMs = Math.max(map.nodes.find(node => node.id === edge.source)?.firstSeenAtMs ?? start, map.nodes.find(node => node.id === edge.target)?.firstSeenAtMs ?? start);
        }
        if (id === "map-empty") {
          // A new Loupist already has its standing Lens, but no World names.
          map.nodes = map.nodes.filter((node) => node.id === "map-lens-0"); map.edges = [];
          for (const count of Object.values(map.counts)) { count.total = 0; count.shown = 0; count.truncated = false; }
          map.counts.lens_perspective.total = map.counts.lens_perspective.shown = map.nodes.length;
        }
        if (id === "map-1000") {
          map.nodes = Array.from({ length: 1000 }, (_, i) => ({ id: `synthetic-${i}`, kind: "entity" as const, label: `Research ${String(i + 1).padStart(4, "0")}`, entityKind: ["Organization", "Product", "Person", "Event", "Entity"][i % 5], degree: 0, watched: i < 3, held: i === 3, firstSeenAtMs: Date.UTC(2026, 9, 1 + i % 28) }));
          map.edges = Array.from({ length: 1500 }, (_, i) => ({ id: `synthetic-edge-${i}`, kind: "claim" as const, source: `synthetic-${i % 1000}`, target: `synthetic-${(i * 31 + 7) % 1000}`, relation: "related_to", status: i % 97 === 0 ? "disputed" as const : "active" as const }));
          for (const edge of map.edges) for (const nodeId of [edge.source, edge.target]) map.nodes[Number(nodeId.split("-")[1])].degree++;
          for (const [kind, count] of Object.entries(map.counts)) { count.total = count.shown = kind === "entity" ? 1000 : kind === "claim" ? 1500 : 0; count.truncated = false; }
          map.worldRevisionId = "synthetic-1000";
        }
        return map;
      };
    },
  })),
  { ...base("map-local-1hop", "A large Map opens around one company, legend open on a first visit (collapsed below 900 px)", () => select("[data-map-around]") && mapCanvas() && legendAtFirstVisit() ? select(".map-stats") : null, async root => { await enterMap(root); }),
    setup: async bridge => { await prepareFixture(bridge); localViewMap(bridge); rememberPreviewMap({ scope: "1" }); } },
  { ...base("map-local-2hops-bounded", "Two hops stop at 150 names around the company", () => select("[data-map-around]") && mapCanvas() ? select(".map-stats") : null, async root => { await enterMap(root); }),
    setup: async bridge => { await prepareFixture(bridge); localViewMap(bridge); rememberPreviewMap({ scope: "2" }); } },
  { ...base("map-local-no-neighbors", "A company with no relationships yet", () => mapCanvas() ? select(".map-no-neighbors") : null, async root => { await enterMap(root); }),
    setup: async bridge => { await prepareFixture(bridge); localViewMap(bridge); rememberPreviewMap({ focalId: "local-kioxia", scope: "1" }); } },
  ...(["map-whats-new", "map-whats-new-empty"] as const).map((id): PreviewScenario => ({
    ...base(id, id === "map-whats-new" ? "What changed since the last visit" : "Nothing new since the last visit", () => mapCanvas() ? select(id === "map-whats-new" ? ".map-whats-new .map-whats-new-row" : ".map-whats-new .map-whats-new-state") : null, async root => {
      const map = await enterMap(root);
      await click("[data-map-whats-new]", map);
    }),
    setup: async bridge => { await prepareFixture(bridge); whatsNewFixture(bridge, id === "map-whats-new"); },
  })),
  { ...base("map-legend-compact", "Legend with at most six rows", () => mapCanvas() ? select("details.map-legend[open] .map-legend-more") : null, async root => { await enterMap(root); }),
    setup: async bridge => {
      await prepareFixture(bridge);
      rememberPreviewMap({ legendOpen: true });
      // The World canvas draws no Lens nodes, so overflow needs the World categories the seed omits.
      const read = bridge.knowledgeMap.bind(bridge);
      bridge.knowledgeMap = async request => {
        const map = await read(request);
        const company = map.nodes.find(node => node.kind === "entity" && node.entityKind === "Organization");
        const person: KnowledgeMapNode = { id: "legend-mina-cho", kind: "entity", label: "Mina Cho", entityKind: "Person", degree: 1, watched: false, held: false };
        const journal: KnowledgeMapNode = { id: "legend-journal", kind: "journal", label: "Inventory days below 50", degree: 0, watched: false, held: false };
        map.nodes = [...map.nodes, person, journal];
        if (company) map.edges = [...map.edges, { id: "legend-person-claim", kind: "claim", source: company.id, target: person.id, relation: "leads", status: "active" }];
        return map;
      };
    } },
  base("world-sources", "Retained sources and processing", () => {
    const list = select(".source-library > ul");
    return list && list.children.length >= 6 && mapCanvas() ? list : null;
  }, async (root) => { await navigate(root, "map"); await clickMapAction("[data-map-library]", root); }),
  { ...base("lens-reader", "A Lens page read as sections", () => select('.workspace-lens-content .page-reader .page-section'), async root => {
    await navigate(root, "lens");
    const method = await until(() => [...document.querySelectorAll<HTMLButtonElement>(".memory-row")].find(row => row.querySelector(".memory-marker.is-method")), "Method did not list");
    method.click();
    await until(() => select(".workspace-lens-content .page-reader .page-section"), "Method did not open");
  }), setup: prepareFixture },
  { ...base("lens-reader-many", "A long page list keeps its page readable when the Lens is expanded", () => select('.workspace-lens-content .page-reader'), async root => {
    await navigate(root, "lens");
    const method = await until(() => [...document.querySelectorAll<HTMLButtonElement>(".memory-row")].find(row => row.querySelector(".memory-marker.is-method")), "Method did not list");
    method.click();
    await until(() => select('.workspace-lens-content .page-reader'), "Method did not open");
    if (!window.matchMedia(workspaceOverlayMedia).matches) await click('.workspace-kind-heading button:first-child');
  }), setup: async bridge => {
    await prepareFixture(bridge);
    const read = bridge.knowledgeCatalog.bind(bridge);
    bridge.knowledgeCatalog = async request => {
      const catalog = await read(request);
      const method = catalog.lenses.find(item => item.lensKind === "method")!;
      return { ...catalog, lenses: [...catalog.lenses, ...Array.from({ length: 20 }, (_, index) => ({ ...method, recordId: `many-method-${index}`, title: `${method.title} · ${index + 1}` }))] };
    };
  } },
];
