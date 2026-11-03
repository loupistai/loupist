import type { ComponentProps } from "react";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { SensitivityGrid } from "../../types";
import { previewSensitivityAnswer, previewSensitivityGrid, SENSITIVITY_EVIDENCE } from "../../preview/korea";
import { TooltipProvider } from "../ui/tooltip";
import { ChatMessageCard } from "./ChatMessageCard";
import { SensitivityCard } from "./SensitivityCard";

const evidenceRefs = Object.values(SENSITIVITY_EVIDENCE);
const user = () => userEvent.setup({ skipHover: true });

function renderCard(grid: SensitivityGrid, extra: Partial<ComponentProps<typeof SensitivityCard>> = {}) {
  const actions = { onOpenEvidence: vi.fn(), onShowStep: vi.fn() };
  const onChangeRates = vi.fn();
  render(<SensitivityCard grid={grid} subject="Solsup Foods" evidence={SENSITIVITY_EVIDENCE.grid} evidenceRefs={evidenceRefs} actions={actions} onChangeRates={onChangeRates} {...extra} />);
  const card = screen.getByRole("region", { name: /What the price implies · Solsup Foods/u });
  return { card, table: within(card).getByRole("table"), actions, onChangeRates };
}

/** The cell at a discount rate (row header) and long-run growth (column). */
function cell(table: HTMLElement, discount: string, column: number) {
  const row = within(table).getByRole("rowheader", { name: `Discount rate ${discount}` }).closest("tr")!;
  return row.querySelectorAll("td")[column]!;
}

it("outlines the base case and shades only cells that need more than the last five years", () => {
  const { card, table } = renderCard(previewSensitivityGrid("reference"));
  expect(within(card).getByRole("table", { name: /What the price implies · Solsup Foods · ₩38,200 on Apr 16/u })).toBe(table);
  expect(within(table).getByRole("columnheader", { name: "Long-run growth 2.0%" })).toBeVisible();
  // Base case: the middle row and column (10%, 2.5%).
  const base = cell(table, "10%", 2);
  expect(base).toHaveAttribute("data-base");
  expect(base).toHaveTextContent("7.3%, base case");
  expect(getComputedStyle(base).outline).toMatch(/^2px solid/u);
  expect(table.querySelectorAll("[data-base]")).toHaveLength(1);
  // 8.4% at 10% needs more than the 8.0% reference; 7.9% beside it does not.
  expect(cell(table, "10%", 0)).toHaveAttribute("data-above");
  expect(cell(table, "10%", 0)).toHaveTextContent("needs more than the last 5 years");
  expect(cell(table, "10%", 1)).not.toHaveAttribute("data-above");
  expect(table.querySelectorAll("[data-above]")).toHaveLength(11);
  expect(within(card).getByText(/More than the last 5 years delivered/u)).toBeVisible();
  expect(within(card).getByText("A range, not a forecast.")).toBeVisible();
  // The discount rate is said in plain words where it first appears, and inputs use the app's money format.
  expect(within(card).getByText("Discount rate: the yearly return investors require")).toBeVisible();
  expect(card.querySelector(".sensitivity-inputs")).toHaveTextContent(/Free cash flow FY2025 ₩24\.5bn.*34\.6m shares.*₩61\.2bn/u);
  // Never the formula's jargon in the card's words.
  expect(card.textContent).not.toMatch(/WACC|terminal/iu);
});

it("has no shading and no legend without a reference, and says how many cells it could not solve", async () => {
  const { card, table } = renderCard(previewSensitivityGrid("nulls"));
  expect(card).not.toHaveAttribute("data-reference");
  expect(table.querySelectorAll("[data-above]")).toHaveLength(0);
  expect(within(card).queryByText(/More than the last/u)).toBeNull();
  expect(within(card).getByText("2 cells are outside what can be calculated")).toBeVisible();
  const outside = within(table).getAllByRole("button", { name: "Outside the range this tool solves" });
  expect(outside).toHaveLength(2);
  expect(outside[0]).toHaveTextContent("—");
});

it("titles a value grid by rates and never shades it", () => {
  const grid: SensitivityGrid = { ...previewSensitivityGrid("reference"), operation: "dcf_grid", unit: "per share", values: [[52100, 48000], [44000, 41000]], row_values: [9, 10], column_values: [2, 3] };
  render(<SensitivityCard grid={grid} subject="Solsup Foods" />);
  const card = screen.getByRole("region", { name: "Value per share under different rates" });
  expect(card.querySelectorAll("[data-above]")).toHaveLength(0);
  expect(within(card).queryByText(/More than the last/u)).toBeNull();
  expect(within(card).getByText("₩41,000")).toBeVisible();
});

it("Show step reveals the grid's own step, sources open evidence, and Change the rates only fills the composer", async () => {
  const { card, actions, onChangeRates } = renderCard(previewSensitivityGrid("reference"));
  await user().click(within(card).getByRole("button", { name: "How this was calculated" }));
  expect(actions.onShowStep).toHaveBeenCalledWith(SENSITIVITY_EVIDENCE.grid);
  const source = within(card).getAllByRole("button", { name: /^Source 1:/u })[0]!;
  await user().click(source);
  expect(actions.onOpenEvidence).toHaveBeenCalledWith(SENSITIVITY_EVIDENCE.statements, undefined, source);
  expect(actions.onOpenEvidence.mock.calls[0]?.[2]).toBe(source);
  await user().click(within(card).getByRole("button", { name: "Change the rates · Solsup Foods" }));
  expect(onChangeRates).toHaveBeenCalledWith("Recalculate with discount rates 8%, 9%, 10%, 11%, 12% and long-run growth 1.5%, 2.0%, 2.5%, 3.0%, 3.5%.");
});

it("replaces the answer's grid paragraph with the card and fills the composer without sending", async () => {
  const onFillComposer = vi.fn();
  const view = render(<TooltipProvider><ChatMessageCard message={previewSensitivityAnswer("reference")} loupistId="loupist-a" chatId="chat-a" loupistName="Loupist"
    onOpenMemory={vi.fn()} onOpenMemoryPage={vi.fn()} onOpenArtifact={vi.fn()} onOpenLink={vi.fn()} onReadAttachment={vi.fn()} onOpenEvidence={vi.fn()} onFillComposer={onFillComposer} /></TooltipProvider>);
  const card = screen.getByRole("region", { name: /What the price implies · Solsup Foods/u });
  // The card stands where its link paragraph was: after the paragraph before it, before the one after.
  const [before, after] = [...view.container.querySelectorAll<HTMLElement>(".message-content")];
  expect(before).toHaveTextContent(/^Solsup Foods’ price already assumes cash flow keeps growing/u);
  expect(before!.compareDocumentPosition(card) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(card.compareDocumentPosition(after!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  // The bare link is gone from the text, and the number after it keeps its own check.
  for (const text of [before!, after!]) expect(within(text).queryByText("Solsup Foods")).toBeNull();
  // The prose quotes the card's base cell.
  expect(after).toHaveTextContent(/^The middle case needs about 7\.3% a year/u);
  expect(card.querySelector("[data-base]")).toHaveTextContent(/^7\.3%/u);
  expect(await within(after!).findByRole("button", { name: /^8 percent, calculated/u })).toHaveAttribute("data-kind", "calculated");
  await user().click(within(card).getByRole("button", { name: "Change the rates · Solsup Foods" }));
  expect(onFillComposer).toHaveBeenCalledWith(expect.stringMatching(/^Recalculate with discount rates 8%/u));
});
