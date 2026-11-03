import {
  backKnowledgeLocation,
  initialKnowledgeNavigation,
  openKnowledgeLocation,
} from "./location";

describe("knowledge navigation", () => {
  it("starts on saved work and restores an explicit world selection on back", () => {
    let nav = initialKnowledgeNavigation("loupist-quality");
    expect(nav.current).toEqual({ loupistId: "loupist-quality", surface: "home" });
    expect(nav.past).toEqual([]);
    nav = openKnowledgeLocation(nav, {
      loupistId: "loupist-quality",
      surface: "world",
      recordId: "world-protocols",
      revisionId: "rev-owned-1",
      selection: { kind: "node", id: "node-tcp" },
    });
    nav = openKnowledgeLocation(
      nav,
      {
        loupistId: "loupist-quality",
        surface: "source",
        recordId: "source-pack",
        revisionId: "rev-source-1",
        selection: { kind: "source", sourceId: "source-rfc-9110", spanId: "span-status-codes" },
      },
      { x: 48, y: 0, scale: 1 },
    );
    nav = backKnowledgeLocation(nav);
    expect(nav.current.surface).toBe("world");
    expect(nav.current.selection).toEqual({ kind: "node", id: "node-tcp" });
    expect(nav.current.view?.camera).toEqual({ x: 48, y: 0, scale: 1 });
    nav = backKnowledgeLocation(nav);
    expect(nav.current).toEqual({ loupistId: "loupist-quality", surface: "home" });
  });
});
