/**
 * The sensitivity card's paragraph rule and its words (finance-korea-retail-disagreement.md
 * §2.2). Rust decides which paragraphs are cards and copies each grid from the
 * host's own calculation; this only finds the paragraph, formats the axes and
 * cells in the reader's language, and words the copy and the composer prompt.
 */
import i18n from "@/i18n";
import { formatAmount, formatDate, formatNumber, formatShareCount } from "@/i18n/format";
import type { ChatEvidenceRef, NumberCheck, SensitivityCard, SensitivityGrid } from "../contracts/chat";
import { humanize, plainLabel } from "./numberText";

const t = i18n.getFixedT(null, "chat");

/** Evidence ids as Rust reads them from `[text](#evidence-…)` links. */
const LINK = /\]\(#evidence-([A-Za-z0-9_]+)\)/g;
const ONLY_LINK = /^\s*\[[^\]\n]*\]\(#evidence-([A-Za-z0-9_]+)\)\s*$/;

/** Answer text up to a card, with the number checks of its own links, and the card that follows it. */
export type SensitivitySegment = { text: string; numberChecks: NumberCheck[] | undefined; card?: SensitivityCard };

const tidy = (text: string) => text.replace(/\n[ \t]*\n(?:[ \t]*\n)+/g, "\n\n").trim();

/**
 * Paragraphs that are only a link to a `reverse_dcf` or `dcf_grid` result
 * become sensitivity cards, as Rust listed them: each such paragraph leaves the
 * text and the number checks after it keep pointing at their own links. A card
 * whose paragraph is not found is not shown; its link stays in the text.
 * `segments` splits the answer where its cards stand, so each card follows
 * the paragraph before its link; each segment's checks count its own links.
 */
export function sensitivityParagraphs(content: string, listed: readonly SensitivityCard[] = [], checks?: NumberCheck[]): { text: string; numberChecks: NumberCheck[] | undefined; cards: SensitivityCard[]; segments: SensitivitySegment[] } {
  const whole = { text: content, numberChecks: checks, cards: [], segments: [{ text: content, numberChecks: checks }] };
  if (!listed.length) return whole;
  const split = splitAtLinkParagraphs(content, (linkIndex, evidenceId) => listed.find(item => item.link_index === linkIndex && item.evidence_id === evidenceId), checks);
  return split.cards.length ? split : { ...whole, cards: [] };
}

/** Results a lone link paragraph becomes a card for, once Rust confirms it as the reply settles. */
const GRID_OPERATIONS = new Set(["reverse_dcf", "dcf_grid"]);

/**
 * While the answer streams, the paragraphs Rust will make cards: a lone link
 * to a `reverse_dcf` or `dcf_grid` result. The split is the one
 * `sensitivityParagraphs` makes when the reply settles, so each piece of text
 * keeps its place and the held paragraph's frame becomes its card (`held` is
 * the result's evidence id). A paragraph still being written that is only that
 * link so far is held too; if more text follows on it, it is text again.
 */
export function heldSensitivityParagraphs(content: string, evidence: readonly ChatEvidenceRef[] = []): { text: string; held?: string }[] {
  const grids = new Set(evidence.flatMap((item) => item.calc_operation && GRID_OPERATIONS.has(item.calc_operation) ? [item.id] : []));
  if (!grids.size) return [{ text: content }];
  const split = splitAtLinkParagraphs(content, (_linkIndex, evidenceId) => grids.has(evidenceId) ? evidenceId : undefined, undefined);
  return split.cards.length ? split.segments.map((segment) => ({ text: segment.text, held: segment.card })) : [{ text: content }];
}

function splitAtLinkParagraphs<T>(content: string, match: (linkIndex: number, evidenceId: string) => T | undefined, checks: NumberCheck[] | undefined) {
  const parts = content.split(/(\n[ \t]*\n)/);
  const removed: number[] = [];
  const cards: T[] = [];
  const kept: string[] = [];
  /** Segments by the original link indices they span: [start, end). */
  const spans: { parts: string[]; start: number; end: number; card?: T }[] = [{ parts: [], start: 0, end: 0 }];
  let linkIndex = 0;
  for (const part of parts) {
    const only = ONLY_LINK.exec(part);
    const card = only ? match(linkIndex, only[1]!) : undefined;
    if (card !== undefined) {
      removed.push(linkIndex);
      spans.at(-1)!.card = card;
      linkIndex += 1;
      spans.push({ parts: [], start: linkIndex, end: linkIndex });
      cards.push(card);
      continue;
    }
    linkIndex += [...part.matchAll(LINK)].length;
    spans.at(-1)!.parts.push(part);
    spans.at(-1)!.end = linkIndex;
    kept.push(part);
  }
  const text = tidy(kept.join(""));
  const numberChecks = checks?.filter(check => !removed.includes(check.link_index))
    .map(check => ({ ...check, link_index: check.link_index - removed.filter(index => index < check.link_index).length }));
  const segments = spans.map(span => ({
    text: tidy(span.parts.join("")),
    numberChecks: checks?.filter(check => check.link_index >= span.start && check.link_index < span.end).map(check => ({ ...check, link_index: check.link_index - span.start })),
    card: span.card,
  })).filter(segment => segment.text || segment.card !== undefined);
  return { text, numberChecks, cards, segments };
}

const MINUS = "−";
const minus = (text: string) => text.replace(/^-/u, MINUS);

/** A discount rate as its row header: "8%", "10.5%". */
export const discountRate = (value: number) => `${formatNumber(value, { maximumFractionDigits: 2 })}%`;
/** A long-run growth rate as its column header: "2.0%", "2.25%". */
export const growthRate = (value: number) => minus(`${formatNumber(value, { minimumFractionDigits: 1, maximumFractionDigits: 2 })}%`);

const currencyCode = (value?: string) => (value && /^[A-Z]{3}$/u.test(value) ? value : undefined);
/** The currency the amount inputs are in ("USD millions" → USD). */
const amountCurrency = (grid: Pick<SensitivityGrid, "amount_unit" | "unit" | "price">) =>
  currencyCode(grid.unit) ?? currencyCode(grid.amount_unit?.trim().split(/\s+/u)[0]) ?? currencyCode(grid.price?.currency);

/** One cell: implied growth for reverse_dcf ("7.9%"), value per share for dcf_grid. */
export function cellText(grid: Pick<SensitivityGrid, "operation" | "unit" | "amount_unit" | "price">, value: number) {
  if (grid.operation === "reverse_dcf") return minus(`${formatNumber(value, { minimumFractionDigits: 1, maximumFractionDigits: 1 })}%`);
  const currency = amountCurrency(grid);
  return minus(currency
    ? formatNumber(value, { style: "currency", currency, minimumFractionDigits: 0, maximumFractionDigits: 2 })
    : formatNumber(value, { maximumFractionDigits: 2 }));
}

/** The price the grid solves for, in its own currency when known. */
export function priceText(price: NonNullable<SensitivityGrid["price"]>) {
  const currency = currencyCode(price.currency);
  const digits = price.value % 1 === 0 ? 0 : 2;
  return currency ? formatNumber(price.value, { style: "currency", currency, minimumFractionDigits: digits, maximumFractionDigits: digits }) : price.display;
}

/** "Oct 31": a calendar day without a time-zone shift. */
export const cardDay = (day: string) => formatDate(Date.parse(`${day.slice(0, 10)}T00:00:00Z`), { timeZone: "UTC", year: undefined, month: "short", day: "numeric" });

const SCALES: [RegExp, number][] = [[/\btrillions?\b|조/iu, 1e12], [/\bbillions?\b/iu, 1e9], [/\bmillions?\b/iu, 1e6], [/\bthousands?\b/iu, 1e3], [/억/u, 1e8], [/만/u, 1e4]];

/**
 * An input's printed value in the app's one money format: amounts in their
 * currency and scale ("$402m" / "4억 200만 달러"), share counts at the same
 * scale ("34.6m shares" / "3,456만 주"), growth as a percent.
 */
export function inputValue(grid: Pick<SensitivityGrid, "amount_unit" | "unit" | "price">, input: SensitivityGrid["inputs"][number]) {
  const value = Number(input.display.replace(/[,\s]/gu, "").replace(MINUS, "-"));
  if (!Number.isFinite(value)) return input.display;
  if (input.name === "growth_pct") return growthRate(value);
  if (input.name === "shares") return formatShareCount(value);
  const scale = SCALES.find(([pattern]) => pattern.test(grid.amount_unit ?? ""))?.[1] ?? 1;
  return minus(formatAmount(value * scale, amountCurrency(grid)));
}

/** An input's name in the reader's words: the label the calculation was given, else its known term. */
export function inputLabel(input: SensitivityGrid["inputs"][number]) {
  const plain = plainLabel(input.label ?? input.name).text;
  return plain === input.name ? humanize(input.name) : plain;
}

/** The base case: the middle row and column (Rust uses integer division). */
export const baseCell = (grid: Pick<SensitivityGrid, "row_values" | "column_values">) =>
  [Math.floor(grid.row_values.length / 2), Math.floor(grid.column_values.length / 2)] as const;

/** Cells reverse_dcf could not solve within −50%…100%. */
export const outsideCount = (grid: Pick<SensitivityGrid, "values">) => grid.values.reduce((count, row) => count + row.filter(value => value === null).length, 0);

/** Shading only on reverse-DCF cells that need more growth than a reference. */
export const shaded = (grid: Pick<SensitivityGrid, "operation" | "reference" | "above_reference">, row: number, column: number) =>
  grid.operation === "reverse_dcf" && Boolean(grid.reference) && grid.above_reference?.[row]?.[column] === true;

const tsvCell = (value: string) => (/[\t\n\r"]/u.test(value) ? `"${value.replaceAll('"', '""')}"` : value);

/** "Copy data": the grid as tab-separated text, one row per discount rate. */
export function sensitivityTsv(grid: SensitivityGrid) {
  const header = [`${t("sensitivity.discount")} / ${t("sensitivity.longRun")}`, ...grid.column_values.map(growthRate)];
  const rows = grid.row_values.map((rate, row) => [discountRate(rate), ...grid.column_values.map((_, column) => {
    const value = grid.values[row]?.[column];
    return value === null || value === undefined ? "" : cellText(grid, value);
  })]);
  return [header, ...rows].map(line => line.map(tsvCell).join("\t")).join("\n");
}

/** "Change the rates": the composer text with the rates the grid used. */
export const changePrompt = (grid: Pick<SensitivityGrid, "row_values" | "column_values">) =>
  t("sensitivity.changePrompt", { wacc: grid.row_values.map(discountRate).join(", "), growth: grid.column_values.map(growthRate).join(", ") });
