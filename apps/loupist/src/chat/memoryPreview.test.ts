import { evidenceFixture } from "../test/evidenceFixtures";
import { memoryPreview } from "./memoryPreview";

const saved = (tool_name: string, payload: unknown) => evidenceFixture({ kind: "saved_context", tool_name, excerpt: JSON.stringify(payload) });

it("presents search titles and passages without host routing metadata", async () => {
  const evidence = saved("world_search", { hits: [{ title: "Samsung", snippet: "Makes memory chips.", recordId: "rec-internal", readArgs: { revisionId: "rev-internal" } }], hasMore: true });
  expect(await memoryPreview(evidence)).toBe("Samsung\nMakes memory chips.\n\nMore matches are available in Memory.");
  expect(evidence.excerpt).toContain("rec-internal");
});

it("presents graph names, relationship status and attribution from a read envelope", async () => {
  const graph = { recordId: "rec-internal", revisionDigest: "hash-internal", sourceTargets: { "span-internal": {} }, nodes: [{ id: "node-internal", name: "Samsung", description: "Memory manufacturer" }], edges: [{ id: "edge-internal", claim: { proposition: "Supplies memory", status: "disputed", attribution: "Annual report" } }] };
  expect(await memoryPreview(saved("world_read", { readRef: "read-internal", text: JSON.stringify(graph) }))).toBe("Entities\nSamsung — Memory manufacturer\n\nRelationships\nSupplies memory\ndisputed\nAnnual report");
});

it("explains incomplete host previews without exposing their partial identifiers", async () => {
  const evidence = evidenceFixture({ kind: "saved_context", tool_name: "world_read", excerpt: '{"sourceTargets":{"span-internal":', excerpt_truncated: true });
  expect(await memoryPreview(evidence)).toMatch(/preview is incomplete/);
  expect(await memoryPreview(evidence)).not.toContain("span-internal");
  const search = { ...evidence, tool_name: "world_search", excerpt: '{"hits":[{"title":"Samsung","snippet":"Memory maker","recordId":"private' };
  expect(await memoryPreview(search)).toContain("Samsung\nMemory maker");
  expect(await memoryPreview(search)).not.toContain("private");
});

it("keeps source JSON, user content and dataset results intact", async () => {
  expect(await memoryPreview(saved("world_read", { id: "customer-123", revenue: 42 }))).toBeNull();
  expect(await memoryPreview(saved("dataset_query", { hits: [{ id: "customer-123" }] }))).toBeNull();
  expect(await memoryPreview(evidenceFixture({ excerpt: "A SHA-256 hash identifies these bytes." }))).toBeNull();
});
