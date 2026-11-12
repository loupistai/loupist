import type { ComponentProps } from "react";
import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "@/i18n";
// Citation links open a delayed HoverCard; skipping hover keeps its timer from firing after teardown.
import type { ChatMessage, ChatPerformance, KnowledgeReference, MemoryRef } from "../../types";
import { evidenceFixture } from "../../test/evidenceFixtures";
import { TooltipProvider } from "../ui/tooltip";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { ChatMessageCard } from "./ChatMessageCard";
import { renderWithOpen, destinations } from "../../test/open";
import type { ChatDestination, Open } from "../../chat/navigation";
import type { Mock } from "vitest";

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

it.each([false, true])("keeps one stable research summary from first call to settlement (expanded: %s)", async (expanded) => {
  const first = { id: "a", kind: "tool" as const, category: "files" as const, operation: "read" as const, action: "Read file", running: "Reading file", target: "q1.csv", status: "running" as const, startedAtMs: 2, href: null, finishedAtMs: null };
  const second = { ...first, id: "b", target: "q2.csv", startedAtMs: 3 };
  const plan = { items: [{ id: "compare", label: "Compare quarters", status: "active" as const, activatedAtMs: 1 }] };
  const message = (status: "streaming" | "complete", items: NonNullable<ChatMessage["progress"]>["items"]) =>
    answer({ content: status === "complete" ? "Done." : "", status, phase: "tool", progress: { startedAtMs: 1, plan, items, finishedAtMs: null } });
  const { view, ...props } = renderAnswer(message("streaming", [first]));
  // One row: the running step while it runs, the plan's stage once the run settles.
  const digest = screen.getByRole("button", { name: /^Reading file/ });
  expect(digest).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByText("q1.csv")).toBeNull();
  if (expanded) await user().click(digest);
  const both = [{ ...first, status: "succeeded" as const }, second];
  view.rerender(<TooltipProvider><ChatMessageCard {...props} message={message("streaming", both)} /></TooltipProvider>);
  expect(screen.getByRole("button", { name: /^Reading file/ })).toBe(digest);
  view.rerender(<TooltipProvider><ChatMessageCard {...props} message={message("complete", both.map(item => ({ ...item, status: "succeeded" })))} /></TooltipProvider>);
  expect(screen.getByRole("button", { name: /Compare quarters/ })).toBe(digest);
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
  expect(toggle).toHaveAccessibleName(/1 source.*investor.apple.com/i);
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
  await user().click(within(screen.getByLabelText("Work progress")).getByRole("button"));
  await user().click(screen.getByText("Consulted 2 saved notes"));
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
    expect(screen.getByRole("status")).toHaveTextContent("중지됨 · 지금까지의 글을 남겼어요");
    view.rerender(<TooltipProvider><ChatMessageCard {...props} message={answer({ status: "aborted", content: "Response stopped." })} /></TooltipProvider>);
    expect(screen.queryByText("Response stopped.")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("답변이 끝나기 전에 중지되었어요.");
    expect(screen.queryByRole("button", { name: "답변 복사" })).not.toBeInTheDocument();
    view.rerender(<TooltipProvider><ChatMessageCard {...props} message={answer({ status: "aborted", content: "Response stopped.", progress: { startedAtMs: 1, items: [{ id: "draft", kind: "commentary", text: "쓰다 멈춘 문장" }], finishedAtMs: null } })} /></TooltipProvider>);
    expect(screen.getByRole("status")).toHaveTextContent("중지됨 · 쓰던 글을 남겼어요");
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

it("cites external grounding once, opens it, and routes saved reads to Memory", async () => {
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
  await user().click(within(sources).getByText("Sources (2)"));
  expect(within(sources).getAllByRole("listitem")).toHaveLength(2);
  // Source rows open in the side panel, and say so with the panel icon.
  for (const row of within(sources).getAllByRole("button")) expect(row.querySelector("[data-open-icon]")).toHaveAttribute("data-open-icon", "panel");
  expect(screen.queryByRole("button", { name: /^Source 3:/ })).toBeNull();
  await user().click(screen.getByRole("button", { name: "Source 2: Market quote" }));
  expect(opened(props.open, "evidence")).toEqual([expect.objectContaining({ evidence: expect.objectContaining({ id: connection.id }) })]);
  await user().click(screen.getByRole("button", { name: "world read · Memory: Annual report" }));
  expect(destinations(props.open).at(-1)).toEqual({ kind: "memory", location: expect.objectContaining({ surface: "source", recordId: "source-record" }), title: "Annual report" });
  await user().click(screen.getByRole("button", { name: "world search · Result: Map search" }));
  expect(opened(props.open, "evidence")).toHaveLength(1);
  expect(within(screen.getByLabelText("Work progress")).getByLabelText("Tool result")).toHaveTextContent(search.excerpt);
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

it("shows guidance a finished reply accepted as a Guided row, not a question bubble", async () => {
  const steer: ChatMessage = { id: "steer", role: "user", status: "complete", content: "Focus on margins", created_at: "2026-10-10T00:00:00Z", steer: true };
  const { view } = renderAnswer(steer);
  expect(screen.getByText("Guided: “Focus on margins”")).toBeInTheDocument();
  expect(view.container.querySelector("article.message.user")).toBeNull();
  await i18n.changeLanguage("ko");
  view.rerender(<TooltipProvider><ChatMessageCard message={steer} loupistId="loupist-a" chatId="chat-a" loupistName="Loupist" onReadAttachment={vi.fn()} /></TooltipProvider>);
  expect(screen.getByText("안내했어요: “Focus on margins”")).toBeInTheDocument();
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

it("keeps a waiting approval actionable while folded and leaves its settled receipt with the step", async () => {
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
  const { view } = renderAnswer(running, { bridge });
  const card = screen.getByRole("region", { name: "Approval for Read company filings" });
  expect(card.closest(".chat-progress")).toHaveAttribute("data-expanded", "false");
  expect(screen.getAllByRole("button", { name: "Allow once" })).toHaveLength(1);

  // Decisions that need no further input move into the retained activity.
  const done = answer({ progress: { startedAtMs: 0, finishedAtMs: 5_000, items: items("succeeded"), approvals: [approval("allowed_once")] } });
  view.rerender(<TooltipProvider><ChatMessageCard message={done} loupistId="loupist-a" chatId="chat-a" loupistName="Loupist" bridge={bridge} onReadAttachment={vi.fn()} /></TooltipProvider>);
  expect(screen.queryByText("Allowed once · Read company filings")).toBeNull();
  await user().click(within(screen.getByLabelText("Work progress")).getByRole("button"));
  expect(screen.getAllByText("Allowed once · Read company filings")).toHaveLength(1);
  expect(screen.getByText("Allowed once · Read company filings").closest('[data-progress-status="succeeded"]')).not.toBeNull();
});

it("keeps a waiting question actionable while folded and leaves its settled line with the step", async () => {
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
  expect(card.closest(".chat-progress")).toHaveAttribute("data-expanded", "false");
  await user().click(within(card).getByRole("button", { name: /^Last 3 years/ }));
  expect(answerQuestion).toHaveBeenCalledWith({ loupist_id: "loupist-a", request_id: "q-1", choice: 1 });

  const done = answer({ progress: { startedAtMs: 0, finishedAtMs: 5_000, items: items("succeeded"), questions: [question("answered")] } });
  view.rerender(<TooltipProvider><ChatMessageCard message={done} loupistId="loupist-a" chatId="chat-a" loupistName="Loupist" bridge={bridge} onReadAttachment={vi.fn()} /></TooltipProvider>);
  expect(screen.queryByText("You answered: Last 3 years")).toBeNull();
  await user().click(within(screen.getByLabelText("Work progress")).getByRole("button"));
  expect(screen.getAllByText("You answered: Last 3 years")).toHaveLength(1);
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
