import type { ComponentProps } from "react";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ChatEvidenceRef, ChatMessage, NumberCheck } from "../../types";
import { evidenceFixture } from "../../test/evidenceFixtures";
import { chatMarkdown } from "../../chat/exportChat";
import { chatReceipt } from "../../chat/receipt";
import { TooltipProvider } from "../ui/tooltip";
import { ChatMessageCard } from "./ChatMessageCard";

const dart = evidenceFixture({ id: "ev_dart", title: "Solsup Foods FY2025 statements", tool_name: "dart_financials", provider: "OpenDART", url: undefined, published_at: "2026-03-18",
  excerpt: JSON.stringify({ facts: [{ corp_name: "Solsup Foods", canonical_concept: "revenue", value: "452312000000" }, { corp_name: "Solsup Foods", canonical_concept: "operating_income", value: "38104000000" }] }, null, 2), dataset_ref: { dataset_id: "ds", revision: 1, digest: "d".repeat(64) } });
const passageText = "Management guided to an operating margin of 9.1% for FY2025.";
const guide = evidenceFixture({ id: "ev_guide", title: "February guidance call", tool_name: "web_fetch", url: "https://solsup.example/guidance", published_at: "2026-02-12", excerpt: passageText });
const calc: ChatEvidenceRef = evidenceFixture({ id: "ev_calc", title: "Margin", kind: "derived", tool_name: "calculate", url: undefined, excerpt: "" });
const span = (needle: string) => {
  const start = new TextEncoder().encode(passageText.slice(0, passageText.indexOf(needle))).length;
  return { start, end: start + new TextEncoder().encode(needle).length };
};

/** What Rust ships with each input of a calculation whose `unit` was "KRW billions". */
const KRW_BILLIONS = { kind: "amount", currency: "KRW", scale: 9 } as const;

const checks = (marginStatus: NumberCheck["status"] = "matched"): NumberCheck[] => [
  { link_index: 0, text: marginStatus === "differs" ? "8.5%" : "8.4%", evidence_id: calc.id, kind: "calculated", status: marginStatus, ...(marginStatus === "differs" ? { nearest: "8.42%" } : {}),
    calc: { title: "Margin", formulas: ["Operating income / Revenue = 38.1 / 452.3 = 8.42%"], inputs: [
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
  progress: { startedAtMs: 1, finishedAtMs: 2, items: [{ id: "step-calc", kind: "tool", toolName: "calculate", category: "compute", operation: "calculate", action: "Calculated", status: "succeeded", resultIds: [calc.id], startedAtMs: 1, finishedAtMs: 2 }] },
  ...extra,
});

function renderAnswer(message: ChatMessage, extra: Partial<ComponentProps<typeof ChatMessageCard>> = {}) {
  const props = { message, loupistId: "loupist-a", chatId: "chat-a", loupistName: "Quality", onOpenMemory: vi.fn(), onOpenMemoryPage: vi.fn(), onOpenArtifact: vi.fn(), onOpenLink: vi.fn(), onReadAttachment: vi.fn(), onOpenEvidence: vi.fn(), ...extra };
  const view = render(<TooltipProvider><ChatMessageCard {...props} /></TooltipProvider>);
  return { ...props, view };
}

const chip = async (text: string) => {
  const found = await screen.findAllByRole("button", { name: new RegExp(`^${text.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&")}`) });
  return found.find(element => element.classList.contains("number-chip"))!;
};

it("renders a numeric link as a number chip and a non-numeric one as a numbered citation", async () => {
  renderAnswer(answer());
  const margin = await chip("8.4 percent");
  expect(margin).toHaveAttribute("aria-haspopup", "dialog");
  expect(margin).toHaveAccessibleName("8.4 percent, calculated. Show the math");
  expect(await chip("₩452.3bn")).toHaveAccessibleName("₩452.3bn, from OpenDART");
  const citation = await screen.findByRole("button", { name: "Source 2: February guidance call" });
  expect(citation).toHaveClass("loupist-citation");
  expect(citation).toHaveTextContent("2");
});

it("keeps a bold attached-note token linked through the workspace-aware answer renderer", async () => {
  const note = evidenceFixture({ title: "northwind-loom.md", kind: "local_file", tool_name: "workspace_capture", url: undefined, provider: undefined,
    excerpt: "On 12 March 2026 the internal note recorded a loom-yield token of 18420." });
  const { view } = renderAnswer(answer({
    content: `Northwind Loom recorded loom-yield token **[18420](#evidence-${note.id})** on 12 March 2026. The note identifies it as a fixture identifier, not a market value.`,
    evidence_refs: [note],
    number_checks: [{ link_index: 0, text: "18420", evidence_id: note.id, kind: "step", status: "matched", pointer: "/content" }],
  }), { workspacePath: "/fixture/workspace", onOpenFile: vi.fn() });
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
  expect(dialog).toHaveTextContent("Calculated on this computer");
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
  expect(onOpenEvidence).toHaveBeenCalledWith(dart, expect.objectContaining({ cell: { pointer: "/facts/0/value", key: "value", value: "452312000000" } }));
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
  // Matched and rounded numbers never reach the receipt.
  expect(chatReceipt(answer()).numbersToCheck).toBe(0);
  expect(chatReceipt(answer({ number_checks: differs })).numbersToCheck).toBe(1);
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
