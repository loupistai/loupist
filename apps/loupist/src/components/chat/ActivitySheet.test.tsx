import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { evidenceFixture } from "../../test/evidenceFixtures";
import { destinations, renderWithOpen } from "../../test/open";
import { formatTime } from "@/i18n/format";
import i18n from "@/i18n";
import userEvent from "@testing-library/user-event";
import { ActivitySheet, OPEN_ACTIVITY_EVENT, useActivitySheet } from "./ActivitySheet";
import { ChatLightboxProvider } from "./ChatLightbox";
import { MOCK_LOUPISTS } from "../../bridge/mock/fixtures";
import type { RailData } from "../../chat/chatRail";
import type { ChatMessage, LoupistBridge } from "../../types";

const step = (id: string, toolName: string, extra: Record<string, unknown> = {}) => ({
  id, kind: "tool", toolName, category: "finance", operation: "read", action: "Read", target: "NVDA", href: null,
  status: "succeeded", startedAtMs: 1_000, finishedAtMs: 1_400, ...extra,
});
const question: ChatMessage = { id: "q1", role: "user", content: "How is NVIDIA doing", created_at: "2026-10-28T00:00:00Z", status: "complete" };
const first = {
  id: "a1", role: "assistant", status: "complete", content: "NVIDIA closed lower.", created_at: "2026-10-28T00:00:10Z",
  progress: { startedAtMs: 0, finishedAtMs: 9_000, items: [
    step("s1", "mcp__openbb__equity_price_quote"),
    step("s2", "mcp__openbb__equity_price_quote", { target: "AMD" }),
    step("s3", "mcp__openbb__equity_estimates_forward_eps", { status: "failed", finishedAtMs: 3_000 }),
  ] },
  evidence_refs: [{ id: "e1", title: "NVDA quote", kind: "original", tool_name: "equity_price_quote", provider: "yfinance", retrieved_at: "2026-10-28T00:00:00Z", excerpt: "", excerpt_truncated: false, content_sha256: "", warnings: [] }],
  number_checks: [{ link_index: 0, text: "$162.85", evidence_id: "e1", kind: "data", status: "matched" }],
} as unknown as ChatMessage;
const second = { ...first, id: "a2", created_at: "2026-10-28T00:05:10Z", progress: { startedAtMs: 0, finishedAtMs: 2_000, items: [step("t1", "web_search", { category: "web", operation: "search", target: "chip export rules" })] }, number_checks: [], evidence_refs: [] } as unknown as ChatMessage;

const rail: RailData = {
  files: [{ key: "memo", kind: "file", filename: "memo.md", path: "memo.md", fromLoupist: true }],
  using: [{ key: "principle:p1", kind: "principle", label: "Check operating cash flow", recordId: "p1" }, { key: "connection:Morrow", kind: "connection", label: "Morrow" }],
  learned: { names: 2, relationships: 1, methods: 0, chips: [{ id: "hanul", label: "Hanul Precision", swatch: "company" }], highlightIds: new Set(["hanul", "morrow"]) },
  learningPending: false, needs: [], waiting: [], questions: [], hasCards: true,
};

function Harness({ data = rail, messages = [question, first, { ...question, id: "q2" }, second], onShowOnMap = vi.fn(), onOpenFile = vi.fn(), onOpenUsing = vi.fn() }: {
  data?: RailData; messages?: ChatMessage[]; onShowOnMap?: (ids?: ReadonlySet<string>) => void; onOpenFile?: (path: string) => void; onOpenUsing?: () => void;
}) {
  const sheet = useActivitySheet(true);
  return <ChatLightboxProvider messages={messages} onRead={async () => ""}>
    <section ref={sheet.layoutRef} className="chat-layout">
      <button type="button" data-header-activity onClick={() => window.dispatchEvent(new CustomEvent(OPEN_ACTIVITY_EVENT, { detail: { toggle: true } }))}>Activity</button>
      <textarea aria-label="Composer" />
      <ActivitySheet sheet={sheet} loupist={MOCK_LOUPISTS[0]!} messages={messages} running={false} rail={data} bridge={{} as LoupistBridge} execution={null}
        onOpenUsing={onOpenUsing} onOpenFile={onOpenFile} onShowOnMap={onShowOnMap} />
    </section>
  </ChatLightboxProvider>;
}

const openActivity = (detail?: Record<string, unknown>) => act(() => { window.dispatchEvent(new CustomEvent(OPEN_ACTIVITY_EVENT, { detail })); });

it("shows recorded save and wait times and identifies a save interrupted before it finished", () => {
  const message: ChatMessage = { ...first, progress: { ...first.progress!, saveActivity: {
    startedAtMs: 1_000, finishedAtMs: 5_350, waitedMs: 4_300,
  } } };
  const { rerender } = render(<Harness messages={[question, message]} />);
  openActivity();
  const row = screen.getByText("Saving to Memory").closest("li")!;
  expect(row.querySelector("time")).toHaveTextContent("4.4");
  expect(row).toHaveTextContent(/Waited 4.3.*for other conversations in this Loupist/);
  rerender(<Harness messages={[question, { ...message,
    progress: { ...message.progress!, saveActivity: { ...message.progress!.saveActivity!, finishedAtMs: null } },
    memory_update: { status: "no_change", skipReason: "interrupted", commitId: null, changes: [] },
  }]} />);
  expect(row).toHaveTextContent("Saving did not finish");
  expect(row.querySelector("time")).toBeNull();
});

it("heads the sheet with its tabs and close alone: no face or name, the answer's time once, Working only while a reply runs", () => {
  function Sheet({ running }: { running: boolean }) {
    const sheet = useActivitySheet(true);
    const messages = [question, first];
    return <ChatLightboxProvider messages={messages} onRead={async () => ""}>
      <section ref={sheet.layoutRef} className="chat-layout">
        <ActivitySheet sheet={sheet} loupist={MOCK_LOUPISTS[0]!} messages={messages} running={running} rail={rail} bridge={{} as LoupistBridge} execution={null} onOpenUsing={vi.fn()} onOpenFile={vi.fn()} />
      </section>
    </ChatLightboxProvider>;
  }
  const { rerender } = render(<Sheet running={false} />);
  openActivity();
  const sheet = screen.getByTestId("activity-sheet");
  const header = sheet.querySelector("header")!;
  expect(header).not.toHaveTextContent(MOCK_LOUPISTS[0]!.name);
  expect(header.querySelector(".loupist-avatar")).toBeNull();
  // The time is shown once, in What it did; the header only speaks its status.
  expect(within(header).getByRole("status")).toHaveClass("sr-only");
  expect(within(header).getByRole("status")).toHaveTextContent(/^Answered at /);
  expect(sheet.querySelector("[data-activity-section=steps] h3")).toHaveTextContent(/This answer · /);
  rerender(<Sheet running />);
  expect(within(header).getByRole("status")).toHaveTextContent("Working");
  expect(within(header).getByRole("status")).not.toHaveClass("sr-only");
});

it.each([undefined, "rate_limited"] as const)("shows and announces the failed reply's time and recorded reason (%s)", (failure_kind) => {
  render(<Harness messages={[question, { ...first, status: "error", failure_kind }]} />);
  openActivity();
  const sheet = within(screen.getByTestId("activity-sheet"));
  const time = `Failed at ${formatTime(Date.parse(first.created_at))}`;
  const summary = failure_kind === "rate_limited" ? `${time} · Your AI account was busy.` : time;
  expect(sheet.getByRole("status")).toHaveTextContent(summary);
  expect(sheet.getByRole("heading", { name: /What it did/ })).toHaveTextContent(summary);
  expect(sheet.queryByText(/Answered at|This answer ·/)).toBeNull();
});

it("groups two citations of one saved Source and opens its exact passage", async () => {
  const ref = { kind: "source" as const, recordId: "apple", revisionId: "rev-2", objectId: "apple-text", spanId: "passage-a" };
  const read = { read_ref: "read-a", kind: "source" as const, title: "Apple announces CFO transition", excerpt: "", access: "exact_read" as const, as_of: null, section: null, knowledge: ref };
  const evidence = evidenceFixture({ id: "ev_a", kind: "saved_context", tool_name: "world_read", title: "world read", read_ref: "read-a", user_provided: true });
  const message = { ...first, content: "The [announcement](#evidence-ev_a) set a [date][d].\n\n[d]: #evidence-ev_b",
    evidence_refs: [evidence, { ...evidence, id: "ev_b", read_ref: "read-b" }, { ...evidence, id: "ev_unused", read_ref: "unused" }],
    memory_refs: [read, { ...read, read_ref: "read-b", knowledge: { ...ref, spanId: "passage-b" } },
      { ...read, read_ref: "unused", title: "Other filing", knowledge: { ...ref, recordId: "other" } }] };
  const { open, rerender } = renderWithOpen(<Harness messages={[question, message]} />);
  openActivity();
  const sources = () => within(screen.getByRole("heading", { name: /What it read/ }).closest("section")!);
  expect(sources().getByRole("heading")).toHaveTextContent("What it read1");
  expect(sources().getAllByRole("listitem")).toHaveLength(1);
  const row = sources().getByRole("button", { name: /Apple announces CFO transition/ });
  expect(row).toHaveTextContent("User-provided");
  expect(row).toHaveTextContent("2 citations");
  expect(sources().queryByText("This answer cited no sources.")).toBeNull();
  await userEvent.setup().click(row);
  expect(destinations(open)).toMatchObject([{ kind: "memory", title: read.title, location: {
    loupistId: MOCK_LOUPISTS[0]!.id, surface: "source", recordId: "apple", revisionId: "rev-2", selection: { spanId: "passage-a" },
  } }]);
  rerender(<Harness messages={[question, { ...message, content: "Only code: `[example](#evidence-ev_a)`." }]} />);
  expect(sources().getByText("This answer cited no sources.")).toBeVisible();
});

it("never opens by itself, then opens on the event at the latest answer and closes with Esc", async () => {
  render(<Harness />);
  expect(screen.queryByTestId("activity-sheet")).toBeNull();
  openActivity();
  const sheet = screen.getByRole("complementary", { name: "Activity" });
  expect(within(sheet).getByRole("tab", { name: "Activity" })).toHaveAttribute("aria-selected", "true");
  expect(sheet).toHaveTextContent("Searched the web");
  expect(document.querySelector("[data-header-activity]")).toHaveAttribute("aria-pressed", "true");
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByTestId("activity-sheet")).toBeNull();
  expect(document.querySelector("[data-header-activity]")).toHaveAttribute("aria-pressed", "false");
});

it("updates chart and artifact result links and their step headings when the interface language changes", async () => {
  const chart = evidenceFixture({ id: "chart-result", kind: "derived", tool_name: "chart_query", title: "chart query" });
  const artifact = evidenceFixture({ id: "artifact-result", kind: "derived", tool_name: "artifact_read", title: "artifact read" });
  const message = {
    ...first,
    progress: { ...first.progress, items: [
      step("chart", "chart_query", { resultIds: [chart.id] }),
      step("artifact", "artifact_read", { resultIds: [artifact.id] }),
    ] },
    evidence_refs: [chart, artifact], number_checks: [],
  } as unknown as ChatMessage;
  const view = renderWithOpen(<Harness messages={[question, message]} />);
  try {
    openActivity();
    const sheet = within(screen.getByTestId("activity-sheet"));
    expect(sheet.getByRole("button", { name: "chart data" })).toBeVisible();
    expect(sheet.getByRole("button", { name: "conversation document" })).toBeVisible();
    await act(async () => { await i18n.changeLanguage("ko"); });
    expect(sheet.getByRole("button", { name: "차트 데이터" })).toBeVisible();
    expect(sheet.getByRole("button", { name: "대화 문서" })).toBeVisible();
    expect(sheet.getByText("차트 데이터 읽음")).toBeVisible();
    expect(sheet.getByText("대화 문서 읽음")).toBeVisible();
  } finally {
    view.unmount();
    await i18n.changeLanguage("en");
  }
});

it("moves between tabs with the arrows, Home and End", async () => {
  const user = userEvent.setup();
  render(<Harness />);
  openActivity();
  const sheet = screen.getByRole("complementary", { name: "Activity" });
  within(sheet).getByRole("tab", { name: "Activity" }).focus();
  await user.keyboard("{End}");
  expect(within(sheet).getByRole("tab", { name: "In use" })).toHaveFocus();
  expect(within(sheet).getByRole("tab", { name: "In use" })).toHaveAttribute("aria-selected", "true");
  await user.keyboard("{Home}");
  expect(within(sheet).getByRole("tab", { name: "Activity" })).toHaveFocus();
  await user.keyboard("{ArrowLeft}");
  expect(within(sheet).getByRole("tab", { name: "In use" })).toHaveAttribute("aria-selected", "true");
});

it("heads files only when the chat has some", async () => {
  const user = userEvent.setup();
  const view = render(<Harness data={{ ...rail, files: [] }} />);
  openActivity();
  await user.click(screen.getByRole("tab", { name: "In use" }));
  expect(screen.queryByText("Files in this chat")).toBeNull();
  expect(screen.getByRole("button", { name: "Open chat materials" })).toBeVisible();
  view.unmount();
  function NoMap() {
    const sheet = useActivitySheet(true);
    return <section ref={sheet.layoutRef} className="chat-layout">
      <ActivitySheet sheet={sheet} loupist={MOCK_LOUPISTS[0]!} messages={[question, first]} running={false} rail={{ ...rail, files: [] }} bridge={{} as LoupistBridge} execution={null} onOpenUsing={vi.fn()} onOpenFile={vi.fn()} />
    </section>;
  }
  render(<NoMap />);
  openActivity({ tab: "connections" });
  expect(screen.getByTestId("activity-sheet").querySelector("[data-activity-section=files]")).toBeNull();
});

it("does not count an empty result as a failed run in the activity header, and does not paint it as one", async () => {
  const message = {
    ...first,
    id: "empty-mix",
    progress: { startedAtMs: 0, finishedAtMs: 4_000, items: [
      step("empty", "mcp__openbb__dart_financials", { status: "failed", failureReason: "empty_result", finishedAtMs: 2_100 }),
      step("refused", "mcp__openbb__equity_fundamental_metrics", { status: "failed", failureReason: "invalid_input", finishedAtMs: 3_000 }),
    ] },
    evidence_refs: [], number_checks: [],
  } as unknown as ChatMessage;
  const view = render(<Harness messages={[question, message]} />);
  openActivity({ messageId: "empty-mix" });
  const sheet = screen.getByTestId("activity-sheet");
  const header = sheet.querySelector("header")!;
  const section = sheet.querySelector("[data-activity-section=steps] h3")!;
  const groups = within(sheet).getAllByTestId("activity-step-group");
  expect(groups.map((group) => group.getAttribute("data-state"))).toEqual(["empty", "bad"]);
  expect(header).not.toHaveTextContent(/2 failed|실행 2건 실패/);
  expect(section).not.toHaveTextContent(/2 failed|실행 2건 실패/);
  expect(groups[0]).toHaveTextContent("The source had nothing for this request. It was not used in the answer.");
  expect(groups[1]).toHaveTextContent("The source did not accept one of the request's values. It was not used in the answer.");
  await act(async () => { await i18n.changeLanguage("ko"); });
  try {
    expect(header).not.toHaveTextContent("실행 2건 실패");
    expect(section).not.toHaveTextContent("실행 2건 실패");
    expect(groups[0]).toHaveTextContent("자료 제공처에 이 요청에 해당하는 내용이 없었습니다. 답변에 사용하지 않았습니다.");
    expect(groups[0]).toHaveAttribute("data-state", "empty");
    expect(groups[1]).toHaveAttribute("data-state", "bad");
  } finally {
    view.unmount();
    await i18n.changeLanguage("en");
  }
});

it("selects the asked-for answer: same-kind steps collapse with a count, failures keep their reason, sources and checks open", async () => {
  const user = userEvent.setup();
  render(<Harness />);
  openActivity({ messageId: "a1", tab: "sources" });
  const sheet = screen.getByTestId("activity-sheet");
  const groups = within(sheet).getAllByTestId("activity-step-group");
  expect(groups.map((group) => group.querySelector(".activity-step-label")?.textContent)).toEqual(["Read current quote", "Read earnings estimates"]);
  expect(groups[0]).toHaveTextContent("2");
  expect(groups[0]).toHaveTextContent("NVDA, AMD");
  expect(groups[0]).toHaveTextContent("0.8s");
  expect(groups[1]).toHaveAttribute("data-state", "bad");
  expect(groups[1]).toHaveTextContent("It ended with an error. It was not used in the answer.");
  expect(groups[1]).toHaveTextContent("2.0s");
  await user.click(within(groups[0]!).getByRole("button", { name: "Show each of 2" }));
  expect(within(groups[0]!).getAllByTestId("activity-step").map((row) => row.textContent)).toEqual(["NVDA0.4s", "AMD0.4s"]);
  // The source is listed under "What it read" and named again beside the figure it backs.
  expect(within(sheet.querySelector("[data-activity-section=sources]") as HTMLElement).getByText("NVDA quote")).toBeInTheDocument();
  expect(within(sheet.querySelector("[data-activity-section=checks]") as HTMLElement).getByText("NVDA quote")).toBeInTheDocument();
  expect(sheet).toHaveTextContent("1 figure checked against its source");
  expect(sheet).toHaveTextContent("$162.85Matches");
  // Earlier answers switch the sheet to that answer. A question too long for its line ends in an
  // ellipsis; its whole text stays in the row's accessible name and native title.
  const earlier = within(sheet).getByRole("button", { name: /How is NVIDIA doing/ });
  const question = earlier.querySelector(".activity-step-label")!.textContent!;
  expect(earlier).toHaveAttribute("title", question);
  expect(earlier).toHaveAccessibleName(expect.stringContaining(question));
  await user.click(earlier);
  expect(sheet).toHaveTextContent("Searched the web");
});

it("keeps what was learned and what is in use behind their tabs, with World and the chat's materials one click away", async () => {
  const user = userEvent.setup();
  const onShowOnMap = vi.fn();
  const onOpenFile = vi.fn();
  const onOpenUsing = vi.fn();
  const { rerender } = render(<Harness onShowOnMap={onShowOnMap} onOpenFile={onOpenFile} onOpenUsing={onOpenUsing} />);
  openActivity({ tab: "learned" });
  const sheet = screen.getByTestId("activity-sheet");
  expect(sheet).toHaveTextContent("+2 in World");
  await user.click(within(sheet).getByRole("button", { name: "Hanul Precision" }));
  expect(onShowOnMap).toHaveBeenCalledWith(new Set(["hanul"]));
  await user.click(within(sheet).getByRole("button", { name: "Show in World" }));
  expect(onShowOnMap).toHaveBeenLastCalledWith(rail.learned!.highlightIds);
  // A source review still running is a quiet line, not a spinner.
  rerender(<Harness data={{ ...rail, learningPending: true }} onShowOnMap={onShowOnMap} />);
  expect(within(sheet).getAllByRole("status").some((line) => line.textContent === "Reviewing sources…")).toBe(true);
  expect(sheet).toHaveTextContent("Reviewing sources");
  expect(sheet.querySelector(".animate-spin")).toBeNull();

  rerender(<Harness onShowOnMap={onShowOnMap} onOpenFile={onOpenFile} onOpenUsing={onOpenUsing} />);
  await user.click(within(sheet).getByRole("tab", { name: "In use" }));
  await user.click(within(sheet).getByRole("button", { name: /Check operating cash flow/ }));
  expect(onOpenUsing).toHaveBeenCalledWith(rail.using[0]);
  expect(within(sheet).queryByRole("button", { name: /Morrow/ })).toBeNull();
  await user.click(within(sheet).getByRole("button", { name: /memo\.md/ }));
  expect(onOpenFile).toHaveBeenCalledWith("memo.md");
  await user.click(within(sheet).getByRole("button", { name: "Open chat materials" }));
  expect(onShowOnMap).toHaveBeenLastCalledWith();
});

it("toggles from the header's button, keeps Esc for the composer while docked, and closes on a click outside while it overlays", async () => {
  const user = userEvent.setup();
  render(<Harness />);
  await user.click(screen.getByRole("button", { name: "Activity" }));
  expect(screen.getByTestId("activity-sheet")).toHaveAttribute("data-placement", "overlay");
  await user.click(screen.getByRole("button", { name: "Activity" }));
  expect(screen.queryByTestId("activity-sheet")).toBeNull();
  openActivity();
  await user.click(screen.getByRole("textbox", { name: "Composer" }));
  expect(screen.queryByTestId("activity-sheet")).toBeNull();
});

it("stops an overlaying sheet above the composer, says the reply is done while sources are reviewed, and lists no host post as an answer", () => {
  const post = { id: "post", role: "assistant", status: "complete", content: "World now also tracks process nodes.", created_at: "2026-10-28T00:06:00Z", host_composed: true,
    vocabulary: { added: [] } } as unknown as ChatMessage;
  const box = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    const edges = this.classList.contains("composer-shell") ? { top: 640, bottom: 760 } : { top: 0, bottom: 760 };
    return { ...edges, left: 0, right: 600, width: 600, height: edges.bottom - edges.top, x: 0, y: edges.top, toJSON: () => ({}) } as DOMRect;
  });
  function WithComposer() {
    const sheet = useActivitySheet(true);
    const messages = [question, first, { ...question, id: "q2" }, second, post];
    return <ChatLightboxProvider messages={messages} onRead={async () => ""}>
      <section ref={sheet.layoutRef} className="chat-layout"><div className="composer-shell" />
        <ActivitySheet sheet={sheet} loupist={MOCK_LOUPISTS[0]!} messages={messages} running={false} rail={{ ...rail, learningPending: true }} bridge={{} as LoupistBridge} execution={null}
          onOpenUsing={vi.fn()} onOpenFile={vi.fn()} />
      </section>
    </ChatLightboxProvider>;
  }
  render(<WithComposer />);
  openActivity();
  const sheet = screen.getByTestId("activity-sheet");
  expect(sheet).toHaveAttribute("data-placement", "overlay");
  expect(sheet.style.bottom).toBe("120px");
  expect(document.querySelector<HTMLElement>(".activity-sheet-scrim")!.style.bottom).toBe("120px");
  expect(within(sheet).getAllByRole("status")[0]).toHaveTextContent(/^Answered at /);
  // The newest answer is shown; the host's own post is not one of the earlier answers.
  expect(sheet.querySelector("[data-activity-section=earlier]")!.querySelectorAll(":scope > ol > li")).toHaveLength(1);
  box.mockRestore();
});

it("opens Bot's flagged calculation in Activity at the selected output", async () => {
  const result = evidenceFixture({ id: "ev_calc", kind: "derived", tool_name: "run_python",
    excerpt: JSON.stringify({ input: "50368.7433", stdout: "Total 50368.7433" }) });
  const message: ChatMessage = { ...first, evidence_refs: [result], number_checks: [{
    link_index: 2, evidence_id: result.id, text: "50.4억 달러", kind: "step", status: "not_found",
    comparisonReason: "scale_conflict", comparedValue: "50368.7433", pointer: "/stdout",
  }] };
  render(<Harness messages={[message, second]} />);
  openActivity({ messageId: message.id, tab: "steps", figureIndex: 2 });
  const output = await screen.findByLabelText("In the result");
  expect(output).toHaveTextContent("Total 50368.7433");
  expect(output.querySelector("mark")).toHaveTextContent("50368.7433");
  openActivity({ messageId: second.id, tab: "steps" });
  expect(screen.queryByLabelText("In the result")).toBeNull();
});
