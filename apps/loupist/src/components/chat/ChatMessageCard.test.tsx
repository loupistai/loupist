import type { ComponentProps } from "react";
import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "@/i18n";
// Citation links open a delayed HoverCard; skipping hover keeps its timer from firing after teardown.
import type { ChatArtifactView, ChatMessage, ChatPerformance, KnowledgeReference, MemoryRef } from "../../types";
import { evidenceFixture } from "../../test/evidenceFixtures";
import { TooltipProvider } from "../ui/tooltip";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { ChatMessageCard } from "./ChatMessageCard";
import { ChatSurfaceProvider } from "./turnMotion";
import { ExtensionAvailabilityProvider } from "../../app/extensionAvailability";
import { renderWithOpen, destinations } from "../../test/open";
import type { ChatDestination, Open } from "../../chat/navigation";
import { onTestFinished, type Mock } from "vitest";
import { previewGridLinkAnswer, SENSITIVITY_EVIDENCE } from "../../preview/korea";

const source: KnowledgeReference = { kind: "source", recordId: "source-record", revisionId: "source-v1", objectId: "source-original", spanId: "passage-1", receiptId: "selected-receipt" };
const read = (read_ref: string, knowledge = source): MemoryRef => ({ read_ref, kind: "source", title: "Annual report", excerpt: "Original passage", access: "exact_read", knowledge, as_of: null, section: null });
const answer = (extra: Partial<ChatMessage> = {}): ChatMessage => ({ id: "answer", role: "assistant", status: "complete", content: "Answer.", created_at: "2026-10-10T00:00:00Z", ...extra });
const user = () => userEvent.setup({ skipHover: true });

function renderAnswer(message: ChatMessage, extra: Partial<ComponentProps<typeof ChatMessageCard>> = {}) {
  const props = { message, loupistId: "loupist-a", chatId: "chat-a", loupistName: "Loupist", onReadAttachment: vi.fn(), ...extra };
  const { open, ...view } = renderWithOpen(<TooltipProvider><ChatMessageCard {...props} /></TooltipProvider>);
  return { ...props, view, open };
}

/** What the answer handed up to the app, of one kind. */
const opened = <K extends ChatDestination["kind"]>(open: Mock<Open>, kind: K) =>
  open.mock.calls.map(([to]) => to).filter((to): to is Extract<ChatDestination, { kind: K }> => to.kind === kind);

it("clears successful copy feedback after two seconds", async () => {
  renderAnswer(answer());
  const copy = await screen.findByRole("button", { name: "Copy answer" });
  vi.useFakeTimers();
  try {
    vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue(undefined);
    await act(async () => { fireEvent.click(copy); });
    expect(screen.getByRole("button", { name: "Answer copied" })).toBeVisible();
    await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });
    expect(screen.getByRole("button", { name: "Copy answer" })).toBeVisible();
  } finally {
    vi.useRealTimers();
  }
});

it("copies the answer as written, keeping the indentation that makes its first block code", async () => {
  const calc = evidenceFixture({ id: "ev_calc", kind: "derived", tool_name: "calculate", calc_operation: "dcf_grid", url: undefined, title: "DCF grid" });
  const content = "    cash_flow = 110.25\n    shares = 34,560,000\n\n[Cash flow](#evidence-ev_calc)\n";
  const { view } = renderAnswer(answer({ content, evidence_refs: [calc] }));
  expect(view.container.querySelector(".message-content pre")).toHaveTextContent("cash_flow = 110.25");
  // user-event replaces the clipboard when it is set up: spy on the one the click writes to.
  const reader = user();
  const write = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue(undefined);
  await reader.click(await screen.findByRole("button", { name: "Copy answer" }));
  expect(write).toHaveBeenCalledWith(content);
});

it("keeps a pending copy across message snapshots with the same id", async () => {
  let resolve!: () => void;
  vi.spyOn(navigator.clipboard, "writeText").mockImplementationOnce(() => new Promise<void>(done => { resolve = done; }));
  const message = answer();
  const { view, ...props } = renderAnswer(message);
  fireEvent.click(await screen.findByRole("button", { name: "Copy answer" }));
  view.rerender(<TooltipProvider><ChatMessageCard {...props} message={{ ...message }} /></TooltipProvider>);
  await act(async () => { resolve(); });
  expect(screen.getByRole("button", { name: "Answer copied" })).toBeVisible();
  view.rerender(<TooltipProvider><ChatMessageCard {...props} message={{ ...message, id: "another-answer" }} /></TooltipProvider>);
  expect(screen.getByRole("button", { name: "Copy answer" })).toBeVisible();
});

it("draws a setup card from a completed connection_setup step and asks the question again once ready", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await bridge.loupistCapabilityDisable({ loupist_id: "loupist-a", entry_id: "world-bank.indicators" });
  const onFillComposer = vi.fn();
  renderAnswer(answer({ content: "World Bank data can be added; finish the card below.", progress: { startedAtMs: 0, items: [
    { id: "setup", kind: "tool", toolName: "connection_setup", category: "capability", operation: "generic", action: "Prepared a connection", target: "world-bank.indicators", source: { kind: "connector", name: "World Bank Indicators" }, status: "succeeded", href: null, startedAtMs: 0, finishedAtMs: null },
    { id: "failed", kind: "tool", toolName: "connection_setup", category: "capability", operation: "generic", action: "Prepared a connection", target: "Bloomberg", status: "failed", href: null, startedAtMs: 0, finishedAtMs: null },
  ], finishedAtMs: null } }), { bridge, onFillComposer, previousQuestion: "How did Korea's GDP grow last year?" });
  const card = await screen.findByRole("region", { name: "Set up World Bank Indicators" });
  expect(screen.getAllByRole("region", { name: /^Set up / })).toHaveLength(1);
  await user().click(await within(card).findByRole("button", { name: "Use in this Loupist" }));
  await user().click(await within(card).findByRole("button", { name: "Ask again" }));
  expect(onFillComposer).toHaveBeenCalledWith("How did Korea's GDP grow last year?");
});

describe("a note as the reply's last action", () => {
  const between = (status: "streaming") => answer({ status, content: "", phase: "thinking", progress: { startedAtMs: 1, finishedAtMs: null, items: [
    { id: "read", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", status: "succeeded", target: null, href: null, startedAtMs: 1, finishedAtMs: 2 },
    { id: "note", kind: "commentary", text: "Revenue matches in both filings. Next I’ll compare margins." },
  ] } });

  it("is not repeated under the note in a session reply: the line reads Thinking", () => {
    renderAnswer(between("streaming"));
    expect(document.querySelector("article")).toHaveTextContent("Revenue matches in both filings. Next I’ll compare margins.");
    expect(screen.getByLabelText("Response status")).toHaveTextContent("Thinking");
    expect(screen.getByLabelText("Response status")).not.toHaveTextContent("compare margins");
  });

  it("is named on Bot's working line, which does not show the note", () => {
    renderWithOpen(<TooltipProvider><ChatSurfaceProvider value={{ kind: "bot", loupist: { id: "loupist-a", name: "Lumi" } }}>
      <ChatMessageCard message={between("streaming")} loupistId="loupist-a" chatId="chat-a" loupistName="Lumi" onReadAttachment={vi.fn()} />
    </ChatSurfaceProvider></TooltipProvider>);
    const working = screen.getByTestId("turn-live");
    expect(working).toHaveAttribute("data-kind", "note");
    expect(working).toHaveTextContent("Next I’ll compare margins.");
    expect(document.querySelector("article")!.querySelector(".message-content")).toBeNull();
  });
});

it.each([false, true])("keeps one stable research summary from first call to settlement (expanded: %s)", async (expanded) => {
  const first = { id: "a", kind: "tool" as const, category: "files" as const, operation: "read" as const, action: "Read file", running: "Reading file", target: "q1.csv", status: "running" as const, startedAtMs: 2, href: null, finishedAtMs: null };
  const second = { ...first, id: "b", target: "q2.csv", startedAtMs: 3 };
  const plan = { items: [{ id: "compare", label: "Compare quarters", status: "active" as const, activatedAtMs: 1 }] };
  const message = (status: "streaming" | "complete", items: NonNullable<ChatMessage["progress"]>["items"]) =>
    answer({ content: status === "complete" ? "Done." : "", status, phase: "tool", progress: { startedAtMs: 1, plan, items, finishedAtMs: null } });
  const { view, ...props } = renderAnswer(message("streaming", [first]));
  // One ledger line: the step count while it runs, even with a plan, the receipt once the run settles; the live line names the step.
  const digest = screen.getByRole("button", { name: "1 step" });
  expect(digest).toHaveAttribute("aria-expanded", "false");
  expect(screen.getByLabelText("Response status")).toHaveTextContent("Reading file");
  expect(within(digest.closest<HTMLElement>("section")!).queryByText("q1.csv")).toBeNull();
  if (expanded) await user().click(digest);
  const both = [{ ...first, status: "succeeded" as const }, second];
  view.rerender(<TooltipProvider><ChatMessageCard {...props} message={message("streaming", both)} /></TooltipProvider>);
  expect(screen.getByRole("button", { name: "2 steps" })).toBe(digest);
  view.rerender(<TooltipProvider><ChatMessageCard {...props} message={message("complete", both.map(item => ({ ...item, status: "succeeded" })))} /></TooltipProvider>);
  expect(screen.getByRole("button", { name: /2 steps/ })).toBe(digest);
  expect(digest).toHaveAttribute("aria-expanded", String(expanded));
  if (!expanded) {
    expect(screen.queryByText("q2.csv")).toBeNull();
    await user().click(digest);
  }
  const details = within(digest.closest<HTMLElement>("section")!);
  expect(details.getByText("q1.csv").compareDocumentPosition(details.getByText("q2.csv")) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});

it("foregrounds the source of a settled multi-stage answer and opens it without fetching tool output", async () => {
  const report = evidenceFixture();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loadDetail = vi.spyOn(bridge, "chatActivityRead");
  const { open } = renderAnswer(answer({ content: "Operating cash flow increased.", evidence_refs: [report], progress: {
    startedAtMs: 0, finishedAtMs: 5,
    plan: { items: [
      { id: "read", label: "Review the filing", status: "done", activatedAtMs: 1 },
      { id: "calc", label: "Reconcile the change", status: "done", activatedAtMs: 3 },
    ] },
    items: [
      { id: "filing", startedAtMs: 2, kind: "tool", operation: "read", category: "web", action: "Read report", status: "succeeded", resultIds: [report.id], target: null, href: null, finishedAtMs: null },
      { id: "cmd", startedAtMs: 4, kind: "tool", operation: "execute", category: "compute", action: "Ran command", status: "succeeded", target: "private-analysis.sh", detailId: "raw-output", href: null, finishedAtMs: null },
    ],
  } }), { bridge });
  const digest = within(screen.getByLabelText("Work progress"));
  const toggle = digest.getByRole("button");
  expect(toggle).toHaveAccessibleName("Worked 1s · 2 steps · 1 source");
  expect(screen.getByText("Operating cash flow increased.")).toBeVisible();
  expect(digest.queryByText("Reconcile the change")).toBeNull();
  expect(digest.queryByText("private-analysis.sh")).toBeNull();
  await user().click(toggle);
  expect(digest.getByText("Reconcile the change")).toBeVisible();
  await user().click(digest.getByRole("button", { name: report.title }));
  expect(opened(open, "evidence")[0]?.evidence).toBe(report);
  expect(loadDetail).not.toHaveBeenCalled();
});

it("lists distinct exact Memory reads in progress without changing the saved receipts", async () => {
  const refs = [
    { ...read("search-result"), access: "search_discovered" as const, title: "Search hit" },
    read("selected-read"),
    read("explicit-read", { ...source, receiptId: "explicit-receipt" }),
    read("other-version", { ...source, revisionId: "source-v2" }),
  ];
  const before = structuredClone(refs);
  const { message, open } = renderAnswer(answer({ memory_refs: refs }));
  // Notes read with no step of their own: the ledger's line says so, and its timeline lists them.
  const toggle = within(screen.getByLabelText("Work progress")).getByRole("button", { name: "Consulted 2 saved notes" });
  await user().click(toggle);
  await user().click(screen.getAllByText("Consulted 2 saved notes").find((element) => !toggle.contains(element))!);
  const used = screen.getByLabelText("Used in this answer");
  expect(within(used).getAllByRole("listitem")).toHaveLength(2);
  expect(within(used).queryByText("Search hit")).toBeNull();
  expect(open).not.toHaveBeenCalled();
  expect(message.memory_refs).toEqual(before);
});

it.each(["error", "aborted"] as const)("keeps Try again beside an inline %s notice", async (status) => {
  const onRetry = vi.fn();
  renderAnswer(answer({ id: "failed", content: "", status }), { onRetry });
  const notice = screen.getByRole(status === "error" ? "alert" : "status");
  await user().click(within(notice).getByRole("button", { name: "Try again" }));
  expect(onRetry).toHaveBeenCalledOnce();
});

it("says the general failure sentence for a failure kind this build does not know", () => {
  const failed = answer({ content: "Response could not be completed.", placeholder: true, status: "error", failure_kind: "quota_reset" as ChatMessage["failure_kind"] });
  renderAnswer(failed, { onRetry: vi.fn() });
  expect(screen.getByRole("alert")).toHaveTextContent(i18n.t("chat:chatMessageCard.other"));
});

it.each(["en", "ko"])("shows one localized failure notice in %s and preserves a partial answer", async (language) => {
  await i18n.changeLanguage(language);
  try {
    const failed = answer({ content: "Response could not be completed.", placeholder: true, status: "error", failure_kind: "other" });
    const { view, ...props } = renderAnswer(failed, { onRetry: vi.fn() });
    expect(screen.getByRole("alert")).toHaveTextContent(i18n.t("chat:chatMessageCard.other"));
    expect(screen.queryByText(failed.content)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: i18n.t("chat:chatMessageCard.copyAnswer") })).not.toBeInTheDocument();
    await user().click(screen.getByRole("button", { name: i18n.t("chat:chatMessageCard.tryAgain") }));
    expect(props.onRetry).toHaveBeenCalledOnce();
    const partial = { ...failed, content: "The observed revenue increased.", placeholder: undefined };
    view.rerender(<TooltipProvider><ChatMessageCard {...props} message={partial} /></TooltipProvider>);
    expect(screen.getByText(partial.content)).toBeVisible();
    expect(screen.getByRole("button", { name: i18n.t("chat:chatMessageCard.copyAnswer") })).toBeVisible();
    expect(screen.getByRole("alert")).toBeVisible();
  } finally {
    await i18n.changeLanguage("en");
  }
});

it("keeps stopped text without exposing the runtime's English stop marker", async () => {
  await i18n.changeLanguage("ko");
  try {
    const { view, ...props } = renderAnswer(answer({ status: "aborted", content: "첫 문장.\n\nResponse stopped." }), { onRetry: vi.fn() });
    expect(screen.getByText("첫 문장.")).toBeVisible();
    expect(screen.queryByText("Response stopped.")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("중지됨 · 지금까지의 글을 남겼습니다");
    view.rerender(<TooltipProvider><ChatMessageCard {...props} message={answer({ status: "aborted", content: "Response stopped." })} /></TooltipProvider>);
    expect(screen.queryByText("Response stopped.")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("답변이 끝나기 전에 중지되었습니다.");
    expect(screen.queryByRole("button", { name: "답변 복사" })).not.toBeInTheDocument();
    view.rerender(<TooltipProvider><ChatMessageCard {...props} message={answer({ status: "aborted", content: "Response stopped.", progress: { startedAtMs: 1, items: [{ id: "draft", kind: "commentary", text: "쓰다 멈춘 문장" }], finishedAtMs: null } })} /></TooltipProvider>);
    expect(screen.getByRole("status")).toHaveTextContent("중지됨 · 쓰던 글을 남겼습니다");
  } finally {
    await i18n.changeLanguage("en");
  }
});

it.each([
  ["/research", "reports/summary.md"],
  [String.raw`\\?\C:\Research`, "c:/research/reports/summary.md"],
  [String.raw`\\?\UNC\Server\Share\Research`, "//server/share/research/reports/summary.md"],
])("opens workspace file links for %s and keeps external links separate", async (workspacePath, href) => {
  const { open } = renderAnswer(answer({ content: `[Report](${href}) and [Source](https://example.com/report).` }), { workspacePath });
  await user().click(await screen.findByRole("link", { name: "Report" }));
  expect(destinations(open)).toEqual([{ kind: "file", path: "reports/summary.md" }]);
  await user().click(screen.getByRole("link", { name: "Source" }));
  expect(destinations(open).at(-1)).toEqual({ kind: "link", url: "https://example.com/report" });
});

it("opens a Lens link from the answer as a Lens record, the one way chat opens Lens", async () => {
  const { open } = renderAnswer(answer({ content: "Use [Cash conversion](lens:rec_0123456789abcdef0123456789abcdef)." }));
  await user().click(await screen.findByRole("link", { name: "Cash conversion" }));
  expect(destinations(open)).toContainEqual({ kind: "lens", recordId: "rec_0123456789abcdef0123456789abcdef" });
});

describe("response details", () => {
  const performance: ChatPerformance = { setupMs: 100, firstTextMs: 400, generationMs: 800, totalMs: 900, sessionCache: "warm", inputTokens: 25, outputTokens: 30, cacheReadTokens: 150, cacheWriteTokens: 25 };
  async function details(measurement?: ChatPerformance) {
    renderAnswer(answer({ execution_model: { profile_id: "model", name: "Model", provider: "provider", model: "model", thinking_level: "none", run_options: {} }, performance: measurement }));
    await user().click(screen.getByRole("button", { name: "Response details" }));
    return screen.findByRole("dialog", { name: "Response details" });
  }

  it("shows total usage including cache reads and writes, without internal diagnostics", async () => {
    const dialog = await details(performance);
    expect(dialog).toHaveTextContent("Input: 200 tokens");
    expect(dialog).toHaveTextContent("Output: 30 tokens");
    expect(dialog).not.toHaveTextContent(/cache|session|setup|first text/);
  });

  it("never manufactures usage from missing or nonfinite counters, and keeps zero distinct", async () => {
    expect(await details({ ...performance, cacheWriteTokens: undefined } as unknown as ChatPerformance)).toHaveTextContent("Input: unavailable");
    cleanup();
    expect(await details({ ...performance, outputTokens: Infinity })).toHaveTextContent("Output: unavailable");
    cleanup();
    expect(await details({ ...performance, inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0 })).toHaveTextContent("Input: 0 tokens");
  });
});

it("shows only explicitly delivered, successfully written files as file chips, which open and show the file in Finder", async () => {
  const progress = { startedAtMs: 1, finishedAtMs: 20, items: [
    { id: "script", kind: "tool", toolName: "write", category: "files", operation: "write", action: "Wrote a file", filePath: "analysis.py", status: "succeeded", target: null, href: null, startedAtMs: 0, finishedAtMs: null },
    { id: "report", kind: "tool", toolName: "write", category: "files", operation: "write", action: "Wrote a file", filePath: "reports/summary.csv", status: "succeeded", target: null, href: null, startedAtMs: 0, finishedAtMs: null },
    { id: "failed", kind: "tool", toolName: "write", category: "files", operation: "write", action: "Wrote a file", filePath: "reports/failed.csv", status: "failed", target: null, href: null, startedAtMs: 0, finishedAtMs: null },
  ] } as ChatMessage["progress"];
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const reveal = vi.spyOn(bridge, "workspaceReveal").mockResolvedValueOnce().mockRejectedValueOnce(new Error("File not found."));
  const { open } = renderAnswer(answer({
    content: "analysis.py\n\n`[Script](analysis.py)`\n\n![Preview](analysis.py)\n\n[Failed](reports/failed.csv)\n\n[Report][download]\n\n[download]: reports/summary.csv",
    progress,
  }), { workspacePath: "/research", bridge });
  const files = screen.getByRole("group", { name: "Files" });
  expect(screen.queryByLabelText("Outputs")).toBeNull();
  await user().click(within(files).getByRole("button", { name: "summary.csv" }));
  expect(within(files).queryByRole("button", { name: /analysis.py|failed.csv/ })).toBeNull();
  expect(destinations(open)).toEqual([{ kind: "file", path: "reports/summary.csv" }]);
  // The ⋯ menu reveals the file through the workspace command, scoped to this Loupist.
  await user().click(within(files).getByRole("button", { name: "More for summary.csv" }));
  await user().click(await screen.findByRole("menuitem", { name: "Show in Finder" }));
  expect(reveal).toHaveBeenCalledExactlyOnceWith({ loupist_id: "loupist-a", path: "reports/summary.csv" });
  expect(within(files).queryByRole("status")).toBeNull();
  // A file that moved since the answer says so instead of failing silently.
  await user().click(within(files).getByRole("button", { name: "More for summary.csv" }));
  await user().click(await screen.findByRole("menuitem", { name: "Show in Finder" }));
  expect(await within(files).findByRole("status")).toHaveTextContent("Could not show summary.csv.");
});

it("keeps documents as output cards and adds no file row when the answer delivered no file", () => {
  renderAnswer(answer({ artifact_refs: [{ artifact_id: "doc", revision: 1, kind: "markdown", title: "Margin memo", digest: "d" }] }));
  expect(within(screen.getByLabelText("Outputs")).getByRole("button", { name: "Open document Margin memo" })).toBeVisible();
  expect(screen.queryByRole("group", { name: "Files" })).toBeNull();
});

it("summarizes a saved Source and opens it", async () => {
  const props = renderAnswer(answer({
    memory_refs: [read("selected-read")],
    memory_update: { status: "applied", commitId: "saved", changes: [
      { recordId: "source-1", kind: "Source", operation: "create", title: "Filing excerpt", knowledge: { kind: "source", recordId: "source-record", revisionId: "source-v1", objectId: "source-1" }, lesson: "Captured.", basis: "Filing", futureUse: "Later." },
    ] },
  }));
  const receipt = screen.getByRole("navigation", { name: "What this answer used and saved" });
  expect(receipt).toHaveTextContent("Saved 1 source");
  // A single saved Source opens directly from its receipt segment; the list stays open for the rest.
  await user().click(within(receipt).getByRole("button", { name: "Saved 1 source" }));
  expect(screen.getByRole("list", { name: "Saved in this answer" })).toBeVisible();
  expect(destinations(props.open)).toEqual([{ kind: "memory", location: expect.objectContaining({ surface: "source", recordId: "source-record" }), title: "Filing excerpt" }]);
});

it("says when nothing new was saved, and adds no receipt to a private chat or a running answer", () => {
  const { view } = renderAnswer(answer({ memory_update: { status: "no_change", commitId: null, changes: [] } }));
  expect(screen.getByRole("navigation", { name: "What this answer used and saved" })).toHaveTextContent("Nothing new saved");
  view.unmount();
  const privateAnswer = renderAnswer(answer({ id: "private" }));
  expect(screen.queryByRole("navigation", { name: "What this answer used and saved" })).toBeNull();
  privateAnswer.view.unmount();
  renderAnswer(answer({ id: "live", status: "streaming", memory_refs: [read("selected-read")], evidence_refs: [evidenceFixture()] }));
  expect(screen.queryByRole("navigation", { name: "What this answer used and saved" })).toBeNull();
  // Sources arrive with the settled answer, in the row where its actions appear.
  expect(screen.queryByLabelText("Research sources")).toBeNull();
});

it("lists external sources and a cited saved Source, and opens the saved reader", async () => {
  const web = evidenceFixture({ id: `ev_${"1".repeat(32)}`, title: "Rates outlook", tool_name: "web_fetch", kind: "original" });
  const connection = evidenceFixture({ id: `ev_${"2".repeat(32)}`, title: "Market quote", tool_name: "krx_daily_quote", kind: "original", url: undefined, provider: "KRX" });
  const sourceRead = evidenceFixture({ id: `ev_${"3".repeat(32)}`, title: "world read", tool_name: "world_read", kind: "saved_context", url: undefined, read_ref: "source-read" });
  const search = evidenceFixture({ id: `ev_${"5".repeat(32)}`, title: "world search", tool_name: "world_search", kind: "saved_context", url: undefined, excerpt: "Search hits for the filing." });
  const evidence = [web, connection, sourceRead, search];
  const props = renderAnswer(answer({
    content: evidence.map((item) => `[${item.title}](#evidence-${item.id})`).join(" "),
    evidence_refs: [...evidence, web],
    memory_refs: [read("source-read")],
  }));

  const sources = screen.getByLabelText("Research sources");
  await user().click(within(sources).getByText("Sources (3)"));
  expect(within(sources).getAllByRole("listitem")).toHaveLength(3);
  const saved = within(sources).getByRole("button", { name: /Annual report/ });
  expect(saved).toHaveTextContent("Saved source");
  expect(saved).toHaveTextContent("1 citation");
  await user().click(saved);
  expect(destinations(props.open).at(-1)).toMatchObject({ kind: "memory", location: { surface: "source", recordId: "source-record" } });
  // Source rows open in the side panel, and say so with the panel icon.
  for (const row of within(sources).getAllByRole("button")) expect(row.querySelector("[data-open-icon]")).toHaveAttribute("data-open-icon", "panel");
  expect(screen.queryByRole("button", { name: /^Source 3:/ })).toBeNull();
  await user().click(screen.getByRole("button", { name: "Source 2: Market quote" }));
  expect(opened(props.open, "evidence")).toEqual([expect.objectContaining({ evidence: expect.objectContaining({ id: connection.id }) })]);
  await user().click(screen.getByRole("button", { name: "world read · Memory: Annual report" }));
  expect(destinations(props.open).at(-1)).toEqual({ kind: "memory", location: expect.objectContaining({ surface: "source", recordId: "source-record" }), title: "Annual report" });
  await user().click(await screen.findByRole("button", { name: "Result: World search" }));
  expect(opened(props.open, "evidence")).toHaveLength(1);
  expect(within(screen.getByLabelText("Work progress")).getByLabelText("Tool result")).toHaveTextContent(search.excerpt);
});

it("dates a source row as its citation card does: published, else read", async () => {
  const filing = evidenceFixture({ id: `ev_${"7".repeat(32)}`, title: "Annual filing", published_at: "2026-10-27", retrieved_at: "2026-10-28T03:00:00Z" });
  const page = evidenceFixture({ id: `ev_${"8".repeat(32)}`, title: "Rates page", url: "https://rates.example/today", published_at: undefined, retrieved_at: "2026-10-28T03:00:00Z" });
  renderAnswer(answer({ content: `Margins rose [1](#evidence-${filing.id}) and rates held [2](#evidence-${page.id}).`, evidence_refs: [filing, page] }));
  const sources = screen.getByLabelText("Research sources");
  await user().click(within(sources).getByText("Sources (2)"));
  const [first, second] = within(sources).getAllByRole("listitem");
  expect(first).toHaveTextContent(/Oct 27, 2026/u);
  expect(first).not.toHaveTextContent(/Oct 28, 2026/u);
  expect(second).toHaveTextContent(/Oct 28, 2026/u);
});

it("keeps a numbered citation on the line of the word before it, but leaves code as written", async () => {
  const filing = evidenceFixture({ id: `ev_${"6".repeat(32)}`, title: "Annual filing" });
  const cite = `[1](#evidence-${filing.id})`;
  renderAnswer(answer({
    content: `Margins rose ${cite}.\n\nRun \`total   ${cite}\` here.\n\n\`\`\`text\nrow  ${cite}\n\`\`\`\n\n    indented   ${cite}\n`,
    evidence_refs: [filing],
  }));
  const citation = await screen.findByRole("button", { name: /Source 1: Annual filing/u });
  expect(citation.closest(".citation-keep")?.textContent).toBe("rose 1.");
  expect(citation.closest("p")?.textContent).toBe("Margins rose 1.");
  const code = [...document.querySelectorAll(".message-content code")].map((node) => node.textContent);
  expect(code).toEqual(expect.arrayContaining([`total   ${cite}`]));
  const blocks = [...document.querySelectorAll(".message-content pre")].map((node) => node.textContent ?? "");
  expect(blocks.some((text) => text.includes(`row  ${cite}`))).toBe(true);
  expect(blocks.some((text) => text.includes(`indented   ${cite}`))).toBe(true);
  expect(document.querySelector(".message-content")!.textContent).not.toMatch(/ \[1\]/u);
});

it("opens a quote basket from the source list and the citation without the raw reader", async () => {
  const excerpt = JSON.stringify({ content: [{ type: "text", text: JSON.stringify({ results: [
    { symbol: "AAPL", name: "Apple", currency: "USD", last_price: 271.4 },
    { symbol: "MSFT", name: "Microsoft", currency: "USD", last_price: 415.2 },
  ] }) }] });
  const quote = evidenceFixture({
    id: "ev_quote", title: "Equity price quote", tool_name: "mcp__openbb__equity_price_quote", provider: "Yahoo Finance",
    url: "https://finance.example/quotes", retrieved_at: "2026-10-11T15:04:00Z", published_at: undefined, excerpt,
  });
  const cut = excerpt.slice(0, excerpt.indexOf("MSFT"));
  const truncated = evidenceFixture({
    id: "ev_cut", title: "Cut quote", tool_name: "equity_price_quote", provider: "Yahoo Finance", url: undefined,
    excerpt: cut, excerpt_truncated: true,
  });
  const props = renderAnswer(answer({
    content: `Quotes [Equity price quote](#evidence-${quote.id}) and [Cut quote](#evidence-${truncated.id}).`,
    evidence_refs: [quote, truncated],
  }));

  const sources = screen.getByLabelText("Research sources");
  await user().click(within(sources).getByText("Sources (2)"));
  await user().click(within(sources).getByRole("button", { name: /Equity price quote/ }));
  let receipt = await screen.findByRole("dialog", { name: "Price quotes" });
  expect(receipt).toHaveTextContent("AAPL");
  await user().keyboard("{Escape}");

  await user().click(screen.getByRole("button", { name: "Source 1: Equity price quote" }));
  receipt = await screen.findByRole("dialog", { name: "Price quotes" });
  expect(receipt).toHaveTextContent("AAPL");
  expect(receipt).toHaveTextContent("271.4 USD");
  expect(receipt).toHaveTextContent("MSFT");
  expect(receipt).toHaveTextContent("415.2 USD");
  expect(receipt).toHaveTextContent("Retrieved");
  expect(receipt).toHaveTextContent("Yahoo Finance");
  expect(receipt).not.toHaveTextContent("Quote time");
  expect(opened(props.open, "evidence")).toHaveLength(0);
  await user().keyboard("{Escape}");

  await user().click(within(sources).getByRole("button", { name: /Cut quote/ }));
  const cutReceipt = await screen.findByRole("dialog", { name: "Price quotes" });
  expect(cutReceipt).toHaveTextContent("No complete quote was in the saved result.");
  expect(within(cutReceipt).queryByRole("term", { name: "Last price" })).toBeNull();
  expect(opened(props.open, "evidence")).toHaveLength(0);
  await user().keyboard("{Escape}");

  // A pointer click in WKWebView can leave focus on the page. The receipt still measures the button.
  document.body.tabIndex = -1;
  // Later tests move focus with Tab, which never leaves a page whose body is out of the tab order.
  onTestFinished(() => document.body.removeAttribute("tabindex"));
  document.body.focus();
  const citation = screen.getByRole("button", { name: "Source 1: Equity price quote" });
  let focusedAtClick: Element | null = null;
  citation.addEventListener("click", () => { focusedAtClick = document.activeElement; });
  vi.spyOn(citation, "getBoundingClientRect").mockReturnValue(new DOMRect(80, 180, 36, 18));
  vi.spyOn(document.body, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 0, 1280, 820));
  fireEvent.click(citation);
  expect(focusedAtClick).toBe(document.body);
  const anchored = await screen.findByRole("dialog", { name: "Price quotes" });
  await waitFor(() => expect(anchored.parentElement?.style.getPropertyValue("--radix-popper-anchor-width")).toBe("36px"));
});

it.each([
  [{ failure_kind: "provider_error", status: "error" }],
  [{ host_composed: true, status: "complete" }],
] as const)("explains host-authored response metadata without a model receipt: %o", async (metadata) => {
  renderAnswer(answer({ id: "host-answer", content: "Saved Memory.", ...metadata }));
  if (metadata.status === "error") {
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Response details" })).toBeNull();
  } else {
    await user().click(screen.getByRole("button", { name: "Response details" }));
    expect(await screen.findByRole("dialog", { name: "Response details" })).toHaveTextContent("returned no text");
  }
});

it("does not call a busy answer failed while it still counts down to its own retry", () => {
  const busy = answer({ id: "busy", content: "", status: "error", failure_kind: "rate_limited" });
  const { view, ...props } = renderAnswer(busy, { onRetry: vi.fn(), retryAttempt: 1 });
  expect(screen.getByRole("alert")).toHaveTextContent(/Retrying in 20 s/);
  expect(screen.queryByText(/Try again later/)).toBeNull();
  view.rerender(<TooltipProvider><ChatMessageCard {...props} message={busy} retryAttempt={3} /></TooltipProvider>);
  expect(screen.getByRole("alert")).toHaveTextContent(/Try again later/);
  expect(screen.queryByText(/Retrying in/)).toBeNull();
});

it("counts a busy answer down from the wait its provider asked for", () => {
  renderAnswer(answer({ id: "busy", content: "", status: "error", failure_kind: "rate_limited", retry_after_seconds: 12 }), { onRetry: vi.fn(), retryAttempt: 1 });
  expect(screen.getByText(/Retrying in 12 s/)).toBeInTheDocument();
});

it.each([false, true])("shows a save failure without promising retention (placeholder=%s)", (placeholder) => {
  const content = placeholder ? "Response could not be completed." : "The generated answer.";
  renderAnswer(answer({ status: "error", failure_kind: "store_failed", content, placeholder,
    progress: { startedAtMs: 0, finishedAtMs: 2, items: [
      { id: "s1", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", status: "succeeded", target: null, href: null, startedAtMs: 0, finishedAtMs: 1 },
    ] },
  }));
  expect(screen.getByRole("alert")).toHaveTextContent("The response could not be saved.");
  expect(screen.queryByText("The steps and the text so far were kept.")).toBeNull();
  if (placeholder) {
    expect(screen.queryByText(content)).toBeNull();
  } else {
    expect(screen.getByText(content)).toBeVisible();
    expect(screen.getByRole("button", { name: "Copy answer" })).toBeVisible();
  }
});

it("shows guidance a finished reply accepted as a Guided row, not a question bubble", async () => {
  const steer: ChatMessage = { id: "steer", role: "user", status: "complete", content: "Focus on margins", created_at: "2026-10-10T00:00:00Z", steer: true };
  const { view } = renderAnswer(steer);
  expect(screen.getByText("Guided: “Focus on margins”")).toBeInTheDocument();
  expect(view.container.querySelector("article.message.user")).toBeNull();
  await i18n.changeLanguage("ko");
  view.rerender(<TooltipProvider><ChatMessageCard message={steer} loupistId="loupist-a" chatId="chat-a" loupistName="Loupist" onReadAttachment={vi.fn()} /></TooltipProvider>);
  expect(screen.getByText("보낸 지침: “Focus on margins”")).toBeInTheDocument();
  await i18n.changeLanguage("en");
  // The same words as a plain question still read as a bubble.
  view.rerender(<TooltipProvider><ChatMessageCard message={{ ...steer, steer: undefined }} loupistId="loupist-a" chatId="chat-a" loupistName="Loupist" onReadAttachment={vi.fn()} /></TooltipProvider>);
  expect(view.container.querySelector("article.message.user")).not.toBeNull();
});

it("keeps saved schedule cards current, even after a stopped reply, and opens the exact task", async () => {
  const receipt = { task_id: "task-a", title: "Morning review", revision: 1, action: "created" as const, status: "active" as const,
    schedule: { kind: "daily" as const, hour: 9, minute: 0, timezone: "Asia/Seoul" }, target: { kind: "dedicated_chat" as const },
    next_run_at_ms: 1799973000000, use_memory: false, update_memory: false, awaiting_confirmation: false };
  const message = answer({ status: "aborted", content: "", schedule_receipts: [receipt, { ...receipt, task_id: "task-b", title: "Evening review", target: { kind: "new_chat" } }] });
  const { view, open, ...props } = renderAnswer(message);
  const paused = { ...message, schedule_receipts: [{ ...receipt, revision: 2, action: "paused" as const, status: "paused" as const }, message.schedule_receipts![1]] };
  view.rerender(<TooltipProvider><ChatMessageCard {...props} message={paused} /></TooltipProvider>);
  expect(screen.getAllByRole("article", { name: /Scheduled task:/ })).toHaveLength(2);
  expect(screen.getByRole("article", { name: "Scheduled task: Morning review" })).toHaveTextContent("Paused");
  await user().click(screen.getByRole("button", { name: /Evening review/ }));
  expect(destinations(open)).toEqual([{ kind: "schedule", taskId: "task-b" }]);
});

it("says what a stopped reply kept: finished steps and the text so far", () => {
  const steps = { startedAtMs: 0, finishedAtMs: null, items: [
    { id: "s1", kind: "tool" as const, toolName: "web_search", category: "web" as const, operation: "search" as const, action: "Searched the web", status: "succeeded" as const, target: null, href: null, startedAtMs: 0, finishedAtMs: null },
    { id: "s2", kind: "tool" as const, toolName: "web_fetch", category: "web" as const, operation: "read" as const, action: "Read a web source", status: "succeeded" as const, target: null, href: null, startedAtMs: 0, finishedAtMs: null },
    { id: "s3", kind: "tool" as const, toolName: "web_fetch", category: "web" as const, operation: "read" as const, action: "Read a web source", status: "stopped" as const, target: null, href: null, startedAtMs: 0, finishedAtMs: null },
  ] };
  renderAnswer(answer({ status: "aborted", content: "Margin fell while", progress: steps }));
  expect(screen.getByText("Stopped · kept 2 steps and the text so far")).toBeVisible();
  cleanup();
  renderAnswer(answer({ status: "aborted", content: "" }));
  expect(screen.getByText("Stopped before the answer finished.")).toBeVisible();
});

it("places a waiting approval in the turn after the step that asked, and keeps its settled line there", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const approval = (state: "waiting" | "allowed_once") => ({
    requestId: "req-1", itemId: "step-filings", connectionId: "custom-morrow", connectionName: "Morrow research tools",
    toolName: "read_company_filings", target: "Hanul Precision · 2026 Q3", state, requestedAtMs: Date.now() - 12_000,
  });
  const items = (status: "running" | "succeeded") => [
    { id: "step-notes", kind: "tool" as const, toolName: "world_read", category: "memory" as const, operation: "read" as const, action: "Read saved notes", status: "succeeded" as const, startedAtMs: 1, finishedAtMs: 2, target: null, href: null },
    { id: "step-filings", kind: "tool" as const, toolName: "mcp__custom-morrow__read_company_filings", category: "finance" as const, operation: "read" as const, action: "Read Read company filings", running: "Reading company filings", status, startedAtMs: 3, finishedAtMs: status === "succeeded" ? 4 : null, target: null, href: null },
  ];
  const running = answer({ status: "streaming", content: "", progress: { startedAtMs: 0, items: items("running"), approvals: [approval("waiting")], finishedAtMs: null } });
  const turn = (message: ChatMessage) => <ExtensionAvailabilityProvider value={{ userSkills: false, customConnections: true }}>
    <TooltipProvider><ChatMessageCard message={message} loupistId="loupist-a" chatId="chat-a" loupistName="Loupist" bridge={bridge} onReadAttachment={vi.fn()} /></TooltipProvider>
  </ExtensionAvailabilityProvider>;
  const view = renderWithOpen(turn(running));
  const card = screen.getByRole("region", { name: "Approval for Read company filings" });
  // The card is a block of the turn, not inside the folded ledger; amber waits in the live line.
  expect(card.closest(".chat-progress")).toBeNull();
  const ledger = screen.getByLabelText("Work progress");
  expect(ledger.compareDocumentPosition(card) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(screen.getByLabelText("Response status")).toHaveTextContent("Waiting for you");
  expect(screen.getAllByRole("button", { name: "Allow once" })).toHaveLength(1);

  // A decision settles into one line at the same place.
  const done = answer({ progress: { startedAtMs: 0, finishedAtMs: 5_000, items: items("succeeded"), approvals: [approval("allowed_once")] } });
  view.rerender(turn(done));
  const line = screen.getByText("Allowed once · Read company filings");
  expect(screen.getAllByText("Allowed once · Read company filings")).toHaveLength(1);
  expect(screen.getByLabelText("Work progress").compareDocumentPosition(line) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});

it("places a waiting question where it was asked and settles it into one line at the same place", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const answerQuestion = vi.spyOn(bridge, "chatQuestionAnswer").mockResolvedValue();
  const question = (state: "waiting" | "answered") => ({
    requestId: "q-1", itemId: "step-ask", question: "Compare on which period?", recommended: 0, state,
    options: [{ label: "Last 4 quarters" }, { label: "Last 3 years" }],
    ...(state === "answered" ? { answer: { choice: 1 } } : {}), requestedAtMs: Date.now() - 5_000,
  });
  const items = (status: "running" | "succeeded") => [
    { id: "step-ask", kind: "tool" as const, toolName: "ask_user", category: "capability" as const, operation: "generic" as const, action: "Asked you a question", running: "Asking you a question", status, startedAtMs: 1, finishedAtMs: status === "succeeded" ? 4 : null, target: null, href: null },
  ];
  const running = answer({ status: "streaming", content: "", progress: { startedAtMs: 0, items: items("running"), questions: [question("waiting")], finishedAtMs: null } });
  const { view } = renderAnswer(running, { bridge });
  const card = screen.getByRole("region", { name: "Question: Compare on which period?" });
  expect(card.closest(".chat-progress")).toBeNull();
  // The question tool is not a step: no ledger for a turn that only asked.
  expect(screen.queryByLabelText("Work progress")).toBeNull();
  await user().click(within(card).getByRole("button", { name: /^Last 3 years/ }));
  expect(answerQuestion).toHaveBeenCalledWith({ loupist_id: "loupist-a", request_id: "q-1", choice: 1 });

  const done = answer({ progress: { startedAtMs: 0, finishedAtMs: 5_000, items: items("succeeded"), questions: [question("answered")] } });
  view.rerender(<TooltipProvider><ChatMessageCard message={done} loupistId="loupist-a" chatId="chat-a" loupistName="Loupist" bridge={bridge} onReadAttachment={vi.fn()} /></TooltipProvider>);
  const line = screen.getByText("You answered: Last 3 years");
  expect(screen.getAllByText("You answered: Last 3 years")).toHaveLength(1);
  expect(line.compareDocumentPosition(screen.getByText("Answer.")) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
});

it("keeps inline references in the question and shows saved tool and date selections below it", () => {
  const { view } = renderAnswer(answer({
    role: "user", as_of: "2026-03-05",
    content: "$lens\n\nCompare @filings across periods.\n\nKeep this second paragraph.",
  }), { mentionSkills: [{ id: "lens", name: "Lens" }], mentionPlugins: [{ id: "filings", name: "Filings" }] });
  const body = view.container.querySelector(".message-user-text")!;
  expect(body.textContent).toBe("Compare Filings across periods.\n\nKeep this second paragraph.");
  expect(body.nextElementSibling).toHaveTextContent("Lens");
  expect(body.nextElementSibling?.querySelector('[data-kind="request"]')).toBeTruthy();
});

it("opens a cited document read as saved work instead of expanding the old JSON receipt", async () => {
  const artifactId = `artifact-${"a".repeat(32)}`;
  const receipt = evidenceFixture({ id: "ev_document", kind: "derived", tool_name: "artifact_read", url: undefined, excerpt: JSON.stringify({ artifact: { id: artifactId, kind: "markdown", current_revision: 1, title: "Analysis" }, revision: { payload: "old body" } }) });
  const { open } = renderAnswer(answer({ content: "[Final analysis](#evidence-ev_document)", evidence_refs: [receipt] }));
  await user().click(screen.getByRole("button", { name: /Final analysis/ }));
  expect(opened(open, "artifact-link")).toEqual([{ kind: "artifact-link", artifactId }]);
  expect(screen.queryByText("old body")).toBeNull();
});

describe("the in-progress reply", () => {
  const chartStep = (status: "running" | "succeeded") => ({ id: "chart-step", kind: "tool" as const, toolName: "chart_publish", category: "artifact" as const, operation: "publish" as const, action: "Made a chart", running: "Making a chart", target: "Margins", status, startedAtMs: 1, finishedAtMs: null, href: null });
  const draftText = "Margin fell while revenue grew.";

  it("keeps a card published before the answer above it, live and settled", () => {
    const live = answer({ status: "streaming", phase: "writing", content: "", draft: { id: "c9", answerLength: 0, settled: false }, progress: { startedAtMs: 0, finishedAtMs: null, items: [chartStep("running"), { id: "c9", kind: "commentary", text: draftText }] } });
    const { view, ...props } = renderAnswer(live);
    const article = view.container.querySelector("article")!;
    const order = () => [...article.querySelectorAll(":scope > [data-turn-item]")].map((block) => block.getAttribute("data-turn-item"));
    expect(order()).toEqual(["tools", "card", "prose"]);
    expect(screen.getByRole("region", { name: "Chart: Margins" })).toHaveAttribute("aria-busy", "true");
    view.rerender(<TooltipProvider><ChatMessageCard {...props} message={answer({ content: draftText, answer_key: "c9", artifact_refs: [{ artifact_id: "chart-1", revision: 1, kind: "chart", title: "Margins", digest: "a".repeat(64) }], progress: { startedAtMs: 0, finishedAtMs: 9, items: [chartStep("succeeded")] } })} /></TooltipProvider>);
    expect(order()).toEqual(["tools", "card", "prose"]);
  });

  /** The settled answer as the reply streamed it: its text arriving in the draft, its results already retained. */
  const streamingOf = (settled: ChatMessage): ChatMessage => {
    const progress = settled.progress!;
    return { ...settled, status: "streaming", phase: "writing", content: "", number_checks: undefined, answer_key: undefined,
      draft: { id: "c9", answerLength: 0, settled: false }, progress: { ...progress, finishedAtMs: null, items: [...progress.items, { id: "c9", kind: "commentary", text: settled.content }] } };
  };

  it("keeps a paragraph that is only a calculation link where it was written and operable, streaming and settled, and copies the answer as written", async () => {
    const settled = { ...previewGridLinkAnswer(), answer_key: "c9" };
    const grid = SENSITIVITY_EVIDENCE.grid;
    // The grid as Rust retained it: the operation the retired renderer held the paragraph for.
    expect(grid.calc_operation).toBe("reverse_dcf");
    const { view, open, ...props } = renderAnswer(streamingOf(settled));
    const article = view.container.querySelector("article")!;
    const prose = () => [...article.querySelectorAll<HTMLElement>(':scope > [data-turn-item="prose"]')];
    const paragraphs = () => [...prose()[0]!.querySelectorAll<HTMLElement>(".message-content p")];
    const ledger = () => article.querySelector<HTMLElement>(':scope > [data-turn-item="tools"]')!;
    const citation = () => within(paragraphs()[1]!).getByRole("button", { name: "Solsup Foods · Result: Value at different growth and discount rates" });
    const expectThreeParagraphs = () => {
      expect(prose()).toHaveLength(1);
      expect(paragraphs().map((paragraph) => paragraph.textContent?.trim())).toEqual([
        expect.stringMatching(/^Solsup Foods’ price already assumes/u), "Solsup Foods", expect.stringMatching(/^The middle case needs about 7\.3% a year/u),
      ]);
      // No frame is held for the link while it streams, and no card stands in for it when it settles.
      expect(article.querySelector(".sensitivity-slot, .sensitivity-card, .earnings-card")).toBeNull();
    };
    expectThreeParagraphs();
    // While the answer streams, the written link is already the citation of the grid's result:
    // it opens at its own step in the turn, with the grid the turn retained.
    expect(citation()).toHaveAttribute("data-evidence-id", grid.id);
    await user().click(citation());
    const output = await within(ledger()).findByLabelText("Result output");
    expect(output).toHaveTextContent('"measure": "implied annual growth for 5 years"');
    // The middle row of the grid, with the 7.3% base case the prose quotes.
    expect(output.textContent).toMatch(/\[\s*8\.4,\s*7\.9,\s*7\.3,\s*6\.6,\s*5\.8\s*\]/u);
    expect(open).not.toHaveBeenCalled();
    const shown = prose()[0];
    view.rerender(<TooltipProvider><ChatMessageCard {...props} message={settled} /></TooltipProvider>);
    // The same prose element, before and after the commit: nothing read moved or was replaced.
    await waitFor(() => expect(article.querySelector('[data-testid="number-chip"]')).not.toBeNull());
    expect(prose()[0]).toBe(shown);
    expectThreeParagraphs();
    // Settled, the link is the same citation; the number after it keeps its own check.
    expect(citation()).toHaveAttribute("data-evidence-id", grid.id);
    expect(await within(paragraphs()[2]!).findByRole("button", { name: /^8 percent, calculated/u })).toHaveAttribute("data-kind", "calculated");
    const write = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue(undefined);
    await user().click(screen.getByRole("button", { name: "Copy answer" }));
    expect(write).toHaveBeenCalledWith(settled.content);
  });

  it("keeps the Korean answer's lone calculation link as its own paragraph, streaming and settled", async () => {
    const lang = document.documentElement.lang;
    await i18n.changeLanguage("ko");
    try {
      // The fixture writes the Korean answer under a Korean document, as the preview does.
      document.documentElement.lang = "ko";
      const settled = { ...previewGridLinkAnswer(), answer_key: "c9" };
      const { view, ...props } = renderAnswer(streamingOf(settled));
      const paragraphs = () => [...view.container.querySelectorAll<HTMLElement>(".message-content p")];
      const expectThreeParagraphs = () => {
        expect(paragraphs().map((paragraph) => paragraph.textContent?.trim())).toEqual([
          expect.stringMatching(/^현재 주가는/u), "솔숲식품", expect.stringMatching(/^가운데 경우에는 연 7\.3% 성장이 필요합니다\./u),
        ]);
        expect(paragraphs()[1]!.querySelector(".loupist-citation")).toHaveAttribute("data-evidence-id", SENSITIVITY_EVIDENCE.grid.id);
      };
      expectThreeParagraphs();
      view.rerender(<TooltipProvider><ChatMessageCard {...props} message={settled} /></TooltipProvider>);
      expect(await within(paragraphs()[2]!).findByTestId("number-chip")).toHaveTextContent("8.0%");
      expectThreeParagraphs();
    } finally {
      document.documentElement.lang = lang;
      await i18n.changeLanguage("en");
    }
  });

  it("fades a card still preparing out where it stood when the reply stops, then removes it", async () => {
    vi.useFakeTimers();
    try {
      const live = answer({ status: "streaming", content: "", progress: { startedAtMs: 0, finishedAtMs: null, items: [chartStep("running")] } });
      const { view, ...props } = renderAnswer(live);
      const article = view.container.querySelector("article")!;
      const card = () => article.querySelector(':scope > [data-turn-item="card"]');
      expect(card()).not.toHaveAttribute("data-leaving");
      const stopped = { ...chartStep("running"), status: "stopped" as const, finishedAtMs: 5 };
      view.rerender(<TooltipProvider><ChatMessageCard {...props} message={answer({ status: "aborted", content: "", progress: { startedAtMs: 0, finishedAtMs: 5, items: [stopped] } })} /></TooltipProvider>);
      expect(card()).toHaveAttribute("data-leaving");
      expect(card()).toHaveAttribute("aria-hidden", "true");
      expect(screen.queryByRole("region", { name: "Chart: Margins" })).toBeNull();
      await act(async () => { await vi.advanceTimersByTimeAsync(130); });
      expect(card()).toBeNull();
    } finally { vi.useRealTimers(); }
  });

  it("fades a filled card out in place when the reply stops, holding the turn's height so the page stays still", async () => {
    vi.useFakeTimers();
    try {
      const staged: ChatArtifactView = {
        artifact: { id: "chart-1", chat_session_id: "chat-a", kind: "chart", title: "Margins", current_revision: 1, created_at_ms: 0, updated_at_ms: 0 },
        revision: { artifact_id: "chart-1", revision: 1, digest: "a".repeat(64), source_message_id: "answer", created_at_ms: 0,
          payload: { kind: "chart", schema: "loupist-chart/1", chart: {} as never } },
        chart_dataset: null,
      };
      const filled = answer({ status: "streaming", content: "", staged_artifacts: [staged], progress: { startedAtMs: 0, finishedAtMs: null, items: [chartStep("succeeded")] } });
      const turnOf = (message: ChatMessage, props: Omit<ReturnType<typeof renderAnswer>, "view" | "open">) =>
        <TooltipProvider><section className="turn" data-turn-reserve=""><ChatMessageCard {...props} message={message} /></section></TooltipProvider>;
      const running = answer({ status: "streaming", content: "", progress: { startedAtMs: 0, finishedAtMs: null, items: [chartStep("running")] } });
      const { view, ...props } = renderAnswer(running);
      view.rerender(turnOf(running, props));
      view.rerender(turnOf(filled, props));
      const turn = view.container.querySelector<HTMLElement>(".turn")!;
      vi.spyOn(turn, "getBoundingClientRect").mockReturnValue({ height: 840.2 } as DOMRect);
      const article = turn.querySelector("article")!;
      const card = () => article.querySelector(':scope > [data-turn-item="card"]');
      const shown = card()!.querySelector("section.chat-artifact-block");
      expect(shown).toHaveAttribute("data-state", "ready");
      view.rerender(turnOf(answer({ status: "aborted", content: "", staged_artifacts: [staged], progress: { startedAtMs: 0, finishedAtMs: 5, items: [chartStep("succeeded")] } }), props));
      // The card the reader saw stays drawn, the same element, and fades where it stood.
      expect(card()).toHaveAttribute("data-leaving");
      expect(card()).toHaveAttribute("aria-hidden", "true");
      expect(card()!.querySelector("section.chat-artifact-block")).toBe(shown);
      expect(turn.style.getPropertyValue("--turn-hold")).toBe("841px");
      await act(async () => { await vi.advanceTimersByTimeAsync(130); });
      expect(card()).toBeNull();
      // Gone, the card leaves its room: the turn keeps the height it had, so nothing below or above moves.
      expect(turn.style.getPropertyValue("--turn-hold")).toBe("841px");
    } finally { vi.useRealTimers(); }
  });

  it("reads Writing until the answer's last words are shown, at most 400 ms after the host finished", async () => {
    vi.useFakeTimers();
    try {
      const long = `Margin fell. ${"More words follow here. ".repeat(20)}`;
      const streaming = (text: string) => answer({ status: "streaming", phase: "writing", content: "", draft: { id: "c9", answerLength: 0, settled: false }, progress: { startedAtMs: 0, finishedAtMs: null, items: [{ id: "c9", kind: "commentary", text }] } });
      const { view, ...props } = renderAnswer(streaming("Margin"));
      view.rerender(<TooltipProvider><ChatMessageCard {...props} message={streaming(long)} /></TooltipProvider>);
      view.rerender(<TooltipProvider><ChatMessageCard {...props} message={answer({ content: long, answer_key: "c9" })} /></TooltipProvider>);
      expect(screen.getByLabelText("Response status")).toHaveTextContent("Writing");
      expect(screen.queryByRole("button", { name: "Copy answer" })).toBeNull();
      await act(async () => { await vi.advanceTimersByTimeAsync(450); });
      expect(screen.queryByLabelText("Response status")).toBeNull();
      expect(screen.getByRole("button", { name: "Copy answer" })).toBeVisible();
    } finally { vi.useRealTimers(); }
  });

  it("says a stop in one quiet line with Try again, and keeps a failure's text with a hint", () => {
    const steps = [{ id: "s", kind: "tool" as const, toolName: "web_search", category: "web" as const, operation: "search" as const, action: "Searched the web", status: "succeeded" as const, startedAtMs: 0, finishedAtMs: 1, target: null, href: null }];
    renderAnswer(answer({ status: "aborted", content: "", progress: { startedAtMs: 0, finishedAtMs: 12_000, items: steps } }), { onRetry: vi.fn() });
    const stopped = screen.getByRole("status");
    expect(stopped).toHaveAttribute("data-kind", "stopped");
    expect(stopped).toHaveTextContent("Stopped · kept 1 step");
    expect(within(stopped).getByRole("button", { name: "Try again" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Stopped after 12s · 1 step" })).toBeVisible();
    cleanup();
    renderAnswer(answer({ status: "error", failure_kind: "other", content: "Margin fell while", progress: { startedAtMs: 0, finishedAtMs: 12_000, items: steps } }), { onRetry: vi.fn() });
    expect(screen.getByRole("alert")).toHaveTextContent("The steps and the text so far were kept.");
    expect(screen.getByText("Margin fell while")).toBeVisible();
  });
});

describe("a Bot answer that read a quote", () => {
  const read = evidenceFixture({ id: "ev_nvda", title: "NVDA · Quote", tool_name: "mcp__openbb__equity_price_quote", provider: "yfinance", url: undefined, published_at: undefined, as_of: undefined,
    retrieved_at: "2026-11-14T15:00:00Z", excerpt: "NVDA last 162.85 change_pct -2.01" });
  const snapshotAnswer = answer({
    content: `NVIDIA closed at [$162.85](#evidence-${read.id}) on Friday, [-2.01%](#evidence-${read.id}) on the day.`,
    evidence_refs: [read],
    number_checks: [
      { link_index: 0, text: "$162.85", evidence_id: read.id, kind: "data", status: "matched", pointer: "/price/last", fields: [{ key: "last", value: "162.85", matched: true }] },
      { link_index: 1, text: "-2.01%", evidence_id: read.id, kind: "data", status: "matched", pointer: "/price/change_pct", fields: [{ key: "change_pct", value: "-2.01", matched: true }] },
    ],
  });
  const renderBot = (message: ChatMessage) => renderWithOpen(<TooltipProvider><ChatSurfaceProvider value={{ kind: "bot", loupist: { id: "loupist-a", name: "Lumi" } }}>
    <ChatMessageCard message={message} loupistId="loupist-a" chatId="chat-a" loupistName="Lumi" onReadAttachment={vi.fn()} bridge={new MockLoupistBridge({ delay_ms: 0 })} previousQuestion="Where NVIDIA stands" />
  </ChatSurfaceProvider></TooltipProvider>);
  const expectProseOnly = async () => {
    const article = document.querySelector("article[data-bot-message]")!;
    expect(article).toHaveTextContent("NVIDIA closed at $162.85 on Friday, -2.01% on the day.");
    expect(within(article as HTMLElement).getAllByTestId("number-chip").map((chip) => chip.textContent)).toEqual(["$162.85", "-2.01%"]);
    expect(await screen.findByRole("group", { name: "Sources and checked figures for this answer" })).toHaveTextContent("2 figures checked · 1 source");
    // The answer is what the agent wrote: no card of the read under it, and nothing to take it elsewhere.
    expect(article.querySelector(".bot-card, .snapshot-card")).toBeNull();
    expect(screen.queryByRole("button", { name: "Take to research" })).toBeNull();
  };

  it("shows the prose with its number chips and the trace line, and no card", async () => {
    renderBot(snapshotAnswer);
    await expectProseOnly();
  });

});

describe("a paragraph that is only one link", () => {
  const results = evidenceFixture({ id: `ev_${"e".repeat(32)}`, kind: "saved_context", tool_name: "source_read", url: undefined, title: "Results · Q3 2026 · Morrow Components",
    excerpt: "{\n  \"period\": \"Q3 2026\",\n  \"status\": \"final\"\n}" });
  const release = evidenceFixture({ id: `ev_${"a".repeat(32)}`, title: "Q3 2026 results release" });

  it("to a saved read stays a citation of what the answer read", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const content = `Revenue rose to [1.27bn](#evidence-${release.id}).\n\n[Results · Q3 2026](#evidence-${results.id})\n\nMargin was [14.8%](#evidence-${release.id}).`;
    // Rust counted every link of the answer as written: the lone link is link 1.
    const { view, open } = renderAnswer(answer({ content, evidence_refs: [release, results], number_checks: [
      { link_index: 0, text: "1.27bn", evidence_id: release.id, kind: "data", status: "matched" },
      { link_index: 2, text: "14.8%", evidence_id: release.id, kind: "data", status: "matched" },
    ] }), { bridge });
    const paragraphs = [...view.container.querySelectorAll<HTMLElement>(".message-content p")];
    expect(paragraphs.map((paragraph) => paragraph.textContent?.trim())).toEqual(["Revenue rose to 1.27bn.", "Results · Q3 2026", "Margin was 14.8%."]);
    expect(within(paragraphs[0]!).getByTestId("number-chip")).toHaveTextContent("1.27bn");
    expect(within(paragraphs[2]!).getByTestId("number-chip")).toHaveTextContent("14.8%");
    const link = within(paragraphs[1]!).getByRole("button", { name: /^Source \d+: Results · Q3 2026 · Morrow Components/u });
    // From the keyboard: Tab from the start of the page reaches the citation after the figure before it
    // and that figure's card, Shift+Tab from the figure after it comes back to it, and Enter opens it.
    const reader = user();
    const [before, after] = [paragraphs[0]!, paragraphs[2]!].map((paragraph) => within(paragraph).getByTestId("number-chip"));
    const stops: Element[] = [];
    for (let step = 0; step < 20 && document.activeElement !== link; step += 1) {
      await reader.tab();
      stops.push(document.activeElement!);
    }
    expect(link).toHaveFocus();
    expect(stops).toContain(before);
    await reader.tab();
    expect(after).toHaveFocus();
    await reader.tab({ shift: true });
    expect(link).toHaveFocus();
    await reader.keyboard("{Enter}");
    expect(destinations(open).at(-1)).toEqual({ kind: "evidence", evidence: results });
  });

  it("after a published card keeps the number after it on its own check", async () => {
    const chart = { id: "chart-step", kind: "tool" as const, toolName: "chart_publish", category: "artifact" as const, operation: "publish" as const, action: "Made a chart", target: "Margins", status: "succeeded" as const, startedAtMs: 1, finishedAtMs: 2, href: null };
    const content = `[Q3 2026 results release](#evidence-${release.id})\n\nMargin was [14.8%](#evidence-${release.id}), down from [15.1%](#evidence-${release.id}).`;
    const { view } = renderAnswer(answer({ content, evidence_refs: [release],
      artifact_refs: [{ artifact_id: "chart-1", revision: 1, kind: "chart", title: "Margins", digest: "a".repeat(64) }], progress: { startedAtMs: 0, finishedAtMs: 9, items: [chart] },
      number_checks: [
        { link_index: 1, text: "14.8%", evidence_id: release.id, kind: "data", status: "matched" },
        { link_index: 2, text: "15.1%", evidence_id: release.id, kind: "data", status: "differs", nearest: "15.2%" },
      ] }));
    const article = view.container.querySelector("article")!;
    expect([...article.querySelectorAll(":scope > [data-turn-item]")].map((block) => block.getAttribute("data-turn-item"))).toEqual(["tools", "card", "prose"]);
    const chips = within(article.querySelector<HTMLElement>(':scope > [data-turn-item="prose"]')!).getAllByTestId("number-chip");
    expect(chips.map((chip) => [chip.textContent, chip.getAttribute("data-status")])).toEqual([["14.8%", "matched"], ["15.1%", "differs"]]);
  });

  it("past the 200 checks Rust keeps, and a plain date citation, stay text as written", () => {
    const links = Array.from({ length: 201 }, (_, index) => `[${index}.5%](#evidence-${release.id})`);
    const content = `${links.join(" ")}\n\n[Mar 18, 2026](#evidence-${release.id})`;
    const number_checks = links.slice(0, 200).map((_, index) => ({ link_index: index, text: `${index}.5%`, evidence_id: release.id, kind: "data" as const, status: "matched" as const }));
    const { view } = renderAnswer(answer({ content, evidence_refs: [release], number_checks }));
    const [figures, dated] = [...view.container.querySelectorAll<HTMLElement>(".message-content p")];
    expect(within(figures!).getAllByTestId("number-chip")).toHaveLength(200);
    // The 201st figure has no check: a citation with its own text, never a chip or a changed number.
    const unchecked = within(figures!).getByRole("button", { name: /^200\.5% · Source \d+: Q3 2026 results release/u });
    expect(unchecked).toHaveTextContent(/^200\.5%/u);
    expect(unchecked).not.toHaveAttribute("data-testid", "number-chip");
    expect(figures!.textContent).toContain(links.map((_, index) => `${index}.5%`).join(" "));
    expect(dated!.textContent?.trim()).toBe("Mar 18, 2026");
    expect(within(dated!).queryByTestId("number-chip")).toBeNull();
    expect(within(dated!).getByRole("button", { name: /^Mar 18, 2026 · Source \d+: Q3 2026 results release/u })).toBeVisible();
  });

  it("a repeated figure keeps its own check, and past the 200 checks it borrows none", () => {
    const link = (text: string) => `[${text}](#evidence-${release.id})`;
    const figures = Array.from({ length: 199 }, (_, index) => link(`${index + 1}.5%`));
    // Link 1 repeats link 0 with a check of its own; link 201 repeats link 0 past the bound, with none.
    const content = `Margin was ${link("0.5%")}, then ${link("0.5%")}.\n\n${figures.join(" ")}\n\nAgain ${link("0.5%")}.`;
    const check = (link_index: number, text: string, status: "matched" | "differs") => ({ link_index, text, evidence_id: release.id, kind: "data" as const, status });
    const number_checks = [check(0, "0.5%", "matched"), check(1, "0.5%", "differs"), ...figures.slice(0, 198).map((_, index) => check(index + 2, `${index + 1}.5%`, "matched"))];
    expect(number_checks).toHaveLength(200);
    const { view } = renderAnswer(answer({ content, evidence_refs: [release], number_checks }));
    const [repeated, listed, again] = [...view.container.querySelectorAll<HTMLElement>(".message-content p")];
    expect(within(repeated!).getAllByTestId("number-chip").map((chip) => chip.getAttribute("data-status"))).toEqual(["matched", "differs"]);
    expect(within(listed!).getAllByTestId("number-chip")).toHaveLength(198);
    expect(view.container.querySelectorAll('[data-testid="number-chip"]')).toHaveLength(200);
    expect(within(again!).queryByTestId("number-chip")).toBeNull();
    expect(within(again!).getByRole("button", { name: /^0\.5% · Source \d+: Q3 2026 results release/u })).toHaveTextContent(/^0\.5%$/u);
  });
});

it("opens an asset page citation by the id and name it stored", async () => {
  const read = evidenceFixture({ id: `ev_${"d".repeat(32)}`, kind: "saved_context", tool_name: "asset_page_read", url: undefined, title: "Asset page: Alpha",
    excerpt: JSON.stringify({ asset: { assetId: "ast_00000000000a1fa0", name: "Alpha Inc.", reference: { ticker: "QRS" } } }, null, 2) });
  const { open } = renderAnswer(answer({ content: `[Asset page: Alpha](#evidence-${read.id})`, evidence_refs: [read] }));
  await user().click(screen.getByRole("button", { name: "Source 1: Asset page: Alpha" }));
  expect(opened(open, "asset")).toEqual([{ kind: "asset", target: { loupistId: "loupist-a", assetId: "ast_00000000000a1fa0", name: "Alpha Inc." } }]);
});
