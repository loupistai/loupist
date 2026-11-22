import i18n from "@/i18n";
import { formatNumber } from "@/i18n/format";
import type { ChatEvidenceRef, ChatMessage, NumberCheck } from "../types";
import { byteMatchParts, citationSources, evidenceProvenance } from "./evidence";
import { providerName } from "../lib/marketProviderName";
import { normalizeCurrencyMarkers } from "./numberText";

const t = i18n.getFixedT(null, "chat");

export function calculationCheck(check: NumberCheck, evidence?: ChatEvidenceRef) {
  return check.kind === "step" || check.kind === "calculated" || evidence?.tool_name === "run_python" || evidence?.tool_name === "calculate";
}

/** Follow the host's location only; duplicate figures in other fields are unrelated. */
export function comparisonPassage(excerpt: string, check: NumberCheck | undefined) {
  if (!check) return null;
  if (check.span) return byteMatchParts(excerpt, check.span);
  if (check.context) return byteMatchParts(check.context.text, check.context);
  if (!check.pointer?.startsWith("/") || !check.comparedValue) return null;
  // The host can select a field beyond the excerpt's display limit. The receipt
  // still retains its exact path and printed value, even for incomplete JSON.
  const recordedField = { before: `${check.pointer}\n`, match: check.comparedValue, after: "" };
  try {
    let field: unknown = JSON.parse(excerpt);
    for (const segment of check.pointer.slice(1).split("/")) {
      const key = segment.replaceAll("~1", "/").replaceAll("~0", "~");
      if (!field || typeof field !== "object" || !Object.hasOwn(field, key)) return recordedField;
      field = (field as Record<string, unknown>)[key];
    }
    if (typeof field !== "string") return recordedField;
    const at = field.indexOf(check.comparedValue);
    if (at < 0) return recordedField;
    return { before: field.slice(0, at), match: check.comparedValue, after: field.slice(at + check.comparedValue.length) };
  } catch { return recordedField; }
}

/** A data cell the Evidence reader marks when it opens (`pointer` from the check). */
export type EvidenceCell = { pointer: string; key: string; value: string };

/**
 * Presentation only, while an answer streams: a link whose whole text reads
 * as one number with its unit gets the pending underline. Rust decides which
 * links are numbers when the answer finishes; the renderer never rechecks.
 */
const NUMBER_TEXT = /^[+\-−–]?(?:(?:[A-Za-z]{1,2})?[$₩€£¥￦]|[A-Z]{3}\s)?\s?[\-−]?\d[\d,]*(?:\.\d+)?(?:\s?(?:조|억|만|천)(?:\s?\d[\d,]*(?:\.\d+)?)?)*\s?(?:%p|%pt|%|pp|bps?|x|×|배|[kmbt]n?|mn|bn|tn|thousand|million|billion|trillion|percent|원|달러|엔|유로|krw|usd|eur|jpy|won|dollars?)?(?:\s?(?:원|달러|krw|usd|won|dollars?|\$))?$/iu;

export function looksLikeNumber(text: string): boolean {
  const value = text.trim();
  if (!NUMBER_TEXT.test(value) && !NUMBER_TEXT.test(normalizeCurrencyMarkers(value))) return false;
  // Years and small counts are labels or numbered citations, never number chips.
  return !/^\d{1,3}$/u.test(value) && !/^(?:19|20)\d{2}$/u.test(value);
}

const normalized = (text: string) => text.replace(/[\s ,]/gu, "");

/**
 * The check Rust stored for the answer's evidence link at `linkIndex`, when it
 * still names this evidence and text. A repeated figure has a check per
 * occurrence, and a link past Rust's bound has none to borrow.
 */
export function checkFor(checks: readonly NumberCheck[] | undefined, linkIndex: number | undefined, evidenceId: string, text: string) {
  if (linkIndex === undefined) return undefined;
  const check = checks?.find((item) => item.link_index === linkIndex);
  return check && check.evidence_id === evidenceId && normalized(check.text) === normalized(text) ? check : undefined;
}

export const needsChecking = (check: Pick<NumberCheck, "status">) => check.status === "not_found" || check.status === "differs";

/** Number links in a finished answer that did not match their source. */
export const numbersToCheck = (message: Pick<ChatMessage, "number_checks">) => (message.number_checks ?? []).filter(needsChecking).length;

/** The number as spoken: "8.4 percent" for a percent, the printed text otherwise. */
export function spokenNumber(text: string) {
  const percent = text.trim().match(/^([+\-−]?\d[\d,]*(?:\.\d+)?)\s?%$/u);
  if (!percent) return text.trim();
  const value = Number(percent[1].replace(/[,−]/gu, (c) => (c === "−" ? "-" : "")));
  if (!Number.isFinite(value)) return text.trim();
  return formatNumber(value, { style: "unit", unit: "percent", unitDisplay: "long", maximumFractionDigits: 6 });
}

/** A formula as two reading lines: what it computes, then the substituted values. */
export function formulaLines(formula: string): string[] {
  const at = formula.indexOf(" = ");
  return at < 0 ? [formula] : [formula.slice(0, at), formula.slice(at + 1)];
}

/** A date that must never wrap inside ("Mar 18, 2026" on one line): spaces become no-break spaces. */
export const unbroken = (text: string) => text.replace(/ /gu, " ");

/** The numbered citation an input's source has in this answer, if it has one. */
export function citationNumber(evidence: ChatEvidenceRef, evidenceRefs: readonly ChatEvidenceRef[]) {
  const index = citationSources(evidenceRefs).findIndex((item) => item.id === evidence.id);
  return index < 0 ? null : index + 1;
}

/** The inputs' sources as "OpenDART, 2026-03-18", for copied formulas and exports. */
function inputSources(check: NumberCheck, evidenceRefs: readonly ChatEvidenceRef[]) {
  const seen = new Set<string>();
  const sources: string[] = [];
  for (const input of check.calc?.inputs ?? []) {
    const evidence = evidenceRefs.find((item) => item.id === input.evidence_id);
    if (!evidence || seen.has(evidence.id)) continue;
    seen.add(evidence.id);
    const origin = evidenceProvenance(evidence, providerName(evidence.provider));
    const name = !evidence.memory_origin && evidence.kind === "local_file" ? evidence.title.trim() || t("evidence.files") : origin.name;
    sources.push([name, evidence.memory_origin ? origin.publication?.label : origin.sourceDay, origin.route].filter(Boolean).join(", "));
  }
  return sources.join("; ");
}

/** What "Copy formula" copies: "{title}: {formula lines} (calculated in Loupist; inputs: …)". */
export function copiedFormula(check: NumberCheck, evidenceRefs: readonly ChatEvidenceRef[]) {
  const calc = check.calc;
  if (!calc) return check.text;
  const sources = inputSources(check, evidenceRefs);
  return `${calc.title}: ${calc.formulas.join("; ")} (${t("numberChip.exportCalculated", { inputs: sources || t("numberChip.notLinked") })})`;
}

/** A Markdown footnote for a number link in an export (finance-show-the-math.md §3.3). */
export function exportFootnote(check: NumberCheck, evidence: ChatEvidenceRef | undefined, evidenceRefs: readonly ChatEvidenceRef[]) {
  if (check.calc) {
    const formula = check.calc.formulas[0] ?? "";
    const at = formula.indexOf(" = ");
    const equation = at < 0 ? formula : `${check.calc.title}${formula.slice(at)}`;
    const sources = inputSources(check, evidenceRefs);
    return `${equation} (${t("numberChip.exportCalculated", { inputs: sources || t("numberChip.notLinked") })})`;
  }
  if (!evidence) return check.text;
  const origin = evidenceProvenance(evidence);
  return evidence.memory_origin
    ? [origin.name, origin.publication?.label, origin.route, evidence.url].filter(Boolean).join(" — ")
    : [evidence.title, origin.sourceDay, evidence.url].filter(Boolean).join(" — ");
}
