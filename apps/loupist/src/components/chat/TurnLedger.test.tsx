import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { renderWithOpen, destinations } from "../../test/open";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import i18n from "@/i18n";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatActivityDetail, ChatMessage, ChatProgress } from "../../types";
import { deriveTurnView } from "../../chat/turnView";
import { evidenceFixture } from "../../test/evidenceFixtures";
import { ChatExecutionProvider } from "./ChatExecutionContext";
import { TurnLedger } from "./TurnLedger";
import { ChatMapProvider } from "../workspace/ChatMapContext";

const follows = (first: Element, second: Element) => Boolean(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING);

function withActivity(detail: ChatActivityDetail) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const read = vi.spyOn(bridge, "chatActivityRead").mockResolvedValue(detail);
  const wrapper = ({ children }: { children: ReactNode }) => <ChatExecutionProvider value={{ bridge, loupistId: "loupist", threadId: "chat", surface: "session", theme: "dark", active: true }}>{children}</ChatExecutionProvider>;
  return { read, wrapper };
}

/** The turn's ledger for a message with this progress, as the card derives it. */
function ledger(progress: ChatProgress, status: ChatMessage["status"], extra: Partial<ChatMessage> = {}, props: Partial<Parameters<typeof TurnLedger>[0]> = {}) {
  const message: ChatMessage = { id: "answer", role: "assistant", status, content: status === "complete" ? "Done." : "", created_at: "2026-10-10T00:00:00Z", progress, phase: status === "streaming" ? "tool" : undefined, ...extra };
  const view = deriveTurnView({ message, now: progress.startedAtMs + 12_000, lastActivityAt: progress.startedAtMs + 12_000, stopping: false, retry: { attempt: 1, offered: false } });
  const segment = view.segments.find((item) => item.kind === "ledger")!;
  return <TurnLedger segment={segment} {...props} />;
}

it("counts the steps while running, then becomes a receipt on the same line with the steps in order inside", async () => {
  const user = userEvent.setup();
  const progress: ChatProgress = { startedAtMs: 1, items: [
    { id: "web-1", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", target: "rate outlook", source: { kind: "web", name: "web_search" }, status: "succeeded", href: null, startedAtMs: 0, finishedAtMs: null },
    { id: "web-2", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", running: "Reading example.com", target: "Rate outlook · example.com", href: "https://example.com/rates", status: "running", startedAtMs: 0, finishedAtMs: null },
  ], finishedAtMs: null };
  const { open: openLink, ...view } = renderWithOpen(ledger(progress, "streaming"));
  const toggle = screen.getByRole("button", { name: "2 steps" });
  expect(toggle).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByText("Searched the web")).toBeNull();
  await user.click(toggle);
  const readTarget = screen.getByRole("button", { name: "Rate outlook · example.com" });
  // A running row uses the catalog's running words; the host's own wording stays out.
  expect(follows(screen.getByText("Searched the web"), screen.getByText("Reading a web source"))).toBe(true);
  expect(screen.queryByText("web_search")).toBeNull();
  await user.click(readTarget);
  expect(destinations(openLink)).toContainEqual({ kind: "link", url: "https://example.com/rates" });

  const settled = { ...progress, finishedAtMs: 30_001, items: progress.items.map((item) => ({ ...item, status: "succeeded" as const })) };
  view.rerender(ledger(settled, "complete"));
  // The reader's choice to open it stands.
  expect(screen.getByRole("button", { name: "Worked 30s · 2 steps" })).toHaveAttribute("aria-expanded", "true");
});

it("shows the evidence gathered as chips while running and none once settled", () => {
  vi.useFakeTimers();
  try {
    const source = evidenceFixture({ id: "e1", url: "https://dart.fss.or.kr/filing" });
    const progress: ChatProgress = { startedAtMs: 1, items: [
      { id: "mem", kind: "tool", toolName: "world_read", category: "memory", operation: "read", action: "Read saved notes", status: "succeeded", target: null, href: null, startedAtMs: 0, finishedAtMs: null },
      { id: "fetch", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", status: "succeeded", resultIds: ["e1"], target: null, href: null, startedAtMs: 0, finishedAtMs: null },
      ...["a", "b", "c", "d", "e"].map((id) => ({ id, kind: "tool" as const, toolName: id === "e" ? "krx_daily_quote" : "dart_financials", category: "finance" as const, operation: "read" as const, action: "Read", status: "succeeded" as const, target: null, href: null, startedAtMs: 0, finishedAtMs: null })),
      { id: "calc", kind: "tool", toolName: "run_python", category: "compute", operation: "execute", action: "Ran Python", status: "running", target: null, href: null, startedAtMs: 0, finishedAtMs: null },
    ], finishedAtMs: null };
    const view = render(ledger(progress, "streaming", { evidence_refs: [source] }));
    const chips = [...view.container.querySelectorAll(".turn-ledger-chip")];
    expect(chips.map((chip) => chip.textContent)).toEqual(["Saved notes", "dart.fss.or.kr", "Financial statements4", "Daily prices"]);
    // Chips are not controls and not announced: the whole line is one button named by its text.
    expect(view.container.querySelector(".turn-ledger-chips")).toHaveAttribute("aria-hidden", "true");
    expect(screen.getByRole("button", { name: "8 steps" })).toBeVisible();
    // Both "+N" counts are drawn; CSS picks the one for the turn's width.
    expect(view.container.querySelectorAll(".turn-ledger-more")).toHaveLength(1);
    view.rerender(ledger({ ...progress, finishedAtMs: 9_001, items: progress.items.map((item) => item.kind === "tool" ? { ...item, status: "succeeded" as const } : item) }, "complete", { evidence_refs: [source] }));
    expect(view.container.querySelector(".turn-ledger-chips")).toHaveAttribute("data-leaving");
    act(() => { vi.advanceTimersByTime(150); });
    expect(view.container.querySelector(".turn-ledger-chips")).toBeNull();
  } finally { vi.useRealTimers(); }
});

it("counts more chips than fit as +N for both widths", () => {
  const items = ["a", "b", "c", "d", "e", "f"].map((id) => ({ id, kind: "tool" as const, toolName: "web_fetch", category: "web" as const, operation: "read" as const, action: "Read", status: "succeeded" as const, resultIds: [id], target: null, href: null, startedAtMs: 0, finishedAtMs: null }));
  const evidence = items.map((item) => evidenceFixture({ id: item.id, url: `https://${item.id}.example/page` }));
  const view = render(ledger({ startedAtMs: 1, items, finishedAtMs: null }, "streaming", { evidence_refs: evidence }));
  expect([...view.container.querySelectorAll(".turn-ledger-more")].map((node) => [node.getAttribute("data-k"), node.textContent])).toEqual([["5", "+1"], ["3", "+3"]]);
});

it("keeps a failed step closed and quiet: the line says it in words, the row says Failed", async () => {
  const { read, wrapper } = withActivity({ id: "report-attempt", tool_name: "read", status: "failed", input: "annual-report.pdf", output: "File not found", truncated: false, started_at_ms: 1, finished_at_ms: 2000, file_path: null });
  render(ledger({ startedAtMs: 1, finishedAtMs: 2000, items: [
    { id: "failed", kind: "tool", category: "web", operation: "read", action: "Read annual report", status: "failed", detailId: "report-attempt", target: null, href: null, startedAtMs: 0, finishedAtMs: null },
    { id: "ok", kind: "tool", category: "web", operation: "read", action: "Read filing", status: "succeeded", target: null, href: null, startedAtMs: 10, finishedAtMs: 2_710 },
  ] }, "complete"), { wrapper });
  const header = screen.getByRole("button", { name: "Worked 2s · 2 steps · 1 failed attempt" });
  expect(header).toHaveAttribute("aria-expanded", "false");
  await userEvent.setup().click(header);
  expect(screen.getByText("Failed")).toBeVisible();
  expect(screen.getByText("2.7s")).toBeVisible();
  await userEvent.setup().click(screen.getByRole("button", { name: "Inspect Read annual report" }));
  expect(await within(screen.getByLabelText("Work progress")).findByText("File not found")).toBeVisible();
  expect(read).toHaveBeenCalledExactlyOnceWith({ loupist_id: "loupist", thread_id: "chat", detail_id: "report-attempt" });
});

it("explains a step paused by the untrusted-text rule in the step, calmly and with no override (B92)", async () => {
  render(ledger({ startedAtMs: 1, finishedAtMs: 2000, items: [
    { id: "search", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", status: "succeeded", target: null, href: null, startedAtMs: 0, finishedAtMs: null },
    { id: "instructions", kind: "tool", toolName: "write", category: "files", operation: "write", action: "Wrote a file", target: "AGENTS.md", status: "failed", refusal: "untrusted_text_paused", href: null, startedAtMs: 0, finishedAtMs: null },
    { id: "fetch", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", target: "example.com/review", status: "failed", refusal: "untrusted_text_paused", href: null, startedAtMs: 0, finishedAtMs: null },
  ] }, "complete"));
  // A pause is not a failure.
  const header = screen.getByRole("button", { name: "Worked 2s · 3 steps" });
  await userEvent.setup().click(header);
  expect(screen.queryByText("Failed")).toBeNull();
  expect(screen.getAllByText("Paused").length).toBeGreaterThanOrEqual(2);
  const notes = screen.getAllByRole("note");
  expect(notes[0]).toHaveTextContent("so changing instruction and Skill files is turned off for this answer.");
  expect(notes[1]).toHaveTextContent("so opening this page is turned off for this answer.");
  for (const note of notes) expect(within(note).queryByRole("button")).toBeNull();
  expect(follows(screen.getByText("AGENTS.md"), notes[0]!)).toBe(true);
  expect(follows(notes[0]!, screen.getByText("example.com/review"))).toBe(true);
});

it("explains a step refused for a private app path or a file outside the Loupist folder", async () => {
  render(ledger({ startedAtMs: 1, finishedAtMs: 2000, items: [
    { id: "auth", kind: "tool", toolName: "read", category: "files", operation: "read", action: "Read a file", target: "auth.json", status: "failed", refusal: "protected_path", href: null, startedAtMs: 0, finishedAtMs: null },
    { id: "shared", kind: "tool", toolName: "read", category: "files", operation: "read", action: "Read a file", target: "shared.csv", status: "failed", refusal: "outside_workspace", href: null, startedAtMs: 0, finishedAtMs: null },
  ] }, "complete"));
  await userEvent.setup().click(screen.getByRole("button", { name: /2 failed attempts/ }));
  const notes = screen.getAllByRole("note");
  expect(notes[0]).toHaveTextContent("Not allowed: this path is inside the app's private data");
  expect(notes[1]).toHaveTextContent("Not allowed: this file is outside the Loupist folder. Attach it to the chat or copy it into the folder, and the Loupist can work with it.");
});

it("labels a refused page as paused in Korean and steps by the catalog's words", async () => {
  await i18n.changeLanguage("ko");
  try {
    render(ledger({ startedAtMs: 1, finishedAtMs: 2000, items: [
      { id: "paused", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", target: "example.com/page", status: "failed", refusal: "untrusted_text_paused", detailId: "paused-detail", href: null, startedAtMs: 0, finishedAtMs: null },
      { id: "failed", kind: "tool", toolName: "run_python", category: "compute", operation: "execute", action: "Ran Python", target: "failed.py", status: "failed", href: null, startedAtMs: 0, finishedAtMs: null },
      { id: "succeeded", kind: "tool", toolName: "run_python", category: "compute", operation: "execute", action: "Ran Python", target: "done.py", status: "succeeded", href: null, startedAtMs: 0, finishedAtMs: null },
    ] }, "complete"));
    fireEvent.click(screen.getByRole("button", { name: /2초 작업 · 3단계 · 실행 1건 실패/ }));
    const paused = screen.getByText("example.com/page").closest(".chat-progress-row")!;
    expect(within(paused as HTMLElement).getAllByText("일시 중지").length).toBeGreaterThan(0);
    expect(within(paused as HTMLElement).getByRole("note")).toHaveTextContent("다른 사이트의 글을 읽어서, 이번 답변에서는 이 페이지 열기가 꺼져 있어요.");
    expect(screen.getAllByText("파이썬 실행함")).toHaveLength(2);
  } finally { await i18n.changeLanguage("en"); }
});

it("lists notes in their chronological place in the timeline", () => {
  render(ledger({ startedAtMs: 1, items: [
    { id: "n0", kind: "commentary", text: "Reading the notes first." },
    { id: "a", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", status: "succeeded", target: null, href: null, startedAtMs: 0, finishedAtMs: null },
    { id: "n1", kind: "commentary", text: "Now the filing." },
    { id: "b", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", status: "running", target: null, href: null, startedAtMs: 0, finishedAtMs: null },
  ], finishedAtMs: null }, "streaming"));
  fireEvent.click(screen.getByRole("button", { name: "2 steps" }));
  expect(follows(screen.getByText("Searched the web"), screen.getByText("Now the filing."))).toBe(true);
  expect(follows(screen.getByText("Now the filing."), screen.getByText("Reading a web source"))).toBe(true);
});

it("loads running command output on demand and stops polling when closed", async () => {
  vi.useFakeTimers();
  try {
    const { read, wrapper } = withActivity({ id: "durable-detail", tool_name: "run_python", status: "running", input: JSON.stringify({ code: "print(391)", timeout_seconds: 30 }), output: "Partial result: 391", truncated: true, started_at_ms: 10, file_path: null, finished_at_ms: null });
    render(ledger({ startedAtMs: 1, items: [
      { id: "tool", kind: "tool", toolName: "run_python", category: "compute", operation: "execute", action: "Ran Python", running: "Running Python", status: "running", detailId: "durable-detail", target: null, href: null, startedAtMs: 0, finishedAtMs: null },
    ], finishedAtMs: null }, "streaming"), { wrapper });
    expect(read).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "1 step" }));
    const inspect = screen.getByRole("button", { name: "Inspect Running Python" });
    fireEvent.click(inspect);
    await act(async () => { await Promise.resolve(); });
    expect(within(screen.getByLabelText("Work progress")).getByText("Partial result: 391")).toBeVisible();
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(read).toHaveBeenCalledTimes(2);
    fireEvent.click(inspect);
    await act(async () => { await vi.advanceTimersByTimeAsync(4000); });
    expect(read).toHaveBeenCalledTimes(2);
  } finally { vi.useRealTimers(); }
});

it("reveals a focused command's saved output in the work trace", async () => {
  const { wrapper } = withActivity({ id: "durable-detail", tool_name: "run_python", status: "succeeded", input: "print(391)", output: "Partial result: 391", truncated: false, started_at_ms: 10, finished_at_ms: 20, file_path: null });
  render(ledger({ startedAtMs: 1, finishedAtMs: 20, items: [
    { id: "tool", kind: "tool", toolName: "run_python", category: "compute", operation: "execute", action: "Ran Python", status: "succeeded", detailId: "durable-detail", target: null, href: null, startedAtMs: 0, finishedAtMs: null },
  ] }, "complete", {}, { focusItemId: "tool" }), { wrapper });
  expect(await screen.findByLabelText("Command output")).toHaveTextContent("Partial result: 391");
});

it("keeps internal targets out of live and saved progress", () => {
  const progress: ChatProgress = { startedAtMs: 1, items: [
    { id: "page", kind: "tool", category: "web", operation: "read", action: "Read a page", target: "web:f6495eccf2c0cca04e4c6a03", status: "succeeded", href: null, startedAtMs: 0, finishedAtMs: null },
    { id: "tool", kind: "tool", category: "capability", operation: "read", action: "Opened a tool", target: "private-tool-id", source: { kind: "capability", name: "private-tool-id" }, status: "running", href: null, startedAtMs: 0, finishedAtMs: null },
  ], finishedAtMs: null };
  const { container, rerender } = render(ledger(progress, "streaming"));
  fireEvent.click(screen.getByRole("button", { name: "2 steps" }));
  expect(container).toHaveTextContent("Opened a tool");
  expect(container).not.toHaveTextContent(/private-tool-id|web:f649/);
  rerender(ledger({ ...progress, finishedAtMs: 2000, items: progress.items.map((item) => ({ ...item, status: "succeeded" as const })) }, "complete"));
  expect(container).toHaveTextContent("Read a page");
  expect(container).not.toHaveTextContent(/private-tool-id|web:f649/);
});

describe("names a step read", () => {
  const reading: ChatProgress = { startedAtMs: 1, items: [
    { id: "map-1", kind: "tool", category: "memory", operation: "read", action: "Read the map", running: "Reading the map", status: "running", entityIds: ["a", "b"], target: null, href: null, startedAtMs: 0, finishedAtMs: null },
    { id: "web-1", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", status: "succeeded", target: null, href: null, startedAtMs: 0, finishedAtMs: null },
  ], finishedAtMs: null };

  it("labels the step with its name count and leaves steps without names alone", () => {
    render(ledger(reading, "streaming"));
    fireEvent.click(screen.getByRole("button", { name: "2 steps" }));
    const names = screen.getAllByText("2 on the map");
    expect(names).toHaveLength(1);
    expect(names[0].tagName).toBe("SPAN");
  });

  it("shows those names on the chat map when it holds them", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "knowledgeMap").mockResolvedValue({ nodes: ["a", "b"].map((id) => ({ id, label: id, kind: "entity", degree: 1, watched: false, held: false })), edges: [], worldRevisionId: "rev" } as never);
    vi.spyOn(bridge, "knowledgeGrowthEvents").mockResolvedValue({ events: [], nextCursor: null });
    const onOpen = vi.fn();
    const message = { id: "live", role: "assistant" as const, content: "", created_at: "2026-10-28", status: "streaming" as const, progress: reading };
    render(<ChatMapProvider bridge={bridge} loupistId="loupist" chatId="chat" messages={[message]} onOpen={onOpen}>
      {ledger(reading, "streaming")}
    </ChatMapProvider>);
    fireEvent.click(screen.getByRole("button", { name: "2 steps" }));
    fireEvent.click(await screen.findByRole("button", { name: "Show 2 on the map" }));
    expect(onOpen).toHaveBeenCalledOnce();
  });
});

describe("plan", () => {
  const plan = (statuses: ["done" | "active" | "pending" | "not_done", ...Array<"done" | "active" | "pending" | "not_done">]) => ({
    items: statuses.map((status, index) => ({ id: `item-${index}`, label: ["Read the Q3 operating report", "Compare utilization with inventory days", "Check against your principles"][index]!, status, ...(status === "pending" ? {} : { activatedAtMs: [10, 20, 30][index] }) })),
  });
  const steps: ChatProgress["items"] = [
    { id: "early", kind: "tool", category: "memory", operation: "read", action: "Read Memory", status: "succeeded", startedAtMs: 5, target: null, href: null, finishedAtMs: null },
    { id: "report", kind: "tool", category: "web", operation: "read", action: "Read the operating report", status: "succeeded", startedAtMs: 12, target: null, href: null, finishedAtMs: null },
    { id: "search", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", status: "succeeded", startedAtMs: 21, target: null, href: null, finishedAtMs: null },
    { id: "calc", kind: "tool", category: "compute", operation: "calculate", action: "Calculated the ratio", running: "Calculating the ratio", status: "running", startedAtMs: 22, target: null, href: null, finishedAtMs: null },
  ];

  it("says the plan position on the line and nests each step under the item that was active when it started", () => {
    render(ledger({ startedAtMs: 1, items: steps, plan: plan(["done", "active", "pending"]), finishedAtMs: null }, "streaming"));
    fireEvent.click(screen.getByRole("button", { name: "Plan 2 of 3 · Compare utilization with inventory days" }));
    const list = screen.getByRole("list", { name: "Plan" });
    const [done, active, pending] = within(list).getAllByRole("listitem").filter((item) => item.parentElement === list);
    expect(done).toHaveTextContent("Read the Q3 operating report");
    expect(within(done!).getByText("Read the operating report")).toBeVisible();
    expect(active).toHaveAttribute("aria-current", "step");
    expect(within(active!).getByText("Searched the web")).toBeVisible();
    expect(within(active!).getByText("Calculating the ratio")).toBeVisible();
    expect(pending).toHaveTextContent("Not started");
    const early = screen.getByText("Read Memory");
    expect(list.contains(early)).toBe(false);
    expect(follows(list, early)).toBe(true);
    expect(screen.queryByText(/Updat(ing|ed) the plan/)).toBeNull();
  });

  it("keeps the ticks and marks unfinished items Not done after a stop", () => {
    render(ledger({ startedAtMs: 1, finishedAtMs: 40, items: steps.map((item) => item.kind === "tool" && item.status === "running" ? { ...item, status: "stopped" as const } : item), plan: plan(["done", "not_done", "not_done"]) }, "aborted"));
    fireEvent.click(screen.getByRole("button", { name: /^Stopped after/ }));
    const list = screen.getByRole("list", { name: "Plan" });
    const items = within(list).getAllByRole("listitem").filter((item) => item.parentElement === list);
    expect(items[1]).toHaveTextContent("Not done");
    expect(screen.getByText("Stopped")).toBeVisible();
  });
});
