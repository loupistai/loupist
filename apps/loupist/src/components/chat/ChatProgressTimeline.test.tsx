import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps, ReactNode } from "react";
import i18n from "@/i18n";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatActivityDetail } from "../../types";
import { ChatExecutionProvider } from "./ChatExecutionContext";
import { ChatProgressTimeline } from "./ChatProgressTimeline";
import { ChatMapProvider } from "../workspace/ChatMapContext";

type Progress = ComponentProps<typeof ChatProgressTimeline>["progress"];
const follows = (first: Element, second: Element) => Boolean(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING);

function withActivity(detail: ChatActivityDetail) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const read = vi.spyOn(bridge, "chatActivityRead").mockResolvedValue(detail);
  const wrapper = ({ children }: { children: ReactNode }) => <ChatExecutionProvider value={{ bridge, loupistId: "loupist", threadId: "chat", theme: "dark", active: true }}>{children}</ChatExecutionProvider>;
  return { read, wrapper };
}

it("shows live actions and commentary in order, then settles into a collapsed summary", async () => {
  const user = userEvent.setup();
  const openLink = vi.fn(async () => undefined);
  const progress: Progress = { startedAtMs: 1, items: [
    { id: "web-1", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", target: "rate outlook", source: { kind: "web", name: "web_search" }, status: "succeeded" },
    { id: "web-2", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", target: "Rate outlook · example.com", href: "https://example.com/rates", status: "running" },
    { id: "commentary-3", kind: "commentary", text: "I will compare this with Memory." },
    { id: "memory-4", kind: "tool", category: "memory", operation: "read", action: "Read Memory", status: "succeeded" },
  ] };
  const view = render(<ChatProgressTimeline progress={progress} status="streaming" onOpenLink={openLink} />);
  const live = screen.getByRole("button", { name: /^Read a web source · / });
  expect(live).toHaveAttribute("aria-expanded", "true");
  const readTarget = screen.getByRole("button", { name: "Rate outlook · example.com" });
  const commentary = screen.getByText("I will compare this with Memory.");
  expect(follows(screen.getByText("Searched the web"), readTarget)).toBe(true);
  expect(follows(readTarget, commentary)).toBe(true);
  expect(follows(commentary, screen.getByText("Read Memory"))).toBe(true);
  expect(screen.queryByText("web_search")).toBeNull();
  await user.click(readTarget);
  expect(openLink).toHaveBeenCalledWith("https://example.com/rates");
  await user.click(live);
  expect(screen.queryByText("Searched the web")).toBeNull();

  const settled = { ...progress, finishedAtMs: 10, items: progress.items.map((item) => item.kind === "commentary" ? item : { ...item, status: "succeeded" as const }) };
  view.rerender(<ChatProgressTimeline progress={settled} status="complete" onOpenLink={openLink} />);
  // A run under a second shows its step count without a "0:00" clock.
  const summary = screen.getByRole("button", { name: /^3 steps/ });
  expect(summary).toHaveAttribute("aria-expanded", "false");
  expect(summary.querySelector(".chat-progress-elapsed")).toBeNull();
  await user.click(summary);
  expect(screen.getByText("Read Memory")).toBeVisible();
});

it("names a stopped commentary draft without calling it zero steps", () => {
  render(<ChatProgressTimeline status="aborted" onOpenLink={vi.fn()} progress={{ startedAtMs: 1, finishedAtMs: 30, items: [
    { id: "draft", kind: "commentary", text: "A partial story." },
  ] }} />);
  expect(screen.getByRole("button", { name: "Writing stopped · draft kept" })).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByText("A partial story.")).toBeVisible();
});

it("keeps failed steps visible in a completed reply and opens their exact activity", async () => {
  const { read, wrapper } = withActivity({ id: "report-attempt", tool_name: "read", status: "failed", input: "annual-report.pdf", output: "File not found", truncated: false, started_at_ms: 1, finished_at_ms: 2000 });
  render(<ChatProgressTimeline status="complete" onOpenLink={vi.fn()} progress={{ startedAtMs: 1, finishedAtMs: 2000, items: [
    { id: "failed", kind: "tool", category: "web", operation: "read", action: "Read annual report", status: "failed", detailId: "report-attempt" },
    { id: "ok", kind: "tool", category: "web", operation: "read", action: "Read filing", status: "succeeded" },
  ] }} />, { wrapper });
  expect(screen.getByRole("button", { name: /^Worked/ })).toHaveAttribute("aria-expanded", "true");
  expect(screen.getByLabelText("Failed")).toBeVisible();
  await userEvent.setup().click(screen.getByRole("button", { name: "Inspect Read annual report" }));
  expect(await within(screen.getByLabelText("Work progress")).findByText("File not found")).toBeVisible();
  expect(read).toHaveBeenCalledExactlyOnceWith({ loupist_id: "loupist", thread_id: "chat", detail_id: "report-attempt" });
});

it("explains a step paused by the untrusted-text rule in the step, calmly and with no override (B92)", () => {
  render(<ChatProgressTimeline status="complete" onOpenLink={vi.fn()} progress={{ startedAtMs: 1, finishedAtMs: 2000, items: [
    { id: "search", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", status: "succeeded" },
    { id: "bash", kind: "tool", toolName: "bash", category: "compute", operation: "execute", action: "Ran a command", target: "python margin_model.py", status: "failed", refusal: "untrusted_text_paused" },
    { id: "fetch", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", target: "example.com/review", status: "failed", refusal: "untrusted_text_paused" },
  ] }} />);
  // The steps open on their own so the note is seen; the header counts a pause, not a failure.
  const header = screen.getByRole("button", { name: /^Worked/ });
  expect(header).toHaveAttribute("aria-expanded", "true");
  expect(header).toHaveTextContent("2 paused");
  expect(header).not.toHaveTextContent("failed");
  expect(screen.queryByLabelText("Failed")).toBeNull();
  expect(screen.getAllByRole("img", { name: "Paused" })).toHaveLength(2);
  expect(screen.getAllByText("Paused")).toHaveLength(2);
  expect(screen.queryByText("Ran a command")).toBeNull();
  expect(screen.queryByText("Read a web source")).toBeNull();
  const notes = screen.getAllByRole("note");
  expect(notes[0]).toHaveTextContent("Paused: this answer read text from other sites, so running commands is turned off for this answer.");
  expect(notes[1]).toHaveTextContent("so opening this page is turned off for this answer.");
  // A security rule: nothing in the note offers a way around it.
  for (const note of notes) expect(within(note).queryByRole("button")).toBeNull();
  // Each note sits with its own step.
  expect(follows(screen.getByText("python margin_model.py"), notes[0]!)).toBe(true);
  expect(follows(notes[0]!, screen.getByText("example.com/review"))).toBe(true);
});

it("labels a refused command as paused in Korean while retaining completed and ordinary failed actions", async () => {
  await i18n.changeLanguage("ko");
  try {
    render(<ChatProgressTimeline status="complete" onOpenLink={vi.fn()} progress={{ startedAtMs: 1, finishedAtMs: 2000, items: [
      { id: "paused", kind: "tool", toolName: "bash", category: "compute", operation: "execute", action: "Ran a command", target: "paused.py", status: "failed", refusal: "untrusted_text_paused", detailId: "paused-detail" },
      { id: "failed", kind: "tool", toolName: "bash", category: "compute", operation: "execute", action: "Ran a command", target: "failed.py", status: "failed" },
      { id: "succeeded", kind: "tool", toolName: "bash", category: "compute", operation: "execute", action: "Ran a command", target: "done.py", status: "succeeded" },
    ] }} />);
    const paused = screen.getByText("paused.py").closest(".chat-progress-row")!;
    expect(within(paused as HTMLElement).getByText("일시 중지")).toBeVisible();
    expect(within(paused as HTMLElement).queryByText("명령 실행함")).toBeNull();
    expect(within(paused as HTMLElement).getByRole("note")).toHaveTextContent("이번 답변에서는 명령 실행이 꺼져 있어요.");
    expect(screen.getAllByText("명령 실행함")).toHaveLength(2);
  } finally { await i18n.changeLanguage("en"); }
});

it("keeps an ordinary failure a failure", () => {
  render(<ChatProgressTimeline status="complete" onOpenLink={vi.fn()} progress={{ startedAtMs: 1, finishedAtMs: 2000, items: [
    { id: "bash", kind: "tool", toolName: "bash", category: "compute", operation: "execute", action: "Ran a command", status: "failed" },
  ] }} />);
  expect(screen.getByRole("button", { name: /^Worked/ })).toHaveTextContent("1 failed");
  expect(screen.queryByRole("note")).toBeNull();
});

it("groups a long repeated phase without hiding its failure", () => {
  const items = Array.from({ length: 8 }, (_, index) => ({
    id: `search-${index}`, kind: "tool" as const, category: "web" as const, operation: "search" as const,
    action: "Searched the web", target: `query ${index + 1}`, status: index === 3 ? "failed" as const : "succeeded" as const,
  }));
  render(<ChatProgressTimeline progress={{ startedAtMs: 1, finishedAtMs: 2000, items }} status="complete" onOpenLink={vi.fn()} />);
  expect(screen.getByRole("button", { name: /^Worked/ })).toHaveTextContent("8 steps");
  expect(screen.getByLabelText("Failed")).toBeVisible();
  expect(screen.getByText("query 4")).toBeVisible();
  expect(screen.getByRole("button", { name: "Searched the web · 3 queries" })).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByText("query 1")).toBeNull();
});

it("loads running command output on demand and stops polling when closed", async () => {
  vi.useFakeTimers();
  try {
    const { read, wrapper } = withActivity({ id: "durable-detail", tool_name: "bash", status: "running", input: JSON.stringify({ command: "python calculate.py" }), output: "Partial result: 391", truncated: true, started_at_ms: 10 });
    render(<ChatProgressTimeline status="streaming" onOpenLink={async () => undefined} progress={{ startedAtMs: 1, items: [
      { id: "tool", kind: "tool", toolName: "bash", category: "compute", operation: "execute", action: "Ran a command", status: "running", detailId: "durable-detail" },
    ] }} />, { wrapper });
    expect(read).not.toHaveBeenCalled();
    const inspect = screen.getByRole("button", { name: "Inspect Ran a command" });
    fireEvent.click(inspect);
    await act(async () => { await Promise.resolve(); });
    expect(within(screen.getByLabelText("Work progress")).getByText("Partial result: 391")).toBeVisible();
    expect(screen.getByText("python calculate.py")).toBeVisible();
    await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
    expect(read).toHaveBeenCalledTimes(2);
    fireEvent.click(inspect);
    await act(async () => { await vi.advanceTimersByTimeAsync(4000); });
    expect(read).toHaveBeenCalledTimes(2);
    expect(screen.queryByText("Partial result: 391")).toBeNull();
  } finally { vi.useRealTimers(); }
});

it("reveals a focused command's saved output in the work trace", async () => {
  const { wrapper } = withActivity({ id: "durable-detail", tool_name: "bash", status: "succeeded", input: "python calculate.py", output: "Partial result: 391", truncated: false, started_at_ms: 10, finished_at_ms: 20 });
  render(<ChatProgressTimeline status="complete" focusItemId="tool" onOpenLink={async () => undefined} progress={{ startedAtMs: 1, finishedAtMs: 20, items: [
    { id: "tool", kind: "tool", toolName: "bash", category: "compute", operation: "execute", action: "Ran a command", target: "python calculate.py", status: "succeeded", detailId: "durable-detail" },
  ] }} />, { wrapper });
  expect(await screen.findByLabelText("Command output")).toHaveTextContent("Partial result: 391");
});

it("keeps internal targets out of live and saved progress", () => {
  const progress: Progress = { startedAtMs: 1, items: [
    { id: "page", kind: "tool", category: "web", operation: "read", action: "Read a page", target: "web:f6495eccf2c0cca04e4c6a03", status: "succeeded" },
    { id: "tool", kind: "tool", category: "capability", operation: "read", action: "Opened a tool", target: "private-tool-id", source: { kind: "capability", name: "private-tool-id" }, status: "running" },
  ] };
  const { container, rerender } = render(<ChatProgressTimeline progress={progress} status="streaming" onOpenLink={async () => {}} />);
  expect(container).toHaveTextContent("Opened a tool");
  expect(container).not.toHaveTextContent(/private-tool-id|web:f649/);
  rerender(<ChatProgressTimeline progress={{ ...progress, finishedAtMs: 2000 }} status="complete" onOpenLink={async () => {}} />);
  fireEvent.click(screen.getByRole("button", { name: /^Worked/ }));
  expect(container).toHaveTextContent("Read a page");
  expect(container).not.toHaveTextContent(/private-tool-id|web:f649/);
});

describe("run header", () => {
  const start = Date.UTC(2026, 9, 31, 9, 0, 0);
  const reading: Progress = { startedAtMs: start, items: [
    { id: "search", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", status: "succeeded" },
    { id: "read", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", running: "Reading hanul.example", status: "running" },
  ] };
  beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(start + 12_000); });
  afterEach(() => vi.useRealTimers());
  const header = () => within(screen.getByLabelText("Work progress")).getAllByRole("button")[0]!;

  it("names the current step with a live m:ss clock, never a phase word", () => {
    const { rerender } = render(<ChatProgressTimeline progress={reading} status="streaming" phase="tool" onOpenLink={async () => {}} />);
    expect(header()).toHaveTextContent("Reading hanul.example · 0:12");
    act(() => { vi.advanceTimersByTime(60_000); });
    expect(header()).toHaveTextContent("Reading hanul.example · 1:12");
    const idle = { ...reading, items: reading.items.map((item) => item.kind === "commentary" ? item : { ...item, status: "succeeded" as const }) };
    rerender(<ChatProgressTimeline progress={idle} status="streaming" phase="thinking" onOpenLink={async () => {}} />);
    expect(header()).toHaveTextContent(/^Thinking it through · 1:12$/);
    rerender(<ChatProgressTimeline progress={idle} status="streaming" phase="writing" onOpenLink={async () => {}} />);
    expect(header()).toHaveTextContent(/^Writing · 1:12$/);
    expect(screen.getByLabelText("Work progress")).not.toHaveTextContent(/Using a tool|Working/);
  });

  it("shows Still working after 15 s of silence and recovers on the next event", () => {
    const { container, rerender } = render(<ChatProgressTimeline progress={reading} status="streaming" phase="tool" onOpenLink={async () => {}} />);
    act(() => { vi.advanceTimersByTime(14_000); });
    expect(header()).toHaveTextContent(/^Reading hanul\.example · /);
    act(() => { vi.advanceTimersByTime(2_000); });
    expect(header()).toHaveTextContent(/^Still working · Reading hanul\.example · 0:28$/);
    expect(container.querySelector(".chat-progress-live")).toHaveAttribute("data-heartbeat", "true");
    const next = { ...reading, items: [...reading.items, { id: "compare", kind: "tool" as const, category: "compute" as const, operation: "calculate" as const, action: "Compared utilization with inventory days", status: "running" as const }] };
    rerender(<ChatProgressTimeline progress={next} status="streaming" phase="tool" onOpenLink={async () => {}} />);
    expect(header()).toHaveTextContent(/^Compared utilization with inventory days · 0:28$/);
    expect(container.querySelector(".chat-progress-live")).not.toHaveAttribute("data-heartbeat");
  });

  it("never reads a streaming answer as stalled", () => {
    const writing = { ...reading, items: [] };
    render(<ChatProgressTimeline progress={writing} status="streaming" phase="writing" onOpenLink={async () => {}} />);
    act(() => { vi.advanceTimersByTime(30_000); });
    expect(screen.getByLabelText("Response status")).toHaveTextContent("Writing · 0:42");
  });

  it("settles into the worked duration and the step count, keeping failures", () => {
    const done = { startedAtMs: start, finishedAtMs: start + 64_000, items: [
      { id: "one", kind: "tool" as const, category: "web" as const, operation: "read" as const, action: "Read the report", status: "succeeded" as const },
      { id: "two", kind: "tool" as const, category: "web" as const, operation: "read" as const, action: "Read the deck", status: "failed" as const },
    ] };
    render(<ChatProgressTimeline progress={done} status="complete" onOpenLink={async () => {}} />);
    expect(header()).toHaveTextContent(/^Worked 1:04 · 2 steps · 1 failed$/);
    expect(screen.getByLabelText("Work progress")).not.toHaveTextContent(/Web|Files/);
  });
});

describe("names a step read", () => {
  const reading: Progress = { startedAtMs: 1, items: [
    { id: "map-1", kind: "tool", category: "memory", operation: "read", action: "Read the map", status: "running", entityIds: ["a", "b"] },
    { id: "web-1", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", status: "succeeded" },
  ] };

  it("labels the step with its name count and leaves steps without names alone", () => {
    render(<ChatProgressTimeline progress={reading} status="streaming" onOpenLink={async () => {}} />);
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
      <ChatProgressTimeline progress={reading} status="streaming" onOpenLink={async () => {}} />
    </ChatMapProvider>);
    fireEvent.click(await screen.findByRole("button", { name: "Show 2 on the map" }));
    expect(onOpen).toHaveBeenCalledOnce();
  });
});

describe("plan", () => {
  const plan = (statuses: ["done" | "active" | "pending" | "not_done", ...Array<"done" | "active" | "pending" | "not_done">]) => ({
    items: statuses.map((status, index) => ({ id: `item-${index}`, label: ["Read the Q3 operating report", "Compare utilization with inventory days", "Check against your principles"][index]!, status, ...(status === "pending" ? {} : { activatedAtMs: [10, 20, 30][index] }) })),
  });
  const steps: Progress["items"] = [
    { id: "early", kind: "tool", category: "memory", operation: "read", action: "Read Memory", status: "succeeded", startedAtMs: 5 },
    { id: "report", kind: "tool", category: "web", operation: "read", action: "Read the operating report", status: "succeeded", startedAtMs: 12 },
    { id: "note", kind: "commentary", text: "Comparing the two series." },
    { id: "search", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", status: "succeeded", startedAtMs: 21 },
    { id: "calc", kind: "tool", category: "compute", operation: "calculate", action: "Calculated the ratio", status: "running", startedAtMs: 22 },
  ];

  it("pins the checklist and nests each step under the item that was active when it started", () => {
    render(<ChatProgressTimeline status="streaming" onOpenLink={vi.fn()} progress={{ startedAtMs: 1, items: steps, plan: plan(["done", "active", "pending"]) }} />);
    const list = screen.getByRole("list", { name: "Plan" });
    const [done, active, pending] = within(list).getAllByRole("listitem").filter((item) => item.parentElement === list);
    expect(done).toHaveTextContent("Read the Q3 operating report");
    expect(done).toHaveTextContent("Done");
    expect(within(done!).getByText("Read the operating report")).toBeVisible();
    expect(active).toHaveAttribute("aria-current", "step");
    expect(within(active!).getByText("Searched the web")).toBeVisible();
    expect(within(active!).getByText("Calculated the ratio")).toBeVisible();
    expect(pending).not.toHaveAttribute("aria-current");
    expect(pending).toHaveTextContent("Not started");
    // Steps before the first item, and commentary, stay below the plan in order.
    const early = screen.getByText("Read Memory");
    expect(list.contains(early)).toBe(false);
    expect(follows(list, early)).toBe(true);
    expect(follows(early, screen.getByText("Comparing the two series."))).toBe(true);
    // The plan is not a step.
    expect(screen.queryByText(/Updat(ing|ed) the plan/)).toBeNull();
  });

  it("keeps the ticks and marks unfinished items Not done after a stop", () => {
    render(<ChatProgressTimeline status="aborted" onOpenLink={vi.fn()} progress={{ startedAtMs: 1, finishedAtMs: 40, items: steps.map((item) => item.kind === "tool" && item.status === "running" ? { ...item, status: "stopped" as const } : item), plan: plan(["done", "not_done", "not_done"]) }} />);
    const list = screen.getByRole("list", { name: "Plan" });
    const items = within(list).getAllByRole("listitem").filter((item) => item.parentElement === list);
    expect(items[0]).toHaveTextContent("Done");
    expect(items[1]).toHaveTextContent("Not done");
    expect(items[2]).toHaveTextContent("Not done");
    expect(list.querySelector('[aria-current="step"]')).toBeNull();
  });

  it("renders a run without a plan exactly as before", () => {
    render(<ChatProgressTimeline status="streaming" onOpenLink={vi.fn()} progress={{ startedAtMs: 1, items: steps }} />);
    expect(screen.queryByRole("list", { name: "Plan" })).toBeNull();
    expect(follows(screen.getByText("Read Memory"), screen.getByText("Read the operating report"))).toBe(true);
  });

  it("shows a plan-only run so the checklist is visible before the first step", () => {
    render(<ChatProgressTimeline status="streaming" onOpenLink={vi.fn()} progress={{ startedAtMs: 1, items: [], plan: plan(["active", "pending"]) }} />);
    expect(screen.getByRole("list", { name: "Plan" })).toBeVisible();
  });
});
