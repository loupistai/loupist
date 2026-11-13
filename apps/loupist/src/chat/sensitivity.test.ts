import i18n from "@/i18n";
import type { NumberCheck, SensitivityCard } from "../contracts/chat";
import { morrowWithGrid, previewSensitivityGrid } from "../preview/korea";
import { changePrompt, heldSensitivityParagraphs, inputValue, sensitivityParagraphs, sensitivityTsv } from "./sensitivity";

const GRID = "ev_grid01";
const OTHER = "ev_statements";
const card = (link_index: number, evidence_id = GRID): SensitivityCard => ({ ...previewSensitivityGrid("none"), link_index, evidence_id, subject: "Solsup Foods" });
const check = (link_index: number, text: string): NumberCheck => ({ link_index, text, evidence_id: OTHER, kind: "data", status: "matched" });

it("takes the paragraph that is only the grid link out of the text and keeps later number checks on their links", () => {
  const content = `Revenue was [₩452.3bn](#evidence-${OTHER}).\n\n[Solsup Foods](#evidence-${GRID})\n\nMargin was [8.4%](#evidence-${OTHER}) [1](#evidence-${OTHER}).`;
  const result = sensitivityParagraphs(content, [card(1)], [check(0, "₩452.3bn"), check(2, "8.4%")]);
  expect(result.cards.map(item => item.link_index)).toEqual([1]);
  expect(result.text).toBe(`Revenue was [₩452.3bn](#evidence-${OTHER}).\n\nMargin was [8.4%](#evidence-${OTHER}) [1](#evidence-${OTHER}).`);
  expect(result.numberChecks?.map(item => [item.link_index, item.text])).toEqual([[0, "₩452.3bn"], [1, "8.4%"]]);
  // Split where the card stands: each part's checks count its own links.
  expect(result.segments.map(segment => [segment.text, segment.card?.link_index, segment.numberChecks?.map(item => [item.link_index, item.text])])).toEqual([
    [`Revenue was [₩452.3bn](#evidence-${OTHER}).`, 1, [[0, "₩452.3bn"]]],
    [`Margin was [8.4%](#evidence-${OTHER}) [1](#evidence-${OTHER}).`, undefined, [[0, "8.4%"]]],
  ]);
});

it("holds, while the answer streams, the paragraph Rust will make a card, split as the settled answer is", () => {
  const evidence = [
    { id: GRID, title: "Reverse DCF grid", kind: "derived", tool_name: "calculate", calc_operation: "reverse_dcf", retrieved_at: "2026-04-17T06:00:00Z", excerpt: "", excerpt_truncated: false, content_sha256: "c".repeat(64), warnings: [] },
    { id: OTHER, title: "Statements", kind: "original", tool_name: "dart_financials", retrieved_at: "2026-04-17T06:00:00Z", excerpt: "", excerpt_truncated: false, content_sha256: "c".repeat(64), warnings: [] },
  ] as Parameters<typeof heldSensitivityParagraphs>[1];
  const intro = `Revenue was [₩452.3bn](#evidence-${OTHER}).`;
  // The link is still open: nothing is held yet (the hold-back rules hide the open link itself).
  expect(heldSensitivityParagraphs(`${intro}\n\n[Solsup Foods](#evidence-${GRID.slice(0, 5)}`, evidence)).toHaveLength(1);
  // Closed and alone so far: held, with nothing after it yet.
  expect(heldSensitivityParagraphs(`${intro}\n\n[Solsup Foods](#evidence-${GRID})`, evidence)).toEqual([{ text: intro, held: GRID }]);
  // Text continues on the same paragraph: it was an inline link after all.
  expect(heldSensitivityParagraphs(`${intro}\n\n[Solsup Foods](#evidence-${GRID}) needs 7.3%`, evidence)).toHaveLength(1);
  const content = `${intro}\n\n[Solsup Foods](#evidence-${GRID})\n\nThe middle case needs 7.3%.`;
  const settled = sensitivityParagraphs(content, [card(1)]).segments;
  expect(heldSensitivityParagraphs(content, evidence)).toEqual(settled.map((segment) => ({ text: segment.text, held: segment.card?.evidence_id })));
  // A lone link to an ordinary result stays text.
  expect(heldSensitivityParagraphs(`${intro}\n\n[Statements](#evidence-${OTHER})`, evidence)).toHaveLength(1);
});

it("leaves the text alone when the listed link is inside a sentence or points elsewhere", () => {
  const inline = `See [the grid](#evidence-${GRID}) for the range.`;
  expect(sensitivityParagraphs(inline, [card(0)]).cards).toEqual([]);
  expect(sensitivityParagraphs(inline, [card(0)]).text).toBe(inline);
  // A paragraph link at another index, or to another result, is not the card Rust listed.
  const alone = `[Solsup Foods](#evidence-${GRID})`;
  expect(sensitivityParagraphs(alone, [card(1)]).cards).toEqual([]);
  expect(sensitivityParagraphs(alone, [card(0, "ev_other")]).cards).toEqual([]);
  expect(sensitivityParagraphs(alone, []).text).toBe(alone);
});

it("prints inputs in the app's one money format, with share counts at the same scale", async () => {
  const solsup = previewSensitivityGrid("reference");
  const morrow = morrowWithGrid().implied!.grid!;
  const input = (grid: typeof solsup, name: string) => inputValue(grid, grid.inputs.find(item => item.name === name)!);
  expect([input(solsup, "cash_flow"), input(solsup, "shares"), input(solsup, "net_debt")]).toEqual(["₩24.5bn", "34.6m shares", "₩61.2bn"]);
  expect([input(morrow, "cash_flow"), input(morrow, "shares")]).toEqual(["$402m", "182.4m shares"]);
  await i18n.changeLanguage("ko");
  try {
    expect([input(solsup, "cash_flow"), input(solsup, "shares"), input(solsup, "net_debt")]).toEqual(["245억 원", "3,456만 주", "612억 원"]);
    // No digit the English line keeps is dropped in Korean.
    expect([input(morrow, "cash_flow"), input(morrow, "shares"), input(morrow, "net_debt")]).toEqual(["4억 200만 달러", "1억 8,240만 주", "3억 1,000만 달러"]);
  } finally {
    await i18n.changeLanguage("en");
  }
});

it("words Change the rates with the grid's own rates and copies the grid as tab-separated rows", () => {
  const grid = previewSensitivityGrid("nulls");
  expect(changePrompt(grid)).toBe("Recalculate with discount rates 8%, 9%, 10%, 11%, 12% and long-run growth 1.5%, 2.0%, 2.5%, 3.0%, 3.5%.");
  const lines = sensitivityTsv(grid).split("\n").map(line => line.split("\t"));
  expect(lines).toHaveLength(6);
  expect(lines[0]).toEqual(["Discount rate / Long-run growth", "1.5%", "2.0%", "2.5%", "3.0%", "3.5%"]);
  // Cells the tool could not solve stay empty, never zero.
  expect(lines[1]).toEqual(["8%", "4.1%", "3.4%", "2.6%", "", ""]);
});
