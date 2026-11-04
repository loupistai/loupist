import i18n from "@/i18n";
import { formatDate, formatNumber } from "@/i18n/format";
import type { ChatEvidenceRef, ChatMessage, NumberCheck } from "../types";
import { citationSources, sourceIdentityLabel } from "./evidence";

const t = i18n.getFixedT(null, "chat");

/**
 * Presentation only, while an answer streams: a link whose whole text reads
 * as one number with its unit gets the pending underline. Rust decides which
 * links are numbers when the answer finishes; the renderer never rechecks.
 */
const NUMBER_TEXT = /^[+\-−–]?(?:(?:[A-Za-z]{1,2})?[$₩€£¥￦]|[A-Z]{3}\s)?\s?[\-−]?\d[\d,]*(?:\.\d+)?(?:\s?(?:조|억|만|천)(?:\s?\d[\d,]*(?:\.\d+)?)?)*\s?(?:%p|%pt|%|pp|bps?|x|×|배|[kmbt]n?|mn|bn|tn|thousand|million|billion|trillion|percent|원|달러|엔|유로|krw|usd|eur|jpy|won|dollars?)?(?:\s?(?:원|달러|krw|usd|won|dollars?))?$/iu;

export function looksLikeNumber(text: string): boolean {
  const value = text.trim();
  if (!NUMBER_TEXT.test(value)) return false;
  // Years and small counts are labels or numbered citations, never number chips.
  return !/^\d{1,3}$/u.test(value) && !/^(?:19|20)\d{2}$/u.test(value);
}

const normalized = (text: string) => text.replace(/[\s ,]/gu, "");

/** The check Rust stored for this link, matched by its evidence and printed text. */
export function checkFor(checks: readonly NumberCheck[] | undefined, evidenceId: string, text: string) {
  const printed = normalized(text);
  return checks?.find((check) => check.evidence_id === evidenceId && normalized(check.text) === printed);
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

/** The evidence date to show: publication when known, otherwise retrieval. It never wraps inside. */
export function evidenceDate(evidence: ChatEvidenceRef | undefined, field: "published_at" | "retrieved_at" = "published_at") {
  const value = field === "published_at" ? evidence?.published_at ?? evidence?.retrieved_at : evidence?.retrieved_at;
  const time = value ? Date.parse(value) : Number.NaN;
  if (!Number.isFinite(time)) return "";
  return unbroken(formatDate(time, /^\d{4}-\d{2}-\d{2}$/u.test(value ?? "") ? { timeZone: "UTC" } : {}));
}

/** The numbered citation an input's source has in this answer, if it has one. */
export function citationNumber(evidence: ChatEvidenceRef, evidenceRefs: readonly ChatEvidenceRef[]) {
  const index = citationSources(evidenceRefs).findIndex((item) => item.id === evidence.id);
  return index < 0 ? null : index + 1;
}

const isoDay = (evidence: ChatEvidenceRef) => (evidence.published_at ?? evidence.retrieved_at ?? "").slice(0, 10);

/** The inputs' sources as "OpenDART, 2026-03-18", for copied formulas and exports. */
function inputSources(check: NumberCheck, evidenceRefs: readonly ChatEvidenceRef[]) {
  const seen = new Set<string>();
  const sources: string[] = [];
  for (const input of check.calc?.inputs ?? []) {
    const evidence = evidenceRefs.find((item) => item.id === input.evidence_id);
    if (!evidence || seen.has(evidence.id)) continue;
    seen.add(evidence.id);
    sources.push([evidence.provider ?? sourceIdentityLabel(evidence), isoDay(evidence)].filter(Boolean).join(", "));
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
  return [evidence.title, isoDay(evidence), evidence.url].filter(Boolean).join(" — ");
}
