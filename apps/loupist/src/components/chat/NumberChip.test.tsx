import type { ComponentProps } from "react";
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import i18n from "@/i18n";
import { formatDate } from "@/i18n/format";
import userEvent from "@testing-library/user-event";
import type { ChatEvidenceRef, ChatMessage, NumberCheck } from "../../types";
import { evidenceFixture } from "../../test/evidenceFixtures";
import { chatMarkdown } from "../../chat/exportChat";
import { chatReceipt } from "../../chat/receipt";
import { TooltipProvider } from "../ui/tooltip";
import { ChatMessageCard } from "./ChatMessageCard";
import { NumberChip, popoverCollisionPadding } from "./NumberChip";
import { renderWithOpen, destinations } from "../../test/open";

// The thread's scroll actions: revealing something in the answer keeps the page there.
const scroll = vi.hoisted(() => ({ holdScrollPosition: vi.fn(), jumpToLatest: vi.fn(), blockAppended: vi.fn() }));
vi.mock("../ai-elements/conversation", async (load) => ({ ...(await load<typeof import("../ai-elements/conversation")>()), useConversationScroll: () => scroll }));

const dart = evidenceFixture({ id: "ev_dart", title: "Solsup Foods FY2025 statements", tool_name: "dart_financials", provider: "OpenDART", url: undefined, published_at: "2026-03-18",
  excerpt: JSON.stringify({ facts: [{ corp_name: "Solsup Foods", canonical_concept: "revenue", value: "452312000000" }, { corp_name: "Solsup Foods", canonical_concept: "operating_income", value: "38104000000" }] }, null, 2), dataset_ref: { dataset_id: "ds", revision: 1, digest: "d".repeat(64) } });
const passageText = "Management guided to an operating margin of 9.1% for FY2025.";
const guide = evidenceFixture({ id: "ev_guide", title: "February guidance call", tool_name: "web_fetch", url: "https://solsup.example/guidance", published_at: "2026-02-12", excerpt: passageText });
const calc: ChatEvidenceRef = evidenceFixture({ id: "ev_calc", title: "Margin", kind: "derived", tool_name: "calculate", calc_operation: "margin", url: undefined, excerpt: "" });
const span = (needle: string) => {
  const start = new TextEncoder().encode(passageText.slice(0, passageText.indexOf(needle))).length;
  return { start, end: start + new TextEncoder().encode(needle).length };
};

/** What Rust ships with each input of a calculation whose `unit` was "KRW billions". */
const KRW_BILLIONS = { kind: "amount", currency: "KRW", scale: 9 } as const;

const checks = (marginStatus: NumberCheck["status"] = "matched"): NumberCheck[] => [
  { link_index: 0, text: marginStatus === "differs" ? "8.5%" : "8.4%", evidence_id: calc.id, kind: "calculated", status: marginStatus, ...(marginStatus === "differs" ? { nearest: "8.42%" } : {}),
    calc: { operation: "margin", title: "Margin", formulas: ["Operating income / Revenue = 38.1 / 452.3 = 8.42%"], inputs: [
      { name: "profit", label: "Operating income", unit: KRW_BILLIONS, display: "38.1", evidence_id: dart.id, status: "rounded" },
      { name: "revenue", label: "Revenue", unit: KRW_BILLIONS, display: "452.3" }] } },
  { link_index: 1, text: "₩452.3bn", evidence_id: dart.id, kind: "data", status: "rounded", exact: "452312000000", pointer: "/facts/0/value",
    fields: [{ key: "corp_name", value: "Solsup Foods" }, { key: "canonical_concept", value: "revenue" }, { key: "value", value: "452312000000", matched: true }] },
  { link_index: 2, text: "9.1%", evidence_id: guide.id, kind: "passage", status: "matched", span: span("9.1%") },
];

const answer = (extra: Partial<ChatMessage> = {}): ChatMessage => ({
  id: "answer", role: "assistant", status: "complete", created_at: "2026-03-18T06:00:00Z",
  content: `The margin was [${extra.number_checks?.[0]?.text ?? "8.4%"}](#evidence-ev_calc) on revenue of [₩452.3bn](#evidence-ev_dart), below the [9.1%](#evidence-ev_guide) guided [1](#evidence-ev_guide).`,
  evidence_refs: [dart, guide, calc], number_checks: checks(),
  progress: { startedAtMs: 1, finishedAtMs: 2, items: [{ id: "step-calc", kind: "tool", toolName: "calculate", category: "compute", operation: "calculate", action: "Calculated", status: "succeeded", resultIds: [calc.id], startedAtMs: 1, finishedAtMs: 2, target: null, href: null }] },
  ...extra,
});

function renderAnswer(message: ChatMessage, extra: Partial<ComponentProps<typeof ChatMessageCard>> = {}) {
  const props = { message, loupistId: "loupist-a", chatId: "chat-a", loupistName: "Quality", onReadAttachment: vi.fn(), ...extra };
  const { open, ...view } = renderWithOpen(<TooltipProvider><ChatMessageCard {...props} /></TooltipProvider>);
  // What the answer handed up to the app: evidence its own turn could not show in place.
  return { ...props, view, open, onOpenEvidence: open };
}

it("opens the number evidence of a finished answer while it saves", async () => {
  renderAnswer(answer({ status: "streaming", memorySaving: { run_id: "run", waiting_for_other_chat: false } }));
  const mark = await chip("9.1 percent");
  await userEvent.click(mark);
  expect(await screen.findByRole("dialog")).toHaveTextContent("February guidance call");
  expect(screen.queryByTestId("memory-row")).toBeNull();
});

const chip = async (text: string) => {
  const found = await screen.findAllByRole("button", { name: new RegExp(`^${text.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&")}`) });
  return found.find(element => element.classList.contains("number-chip"))!;
};

it("explains a matched scale conversion in both languages while keeping real rounding distinct", async () => {
  const source = evidenceFixture({ id: "ev_sec", title: "Annual revenue", excerpt: `${"Prior comparative rows. ".repeat(14)}FY2023 value 383285000000 dollars` });
  const exact = "383285000000";
  const start = source.excerpt.indexOf(exact);
  const check = { evidence_id: source.id, kind: "passage", status: "matched", text: "383,285", link_index: 0, exact,
    span: { start, end: start + exact.length }, scale_conversion: { source_scale: 0, answer_scale: 6, currency: "USD" } } satisfies NumberCheck;
  const rounded = { ...check, status: "rounded", text: "383.3 billion", link_index: 1, scale_conversion: undefined } satisfies NumberCheck;
  const { view } = renderAnswer(answer({
    content: "[383,285](#evidence-ev_sec) million. [383.3 billion](#evidence-ev_sec).",
    evidence_refs: [source], number_checks: [check, rounded],
  }));
  const user = userEvent.setup();
  try {
    const mark = await chip("383,285");
    const description = "Matches the source: 383,285,000,000 USD, shown in millions.";
    expect(mark).toHaveAccessibleDescription(description);
    await user.click(mark);
    const popover = await screen.findByRole("dialog");
    expect(within(popover).getByText(description)).toBeVisible();
    expect(popover.querySelector("mark")).toHaveTextContent(exact);
    await act(async () => { await i18n.changeLanguage("ko"); });
    expect(popover).toHaveTextContent("자료와 일치: 383,285,000,000 USD (백만 단위로 표시)");
    await user.keyboard("{Escape}");
    await act(async () => { await i18n.changeLanguage("en"); });
    await user.click(await chip("383.3"));
    expect(await screen.findByRole("dialog")).toHaveTextContent(/Rounded from .*383,285,000,000/);
  } finally {
    view.unmount();
    await i18n.changeLanguage("en");
  }
});

it.each([
  ["source", "en", "Matches the source’s number; the source’s unit is not stated in this passage."],
  ["answer", "en", "Matches the source’s number; the answer does not state a unit."],
  ["source", "ko", "자료의 숫자와 일치합니다. 이 구간에는 자료의 단위가 적혀 있지 않습니다."],
  ["answer", "ko", "자료의 숫자와 일치합니다. 답변에는 단위가 적혀 있지 않습니다."],
] as const)("explains the unstated %s scale as a verified match (%s)", async (unstatedScale, language, detail) => {
  await i18n.changeLanguage(language);
  const source = evidenceFixture({ id: "ev_filing", title: "Annual revenue", excerpt: unstatedScale === "source" ? "Revenue was 383,285." : "Revenue was 383,285 million." });
  const value = unstatedScale === "answer" ? "383,285" : "383,285 million";
  const { view } = renderAnswer(answer({
    content: `[${value}](#evidence-${source.id}).`, evidence_refs: [source],
    number_checks: [{ evidence_id: source.id, kind: "passage", status: "matched", text: value, link_index: 0, unstated_scale: unstatedScale }],
    memory_update: { status: "no_change", commitId: null, changes: [] },
  }));
  try {
    const mark = await chip(value);
    expect(mark).toHaveAttribute("data-status", "matched");
    expect(mark).toHaveAccessibleDescription(detail);
    expect(within(screen.getByTestId("memory-row")).queryByRole("button", { name: /number.*check|확인할 숫자/iu })).not.toBeInTheDocument();
    await userEvent.click(mark);
    const status = within(await screen.findByRole("dialog")).getByText(detail);
    expect(status).toBeVisible();
    expect(status).toHaveAttribute("data-status", "matched");
  } finally {
    view.unmount();
    await i18n.changeLanguage("en");
  }
});

it.each(["en", "ko"])("shows the retained World Bank observation without internal fields (%s)", async language => {
  const value = "123,975,371";
  const line = `Population, total · Japan · 2024 · ${value}`;
  const source = evidenceFixture({ id: "ev_pop", title: "World Bank population", tool_name: "world_bank_series",
    excerpt: '{"dataset_id":"private-row-id","observations":[{"date":"2024","value":"123975371"}]}' });
  const start = new TextEncoder().encode(line.slice(0, line.indexOf(value))).length;
  await i18n.changeLanguage(language);
  const { view, open } = renderAnswer(answer({ content: `[${value}](#evidence-ev_pop)`, evidence_refs: [source],
    number_checks: [{ evidence_id: source.id, kind: "data", status: "matched", text: value, link_index: 0,
      context: { text: line, start, end: start + value.length } }] }));
  try {
    await userEvent.click(await chip(value));
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent(line);
    expect(dialog.querySelector("mark")).toHaveTextContent(value);
    expect(dialog).not.toHaveTextContent(/dataset_id|private-row-id|2,024/);
    await userEvent.click(within(dialog).getByRole("button", { name: i18n.t("chat:numberChip.openEvidence") }));
    expect(destinations(open).at(-1)).toMatchObject({ kind: "evidence", evidence: source });
  } finally {
    view.unmount();
    await i18n.changeLanguage("en");
  }
});

it("uses the matched market-data observation and leaves legacy dates and codes as written", async () => {
  const value = "162.85";
  const line = `Close · NVDA · 2020 · ${value} USD`;
  const source = evidenceFixture({ id: "ev_quote", title: "NVIDIA prices", excerpt: "raw provider payload" });
  const start = new TextEncoder().encode(line.slice(0, line.indexOf(value))).length;
  const base = { evidence_id: source.id, kind: "data", status: "matched", text: value, link_index: 0 } satisfies NumberCheck;
  const legacy = { ...base, link_index: 1, fields: [{ key: "close", value, matched: true }, { key: "date", value: "2020" }, { key: "exchange_code", value: "005930" }] };
  renderAnswer(answer({ content: `[${value}](#evidence-ev_quote) and [${value}](#evidence-ev_quote).`, evidence_refs: [source],
    number_checks: [{ ...base, context: { text: line, start, end: start + value.length } }, legacy] }));
  const user = userEvent.setup();
  const [newMark, oldMark] = await screen.findAllByTestId("number-chip");
  await user.click(newMark!);
  const observation = await screen.findByRole("dialog");
  expect(observation).toHaveTextContent(line);
  expect(observation.querySelector("mark")).toHaveTextContent(value);
  await user.keyboard("{Escape}");
  await user.click(oldMark!);
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText("2020")).toBeVisible();
  expect(within(dialog).getByText("005930")).toBeVisible();
});

it.each([
  ["en", "123,975,371", "2024"], ["en", "126,261,000 people", "2020"], ["ko", "126,261,000명", "2020"],
])("uses the saved Source observation for %s figure %s", async (language, text, period) => {
  await act(() => i18n.changeLanguage(language));
  try {
    const value = text.match(/^[\d,]+/u)![0];
    const lead = `Population, total · Japan · ${period} · `;
    const start = new TextEncoder().encode(lead).length;
    const source = evidenceFixture({ id: "ev_retained", title: "World Bank population", kind: "saved_context", tool_name: "world_read", read_ref: "rdg_retained", url: undefined,
      excerpt: '{"dataset_id":"private-row-id","observations":[{"date":"2024","value":"123975371"}]}' });
    const check: NumberCheck = { text, evidence_id: source.id, kind: "passage", status: "matched", link_index: 0,
      context: { text: lead + value, start, end: start + value.length } };
    const { open } = renderAnswer(answer({ content: `[${text}](#evidence-${source.id})`, evidence_refs: [source], number_checks: [check],
      memory_refs: [{ kind: "source", read_ref: "rdg_retained", title: source.title, excerpt: source.excerpt, as_of: null, section: null, access: "exact_read",
        knowledge: { kind: "source", recordId: "population-record", revisionId: "population-revision", objectId: "population-source", spanId: "population-span" } }],
    }));
    await userEvent.click(await screen.findByTestId("number-chip"));
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent(lead + value);
    expect(dialog.querySelector("mark")).toHaveTextContent(value);
    expect(dialog).not.toHaveTextContent(/dataset_id|private-row-id/);
    await userEvent.click(within(dialog).getByRole("button", { name: i18n.t("chat:numberChip.openEvidence") }));
    expect(destinations(open).at(-1)).toMatchObject({ kind: "memory", location: { recordId: "population-record", revisionId: "population-revision", selection: { spanId: "population-span" } },
      figure: { messageId: "answer", evidenceId: source.id, linkIndex: 0 } });
  } finally {
    await act(() => i18n.changeLanguage("en"));
  }
});

it("keeps copy feedback across snapshots of the same number check", async () => {
  const user = userEvent.setup();
  const check = checks()[0]!;
  const content = (value: NumberCheck) => <NumberChip check={value} evidence={calc} evidenceRefs={[dart, guide, calc]}>8.4%</NumberChip>;
  const view = renderWithOpen(content(check));
  await user.click(await chip("8.4 percent"));
  await user.click(screen.getByRole("button", { name: "Copy formula" }));
  expect(screen.getByRole("button", { name: "Copied" })).toBeVisible();
  view.rerender(content({ ...check }));
  expect(screen.getByRole("button", { name: "Copied" })).toBeVisible();
  view.rerender(content({ ...check, link_index: 9 }));
  expect(screen.getByRole("button", { name: "Copy formula" })).toBeVisible();
});

it("keeps a single preview open across numbers and citations", async () => {
  renderAnswer(answer());
  const margin = await chip("8.4 percent");
  const citation = await screen.findByRole("button", { name: "1 · Source 2: February guidance call" });
  fireEvent.click(margin);
  expect(screen.getByRole("dialog")).toHaveTextContent("Operating income");
  fireEvent.pointerEnter(citation);
  expect(await screen.findByRole("tooltip")).toHaveTextContent("February guidance call");
  expect(screen.queryByRole("dialog")).toBeNull();
  fireEvent.click(margin);
  expect(screen.getByRole("dialog")).toBeVisible();
  expect(screen.queryByRole("tooltip")).toBeNull();
});

it("measures composer clearance only while a number preview is open", async () => {
  const content = (label: string) => <div className="chat-workspace"><div className="composer-shell" /><NumberChip check={checks()[0]!} evidence={calc} evidenceRefs={[dart, guide, calc]}>{label}</NumberChip></div>;
  const view = renderWithOpen(content("8.4%"));
  const measure = vi.spyOn(view.container.querySelector(".composer-shell")!, "getBoundingClientRect");
  // Re-rendering a closed chip sees the composer but must not measure it.
  view.rerender(content("Updated label"));
  expect(measure).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: /8.4 percent/ }));
  expect(measure).toHaveBeenCalled();
});

it("renders a numeric link as a number chip and a non-numeric one as a numbered citation", async () => {
  renderAnswer(answer());
  const margin = await chip("8.4 percent");
  expect(margin).toHaveAttribute("aria-haspopup", "dialog");
  expect(margin).toHaveAccessibleName("8.4 percent, calculated. Show the math");
  expect(await chip("₩452.3bn")).toHaveAccessibleName("₩452.3bn, from OpenDART");
  const citation = await screen.findByRole("button", { name: "1 · Source 2: February guidance call" });
  expect(citation).toHaveClass("loupist-citation");
  expect(citation).toHaveTextContent("1");
});

it("names a price taken from a dataset fetched in the same reply by its tool, not Memory", async () => {
  // The follow reply of Loupist 0.1.5: the price links to a dataset_query of the history it read.
  const history = evidenceFixture({ id: "ev_history", title: "AAPL", tool_name: "equity_price_historical", provider: "yfinance", url: undefined, published_at: undefined,
    excerpt: JSON.stringify({ results: [{ date: "2026-11-20", close: 342.55999755859375 }] }) });
  const slice = (id: string, provider?: string) => evidenceFixture({ id, title: "Dataset slice", kind: "saved_context", tool_name: "dataset_query", provider, url: undefined, published_at: undefined,
    upstream_evidence_ids: provider ? [history.id] : undefined, excerpt: JSON.stringify({ rows: [{ close: 342.55999755859375, date: "2026-11-20" }] }) });
  const fetched = slice("ev_slice_fetched", "yfinance");
  const kept = slice("ev_slice_kept");
  const check = (evidence: ChatEvidenceRef, link_index: number, text: string): NumberCheck => ({ link_index, text, evidence_id: evidence.id, kind: "step", status: "matched", pointer: "/rows/0/close",
    context: { text: "close · 2026-11-20 · 342.55999755859375", start: 23, end: 41 } });
  renderAnswer(answer({
    content: `The close was [$342.56](#evidence-${fetched.id}); a saved dataset says [$342.55](#evidence-${kept.id}).`,
    evidence_refs: [history, fetched, kept],
    number_checks: [check(fetched, 0, "$342.56"), check(kept, 1, "$342.55")],
  }));
  expect(await chip("$342.56")).toHaveAccessibleName("$342.56, from Market data platform");
  // A dataset that no read of this reply fetched still names Memory.
  expect(await chip("$342.55")).toHaveAccessibleName("$342.55, from Memory");
});

it("names pasted material as user-provided where a number was checked against it", async () => {
  const text = "솔숲식품은 2025년 3월 12일 현재 수주잔고 수치가 184.6이고 단위는 십억 원이라고 알렸다.";
  const start = new TextEncoder().encode(text.slice(0, text.indexOf("184.6"))).length;
  const pasted = evidenceFixture({ id: "ev_pasted", title: "pasted capture", kind: "user_provided", tool_name: "pasted_capture", url: undefined, provider: undefined, published_at: undefined, excerpt: text });
  renderAnswer(answer({
    content: "Solsup reported an order backlog of [₩184.6 billion](#evidence-ev_pasted) [1](#evidence-ev_pasted).",
    evidence_refs: [pasted],
    number_checks: [{ link_index: 0, text: "₩184.6 billion", evidence_id: pasted.id, kind: "passage", status: "matched", span: { start, end: start + 5 } }],
  }));
  const figure = await chip("₩184.6 billion");
  expect(figure).toHaveAccessibleName("₩184.6 billion, from User-provided");
  await userEvent.setup().click(figure);
  const popover = await screen.findByRole("dialog");
  expect(within(popover).getByText("Pasted text")).toBeVisible();
  expect(popover).toHaveTextContent("User-provided");
});

it("preserves bare integer data labels instead of substituting source ordinals", async () => {
  const source = evidenceFixture({ id: "ev_csv", title: "Synthetic data", kind: "local_file", tool_name: "workspace_capture", url: undefined });
  renderAnswer(answer({ content: "Last year: Aster [100](#evidence-ev_csv), Boreal [79](#evidence-ev_csv).", evidence_refs: [source], number_checks: [] }));
  expect(await screen.findByRole("button", { name: /^100 ·/ })).toHaveTextContent("100");
  expect(await screen.findByRole("button", { name: /^79 ·/ })).toHaveTextContent("79");
});

it("keeps a bold attached-note token linked through the workspace-aware answer renderer", async () => {
  const note = evidenceFixture({ title: "northwind-loom.md", kind: "local_file", tool_name: "workspace_capture", url: undefined, provider: undefined,
    excerpt: "On 12 March 2026 the internal note recorded a loom-yield token of 18420." });
  const { view } = renderAnswer(answer({
    content: `Northwind Loom recorded loom-yield token **[18420](#evidence-${note.id})** on 12 March 2026. The note identifies it as a fixture identifier, not a market value.`,
    evidence_refs: [note],
    number_checks: [{ link_index: 0, text: "18420", evidence_id: note.id, kind: "step", status: "matched", pointer: "/content" }],
  }), { workspacePath: "/fixture/workspace" });
  const token = await chip("18420");
  expect(token).toBeVisible();
  expect(token).toHaveAttribute("data-status", "matched");
  expect(view.container.querySelector(".loupist-unavailable-citation")).toBeNull();
  expect(view.container.querySelectorAll("button.loupist-citation")).toHaveLength(0);
  expect(view.container.querySelectorAll(".message-content button.loupist-citation, .message-content button.number-chip")).toHaveLength(1);
});

it("pins the calculated popover with its formula and inputs, and Esc returns focus to the number", async () => {
  const user = userEvent.setup();
  renderAnswer(answer());
  const margin = await chip("8.4 percent");
  await user.click(margin);
  // The operation in plain words, named by the profit the formula divides.
  const dialog = await screen.findByRole("dialog", { name: "Operating margin" });
  expect(dialog).toHaveTextContent("Operating income / Revenue");
  expect(dialog).toHaveTextContent("= 38.1 / 452.3 = 8.42%");
  expect(dialog).toHaveTextContent("Inputs");
  // Inputs read like the answer: the source's amount for a scaled input, plain names.
  const inputs = [...dialog.querySelectorAll(".number-chip-inputs > div")].map((row) => [row.querySelector("dt")?.textContent, row.querySelector("dd")?.textContent]);
  expect(inputs).toEqual([["Operating income", "₩38.1bn"], ["Revenue", "₩452.3bn"]]);
  expect(dialog).toHaveTextContent("Not linked to a source");
  expect(dialog).toHaveTextContent("Calculated on this Mac");
  expect(margin).toHaveAttribute("aria-expanded", "true");
  await waitFor(() => expect(within(dialog).getByRole("button", { name: "Show step" })).toHaveFocus());
  await user.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(margin).toHaveFocus();
});

it("opens on hover as a non-modal dialog, never a tooltip, and closes when the pointer leaves", async () => {
  renderAnswer(answer());
  const margin = await chip("8.4 percent");
  fireEvent.pointerEnter(margin);
  const dialog = await screen.findByRole("dialog", { name: "Operating margin" });
  expect(dialog).toHaveAttribute("aria-modal", "false");
  expect(screen.queryByRole("tooltip")).toBeNull();
  expect(margin).toHaveAttribute("aria-expanded", "true");
  expect(margin).toHaveAttribute("aria-controls", dialog.id);
  fireEvent.pointerLeave(margin);
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(margin).toHaveAttribute("aria-expanded", "false");
});

it("keeps the popover's buttons in reading order after the number", async () => {
  const user = userEvent.setup();
  renderAnswer(answer());
  const guided = await chip("9.1 percent");
  guided.focus();
  const dialog = await screen.findByRole("dialog", { name: "February guidance call" });
  const open = within(dialog).getByRole("button", { name: "Open in Evidence" });
  // Tab from the number enters the popover; Shift+Tab from its first button returns.
  await user.tab();
  expect(open).toHaveFocus();
  await user.tab({ shift: true });
  expect(guided).toHaveFocus();
  expect(screen.getByRole("dialog")).toBeVisible();
  await user.tab();
  expect(open).toHaveFocus();
  await user.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(guided).toHaveFocus();
});

it("skips buttons and links outside the tab order when leaving the popover", async () => {
  const user = userEvent.setup();
  renderWithOpen(
    <article>
      <NumberChip check={checks()[2]!} evidence={guide} evidenceRefs={[guide]}>9.1%</NumberChip>
      <button type="button" tabIndex={-1}>Skipped button</button>
      <a href="#next" tabIndex={-1}>Skipped link</a>
      <button id="next" type="button">Next control</button>
    </article>
  );
  await user.tab();
  expect(await chip("9.1 percent")).toHaveFocus();
  const dialog = await screen.findByRole("dialog", { name: "February guidance call" });
  await user.tab();
  expect(within(dialog).getByRole("button", { name: "Open in Evidence" })).toHaveFocus();
  await user.tab();
  expect(screen.getByRole("button", { name: "Next control" })).toHaveFocus();
  expect(dialog).not.toBeInTheDocument();
});

it("shows the passage and data popovers and opens Evidence at the matched cell", async () => {
  const user = userEvent.setup();
  const { onOpenEvidence } = renderAnswer(answer());
  await user.click(await chip("9.1 percent"));
  const passage = await screen.findByRole("dialog", { name: "February guidance call" });
  expect(within(passage).getByText("9.1%", { selector: "mark" })).toBeVisible();
  expect(passage).toHaveTextContent("solsup.example · published");
  await user.keyboard("{Escape}");
  await user.click(await chip("₩452.3bn"));
  const data = await screen.findByRole("dialog", { name: "Solsup Foods FY2025 statements" });
  // Plain field names and the amount with grouping and currency; the title already names the company.
  expect(within(data).getByText("₩452,312,000,000", { selector: "mark" })).toBeVisible();
  expect(within(data).getByRole("rowheader", { name: "Item" })).toBeVisible();
  expect(within(data).getByRole("rowheader", { name: "Amount" })).toBeVisible();
  expect(within(data).queryByRole("rowheader", { name: /corp/i })).toBeNull();
  expect(data).toHaveTextContent("Revenue");
  expect(data).toHaveTextContent("Rounded from ₩452,312,000,000");
  await user.click(within(data).getByRole("button", { name: "Open in Evidence" }));
  expect(destinations(onOpenEvidence)).toContainEqual(expect.objectContaining({ kind: "evidence", evidence: dart, step: expect.objectContaining({ cell: { pointer: "/facts/0/value", key: "value", value: "452312000000" } }) }));
});

describe("a passage the full result matched outside the excerpt", () => {
  // The bounded excerpt lacks the number; the host sent the matched part of the retained content.
  const fallback = evidenceFixture({ id: "ev_fallback", title: "Annual report", tool_name: "web_fetch", url: "https://hanul.example/report", published_at: "2026-02-12",
    excerpt: "The annual report opens with the chair's letter." });
  const passage = (check: Partial<NumberCheck>): NumberCheck =>
    ({ link_index: 0, text: "4,523", evidence_id: fallback.id, kind: "passage", status: "matched", ...check });
  const open = async (check: NumberCheck, text = check.text) => {
    renderWithOpen(<NumberChip check={check} evidence={fallback} evidenceRefs={[fallback]}>{text}</NumberChip>);
    fireEvent.click(screen.getByRole("button", { name: /from hanul\.example/u }));
    return screen.findByRole("dialog", { name: "Annual report" });
  };
  const body = (dialog: HTMLElement) => dialog.querySelector<HTMLElement>(".number-chip-passage")!;

  it.each([
    ["English", "4,523", { text: "The company sold 4,523 cars; later it sold 4,523 more.", start: 17, end: 22 }, "The company sold ", " cars; later it sold 4,523 more."],
    ["Korean, multi-byte offsets", "1조 2,345억원", { text: "매출은 1조 2,345억원이며 증가했다.", start: 10, end: 26 }, "매출은 ", "이며 증가했다."],
  ])("marks the matched number in the host's context (%s)", async (_name, text, context, before, after) => {
    const dialog = await open(passage({ text, context }));
    const marks = body(dialog).querySelectorAll("mark");
    expect(marks).toHaveLength(1);
    expect(marks[0]).toHaveTextContent(text);
    expect(body(dialog).textContent).toBe(`${before}${text}${after}`);
    // The same meta line as a passage span: source and published date.
    expect(dialog).toHaveTextContent("hanul.example · published");
  });

  it("lets a span into the excerpt win over the context", async () => {
    const dialog = await open(passage({ text: "chair", span: { start: 33, end: 38 }, context: { text: "The company sold 4,523 cars.", start: 17, end: 22 } }));
    expect(within(dialog).getByText("chair", { selector: "mark" })).toBeVisible();
    expect(body(dialog)).not.toHaveTextContent("4,523");
  });

  it.each([
    ["past the end", { text: "sold 4,523 cars", start: 5, end: 99 }],
    ["inside a character", { text: "매출은 1조 2,345억원", start: 12, end: 26 }],
    ["reversed", { text: "sold 4,523 cars", start: 10, end: 5 }],
  ])("shows the excerpt unmarked when the context's offsets fall %s", async (_name, context) => {
    const dialog = await open(passage({ context }));
    expect(body(dialog).querySelector("mark")).toBeNull();
    expect(body(dialog).textContent).toBe(fallback.excerpt);
  });

  it("shows the excerpt unmarked for a check with neither span nor context, as before", async () => {
    const dialog = await open(passage({}));
    expect(body(dialog).querySelector("mark")).toBeNull();
    expect(body(dialog).textContent).toBe(fallback.excerpt);
  });
});

it("opens a number's source page in the system browser on Ctrl-click or a middle click, without pinning the popover", async () => {
  const { open } = renderAnswer(answer());
  const number = await chip("9.1 percent");
  fireEvent.click(number, { ctrlKey: true });
  expect(open).toHaveBeenLastCalledWith(expect.objectContaining({ kind: "evidence", evidence: guide }), expect.objectContaining({ external: true }));
  expect(number).toHaveAttribute("aria-expanded", "false");
  fireEvent(number, new MouseEvent("auxclick", { bubbles: true, button: 1 }));
  expect(open).toHaveBeenCalledTimes(2);
  expect(open).toHaveBeenLastCalledWith(expect.objectContaining({ kind: "evidence", evidence: guide }), expect.objectContaining({ external: true }));
});

it("shows a quote cell in the popover and does not open the raw reader", async () => {
  const user = userEvent.setup();
  const excerpt = JSON.stringify({
    structuredContent: { results: [
      { symbol: "AAPL", name: "Apple", currency: "USD", last_price: 271.4 },
      { symbol: "MSFT", name: "Microsoft", currency: "USD", last_price: 271.4 },
    ] },
  });
  const quote = evidenceFixture({
    id: "ev_quote", title: "Equity price quote", tool_name: "equity_price_quote", provider: "Yahoo Finance", url: undefined,
    retrieved_at: "2026-10-11T15:04:00Z", published_at: undefined, excerpt,
  });
  const { onOpenEvidence } = renderAnswer(answer({
    content: `Apple last price [271.4](#evidence-${quote.id}).`,
    evidence_refs: [quote],
    number_checks: [{ link_index: 0, text: "271.4", evidence_id: quote.id, kind: "data", status: "matched", pointer: "/structuredContent/results/0/last_price",
      fields: [{ key: "symbol", value: "AAPL" }, { key: "last_price", value: "271.4", matched: true }] }],
  }));
  await user.click(await chip("271.4"));
  const dialog = await screen.findByRole("dialog");
  expect(dialog).toHaveTextContent("AAPL");
  expect(dialog).toHaveTextContent("271.4 USD");
  expect(dialog).toHaveTextContent("Retrieved");
  expect(dialog).toHaveTextContent("Quote time was not in the saved result.");
  expect(dialog).not.toHaveTextContent("MSFT");
  expect(within(dialog).queryByRole("button", { name: "Open in Evidence" })).toBeNull();
  expect(onOpenEvidence).not.toHaveBeenCalled();
});

/** A bundled quote read of NVDA, cited by an answer. */
const quoteRead = evidenceFixture({ id: "ev_nvda", title: "NVDA · Quote", tool_name: "mcp__openbb__equity_price_quote", provider: "yfinance", url: undefined, published_at: undefined,
  retrieved_at: "2026-11-17T15:00:00Z", excerpt: "NVDA last 156.14" });

it("names a market-data figure by its field and its provider, never a runtime id", async () => {
  const user = userEvent.setup();
  const check: NumberCheck = { link_index: 0, text: "$160.00", evidence_id: quoteRead.id, kind: "data", status: "matched", fields: [{ key: "last", value: "160", matched: true }] };
  renderAnswer(answer({ content: `NVIDIA trades at [$160.00](#evidence-${quoteRead.id}).`, evidence_refs: [quoteRead], number_checks: [check], progress: undefined }));
  await user.click(await chip("$160.00"));
  const dialog = await screen.findByRole("dialog", { name: "NVDA · Quote" });
  expect(within(dialog).getByRole("rowheader", { name: "Last price" })).toBeVisible();
  expect(dialog).toHaveTextContent(/Yahoo Finance · fetched \w{3}\s\d{1,2}/u);
  expect(dialog).not.toHaveTextContent("yfinance");
});

it("shows a host-checked value when the quote excerpt does not parse, without a guessed ticker", async () => {
  const user = userEvent.setup();
  const quote = evidenceFixture({
    id: "ev_cut", title: "Equity price quote", tool_name: "equity_price_quote", provider: "Yahoo Finance", url: undefined,
    retrieved_at: "2026-10-11T15:04:00Z", published_at: undefined, excerpt: "{\"content\":[{\"type\":\"text\",\"text\":\"{\\\"results\\\":[{\"symbol\":\"AAPL\"", excerpt_truncated: true,
  });
  const { onOpenEvidence } = renderAnswer(answer({
    content: `Last price [271.4](#evidence-${quote.id}).`,
    evidence_refs: [quote],
    number_checks: [{ link_index: 0, text: "271.4", evidence_id: quote.id, kind: "data", status: "matched", pointer: "/content/0/text",
      exact: "271.4", fields: [{ key: "last_price", value: "271.4", matched: true }] }],
  }));
  await user.click(await chip("271.4"));
  const dialog = await screen.findByRole("dialog");
  expect(dialog).toHaveTextContent("Checked value");
  expect(dialog).toHaveTextContent("271.4");
  expect(dialog).toHaveTextContent("No complete quote was in the saved result.");
  expect(within(dialog).queryByRole("term", { name: "Ticker" })).toBeNull();
  expect(onOpenEvidence).not.toHaveBeenCalled();
});

it("keeps the thread under a number whose popover the reader pinned, and not under one opened by hover", async () => {
  const user = userEvent.setup();
  renderAnswer(answer());
  scroll.holdScrollPosition.mockClear();
  const margin = await chip("8.4 percent");
  await user.hover(margin);
  expect(await screen.findByRole("dialog")).toBeVisible();
  expect(scroll.holdScrollPosition).not.toHaveBeenCalled();
  await user.click(margin);
  expect(scroll.holdScrollPosition).toHaveBeenCalledTimes(1);
});

it("Show step reveals the calculation inline and never opens Evidence", async () => {
  const user = userEvent.setup();
  const { onOpenEvidence } = renderAnswer(answer());
  await user.click(await chip("8.4 percent"));
  await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Show step" }));
  expect(onOpenEvidence).not.toHaveBeenCalled();
  await waitFor(() => expect(document.querySelector('.chat-progress-row[data-flash="true"]')).not.toBeNull());
});

it("marks a calculation that differs, says so in the popover and counts it in the receipt", async () => {
  const user = userEvent.setup();
  const differs = checks("differs");
  renderAnswer(answer({ number_checks: differs, memory_update: { status: "no_change", commitId: null, changes: [] } }));
  const margin = await chip("8.5 percent");
  expect(margin).toHaveAttribute("data-status", "differs");
  expect(margin).toHaveAccessibleDescription("The calculation gave 8.42%. The answer says 8.5%.");
  const segment = screen.getByRole("button", { name: "1 number to check" });
  await user.click(segment);
  const list = await screen.findByRole("dialog");
  expect(list).toHaveTextContent("Loupist compared each cited figure with its source. These could not be confirmed.");
  expect(list).toHaveTextContent("8.5%");
  expect(list).toHaveTextContent("Different numbers");
  await user.click(within(list).getByRole("button", { name: "Open the step" }));
  expect(scroll.holdScrollPosition).toHaveBeenCalled();
  // Matched and rounded numbers never reach the receipt.
  expect(chatReceipt(answer()).numbersToCheck).toBe(0);
  expect(chatReceipt(answer({ number_checks: differs })).numbersToCheck).toBe(1);
});

it("keeps a legacy figure flagged and shows it in the receipt list", async () => {
  const user = userEvent.setup();
  const unmatched = checks().map((item) => item.link_index === 2 ? { ...item, status: "not_found" as const, span: undefined } : item);
  renderAnswer(answer({ number_checks: unmatched, memory_update: { status: "no_change", commitId: null, changes: [] } }));
  const guided = await chip("9.1 percent");
  expect(guided).toHaveAttribute("data-status", "not_found");
  expect(guided).toHaveAccessibleName(/^9\.1 percent, not found in /);
  expect(guided).toHaveAccessibleDescription("This number isn't in the cited source as written. Check the source.");
  expect(guided.querySelector(".number-chip-unmatched")).not.toBeNull();
  // Its text stays the figure as written.
  expect(guided).toHaveTextContent(/^9\.1%$/);
  // A matched number carries no such mark.
  expect((await chip("₩452.3bn")).querySelector(".number-chip-unmatched")).toBeNull();
  await user.click(screen.getByRole("button", { name: "1 number to check" }));
  const list = await screen.findByRole("dialog");
  expect(list).toHaveTextContent("9.1%");
  expect(list).toHaveTextContent("Unconfirmed");
  expect(within(list).getByRole("button", { name: "Open the source" })).toBeVisible();
  expect(guided).toHaveAttribute("aria-expanded", "false");
  await user.keyboard("{Escape}");
  await user.click(guided);
  expect(screen.getByRole("dialog")).toHaveTextContent("This number isn't in the cited source as written.");
});

it("marks a number whose link named no read of its reply, lists it to check and never draws it as a citation", async () => {
  const user = userEvent.setup();
  const written = "ev_a736684a94e3c46462331372b53d709d";
  const message = answer({
    content: `Total revenues were [81,462백만 달러](#evidence-${written}) in 2022.`,
    evidence_refs: [guide],
    number_checks: [{ link_index: 0, text: "81,462백만 달러", evidence_id: written, kind: "passage", status: "unlinked" }],
    memory_update: { status: "no_change", commitId: null, changes: [] },
  });
  const { view } = renderAnswer(message);
  const figure = await waitFor(() => { const element = view.container.querySelector<HTMLElement>('.number-chip[data-status="unlinked"]'); expect(element).not.toBeNull(); return element!; });
  expect(figure).toHaveTextContent("81,462백만 달러 No source is linked to this number in this reply.");
  expect(figure).toHaveAttribute("title", "No source is linked to this number in this reply.");
  expect(figure.querySelector(".number-chip-unmatched")).not.toBeNull();
  // Not a citation, not a number chip that opens a source, not the plain unavailable text.
  expect(view.container.querySelector("button.loupist-citation, button.number-chip, .loupist-unavailable-citation")).toBeNull();
  expect(chatReceipt(message).numbersToCheck).toBe(1);
  await user.click(screen.getByRole("button", { name: "1 number to check" }));
  const list = await screen.findByRole("dialog");
  expect(list).toHaveTextContent("81,462백만 달러");
  expect(list).toHaveTextContent("No source is linked to this number in this reply.");
  expect(within(list).queryByRole("button", { name: "Open the source" })).toBeNull();
  const markdown = chatMarkdown({ id: "chat", loupist_id: "l", title: "Revenue", updated_at: "", use_memory: true, update_memory: true, messages: [message] }, "Quality", Date.parse("2026-03-20"));
  expect(markdown).toContain("Total revenues were 81,462백만 달러[^1] in 2022.");
  expect(markdown).toContain("[^1]: No source is linked to this number in this reply.");
});

it("keeps number links pending while streaming and plain numbered citations without a check", async () => {
  renderAnswer(answer({ status: "streaming", number_checks: undefined }));
  await waitFor(() => expect(document.querySelectorAll('.number-chip[data-status="pending"]').length).toBe(3));
  expect(document.querySelector("button.number-chip")).toBeNull();
});

it("exports a number with a footnote for its formula or source", () => {
  const markdown = chatMarkdown({ id: "chat", loupist_id: "l", title: "Margin", updated_at: "", use_memory: true, update_memory: true, messages: [answer()] }, "Quality", Date.parse("2026-03-20"));
  expect(markdown).toContain("The margin was 8.4%[^1] on revenue of ₩452.3bn[^2]");
  expect(markdown).toContain("[^1]: Margin = 38.1 / 452.3 = 8.42% (calculated in Loupist; inputs: OpenDART, 2026-03-18)");
  expect(markdown).toContain("[^2]: Solsup Foods FY2025 statements — 2026-03-18");
});

it("lets the popover cover the composer in a short window so its inputs and sources stay visible", () => {
  const height = window.innerHeight;
  try {
    window.innerHeight = 560;
    expect(popoverCollisionPadding(null)).toEqual({ top: 56, right: 8, bottom: 8, left: 8 });
    window.innerHeight = 820;
    expect(popoverCollisionPadding(null)).toMatchObject({ top: 112 });
  } finally { window.innerHeight = height; }
});

describe("a retained currency conversion", () => {
  // The amount's source is a page that prints the figure without a currency; the request declared USD.
  const filing = evidenceFixture({ id: "ev_morrow_q3", title: "Morrow Components Q3 2026 results", tool_name: "web_fetch", provider: undefined, url: "https://morrow.example/ir/q3-2026",
    published_at: "2026-11-05", retrieved_at: "2026-11-17T00:40:00Z", as_of: undefined, excerpt: "Third-quarter revenue was 412.6 million, up 9% from a year earlier." });
  // The `fx_rate` read: published by the ECB on the 16th, read on the 17th.
  const fx = evidenceFixture({ id: "ev_fx_usd_krw", title: "USD/KRW · European Central Bank reference rates", tool_name: "fx_rate", provider: "ecb",
    url: "https://www.ecb.europa.eu/stats/policy_and_exchange_rates/euro_reference_exchange_rates/html/index.en.html", published_at: "2026-11-16", retrieved_at: "2026-11-17T00:41:00Z", as_of: undefined,
    excerpt: JSON.stringify({ from: "USD", to: "KRW", to_per_from: 1334.87, date: "2026-11-16", source: { id: "ecb", name: "European Central Bank euro foreign exchange reference rates" },
      legs: { USD_per_EUR: 1.1014, KRW_per_EUR: 1470.23 }, revision: "ecb:2026-11-16", computed: "cross rate through EUR" }, null, 2) });
  const conversion = evidenceFixture({ id: "ev_convert", title: "Currency conversion", kind: "derived", tool_name: "calculate", calc_operation: "convert", url: undefined, provider: undefined,
    published_at: undefined, as_of: undefined, excerpt: "", upstream_evidence_ids: [filing.id, fx.id] });
  type Input = NonNullable<NumberCheck["calc"]>["inputs"][number];
  /** The inputs Rust ships for `unit: "USD millions"`, `to: "KRW"`, both cited. */
  const amount: Input = { name: "amount", label: "Amount", unit: { kind: "amount", currency: "USD", scale: 6 }, display: "412.6", evidence_id: filing.id, status: "matched", currency_basis: "declared" };
  const rate: Input = { name: "rate", label: "Rate", unit: { kind: "number", text: "KRW per USD" }, display: "1,334.87", evidence_id: fx.id, status: "matched" };
  const cited = "Amount × Rate = 412.6 USD millions × 1,334.87 KRW per USD (ECB, 2026-11-16) = 550,767.36 KRW millions";
  const converted = (inputs: Input[], formula = cited): NumberCheck => ({ link_index: 1, text: "₩550.8bn", evidence_id: conversion.id, kind: "calculated", status: "rounded", exact: "550,767,362,000", pointer: "/values/0/value",
    calc: { operation: "convert", title: "Currency conversion", formulas: [formula], inputs } });
  const convertedAnswer = (check: NumberCheck, rateCited = true, rateSource: ChatEvidenceRef = fx): ChatMessage => ({
    id: "converted", role: "assistant", status: "complete", created_at: "2026-11-17T00:42:00Z",
    content: `Morrow Components reported third-quarter revenue of $412.6 million [1](#evidence-${filing.id}). At ${rateCited ? "the European Central Bank’s November 16 rate" : "1,334.87 won per dollar"}, that is about [₩550.8bn](#evidence-${conversion.id})${rateCited ? ` [2](#evidence-${fx.id})` : ""}.`,
    evidence_refs: rateCited ? [filing, rateSource, conversion] : [filing, conversion], number_checks: [check],
  });
  const day = (iso: string) => formatDate(Date.parse(iso), { timeZone: "UTC" });
  /** The host's formula as the popover prints it, its lines joined again. */
  const formula = (dialog: HTMLElement) => [...dialog.querySelectorAll(".number-chip-formula p")].map((line) => line.textContent).join(" ");
  const open = async () => {
    const user = userEvent.setup();
    const number = (await screen.findAllByRole("button")).find((button) => button.classList.contains("number-chip") && button.dataset.kind === "calculated")!;
    await user.click(number);
    const dialog = await screen.findByRole("dialog");
    const row = (label: string) => [...dialog.querySelectorAll<HTMLElement>(".number-chip-inputs > div")].find((item) => item.querySelector("dt")?.textContent === label)!;
    return { dialog, row };
  };

  it.each([["en", "Currency conversion", "Currency taken from the request, not the source"], ["ko", "통화 환산", "통화는 출처가 아닌 요청에 따름"]])("shows the formula, both inputs and their own sources in %s", async (language, title, note) => {
    await act(async () => { await i18n.changeLanguage(language); });
    onTestFinished(async () => { await act(async () => { await i18n.changeLanguage("en"); }); });
    renderAnswer(convertedAnswer(converted([amount, rate])));
    const { dialog, row } = await open();
    expect(dialog).toHaveAccessibleName(title);
    expect(formula(dialog)).toBe(cited);
    // One note, on the amount: the request named its currency because the source does not.
    expect(within(dialog).getAllByText(note)).toHaveLength(1);
    expect(within(row("Amount")).getByText(note)).toBeVisible();
    expect(row("Rate")).not.toHaveTextContent(note);
    // The rate is a number in target units per source unit, from the ECB on the day it published.
    expect(row("Rate").querySelector("dd")).toHaveTextContent("1,334.87 KRW per USD");
    expect(row("Rate").querySelector(".number-chip-source")).toHaveTextContent(`European Central Bank · ${day("2026-11-16")}`);
    expect(row("Rate")).not.toHaveTextContent(day("2026-11-17"));
    // The amount keeps its own source and date.
    expect(row("Amount").querySelector(".number-chip-source")).toHaveTextContent(`morrow.example · ${day("2026-11-05")}`);
    expect(row("Amount")).not.toHaveTextContent(/European Central Bank/u);
  });

  it("shows no currency note when the host recorded none, and keeps the formula", async () => {
    const { currency_basis: _, ...stated } = amount;
    renderAnswer(convertedAnswer(converted([stated, rate])));
    const { dialog, row } = await open();
    expect(formula(dialog)).toBe(cited);
    expect(dialog.querySelector(".number-chip-currency-note")).toBeNull();
    expect(dialog).not.toHaveTextContent("Currency taken from the request, not the source");
    expect(row("Rate").querySelector(".number-chip-source")).toHaveTextContent("European Central Bank");
  });

  it("leaves an assumed rate provided and unlinked, with no ECB source or date", async () => {
    const assumed = "Amount × Rate = 412.6 USD millions × 1,334.87 KRW per USD (assumed rate) = 550,767.36 KRW millions";
    renderAnswer(convertedAnswer(converted([amount, { name: "rate", label: "Rate", unit: rate.unit, display: "1,334.87" }], assumed), false));
    const { dialog, row } = await open();
    expect(formula(dialog)).toBe(assumed);
    expect(row("Rate")).toHaveTextContent("Not linked to a source");
    expect(dialog).not.toHaveTextContent(/European Central Bank|ECB/u);
    expect(dialog).not.toHaveTextContent(day("2026-11-16"));
    // The amount was still cited from a source without a currency.
    expect(within(row("Amount")).getByText("Currency taken from the request, not the source")).toBeVisible();
  });

  it("dates the rate only by its publication: a reference without one shows its provider and citation, never the day it was read", async () => {
    renderAnswer(convertedAnswer(converted([amount, rate]), true, { ...fx, published_at: undefined }));
    const { row } = await open();
    const source = row("Rate").querySelector(".number-chip-source")!;
    expect(source).toHaveTextContent("European Central Bank");
    expect(source.querySelector(".loupist-citation")).toHaveTextContent("2");
    expect(source).not.toHaveTextContent(day("2026-11-17"));
    expect(source.querySelector(".number-chip-date")).toBeNull();
  });

  it.each(["en", "ko"])("prints the retained formula as written when an input has a custom label, in %s", async language => {
    await act(async () => { await i18n.changeLanguage(language); });
    onTestFinished(async () => { await act(async () => { await i18n.changeLanguage("en"); }); });
    // "Net sales" is a label the app knows as revenue; the conversion's formula still keeps it.
    const labelled = "Net sales × Rate = 412.6 USD millions × 1,334.87 KRW per USD (ECB, 2026-11-16) = 550,767.36 KRW millions";
    renderAnswer(convertedAnswer(converted([{ ...amount, label: "Net sales" }, rate], labelled)));
    const { dialog } = await open();
    expect(formula(dialog)).toBe(labelled);
  });

  it("leaves out an input whose value the record lacks and keeps the formula and the other input", async () => {
    // A blank rate display, as the host's projection defaults a missing one.
    const first = renderAnswer(convertedAnswer(converted([amount, { ...rate, display: "" }])));
    let opened = await open();
    expect(formula(opened.dialog)).toBe(cited);
    expect(opened.row("Rate")).toBeUndefined();
    expect(opened.dialog).not.toHaveTextContent("0 KRW per USD");
    expect(opened.row("Amount")).toHaveTextContent("$412.6m");
    first.view.unmount();
    // An amount record with no display at all.
    const { display: _, ...bare } = amount;
    renderAnswer(convertedAnswer(converted([bare as Input, rate])));
    opened = await open();
    expect(formula(opened.dialog)).toBe(cited);
    expect(opened.row("Amount")).toBeUndefined();
    expect(opened.row("Rate")).toHaveTextContent("1,334.87 KRW per USD");
  });
});

const diagnosticExamples: [NumberCheck, string][] = [
  [{
    evidence_id: "ev", link_index: 0, kind: "passage", status: "not_found",
    text: "575.38억 달러", comparedValue: "$ 57,583", nearest: "$ 57,583",
    nearestSourceScale: 6, nearestSourceUnit: "$", comparisonReason: "digits_differ",
    context: {"end": 124, "start": 116, "text": "pt number of shares, which are reflected in thousands, and per-share amounts) \n \n\n(1) Net sales by category:\niPhone $ 57,583 $ 49,025 $ 254,098 $ 209,586\nMac 10,792 8,726 37,929 33,708\niPad 6,229 6,952 27,929 28,023\nWearables, Home and Acce"},
  }, "The numbers differ."],
  [{
    evidence_id: "ev", link_index: 0, kind: "step", status: "not_found",
    text: "50.4억 달러", comparedValue: "50368.7433", nearest: "50368.7433",
    nearestSourceScale: 6, nearestSourceUnit: "USD", comparisonReason: "scale_conflict",
    pointer: "/stdout",
  }, "The digits agree but the scale does not."],
  [{
    evidence_id: "ev", link_index: 0, kind: "passage", status: "not_found",
    text: "-342.56달러", comparedValue: "342.56", nearest: "342.56",
    nearestSourceScale: 0, nearestSourceUnit: "USD", comparisonReason: "sign_conflict",
    span: {"end": 46, "start": 40},
  }, "The sign differs."],
  [{
    evidence_id: "ev", link_index: 0, kind: "passage", status: "not_found",
    text: "14,570,195달러", comparedValue: "14,570,195", nearest: "14,570,195",
    nearestSourceScale: 3, nearestSourceUnit: "shares", comparisonReason: "incompatible_unit",
    context: {"end": 125, "start": 115, "text": " 33,696\nTotal net sales $ 114,477 $ 102,466 $ 478,834 $ 416,161\nShares used in computing earnings per share:\nBasic 14,570,195 14,815,307 14,661,935 14,948,500\nDiluted 14,630,318 14,863,609 14,720,306 15,004,697\nEarnings per share:\nBasic $ 2"},
  }, "The source's number is a different kind of quantity."],
  [{
    evidence_id: "ev", link_index: 0, kind: "passage", status: "not_found",
    text: "8,761.43달러",
    comparisonReason: "no_candidate",
  }, "This value was not found in the source."],
];

it.each(diagnosticExamples)("explains the host comparison %#", async (check, reason) => {
  const evidence = { ...guide, id: "ev", title: "Annual filing", tool_name: check.kind === "step" ? "run_python" : "web_fetch" };
  renderWithOpen(<NumberChip check={check} evidence={evidence} evidenceRefs={[evidence]}>{check.text}</NumberChip>);
  fireEvent.click(screen.getByTestId("number-chip"));
  const dialog = within(await screen.findByRole("dialog"));
  expect(dialog.getByText(check.kind === "step" ? "Calculation result" : "Annual filing")).toBeVisible();
  expect(dialog.getByText("In the answer")).toBeVisible();
  expect(dialog.getByText(check.text)).toBeVisible();
  expect(dialog.getByText(reason)).toBeVisible();
  if (check.comparedValue) {
    expect(dialog.getByText(check.kind === "step" ? "In the result" : "In the source")).toBeVisible();
    expect(dialog.getByText(check.comparedValue)).toBeVisible();
  } else expect(dialog.queryByText("In the source")).toBeNull();
  if (check.nearestSourceScale === 6) expect(dialog.getByText(/in millions/)).toBeVisible();
  if (check.nearestSourceScale === 3) expect(dialog.getByText("shares · in thousands")).toBeVisible();
  expect(dialog.getAllByRole("button")).toHaveLength(1);
});

it("names the calculation result for a missing step figure, including a legacy check", async () => {
  const check = { ...diagnosticExamples[4]![0], kind: "step" as const };
  const evidence = { ...calc, tool_name: "run_python" };
  const content = (record: NumberCheck) => <NumberChip check={record} evidence={evidence} evidenceRefs={[evidence]}>{record.text}</NumberChip>;
  const view = renderWithOpen(content(check));
  fireEvent.click(screen.getByTestId("number-chip"));
  expect(await screen.findByRole("dialog")).toHaveTextContent("The calculation did not print this value.");
  view.rerender(content({ ...check, comparisonReason: undefined }));
  expect(screen.getByRole("dialog")).toHaveTextContent("Calculation result");
  expect(screen.getByRole("dialog")).toHaveTextContent("This value could not be confirmed in the calculation result.");
});

it("opens each comparison in the receipt at its own passage or calculation output", async () => {
  const user = userEvent.setup();
  const sourceCheck = { ...diagnosticExamples[2]![0], evidence_id: guide.id };
  const resultCheck = { ...diagnosticExamples[1]![0], link_index: 1, evidence_id: calc.id };
  const result = { ...calc, tool_name: "run_python", excerpt: JSON.stringify({ input: "50368.7433", stdout: "Result: 50368.7433 million USD" }) };
  const { onOpenEvidence } = renderAnswer(answer({
    content: `[${sourceCheck.text}](#evidence-${guide.id}), [${resultCheck.text}](#evidence-${result.id})`,
    evidence_refs: [guide, result], number_checks: [sourceCheck, resultCheck],
  }));
  const trigger = await screen.findByRole("button", { name: "2 numbers to check" });
  await user.click(trigger);
  const rows = within(await screen.findByRole("dialog")).getAllByRole("listitem");
  expect(rows).toHaveLength(2);
  expect(rows[0]).toHaveTextContent(guide.title);
  expect(rows[1]).toHaveTextContent("Calculation result");
  await user.click(within(rows[0]!).getByRole("button", { name: "Open the source" }));
  expect(destinations(onOpenEvidence).at(-1)).toMatchObject({ evidence: guide, step: { comparison: sourceCheck } });
  await user.click(trigger);
  await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Open the step" }));
  const field = await screen.findByLabelText("In the result");
  expect(field).toHaveTextContent("Result: 50368.7433 million USD");
  expect(field.querySelector("mark")).toHaveTextContent("50368.7433");
  expect(screen.queryByRole("dialog")).toBeNull();
});

it("explains a Korean filing warning using the stated scale and keeps the printed value", async () => {
  const language = i18n.language;
  try {
    await i18n.changeLanguage("ko");
    const check = diagnosticExamples[0]![0];
    renderWithOpen(<NumberChip check={check} evidence={guide} evidenceRefs={[guide]}>{check.text}</NumberChip>);
    fireEvent.click(screen.getByTestId("number-chip"));
    const dialog = within(await screen.findByRole("dialog"));
    expect(dialog.getByText("자료의 값")).toBeVisible();
    expect(dialog.getByText("$ 57,583")).toBeVisible();
    expect(dialog.getByText("달러 · 백만 단위")).toBeVisible();
    expect(dialog.getByText("두 수치가 다릅니다.")).toBeVisible();
    expect(dialog.getByRole("button", { name: "자료 열기" })).toBeVisible();
  } finally { await i18n.changeLanguage(language); }
});

it.each([
  ["Apple FY2024 Form 10-K · Net sales", "Apple FY2024 Form 10-K · Net sales"],
  ["  ", "data.sec.gov"],
])("names the SEC comparison source in the popover and receipt for %j", async (title, expected) => {
  const user = userEvent.setup();
  const evidence = evidenceFixture({ id: "ev_sec", title, tool_name: "sec_company_concept", provider: "sec",
    url: "https://data.sec.gov/api/xbrl/companyconcept/CIK0000320193/us-gaap/RevenueFromContractWithCustomerExcludingAssessedTax.json" });
  const check: NumberCheck = { evidence_id: evidence.id, link_index: 0, kind: "data", status: "not_found", text: "383,285", comparisonReason: "no_candidate" };
  renderAnswer(answer({ content: `[${check.text}](#evidence-${evidence.id})`, evidence_refs: [evidence], number_checks: [check] }));
  const figure = await chip(check.text);
  expect(figure).toHaveAccessibleName(`383,285, not found in ${expected}`);
  fireEvent.click(figure);
  expect(await screen.findByRole("dialog", { name: expected })).toBeVisible();
  await user.keyboard("{Escape}");
  await user.click(screen.getByRole("button", { name: "1 number to check" }));
  expect(within(await screen.findByRole("dialog")).getByRole("listitem")).toHaveTextContent(expected);
});

it.each(["KRW 372", "372원", "NT$1,385", "US$ 3.2 billion", "372円", "1,385 TWD"])(
  "keeps the entire streamed currency figure %s pending",
  async text => {
    renderAnswer(answer({ status: "streaming", content: `[${text}](#evidence-${guide.id})`, evidence_refs: [guide], number_checks: undefined }));
    const chip = await screen.findByTestId("number-chip");
    expect(chip).toHaveAttribute("data-status", "pending");
    expect(chip.textContent).toBe(text);
  },
);

it.each([
  ["en", "372원", "The source states this amount in USD, the answer in KRW."],
  ["ko", "372원", "자료는 이 금액을 달러로, 답변은 원으로 적었습니다."],
  ["en", "372", "The source's number is a different kind of quantity."],
])("explains a recorded currency mismatch in %s for %s", async (language, text, expected) => {
  await i18n.changeLanguage(language);
  try {
    const check: NumberCheck = { evidence_id: guide.id, link_index: 0, kind: "passage", status: "not_found", text,
      comparedValue: "US$372", nearestSourceUnit: "USD", comparisonReason: "incompatible_unit" };
    renderWithOpen(<NumberChip check={check} evidence={guide} evidenceRefs={[guide]}>{text}</NumberChip>);
    const chip = screen.getByTestId("number-chip");
    expect(chip).toHaveAccessibleDescription(expected);
    fireEvent.click(chip);
    expect(within(await screen.findByRole("dialog")).getByText(expected)).toBeVisible();
  } finally {
    await i18n.changeLanguage("en");
  }
});

const worldBankSaved = {
  route: "source" as const,
  publisher: "world-bank.indicators",
  publication: { kind: "updated" as const, time: { precision: "date" as const, value: "2026-11-11" } },
  saved_at: "2026-11-21T12:00:00Z",
};

it.each([
  { name: "the saved World Bank update", origin: worldBankSaved, main: "World Bank Indicators · updated Nov 11, 2026", route: "from your Sources · saved Nov 21, 2026" },
  { name: "a World fact with no publisher date", origin: { route: "world" as const, publisher: "sec", saved_at: "2026-11-21T12:00:00Z" }, main: "SEC EDGAR", route: "from your World · saved Nov 21, 2026" },
  { name: "a read with multiple backing Sources", origin: { route: "source" as const, saved_at: "2026-11-21T12:00:00Z" }, main: null, route: "from your Sources · saved Nov 21, 2026" },
  { name: "a direct read without a publication date", origin: undefined, main: "data.example.com · fetched Nov 21, 2026", route: null },
])("presents provenance for $name", async ({ origin, main, route }) => {
  const evidence = evidenceFixture({ id: "ev_population", title: "Japan's population figures", url: "https://data.example.com/population", kind: origin ? "saved_context" : "original",
    tool_name: origin ? "world_read" : "web_fetch", provider: "stale_runtime", retrieved_at: "2026-11-21T12:00:00Z",
    published_at: origin ? "2026-11-21" : undefined, excerpt: "The population was 124,516,650.", memory_origin: origin });
  const check: NumberCheck = { evidence_id: evidence.id, link_index: 0, kind: "passage", status: "matched", text: "124,516,650" };
  renderWithOpen(<NumberChip check={check} evidence={evidence} evidenceRefs={[evidence]}>{check.text}</NumberChip>);
  fireEvent.click(screen.getByTestId("number-chip"));
  const dialog = await screen.findByRole("dialog", { name: evidence.title });
  if (main) expect(within(dialog).getByText(main)).toBeVisible();
  if (route) expect(within(dialog).getByText(route)).toBeVisible();
  if (!main) expect(dialog.querySelectorAll(".number-chip-meta")).toHaveLength(1);
  expect(dialog).not.toHaveTextContent(/published Nov 21|stale_runtime/);
});

it("keeps the publisher update and local save separate for each calculated input", async () => {
  const remembered = { ...dart, kind: "saved_context" as const, tool_name: "world_read", memory_origin: worldBankSaved };
  const [calculation, ...rest] = checks();
  const calculationDetail = calculation!.calc!;
  const checked = { ...calculation!, calc: { ...calculationDetail, inputs: calculationDetail.inputs.map(input => ({ ...input, evidence_id: remembered.id })) } };
  renderAnswer(answer({ evidence_refs: [remembered, guide, calc], number_checks: [checked, ...rest] }));
  fireEvent.click(await chip("8.4 percent"));
  const dialog = within(await screen.findByRole("dialog", { name: "Operating margin" }));
  expect(dialog.getAllByText("World Bank Indicators", { exact: false })).toHaveLength(2);
  expect(dialog.getAllByText(/updated\s+Nov\s+11,\s+2026/)).toHaveLength(2);
  expect(dialog.getAllByText("from your Sources · saved Nov 21, 2026")).toHaveLength(2);
});

it("names attachment inputs in copied formulas and exported footnotes", async () => {
  const user = userEvent.setup();
  const file = evidenceFixture({ id: "ev_ledger", kind: "local_file", title: "Supplier ledger.xlsx", tool_name: "workspace_capture",
    provider: undefined, url: undefined, published_at: undefined, retrieved_at: "2026-03-18T06:00:00Z" });
  const unnamed = { ...file, id: "ev_unnamed", title: "" };
  const check = checks()[0]!;
  const numberChecks = [{ ...check, calc: { ...check.calc!, inputs: check.calc!.inputs.map((input, index) => ({
    ...input, evidence_id: index === 0 ? file.id : unnamed.id,
  })) } }];
  const message = answer({ content: "Margin was [8.4%](#evidence-ev_calc).", evidence_refs: [calc, file, unnamed], number_checks: numberChecks });
  renderAnswer(message);
  await user.click(await chip("8.4 percent"));
  await user.click(screen.getByRole("button", { name: "Copy formula" }));
  expect(await navigator.clipboard.readText()).toContain("inputs: Supplier ledger.xlsx, 2026-03-18; Files, 2026-03-18");
  const markdown = chatMarkdown({ id: "chat", loupist_id: "l", title: "Margin", updated_at: "", use_memory: true, update_memory: true,
    messages: [message] }, "Quality", Date.parse("2026-03-20"));
  expect(markdown).toContain("inputs: Supplier ledger.xlsx, 2026-03-18; Files, 2026-03-18");
});

it("opens the Python step from a cited figure", async () => {
  const evidence = evidenceFixture({ id: "ev_python", title: "Python output", kind: "derived", tool_name: "run_python", excerpt: "42.1%" });
  const check: NumberCheck = { link_index: 0, text: "42.1%", evidence_id: evidence.id, kind: "step", status: "matched" };
  const open = vi.fn();
  window.addEventListener("loupist:open-activity", open, { once: true });
  renderWithOpen(<NumberChip check={check} evidence={evidence} evidenceRefs={[evidence]}>42.1%</NumberChip>);
  fireEvent.click(await chip("42.1"));
  fireEvent.click(await screen.findByRole("button", { name: "Show the math" }));
  expect(open).toHaveBeenCalledWith(expect.objectContaining({
    detail: { tab: "steps", pythonEvidenceId: "ev_python" },
  }));
});
