import type { ChatApproval, ChatMessage, ChatProgressItem } from "../types";
import { evidenceFixture } from "../test/evidenceFixtures";
import { deriveTurnView, type TurnInput, type TurnView } from "./turnView";

const start = Date.UTC(2026, 9, 31, 9, 0, 0);
const now = start + 12_000;
const message = (extra: Partial<ChatMessage> = {}): ChatMessage =>
  ({ id: "answer", role: "assistant", status: "streaming", content: "", created_at: new Date(start).toISOString(), ...extra });
const view = (msg: ChatMessage, extra: Partial<Omit<TurnInput, "message">> = {}): TurnView =>
  deriveTurnView({ message: msg, now, lastActivityAt: now, stopping: false, retryPending: false, ...extra });

const search: ChatProgressItem = { id: "search", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", status: "succeeded", startedAtMs: start + 1_000, finishedAtMs: start + 2_000 };
const reading: ChatProgressItem = { id: "read", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", running: "Reading hanul.example", status: "running", startedAtMs: start + 3_000, resultIds: [`ev_${"a".repeat(32)}`] };
const read: ChatProgressItem = { ...reading, status: "succeeded", finishedAtMs: start + 4_000 };
const draft: ChatProgressItem = { id: "commentary-9", kind: "commentary", text: "Margin fell while" };

const rows = (turn: TurnView) => turn.segments.flatMap((segment) => segment.kind === "activity" ? [segment.row] : []);
const prose = (turn: TurnView) => turn.segments.flatMap((segment) => segment.kind === "prose" ? [segment] : []);

describe("state × slot", () => {
  it.each<[string, ChatMessage, Partial<Omit<TurnInput, "message">>, { state: TurnView["state"]["kind"]; footer: string; rows: string[] }]>([
    ["starting", message(), {}, { state: "starting", footer: "status:Waiting for the model", rows: [] }],
    ["working on a step", message({ phase: "tool", progress: { startedAtMs: start, items: [search, reading] } }), {},
      { state: "working", footer: "status:Working", rows: ["live:Reading hanul.example"] }],
    ["working, stalled", message({ phase: "tool", progress: { startedAtMs: start, items: [search, reading] } }), { lastActivityAt: now - 16_000 },
      { state: "working", footer: "status:Still working", rows: ["live:Reading hanul.example:stalled"] }],
    ["working between steps", message({ phase: "thinking", progress: { startedAtMs: start, items: [search, read] } }), {},
      { state: "working", footer: "status:Thinking it through", rows: ["summary:Read 1 source · hanul.example"] }],
    ["awaiting approval", message({ phase: "tool", progress: { startedAtMs: start, items: [search, reading], approvals: [
      { requestId: "req", itemId: "read", connectionId: "web", connectionName: "Web", toolName: "web_fetch", state: "waiting", requestedAtMs: now - 2_000 },
    ] } }), {}, { state: "awaiting_approval", footer: "status:Waiting for you", rows: ["live:Reading hanul.example"] }],
    ["writing", message({ phase: "writing", progress: { startedAtMs: start, items: [search, read, draft] } }), {},
      { state: "writing", footer: "status:Writing", rows: ["summary:Read 1 source · hanul.example"] }],
    ["stopping", message({ phase: "writing", progress: { startedAtMs: start, items: [search, read, draft] } }), { stopping: true },
      { state: "stopping", footer: "status:Stopping", rows: ["summary:Read 1 source · hanul.example"] }],
    ["complete", message({ status: "complete", content: "Done.", progress: { startedAtMs: start, finishedAtMs: start + 64_000, items: [search, read] } }), {},
      { state: "complete", footer: "settled", rows: ["summary:Read 1 source · hanul.example:1:04"] }],
    ["stopped", message({ status: "aborted", content: "Margin fell while", progress: { startedAtMs: start, finishedAtMs: start + 5_000, items: [search, { ...reading, status: "stopped" }] } }), {},
      { state: "stopped", footer: "settled:stopped", rows: ["summary:Read 1 source · hanul.example · 1 stopped:0:05"] }],
    ["failed", message({ status: "error", content: "Response could not be completed.", failure_kind: "rate_limited" }), { retryPending: true },
      { state: "failed", footer: "settled:failed", rows: [] }],
  ])("%s", (_name, msg, extra, expected) => {
    const turn = view({ ...msg, evidence_refs: msg.progress?.items.some((item) => item.id === "read") ? [evidenceFixture({ url: "https://hanul.example/q3" })] : [] }, extra);
    expect(turn.state.kind).toBe(expected.state);
    expect(turn.footer.kind === "status" ? `status:${turn.footer.label}` : `settled${turn.footer.runState ? `:${turn.footer.runState.kind}` : ""}`).toBe(expected.footer);
    expect(rows(turn).map((row) => row.kind === "live"
      ? `live:${row.label}${row.stalled ? ":stalled" : ""}`
      : `summary:${[row.label, row.issues].filter(Boolean).join(" · ")}${row.durationMs !== undefined ? `:${Math.floor(row.durationMs / 60_000)}:${String(Math.floor(row.durationMs / 1_000) % 60).padStart(2, "0")}` : ""}`,
    )).toEqual(expected.rows);
  });

  it("keeps the activity row's citation count out of its own sources until the step linked them", () => {
    const turn = view(message({ status: "complete", content: "Done.", evidence_refs: [evidenceFixture()], progress: { startedAtMs: start, finishedAtMs: start + 500, items: [search] } }));
    expect(rows(turn)).toEqual([{ kind: "summary", label: "Source review", issues: null, durationMs: 500 }]);
    expect(turn.footer).toMatchObject({ kind: "settled", sourceCount: 1, copyText: "Done.", feedback: true, runState: null });
  });

  it("words what a stopped reply kept and shows a draft as narration", () => {
    const steps = [search, read];
    expect(view(message({ status: "aborted", content: "Margin fell while", progress: { startedAtMs: start, items: steps } })).state).toEqual({ kind: "stopped", kept: "steps_and_text" });
    expect(view(message({ status: "aborted", content: "Response stopped.", progress: { startedAtMs: start, items: [...steps, draft] } })).state).toEqual({ kind: "stopped", kept: "steps_and_draft" });
    expect(view(message({ status: "aborted", content: "Response stopped.", progress: { startedAtMs: start, items: steps } })).state).toEqual({ kind: "stopped", kept: "steps" });
    expect(view(message({ status: "aborted", content: "First.\n\nResponse stopped." })).state).toEqual({ kind: "stopped", kept: "text" });
    const stoppedDraft = view(message({ status: "aborted", content: "Response stopped.", progress: { startedAtMs: start, items: [draft] } }));
    expect(stoppedDraft.state).toEqual({ kind: "stopped", kept: "draft" });
    expect(prose(stoppedDraft)).toEqual([{ kind: "prose", key: "commentary-9", text: "Margin fell while", tone: "narration", streaming: false }]);
    expect(stoppedDraft.footer).toMatchObject({ copyText: null, feedback: false, runState: { kind: "stopped" } });
    expect(view(message({ status: "aborted", content: "" })).state).toEqual({ kind: "stopped", kept: "nothing" });
  });

  it("hides the failure placeholder and keeps a partial answer copyable", () => {
    expect(prose(view(message({ status: "error", content: "Response could not be completed.", failure_kind: "other" })))).toEqual([]);
    const partial = view(message({ status: "error", content: "The observed revenue increased.", failure_kind: "other" }));
    expect(prose(partial)).toMatchObject([{ text: "The observed revenue increased.", tone: "answer", streaming: false }]);
    expect(partial.footer).toMatchObject({ copyText: "The observed revenue increased.", feedback: false, runState: { kind: "failed" } });
  });
});

describe("segments", () => {
  const keys = (turn: TurnView) => turn.segments.map((segment) => `${segment.kind}:${segment.key}`);
  const writing = (id: string, answerLength = 0) => ({ id, answerLength });

  it("draws the draft as the answer under one key, which the settled text keeps with no draft recorded", () => {
    const preamble: ChatProgressItem = { id: "commentary-0", kind: "commentary", text: "I’ll start with the notes." };
    const live = view(message({ phase: "writing", draft: writing("commentary-9"), progress: { startedAtMs: start, items: [preamble, search, read, draft] } }));
    expect(keys(live)).toEqual(["prose:commentary-0", "activity:activity", "prose:answer"]);
    expect(prose(live)).toMatchObject([{ tone: "narration", streaming: false }, { tone: "answer", streaming: true, text: "Margin fell while" }]);
    // The message a reconcile reads back from the store carries no draft.
    const settled = view(message({ status: "complete", content: "Margin fell while revenue grew.", progress: { startedAtMs: start, finishedAtMs: start + 9_000, items: [preamble, search, read] } }));
    expect(keys(settled)).toEqual(keys(live));
    expect(prose(settled).at(-1)).toMatchObject({ text: "Margin fell while revenue grew.", streaming: false });
  });

  it("joins a second answer turn to the settled text under the answer's key", () => {
    const second: ChatProgressItem = { id: "commentary-4", kind: "commentary", text: "Then the inventory." };
    const turn = view(message({ phase: "writing", content: "First answer.", draft: writing("commentary-4", "First answer.".length), progress: { startedAtMs: start, items: [search, second] } }));
    expect(prose(turn)).toEqual([{ kind: "prose", key: "answer", text: "First answer.\n\nThen the inventory.", tone: "answer", streaming: true }]);
  });

  it("shows the settled text once while the draft it replaces is still listed", () => {
    const turn = view(message({ phase: "writing", content: "Margin fell while revenue grew.", draft: writing("commentary-9"), progress: { startedAtMs: start, items: [search, draft] } }));
    expect(prose(turn)).toEqual([{ kind: "prose", key: "answer", text: "Margin fell while revenue grew.", tone: "answer", streaming: true, checks: undefined }]);
  });

  it("keeps older narration above the answer after the draft settled and left", () => {
    const narration: ChatProgressItem = { id: "commentary-2", kind: "commentary", text: "Checking the filing." };
    const turn = view(message({ phase: "writing", content: "Revenue rose 8%.", draft: writing("commentary-3"), progress: { startedAtMs: start, items: [search, narration] } }));
    expect(prose(turn)).toMatchObject([{ key: "commentary-2", tone: "narration", text: "Checking the filing." }, { key: "answer", tone: "answer", text: "Revenue rose 8%." }]);
  });

  it("places a plan-only row before the live answer, where it stays once the answer settles", () => {
    const narration: ChatProgressItem = { id: "commentary-2", kind: "commentary", text: "Checking the filing." };
    const live: ChatProgressItem = { id: "commentary-5", kind: "commentary", text: "Revenue rose" };
    const plan = { items: [{ id: "read", label: "Read the report", status: "active" as const, activatedAtMs: start + 1 }] };
    const running = view(message({ phase: "writing", draft: writing("commentary-5"), progress: { startedAtMs: start, plan, items: [narration, live] } }));
    expect(keys(running)).toEqual(["prose:commentary-2", "activity:activity", "prose:answer"]);
    const settled = view(message({ status: "complete", content: "Revenue rose 8%.", progress: { startedAtMs: start, finishedAtMs: start + 100, plan: { items: [{ ...plan.items[0]!, status: "done" }] }, items: [narration] } }));
    expect(keys(settled)).toEqual(keys(running));
  });

  it("renders answer text sent as deltas while streaming, under the answer key", () => {
    const turn = view(message({ phase: "writing", content: "Utilization rose." }));
    expect(prose(turn)).toEqual([{ kind: "prose", key: "answer", text: "Utilization rose.", tone: "answer", streaming: true, checks: undefined }]);
  });

  it("keeps mixed calls together across updates, with a new row after public prose", () => {
    const file = { kind: "tool" as const, toolName: "read", category: "files" as const, operation: "read" as const, action: "Read file", status: "succeeded" as const };
    const note = { kind: "commentary" as const, id: "note", text: "Next source." };
    const items: ChatProgressItem[] = [
      { ...file, id: "a" }, { ...file, id: "b" }, { ...file, id: "c", status: "failed" },
      { ...file, id: "d" }, note, { ...file, id: "e" }, { ...file, id: "f" },
    ];
    const approval: ChatApproval = { requestId: "ask-f", itemId: "f", connectionId: "custom", connectionName: "Reports", toolName: "read", state: "allowed_once", requestedAtMs: 1 };
    const turn = view(message({ status: "complete", content: "Done.", progress: { startedAtMs: start, finishedAtMs: start + 100, items, approvals: [approval] } }));
    expect(turn.segments.map((segment) => segment.kind === "activity" ? segment.progress.items.map((item) => item.id) : segment.key))
      .toEqual([["a", "b", "c", "d"], "note", ["e", "f"], "answer"]);
    const [first, , second] = turn.segments;
    expect(first).toMatchObject({ key: "activity", autoExpand: true, row: { issues: "1 failed attempt" } });
    expect(second).toMatchObject({ key: "step-e", autoExpand: false, progress: { approvals: [approval] } });
    expect(first).not.toHaveProperty("progress.approvals");
  });

  it("keeps each result and source with its step, excluding search hits and command output from the count", () => {
    const original = evidenceFixture({ id: "source" });
    const sameSource = { ...original, id: "second-receipt" };
    const hit = evidenceFixture({ id: "hit", kind: "discovery" });
    const output = evidenceFixture({ id: "result", kind: "derived", tool_name: "calculate" });
    const unrelated = evidenceFixture({ id: "outside", title: "Unrelated filing" });
    const step: ChatProgressItem = { id: "fetch", kind: "tool", action: "Read report", operation: "read", category: "web", status: "succeeded", resultIds: ["hit", "result", "source", "second-receipt"] };
    const turn = view(message({ status: "complete", content: "Done.", evidence_refs: [unrelated, hit, original, sameSource, output], progress: { startedAtMs: start, finishedAtMs: start + 100, items: [step] } }));
    expect(turn.segments[0]).toMatchObject({ kind: "activity", sources: [original], evidence: [hit, output] });
    expect(turn.footer).toMatchObject({ sourceCount: 2 });
  });

  it("does not open the details for a step paused by the untrusted-text rule", () => {
    const paused: ChatProgressItem = { id: "bash", kind: "tool", toolName: "bash", category: "compute", operation: "execute", action: "Ran a command", status: "failed", refusal: "untrusted_text_paused" };
    const turn = view(message({ status: "complete", content: "Done.", progress: { startedAtMs: start, finishedAtMs: start + 100, items: [search, paused] } }));
    expect(turn.segments[0]).toMatchObject({ autoExpand: false, row: { issues: "1 paused" } });
  });

  it("announces the footer while live and the summary once settled", () => {
    expect(view(message({ phase: "tool", progress: { startedAtMs: start, items: [reading] } })).announce).toBe("Working · Reading hanul.example");
    expect(view(message({ status: "complete", content: "Done.", progress: { startedAtMs: start, finishedAtMs: start + 100, items: [search] } })).announce).toBe("Source review");
  });
});
