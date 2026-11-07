import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { renderWithOpen } from "../../test/open";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import i18n from "@/i18n";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatActivityDetail, ChatMessage, ChatProgress } from "../../types";
import { deriveTurnView } from "../../chat/turnView";
import { ChatExecutionProvider } from "./ChatExecutionContext";
import { ActivityRow } from "./ActivityRow";
import { ChatMapProvider } from "../workspace/ChatMapContext";

const follows = (first: Element, second: Element) => Boolean(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING);

function withActivity(detail: ChatActivityDetail) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const read = vi.spyOn(bridge, "chatActivityRead").mockResolvedValue(detail);
  const wrapper = ({ children }: { children: ReactNode }) => <ChatExecutionProvider value={{ bridge, loupistId: "loupist", threadId: "chat", surface: "session", theme: "dark", active: true }}>{children}</ChatExecutionProvider>;
  return { read, wrapper };
}

/** The first activity row of a message with this progress, as the card derives it. */
function row(progress: ChatProgress, status: ChatMessage["status"], extra: Partial<ChatMessage> = {}, props: Partial<Parameters<typeof ActivityRow>[0]> = {}) {
  const message: ChatMessage = { id: "answer", role: "assistant", status, content: status === "complete" ? "Done." : "", created_at: "2026-10-10T00:00:00Z", progress, phase: status === "streaming" ? "tool" : undefined, ...extra };
  const view = deriveTurnView({ message, now: progress.startedAtMs + 12_000, lastActivityAt: progress.startedAtMs + 12_000, stopping: false, retryPending: false });
  const segment = view.segments.find((item) => item.kind === "activity")!;
  return <ActivityRow segment={segment} status={status} {...props} />;
}

it("names the running step, then settles into a collapsed summary with the steps in order inside", async () => {
  const user = userEvent.setup();
  const progress: ChatProgress = { startedAtMs: 1, items: [
    { id: "web-1", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", target: "rate outlook", source: { kind: "web", name: "web_search" }, status: "succeeded" },
    { id: "web-2", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", running: "Reading example.com", target: "Rate outlook · example.com", href: "https://example.com/rates", status: "running" },
  ] };
  const { open: openLink, ...view } = renderWithOpen(row(progress, "streaming"));
  const live = screen.getByRole("button", { name: "Reading example.com" });
  expect(live).toHaveAttribute("aria-expanded", "false");
  expect(screen.queryByText("Searched the web")).toBeNull();
  await user.click(live);
  const readTarget = screen.getByRole("button", { name: "Rate outlook · example.com" });
  expect(follows(screen.getByText("Searched the web"), readTarget)).toBe(true);
  expect(screen.queryByText("web_search")).toBeNull();
  await user.click(readTarget);
  expect(openLink).toHaveBeenCalledWith({ kind: "link", url: "https://example.com/rates" }, undefined);

  const settled = { ...progress, finishedAtMs: 10, items: progress.items.map((item) => ({ ...item, status: "succeeded" as const })) };
  view.rerender(row(settled, "complete"));
  // A run under a second shows its subject without a "0:00" clock; the reader's choice to open it stands.
  const summary = screen.getByRole("button", { name: /^Source review/ });
  expect(summary).toHaveAttribute("aria-expanded", "true");
  expect(summary.querySelector(".chat-progress-elapsed")).toBeNull();
});

it("opens the details for a failed step and offers its exact activity", async () => {
  const { read, wrapper } = withActivity({ id: "report-attempt", tool_name: "read", status: "failed", input: "annual-report.pdf", output: "File not found", truncated: false, started_at_ms: 1, finished_at_ms: 2000 });
  render(row({ startedAtMs: 1, finishedAtMs: 2000, items: [
    { id: "failed", kind: "tool", category: "web", operation: "read", action: "Read annual report", status: "failed", detailId: "report-attempt" },
    { id: "ok", kind: "tool", category: "web", operation: "read", action: "Read filing", status: "succeeded" },
  ] }, "complete"), { wrapper });
  const header = screen.getByRole("button", { name: /1 failed attempt/ });
  expect(header).toHaveAttribute("aria-expanded", "true");
  expect(header).toHaveTextContent("0:01");
  expect(screen.getByLabelText("Failed")).toBeVisible();
  await userEvent.setup().click(screen.getByRole("button", { name: "Inspect Read annual report" }));
  expect(await within(screen.getByLabelText("Work progress")).findByText("File not found")).toBeVisible();
  expect(read).toHaveBeenCalledExactlyOnceWith({ loupist_id: "loupist", thread_id: "chat", detail_id: "report-attempt" });
});

it("explains a step paused by the untrusted-text rule in the step, calmly and with no override (B92)", async () => {
  render(row({ startedAtMs: 1, finishedAtMs: 2000, items: [
    { id: "search", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", status: "succeeded" },
    { id: "bash", kind: "tool", toolName: "bash", category: "compute", operation: "execute", action: "Ran a command", target: "python margin_model.py", status: "failed", refusal: "untrusted_text_paused" },
    { id: "fetch", kind: "tool", toolName: "web_fetch", category: "web", operation: "read", action: "Read a web source", target: "example.com/review", status: "failed", refusal: "untrusted_text_paused" },
  ] }, "complete"));
  // The header counts a pause, not a failure, and the details stay closed.
  const header = screen.getByRole("button", { name: /2 paused/ });
  expect(header).toHaveAttribute("aria-expanded", "false");
  expect(header).not.toHaveTextContent("failed");
  await userEvent.setup().click(header);
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
    render(row({ startedAtMs: 1, finishedAtMs: 2000, items: [
      { id: "paused", kind: "tool", toolName: "bash", category: "compute", operation: "execute", action: "Ran a command", target: "paused.py", status: "failed", refusal: "untrusted_text_paused", detailId: "paused-detail" },
      { id: "failed", kind: "tool", toolName: "bash", category: "compute", operation: "execute", action: "Ran a command", target: "failed.py", status: "failed" },
      { id: "succeeded", kind: "tool", toolName: "bash", category: "compute", operation: "execute", action: "Ran a command", target: "done.py", status: "succeeded" },
    ] }, "complete"));
    const paused = screen.getByText("paused.py").closest(".chat-progress-row")!;
    expect(within(paused as HTMLElement).getByText("일시 중지")).toBeVisible();
    expect(within(paused as HTMLElement).queryByText("명령 실행함")).toBeNull();
    expect(within(paused as HTMLElement).getByRole("note")).toHaveTextContent("이번 답변에서는 명령 실행이 꺼져 있어요.");
    expect(screen.getAllByText("명령 실행함")).toHaveLength(2);
  } finally { await i18n.changeLanguage("en"); }
});

it("loads running command output on demand and stops polling when closed", async () => {
  vi.useFakeTimers();
  try {
    const { read, wrapper } = withActivity({ id: "durable-detail", tool_name: "bash", status: "running", input: JSON.stringify({ command: "python calculate.py" }), output: "Partial result: 391", truncated: true, started_at_ms: 10 });
    render(row({ startedAtMs: 1, items: [
      { id: "tool", kind: "tool", toolName: "bash", category: "compute", operation: "execute", action: "Ran a command", running: "Running a command", status: "running", detailId: "durable-detail" },
    ] }, "streaming"), { wrapper });
    expect(read).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Running a command" }));
    const inspect = screen.getByRole("button", { name: "Inspect Running a command" });
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
  render(row({ startedAtMs: 1, finishedAtMs: 20, items: [
    { id: "tool", kind: "tool", toolName: "bash", category: "compute", operation: "execute", action: "Ran a command", target: "python calculate.py", status: "succeeded", detailId: "durable-detail" },
  ] }, "complete", {}, { focusItemId: "tool" }), { wrapper });
  expect(await screen.findByLabelText("Command output")).toHaveTextContent("Partial result: 391");
});

it("keeps internal targets out of live and saved progress", () => {
  const progress: ChatProgress = { startedAtMs: 1, items: [
    { id: "page", kind: "tool", category: "web", operation: "read", action: "Read a page", target: "web:f6495eccf2c0cca04e4c6a03", status: "succeeded" },
    { id: "tool", kind: "tool", category: "capability", operation: "read", action: "Opened a tool", target: "private-tool-id", source: { kind: "capability", name: "private-tool-id" }, status: "running" },
  ] };
  const { container, rerender } = render(row(progress, "streaming"));
  expect(container).toHaveTextContent("Opened a tool");
  expect(container).not.toHaveTextContent(/private-tool-id|web:f649/);
  rerender(row({ ...progress, finishedAtMs: 2000, items: progress.items.map((item) => ({ ...item, status: "succeeded" as const })) }, "complete"));
  fireEvent.click(screen.getByRole("button", { name: /Source review/ }));
  expect(container).toHaveTextContent("Read a page");
  expect(container).not.toHaveTextContent(/private-tool-id|web:f649/);
});

describe("names a step read", () => {
  const reading: ChatProgress = { startedAtMs: 1, items: [
    { id: "map-1", kind: "tool", category: "memory", operation: "read", action: "Read the map", running: "Reading the map", status: "running", entityIds: ["a", "b"] },
    { id: "web-1", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", status: "succeeded" },
  ] };

  it("labels the step with its name count and leaves steps without names alone", () => {
    render(row(reading, "streaming"));
    fireEvent.click(screen.getByRole("button", { name: "Reading the map" }));
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
      {row(reading, "streaming")}
    </ChatMapProvider>);
    fireEvent.click(screen.getByRole("button", { name: "Reading the map" }));
    fireEvent.click(await screen.findByRole("button", { name: "Show 2 on the map" }));
    expect(onOpen).toHaveBeenCalledOnce();
  });
});

describe("plan", () => {
  const plan = (statuses: ["done" | "active" | "pending" | "not_done", ...Array<"done" | "active" | "pending" | "not_done">]) => ({
    items: statuses.map((status, index) => ({ id: `item-${index}`, label: ["Read the Q3 operating report", "Compare utilization with inventory days", "Check against your principles"][index]!, status, ...(status === "pending" ? {} : { activatedAtMs: [10, 20, 30][index] }) })),
  });
  const steps: ChatProgress["items"] = [
    { id: "early", kind: "tool", category: "memory", operation: "read", action: "Read Memory", status: "succeeded", startedAtMs: 5 },
    { id: "report", kind: "tool", category: "web", operation: "read", action: "Read the operating report", status: "succeeded", startedAtMs: 12 },
    { id: "search", kind: "tool", toolName: "web_search", category: "web", operation: "search", action: "Searched the web", status: "succeeded", startedAtMs: 21 },
    { id: "calc", kind: "tool", category: "compute", operation: "calculate", action: "Calculated the ratio", running: "Calculating the ratio", status: "running", startedAtMs: 22 },
  ];

  it("pins the checklist and nests each step under the item that was active when it started", () => {
    render(row({ startedAtMs: 1, items: steps, plan: plan(["done", "active", "pending"]) }, "streaming"));
    fireEvent.click(screen.getByRole("button", { name: "Calculating the ratio" }));
    const list = screen.getByRole("list", { name: "Plan" });
    const [done, active, pending] = within(list).getAllByRole("listitem").filter((item) => item.parentElement === list);
    expect(done).toHaveTextContent("Read the Q3 operating report");
    expect(done).toHaveTextContent("Done");
    expect(within(done!).getByText("Read the operating report")).toBeVisible();
    expect(active).toHaveAttribute("aria-current", "step");
    expect(within(active!).getByText("Searched the web")).toBeVisible();
    expect(within(active!).getByText("Calculating the ratio")).toBeVisible();
    expect(pending).not.toHaveAttribute("aria-current");
    expect(pending).toHaveTextContent("Not started");
    // Steps before the first item stay below the plan in order.
    const early = screen.getByText("Read Memory");
    expect(list.contains(early)).toBe(false);
    expect(follows(list, early)).toBe(true);
    // The plan is not a step.
    expect(screen.queryByText(/Updat(ing|ed) the plan/)).toBeNull();
  });

  it("keeps the ticks and marks unfinished items Not done after a stop", () => {
    render(row({ startedAtMs: 1, finishedAtMs: 40, items: steps.map((item) => item.kind === "tool" && item.status === "running" ? { ...item, status: "stopped" as const } : item), plan: plan(["done", "not_done", "not_done"]) }, "aborted"));
    const list = screen.getByRole("list", { name: "Plan" });
    const items = within(list).getAllByRole("listitem").filter((item) => item.parentElement === list);
    expect(items[0]).toHaveTextContent("Done");
    expect(items[1]).toHaveTextContent("Not done");
    expect(items[2]).toHaveTextContent("Not done");
    expect(list.querySelector('[aria-current="step"]')).toBeNull();
  });

  it("names the active stage while the run works and shows a plan-only run before the first step", () => {
    const { rerender } = render(row({ startedAtMs: 1, items: [], plan: plan(["active", "pending"]) }, "streaming"));
    fireEvent.click(screen.getByRole("button", { name: "Read the Q3 operating report" }));
    expect(screen.getByRole("list", { name: "Plan" })).toBeVisible();
    rerender(row({ startedAtMs: 1, items: steps.slice(0, 2), plan: plan(["active", "pending"]) }, "streaming"));
    expect(screen.getByRole("button", { name: "Read the Q3 operating report" })).toBeVisible();
  });
});
