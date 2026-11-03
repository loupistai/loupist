import type { ChatApproval, ChatProgressItem } from "../types";
import { evidenceFixture } from "../test/evidenceFixtures";
import { turnTranscript } from "./turnTranscript";

it("keeps mixed calls together across updates, with a new disclosure after public prose", () => {
  const read = { kind: "tool" as const, toolName: "read", category: "files" as const, operation: "read" as const, action: "Read file", status: "succeeded" as const };
  const note = { kind: "commentary" as const, id: "note", text: "Next source." };
  const items: ChatProgressItem[] = [
    { ...read, id: "a" }, { ...read, id: "b" }, { ...read, id: "c", status: "failed" },
    { ...read, id: "d" }, note, { ...read, id: "e" }, { ...read, id: "f" },
    { ...read, id: "g", operation: "write" }, { ...read, id: "h", operation: "write" },
  ];
  const approval: ChatApproval = { requestId: "ask-f", itemId: "f", connectionId: "custom", connectionName: "Reports", toolName: "read", state: "allowed_once", requestedAtMs: 1 };
  const parts = turnTranscript({ startedAtMs: 1, items, approvals: [approval] }, [], false);
  expect(parts.map(part => part.kind === "work" ? part.progress.items.map(item => item.id) : part.id))
    .toEqual([["a", "b", "c", "d"], "note", ["e", "f", "g", "h"]]);
});

it("keeps each approval and result with its tools across two assistant updates", () => {
  const first = evidenceFixture();
  const second = evidenceFixture({ id: "ev_2" });
  const action = (id: string, result: string): ChatProgressItem => ({
    id, kind: "tool", category: "web", operation: "read", action: "Read source", status: "succeeded", resultIds: [result],
  });
  const approval: ChatApproval = { requestId: "ask", itemId: "b", connectionId: "custom", connectionName: "Filings", toolName: "read_filing", target: "Q3", state: "allowed_once", requestedAtMs: 3 };
  const note = { kind: "commentary" as const, id: "note", text: "One more check." };
  const a = action("a", first.id), b = action("b", second.id);
  const parts = turnTranscript({ startedAtMs: 1, items: [a, note, b], approvals: [approval] }, [second, first], true);
  expect(parts).toMatchObject([
    { kind: "work", evidence: [first], progress: { items: [a] } },
    note,
    { kind: "work", evidence: [second], progress: { items: [b], approvals: [approval] } },
  ]);
  expect(parts[0]).not.toHaveProperty("progress.approvals");
});

it("counts distinct linked sources per stage, excluding search hits, command output and unattributed receipts", () => {
  const original = evidenceFixture({ id: "source" });
  const sameSource = { ...original, id: "second-receipt" };
  const hit = evidenceFixture({ id: "hit", kind: "discovery" });
  const output = evidenceFixture({ id: "result", kind: "derived" });
  const unrelated = evidenceFixture({ id: "outside", title: "Unrelated filing" });
  const step = { id: "fetch", kind: "tool" as const, action: "Read report", operation: "read" as const, category: "web" as const,
    status: "succeeded" as const, resultIds: ["hit", "result", "source", "second-receipt"] };
  const parts = turnTranscript({ startedAtMs: 0, items: [step] }, [output, hit], false, [unrelated, hit, original, sameSource, output]);
  expect(parts).toMatchObject([{ sources: [original], evidence: [output, hit] }]);
});
