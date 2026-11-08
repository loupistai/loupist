import type { ChatMessage, MemoryRef, MemoryUpdateChange } from "../types";
import { chatReceipt, learnedSummary, savesMemory, showsReceipt } from "./receipt";

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
      change("principle", "saved-rule"), change("Source", "source-1"), change("Source", "source-2"),
    ] },
  }));
  expect(receipt.usedMethods.map((ref) => ref.recordId)).toEqual(["method-1"]);
  expect(receipt.usedPrinciples).toEqual([{ recordId: "principle-1", title: "Cash before profit" }]);
  expect(receipt.saved.map((group) => [group.kind, group.items.length])).toEqual([["Source", 2], ["principle", 1]]);
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

it("never lists saved items or what was learned for a chat with saving off", () => {
  const message = answer({ memory_update: { status: "applied", commitId: "c", changes: [change("Source", "src-1")] } });
  const digest = { messageId: "answer", pages: [{ recordId: "page", title: "Hanul", kind: "subject" as const, created: true, outcomes: [{ opId: "op", outcome: "added", key: "overview" }] }] };
  expect(chatReceipt(message, undefined, { learned: digest })).toMatchObject({ saved: [expect.anything()], learned: { sections: 1, ideas: 0 } });
  const off = chatReceipt(message, undefined, { saveOff: true, learned: digest });
  expect(off.saved).toEqual([]);
  expect(off.learned).toBeNull();
  expect(savesMemory(message, true)).toBe(false);
});

it("does not infer a used kind from an answer link or an untyped old read", () => {
  const receipt = chatReceipt(answer({ content: "[Rule](lens:fake)", memory_refs: [lensRead("unknown")] }));
  expect(receipt.usedPrinciples).toEqual([]);
  expect(receipt.usedMethods).toEqual([]);
});

it("counts a perspective, a topic and an idea the host delivered as used pages", () => {
  const receipt = chatReceipt(answer({ lens_receipts: [
    { recordId: "outlook", revisionId: "rev", kind: "perspective", title: "Evidence before conviction", delivery: "context" },
    { recordId: "topic", revisionId: "rev", kind: "subject", title: "Hanul Precision", delivery: "exact_read" },
    { recordId: "idea", revisionId: "rev", kind: "idea", title: "Margins recover", delivery: "exact_read" },
    { recordId: "topic", revisionId: "rev", kind: "subject", title: "Hanul Precision", delivery: "context" },
  ] }));
  expect(receipt.usedPages.map(page => [page.recordId, page.kind])).toEqual([["outlook", "perspective"], ["topic", "subject"], ["idea", "idea"]]);
  expect(receipt.empty).toBe(false);
});

it("summarizes what an answer filed: sections changed, and the pages that are ideas", () => {
  const learned = learnedSummary({ messageId: "answer", pages: [
    { recordId: "topic", title: "Hanul", kind: "subject", created: true, outcomes: [{ opId: "1", outcome: "added", key: "overview" }, { opId: "2", outcome: "replaced", key: "risks" }, { opId: "3", outcome: "already_known", key: "facts" }] },
    { recordId: "idea", title: "Margins", kind: "idea", created: true, outcomes: [{ opId: "4", outcome: "added", key: "claim" }, { opId: "5", outcome: "summarized" }] },
    { recordId: "noop", title: "Nothing", kind: "method", created: false, outcomes: [{ opId: "6", outcome: "skipped_stale", key: "steps" }] },
  ] });
  expect(learned).toMatchObject({ sections: 3, ideas: 1 });
  expect(learned?.pages.map(page => page.recordId)).toEqual(["topic", "idea"]);
  expect(learnedSummary(undefined)).toBeNull();
});
