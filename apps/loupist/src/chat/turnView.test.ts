import i18n from "@/i18n";
import type { ChatMessage, ChatProgressItem } from "../types";
import { evidenceFixture } from "../test/evidenceFixtures";
import { deriveTurnStatus, deriveTurnView, isShortNote, receiptDuration, type TurnInput, type TurnView } from "./turnView";

const start = Date.UTC(2026, 9, 31, 9, 0, 0);
const now = start + 12_000;
const message = (extra: Partial<ChatMessage> = {}): ChatMessage =>
  ({ id: "answer", role: "assistant", status: "streaming", content: "", created_at: new Date(start).toISOString(), ...extra });
const view = (msg: ChatMessage, extra: Partial<Omit<TurnInput, "message">> = {}): TurnView =>
  deriveTurnView({ message: msg, now, lastActivityAt: now, stopping: false, retry: { attempt: 1, offered: false }, ...extra });

type Tool = Extract<ChatProgressItem, { kind: "tool" | "system" }>;
const step = (id: string, extra: Partial<Tool> = {}): Tool => ({ id, kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", status: "succeeded", startedAtMs: start + 1_000, finishedAtMs: start + 2_000, target: null, href: null, ...extra });
const search = step("search");
const reading = step("read", { toolName: "web_fetch", operation: "read", action: "Read a web source", running: "Reading hanul.example", status: "running", resultIds: [`ev_${"a".repeat(32)}`], target: "Q3 review · hanul.example", finishedAtMs: null });
const read: Tool = { ...reading, status: "succeeded", finishedAtMs: start + 4_000 };
const draft: ChatProgressItem = { id: "commentary-9", kind: "commentary", text: "Margin fell while" };
const note = (id: string, text = "I’ll read the report first."): ChatProgressItem => ({ id, kind: "commentary", text });

const keys = (turn: TurnView) => turn.segments.map((segment) => `${segment.kind}:${segment.key}`);
const prose = (turn: TurnView) => turn.segments.flatMap((segment) => segment.kind === "prose" ? [segment] : []);
const ledger = (turn: TurnView) => turn.segments.find((segment) => segment.kind === "ledger");
const line = (turn: TurnView) => turn.footer.kind === "live" ? turn.footer.line : null;
const writing = (id: string, answerLength = 0) => ({ id, answerLength, settled: false });

afterEach(async () => { await i18n.changeLanguage("en"); });

describe("the live line", () => {
  it.each<[string, ChatMessage, Partial<Omit<TurnInput, "message">>, { label: string; target?: string | null; glyph: string; sheen: boolean; more?: number }]>([
    ["no signal from the host yet", message(), {}, { label: "Starting", glyph: "phase", sheen: true }],
    ["thinking", message({ phase: "thinking" }), {}, { label: "Thinking", glyph: "phase", sheen: true }],
    ["between steps", message({ phase: "tool", progress: { startedAtMs: start, items: [search, read], finishedAtMs: null } }), {}, { label: "Thinking", glyph: "phase", sheen: true }],
    ["a step running", message({ phase: "tool", progress: { startedAtMs: start, items: [search, reading], finishedAtMs: null } }), {},
      { label: "Reading a web source", target: "Q3 review · hanul.example", glyph: "web", sheen: true }],
    ["a long step, 15 s without host activity", message({ phase: "tool", progress: { startedAtMs: start, items: [reading], finishedAtMs: null } }), { lastActivityAt: now - 16_000 },
      { label: "Reading a web source", target: "Still working", glyph: "web", sheen: true }],
    ["silence with no step", message({ phase: "thinking" }), { lastActivityAt: now - 16_000 }, { label: "Still working", glyph: "phase", sheen: true }],
    ["writing", message({ phase: "writing", draft: writing("commentary-9"), progress: { startedAtMs: start, items: [search, draft], finishedAtMs: null } }), {}, { label: "Writing", glyph: "writing", sheen: true }],
    ["the host finished but the last words are still revealed", message({ status: "complete", content: "Done." }), { holding: true }, { label: "Writing", glyph: "writing", sheen: true }],
    ["stop requested", message({ phase: "tool", progress: { startedAtMs: start, items: [reading], finishedAtMs: null } }), { stopping: true }, { label: "Stopping", glyph: "stopping", sheen: false }],
  ])("%s", (_name, msg, extra, expected) => {
    expect(line(view(msg, extra))).toMatchObject({ target: null, more: 0, ...expected });
  });

  it("names the newest of three running steps and counts the others", () => {
    const steps = ["a", "b", "c"].map((id, index) => step(id, { toolName: "dart_financials", category: "finance", operation: "read", status: "running", target: `Company ${index}`, finishedAtMs: null }));
    expect(line(view(message({ phase: "tool", progress: { startedAtMs: start, items: steps, finishedAtMs: null } })))).toMatchObject({ target: "Company 2", more: 2, glyph: "finance" });
  });

  it("waits for the reader in amber words when a question or approval waits, and never says a step failed", () => {
    const asked = view(message({ phase: "tool", progress: { startedAtMs: start, items: [reading], questions: [
      { requestId: "ask", itemId: "read", question: "Which period?", options: [{ label: "Quarterly" }], state: "waiting", requestedAtMs: now - 2_000 },
    ], finishedAtMs: null } }));
    expect(asked.state.kind).toBe("awaiting_approval");
    expect(line(asked)).toMatchObject({ kind: "waiting", label: "Waiting for you", glyph: "waiting", sheen: false });
    const failed = view(message({ phase: "tool", progress: { startedAtMs: start, items: [{ ...read, status: "failed" }], finishedAtMs: null } }));
    expect(line(failed)?.label).toBe("Thinking");
  });

  it("uses the catalog's label for a known tool, in Korean too, whatever the host wrote", async () => {
    await i18n.changeLanguage("ko");
    const live = line(view(message({ phase: "tool", progress: { startedAtMs: start, items: [reading], finishedAtMs: null } })));
    expect(live?.label).toBe("웹 자료 읽는 중");
    expect(live?.label).not.toMatch(/[A-Za-z]/u);
    expect(line(view(message()))?.label).toBe("시작하는 중");
    expect(line(view(message({ phase: "writing" })))?.label).toBe("답변 작성 중");
  });

  it("does not count or name the plan and the question tool", () => {
    const plan = step("plan", { toolName: "plan_update", category: "system", operation: "generic", status: "running", finishedAtMs: null });
    const turn = view(message({ phase: "tool", progress: { startedAtMs: start, items: [search, plan], finishedAtMs: null } }));
    expect(line(turn)?.label).toBe("Thinking");
    expect(ledger(turn)).toMatchObject({ ledger: { text: "1 step" } });
  });

  it("agrees with the cheap status the pill redraws every second", () => {
    const msg = message({ phase: "tool", progress: { startedAtMs: start, items: [search, reading], finishedAtMs: null } });
    const turn = view(msg);
    expect(deriveTurnStatus({ message: msg, now, lastActivityAt: now, stopping: false, retry: { attempt: 1, offered: false } })).toEqual({ state: turn.state, footer: turn.footer });
  });

  it("announces the label and target while live, and the receipt once settled", () => {
    expect(view(message({ phase: "tool", progress: { startedAtMs: start, items: [reading], finishedAtMs: null } })).announce).toBe("Reading a web source · Q3 review · hanul.example");
    expect(view(message({ status: "complete", content: "Done.", progress: { startedAtMs: start, finishedAtMs: start + 30_000, items: [search] } })).announce).toBe("Worked 30s · 1 step");
  });
});

describe("the ledger", () => {
  it("is one per turn, however notes and batches of steps alternate", () => {
    const items: ChatProgressItem[] = [note("n0", "First I’ll read the notes, then the filings and the review, and check everything against the segment figures in some detail before writing anything at all, because the numbers in the review and in the filing need to agree with each other before they can be compared with anything else in a useful way, which takes a while."), step("a"), step("b"), note("n1", "Now the filings."), step("c"), note("n2", "Then the review."), step("d"), step("e")];
    const turn = view(message({ status: "complete", content: "Done.", progress: { startedAtMs: start, finishedAtMs: start + 5_000, items } }));
    expect(turn.segments.filter((segment) => segment.kind === "ledger")).toHaveLength(1);
    // The ledger stands where the first step happened; a short note gives its place to the next text.
    expect(keys(turn)).toEqual(["prose:n0", "ledger:activity", "prose:answer"]);
    expect(ledger(turn)?.progress.items.map((item) => item.id)).toEqual(["n0", "a", "b", "n1", "c", "n2", "d", "e"]);
  });

  it("counts steps while running, appends failed attempts in plain words, and becomes the receipt", () => {
    const failed = step("f", { status: "failed" });
    const running = view(message({ phase: "tool", progress: { startedAtMs: start, items: [search, failed, reading], finishedAtMs: null } }));
    expect(ledger(running)).toMatchObject({ live: true, ledger: { text: "3 steps", failed: "1 failed attempt", settled: false } });
    const evidence = [evidenceFixture({ id: `ev_${"a".repeat(32)}`, url: "https://hanul.example/q3" })];
    const done = view(message({ status: "complete", content: "Done.", evidence_refs: evidence, progress: { startedAtMs: start, finishedAtMs: start + 72_000, items: [search, failed, read] } }));
    expect(ledger(done)).toMatchObject({ live: false, ledger: { text: "Worked 1m 12s · 3 steps · 1 source", failed: "1 failed attempt", settled: true } });
  });

  it("says how long a stopped reply worked and counts only the steps that finished", () => {
    const stopped = view(message({ status: "aborted", content: "Margin fell", progress: { startedAtMs: start, finishedAtMs: start + 12_000, items: [search, { ...reading, status: "stopped" }] } }));
    expect(ledger(stopped)?.ledger.text).toBe("Stopped after 12s · 1 step");
  });

  it("puts the plan position in place of the count while a plan runs", () => {
    const plan = { items: [
      { id: "one", label: "Read the report", status: "done" as const, activatedAtMs: start },
      { id: "two", label: "Compare inventory", status: "active" as const, activatedAtMs: start + 1 },
      { id: "three", label: "Check your rules", status: "pending" as const },
    ] };
    expect(ledger(view(message({ phase: "tool", progress: { startedAtMs: start, plan, items: [search], finishedAtMs: null } })))?.ledger.text).toBe("Plan 2 of 3 · Compare inventory");
  });

  it("gathers what the answer can cite as chips, in arrival order, counting repeats", () => {
    const site = (id: string) => evidenceFixture({ id, url: "https://dart.fss.or.kr/filing" });
    const statements = step("fin", { toolName: "dart_financials", category: "finance", operation: "read" });
    const notes = step("mem", { toolName: "world_read", category: "memory", operation: "read" });
    const python = step("py", { toolName: "run_python", category: "compute", operation: "execute" });
    const first = step("s1", { toolName: "web_fetch", operation: "read", resultIds: ["e1"] });
    const second = step("s2", { toolName: "web_fetch", operation: "read", resultIds: ["e2"] });
    const turn = view(message({ phase: "tool", evidence_refs: [site("e1"), { ...site("e2"), title: "Another filing" }], progress: { startedAtMs: start, items: [notes, first, statements, second, python, { ...statements, id: "fin2", status: "failed" }], finishedAtMs: null } }));
    expect(ledger(turn)?.ledger.chips.map((chip) => [chip.kind, chip.label, chip.count])).toEqual([
      ["memory", "Saved notes", 1], ["source", "dart.fss.or.kr", 2], ["data", "Financial statements", 1], ["calculation", "Calculations", 1],
    ]);
  });

  it("gives no chip to loading a research tool, which reads nothing the answer can cite", () => {
    const load = step("load", { toolName: "capability_load", category: "finance", operation: "read", action: "Prepared a research tool" });
    const statements = step("fin", { toolName: "dart_financials", category: "finance", operation: "read" });
    const turn = view(message({ phase: "tool", progress: { startedAtMs: start, items: [load, statements], finishedAtMs: null } }));
    expect(ledger(turn)?.ledger.chips.map((chip) => chip.label)).toEqual(["Financial statements"]);
  });

  it("never opens by itself and has no alarm state", () => {
    const turn = view(message({ status: "complete", content: "Done.", progress: { startedAtMs: start, finishedAtMs: start + 100, items: [{ ...search, status: "failed" }] } }));
    expect(ledger(turn)).not.toHaveProperty("autoExpand");
    expect(ledger(turn)).not.toHaveProperty("tone");
  });

  it("has no ledger for a fast answer with no steps", () => {
    expect(ledger(view(message({ status: "complete", content: "Done." })))).toBeUndefined();
  });

  it("writes durations as words", async () => {
    expect([receiptDuration(400), receiptDuration(34_200), receiptDuration(72_000)]).toEqual(["1s", "34s", "1m 12s"]);
    await i18n.changeLanguage("ko");
    expect(receiptDuration(72_000)).toBe("1분 12초");
  });
});

describe("segments, in the order they happened", () => {
  it("keys the live answer by its draft and keeps that key once it settles, in the store's copy too", () => {
    const preamble = note("commentary-0", "I’ll start with the notes. ".repeat(12));
    const live = view(message({ phase: "writing", draft: writing("commentary-9"), progress: { startedAtMs: start, items: [preamble, search, read, draft], finishedAtMs: null } }));
    expect(keys(live)).toEqual(["prose:commentary-0", "ledger:activity", "prose:commentary-9"]);
    expect(prose(live)).toMatchObject([{ tone: "note", short: false, streaming: false }, { tone: "answer", streaming: true, text: "Margin fell while" }]);
    const settled = message({ status: "complete", content: "Margin fell while revenue grew.", answer_key: "commentary-9", progress: { startedAtMs: start, finishedAtMs: start + 9_000, items: [preamble, search, read] } });
    expect(keys(view(settled))).toEqual(keys(live));
    expect(keys(view({ ...settled, answer_key: undefined }))).toEqual(["prose:commentary-0", "ledger:activity", "prose:answer"]);
  });

  it("joins a second answer turn to the settled text under the answer's key", () => {
    const second = note("commentary-4", "Then the inventory.");
    const turn = view(message({ phase: "writing", content: "First answer.", answer_key: "commentary-1", draft: writing("commentary-4", "First answer.".length), progress: { startedAtMs: start, items: [search, second], finishedAtMs: null } }));
    expect(prose(turn)).toEqual([{ kind: "prose", key: "commentary-1", text: "First answer.\n\nThen the inventory.", tone: "answer", streaming: true }]);
  });

  it("turns a draft a tool call followed into a note, in place, under the same key", () => {
    const preamble = note("commentary-1");
    const live = view(message({ phase: "writing", draft: writing("commentary-1"), progress: { startedAtMs: start, items: [preamble], finishedAtMs: null } }));
    const closed = view(message({ phase: "tool", draft: writing("commentary-1"), progress: { startedAtMs: start, items: [preamble, reading], finishedAtMs: null } }));
    expect(prose(live)[0]).toMatchObject({ key: "commentary-1", tone: "answer" });
    expect(prose(closed)[0]).toMatchObject({ key: "commentary-1", tone: "note", short: true });
  });

  it("lets a short note give its place to the text after it; a long note stays as content", () => {
    const short = note("n1", "Checking the filing.");
    const replaced = view(message({ phase: "writing", content: "Revenue rose 8%.", answer_key: "c3", draft: { id: "c3", answerLength: 0, settled: true }, progress: { startedAtMs: start, items: [search, short], finishedAtMs: null } }));
    expect(keys(replaced)).toEqual(["ledger:activity", "prose:c3"]);
    expect(ledger(replaced)?.progress.items.map((item) => item.id)).toContain("n1");
    const long = note("n1", "Checking the filing.\n\nThe segment table is on page twelve.");
    expect(keys(view(message({ status: "complete", content: "Revenue rose 8%.", progress: { startedAtMs: start, finishedAtMs: start + 1, items: [search, long] } }))))
      .toEqual(["ledger:activity", "prose:n1", "prose:answer"]);
    expect([isShortNote("One line."), isShortNote("- a list"), isShortNote("x".repeat(300)), isShortNote("| a | b |")]).toEqual([true, false, false, false]);
  });

  it("copies every note left on the page and the answer, in page order, but not a note that gave its place", () => {
    const long = note("n1", "Checking the filing.\n\nThe segment table is on page twelve.");
    const short = note("n2", "Now the margins.");
    const done = view(message({ status: "complete", content: "Revenue rose 8%.", progress: { startedAtMs: start, finishedAtMs: start + 1, items: [long, search, short, read] } }));
    expect(keys(done)).toEqual(["prose:n1", "ledger:activity", "prose:answer"]);
    expect(done.footer).toMatchObject({ copyText: "Checking the filing.\n\nThe segment table is on page twelve.\n\nRevenue rose 8%." });
    const notesOnly = view(message({ status: "aborted", content: "", progress: { startedAtMs: start, finishedAtMs: start + 1, items: [long, search] } }));
    expect(notesOnly.footer).toMatchObject({ copyText: "Checking the filing.\n\nThe segment table is on page twelve." });
  });

  it("places a card where its publish step started, so a card made before the answer stands above it, live and after reload", () => {
    const chart = step("chart", { toolName: "chart_publish", category: "artifact", operation: "publish", status: "running", target: "Margins", finishedAtMs: null });
    const live = view(message({ phase: "writing", draft: writing("commentary-9"), progress: { startedAtMs: start, items: [search, chart, draft], finishedAtMs: null } }));
    expect(keys(live)).toEqual(["ledger:activity", "card:card:chart", "prose:commentary-9"]);
    const settled = view(message({ status: "complete", content: "Margin fell.", answer_key: "commentary-9", progress: { startedAtMs: start, finishedAtMs: start + 1, items: [search, { ...chart, status: "succeeded" }] } }));
    expect(keys(settled)).toEqual(keys(live));
  });

  it("places a question where it was asked and keeps it there when it settles", () => {
    const ask = step("ask-step", { toolName: "ask_user", category: "capability", operation: "generic", status: "running", finishedAtMs: null });
    const waiting = { requestId: "q", itemId: "ask-step", question: "Which period?", options: [{ label: "Quarterly" }, { label: "Annual" }], state: "waiting" as const, requestedAtMs: 1 };
    const asked = view(message({ phase: "tool", progress: { startedAtMs: start, items: [search, ask, step("after")], questions: [waiting], finishedAtMs: null } }));
    expect(keys(asked)).toEqual(["ledger:activity", "question:question:q"]);
    const answered = view(message({ status: "complete", content: "Done.", progress: { startedAtMs: start, finishedAtMs: start + 1, items: [search, { ...ask, status: "succeeded" }, step("after")], questions: [{ ...waiting, state: "answered", answer: { choice: 1 } }] } }));
    expect(keys(answered)).toEqual(["ledger:activity", "question:question:q", "prose:answer"]);
    expect(ledger(answered)?.ledger.text).toBe("Worked 1s · 2 steps");
  });

  it("places an approval after the step that waits for it", () => {
    const approval = { requestId: "ok", itemId: "read", connectionId: "web", connectionName: "Web", toolName: "web_fetch", state: "waiting" as const, requestedAtMs: now };
    expect(keys(view(message({ phase: "tool", progress: { startedAtMs: start, items: [search, reading], approvals: [approval], finishedAtMs: null } }))))
      .toEqual(["ledger:activity", "approval:approval:ok"]);
  });

  it("places a plan-only ledger before the live answer, where it stays once the answer settles", () => {
    const live: ChatProgressItem = { id: "commentary-5", kind: "commentary", text: "Revenue rose" };
    const plan = { items: [{ id: "read", label: "Read the report", status: "active" as const, activatedAtMs: start + 1 }] };
    const running = view(message({ phase: "writing", draft: writing("commentary-5"), progress: { startedAtMs: start, plan, items: [live], finishedAtMs: null } }));
    expect(keys(running)).toEqual(["ledger:activity", "prose:commentary-5"]);
    const settled = view(message({ status: "complete", content: "Revenue rose 8%.", answer_key: "commentary-5", progress: { startedAtMs: start, finishedAtMs: start + 100, plan: { items: [{ ...plan.items[0]!, status: "done" }] }, items: [] } }));
    expect(keys(settled)).toEqual(keys(running));
  });

  it("keeps each source with the ledger, excluding search hits and command output from the count", () => {
    const original = evidenceFixture({ id: "source" });
    const hit = evidenceFixture({ id: "hit", kind: "discovery" });
    const output = evidenceFixture({ id: "result", kind: "derived", tool_name: "calculate" });
    const fetch = step("fetch", { operation: "read", resultIds: ["hit", "result", "source"] });
    const turn = view(message({ status: "complete", content: "Done.", evidence_refs: [hit, original, output], progress: { startedAtMs: start, finishedAtMs: start + 100, items: [fetch] } }));
    expect(ledger(turn)).toMatchObject({ sources: [original], evidence: [hit, output] });
  });
});

describe("endings", () => {
  it("counts a busy answer down to its own retry, then calls it failed", () => {
    const busy = message({ status: "error", content: "", failure_kind: "rate_limited" });
    expect(view(busy, { retry: { attempt: 1, offered: true } }).footer).toMatchObject({ runState: { kind: "failed", failure: "rate_limited", retryInSeconds: 20 } });
    expect(view(busy, { retry: { attempt: 2, offered: true } }).footer).toMatchObject({ runState: { retryInSeconds: 40 } });
    expect(view({ ...busy, retry_after_seconds: 12 }, { retry: { attempt: 2, offered: true } }).footer).toMatchObject({ runState: { retryInSeconds: 12 } });
    expect(view(busy, { retry: { attempt: 3, offered: true } }).footer).toMatchObject({ runState: { kind: "failed", retryInSeconds: undefined } });
  });

  it("words what a stopped reply kept", () => {
    const steps = [search, read];
    expect(view(message({ status: "aborted", content: "Margin fell while", progress: { startedAtMs: start, items: steps, finishedAtMs: null } })).footer).toMatchObject({ runState: { kind: "stopped", label: "Stopped · kept 2 steps and the text so far" } });
    expect(view(message({ status: "aborted", content: "Response stopped.", progress: { startedAtMs: start, items: [...steps, draft], finishedAtMs: null } })).state).toEqual({ kind: "stopped", kept: "steps_and_draft" });
    expect(view(message({ status: "aborted", content: "" })).state).toEqual({ kind: "stopped", kept: "nothing" });
  });

  it("hides the failure placeholder, keeps a partial answer, and says what was kept", () => {
    expect(prose(view(message({ status: "error", content: "Response could not be completed.", placeholder: true, failure_kind: "other" })))).toEqual([]);
    const partial = view(message({ status: "error", content: "The observed revenue increased.", failure_kind: "other" }));
    expect(prose(partial)).toMatchObject([{ text: "The observed revenue increased.", tone: "answer", streaming: false }]);
    expect(partial.footer).toMatchObject({ copyText: "The observed revenue increased.", feedback: false, runState: { kind: "failed", kept: true } });
    expect(view(message({ status: "error", content: "x", placeholder: true, failure_kind: "other" })).footer).toMatchObject({ runState: { kept: false } });
  });
});
