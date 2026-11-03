import type { ChatEvidenceRef, ChatMessage, MemoryRef, MemoryUpdateChange } from "../types";
import { chatReceipt, savesMemory, showsReceipt } from "./receipt";

const evidence = (id: string, kind: ChatEvidenceRef["kind"], extra: Partial<ChatEvidenceRef> = {}): ChatEvidenceRef => ({
  id, title: `Title ${id}`, kind, tool_name: "web_fetch", url: `https://example.com/${id}`, retrieved_at: "2026-10-28T00:00:00Z",
  excerpt: "Excerpt", excerpt_truncated: false, content_sha256: "a".repeat(64), warnings: [], ...extra,
});
const lensRead = (recordId: string, readRef = `read-${recordId}`): MemoryRef => ({
  read_ref: readRef, kind: "lens", title: `Method ${recordId}`, excerpt: "", access: "exact_read",
  knowledge: { kind: "lens", recordId, revisionId: "rev", objectId: recordId },
});
const change = (kind: MemoryUpdateChange["kind"], recordId: string, operation: MemoryUpdateChange["operation"] = "create") => ({
  kind, recordId, operation, title: recordId, lesson: "", basis: "", futureUse: "",
  ...(kind === "principle" ? { statement: recordId } : {}),
  ...(kind === "Research" ? {} : { knowledge: { kind: kind === "principle" || kind === "Lens" ? "lens" : kind.toLowerCase(), recordId, revisionId: "rev", objectId: recordId } }),
}) as MemoryUpdateChange;
const answer = (extra: Partial<ChatMessage> = {}): ChatMessage => ({ id: "answer", role: "assistant", content: "Answer", created_at: "2026-10-28", status: "complete", ...extra });

it("counts read sources once per subject and leaves out search previews, computation and Memory receipts", () => {
  const receipt = chatReceipt(answer({ evidence_refs: [
    evidence("filing", "original"),
    evidence("filing-again", "original", { url: "https://example.com/filing", content_sha256: "a".repeat(64), title: "Title filing" }),
    evidence("preview", "discovery", { tool_name: "web_search" }),
    evidence("calc", "derived", { tool_name: "bash" }),
    evidence("memory", "saved_context", { tool_name: "lens_read", url: undefined }),
    evidence("local", "local_file", { tool_name: "workspace_capture", url: undefined }),
  ] }));
  expect(receipt.readSources.map((item) => item.id)).toEqual(["filing", "local"]);
  expect(receipt.empty).toBe(false);
});

it("uses host-recorded kinds for exact reads and context delivery, regardless of answer links", () => {
  const receipt = chatReceipt(answer({
    content: "Checked against your principles\n- [Cash before profit](lens:principle-1): meets\n- [Margin method](lens:method-1)\n- [New rule](lens:saved-rule)",
    lens_receipts: [
      { recordId: "principle-1", revisionId: "rev", kind: "principle", title: "Cash before profit", delivery: "exact_read" },
      { recordId: "method-1", revisionId: "rev", kind: "method", title: "Margin method", delivery: "exact_read" },
      { recordId: "method-1", revisionId: "rev", kind: "method", title: "Margin method", delivery: "context" },
    ],
    memory_refs: [lensRead("method-1"), lensRead("method-1", "again"), { ...lensRead("discovered"), access: "search_discovered" }],
    memory_update: { status: "applied", commitId: "c", changes: [
      change("principle", "saved-rule"), change("Source", "source-1"), change("Lens", "lens-proposal", "proposed"), change("Source", "source-2"), change("Lens", "method-2", "revise"),
    ] },
  }));
  expect(receipt.usedMethods.map((ref) => ref.recordId)).toEqual(["method-1"]);
  expect(receipt.usedPrinciples).toEqual([{ recordId: "principle-1", title: "Cash before profit" }]);
  expect(receipt.saved.map((group) => [group.kind, group.items.length])).toEqual([["Source", 2], ["Lens", 1], ["principle", 1]]);
  expect(receipt.proposals.map((item) => item.recordId)).toEqual(["lens-proposal"]);
});

it("reads map growth from this message's additions only", () => {
  const additions = { nodeIds: new Set(["a", "b"]), edgeIds: new Set(["ab"]), highlightIds: new Set(["a", "b"]) };
  expect(chatReceipt(answer(), additions)).toMatchObject({ names: 2, relationships: 1, mapHighlightIds: additions.highlightIds, empty: false });
  expect(chatReceipt(answer(), { nodeIds: new Set(), edgeIds: new Set(), highlightIds: new Set(["stale"]) }).mapHighlightIds.size).toBe(0);
});

it("is empty for an answer that used and saved nothing, and says so only when saving was allowed", () => {
  const unchanged = answer({ memory_update: { status: "no_change", commitId: null, changes: [] } });
  expect(chatReceipt(unchanged).empty).toBe(true);
  expect(savesMemory(unchanged)).toBe(true);
  expect(savesMemory(answer())).toBe(false);
});

it("shows only for finished assistant answers", () => {
  expect(showsReceipt(answer())).toBe(true);
  expect(showsReceipt(answer({ status: undefined }))).toBe(true);
  for (const status of ["streaming", "error", "aborted"] as const) expect(showsReceipt(answer({ status }))).toBe(false);
  expect(showsReceipt(answer({ role: "user" }))).toBe(false);
});

it("never lists saved items or proposals for a chat with saving off", () => {
  const message = answer({ memory_update: { status: "applied", commitId: "c", changes: [change("Source", "src-1"), change("principle", "p-1", "proposed")] } });
  expect(chatReceipt(message).saved).toHaveLength(1);
  const off = chatReceipt(message, undefined, { saveOff: true });
  expect(off.saved).toEqual([]);
  expect(off.proposals).toEqual([]);
  expect(savesMemory(message, true)).toBe(false);
});

it("does not infer a used kind from an answer link or an untyped old read", () => {
  const receipt = chatReceipt(answer({ content: "[Rule](lens:fake)", memory_refs: [lensRead("unknown")] }));
  expect(receipt.usedPrinciples).toEqual([]);
  expect(receipt.usedMethods).toEqual([]);
});
