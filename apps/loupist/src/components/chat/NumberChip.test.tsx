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

const chip = async (text: string) => {
  const found = await screen.findAllByRole("button", { name: new RegExp(`^${text.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&")}`) });
  return found.find(element => element.classList.contains("number-chip"))!;
};

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
  expect(dialog).toHaveTextContent(/Yahoo Finance · \w{3}\s\d{1,2}/u);
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
  expect(await screen.findByRole("dialog")).toHaveTextContent("The calculation gave 8.42%. The answer says 8.5%.");
  // The thread stays at the number: a chat opened at its end no longer returns there.
  expect(scroll.holdScrollPosition).toHaveBeenCalled();
  // Matched and rounded numbers never reach the receipt.
  expect(chatReceipt(answer()).numbersToCheck).toBe(0);
  expect(chatReceipt(answer({ number_checks: differs })).numbersToCheck).toBe(1);
});

it("never draws a number its source does not hold as a sourced one, and the receipt opens it", async () => {
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
  expect(await screen.findByRole("dialog")).toHaveTextContent("This number isn't in the cited source as written.");
  expect(guided).toHaveAttribute("aria-expanded", "true");
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
