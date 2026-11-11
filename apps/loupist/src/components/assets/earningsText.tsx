/**
 * Words and chips for results snapshots (finance-earnings-snapshot.md). Rust
 * computes every value, difference and chain state; this only formats them in
 * the user's language, values in their own currency. Shared by the asset page
 * card, the Thread message, briefings and the chat card.
 */
import "@/styles/answer-checks.css";
import i18n, { useTranslation } from "@/i18n";
import { formatAmount, formatAmountRange, formatNumber } from "@/i18n/format";
import { Popover } from "radix-ui";
import { useId, useState, type ReactNode } from "react";
import type { AssetEvidence } from "../../contracts/assets";
import type { EarningsAmount, EarningsCalc, EarningsDifference, EarningsMeasure, EarningsSnapshot, EarningsValue } from "../../contracts/earnings";
import { dartReport, shortDay } from "./assetText";
import { connectionText } from "../marketplace/connectionCopy";

const t = i18n.getFixedT(null, "assets");
/** Keys chosen at runtime from Rust-provided kinds. */
const tDynamic = t as unknown as (key: string, options?: Record<string, unknown>) => string;

/** The measure's name: a fixed key, else the run's own label. */
export function measureLabel(measure: Pick<EarningsMeasure, "key" | "label">) {
  return measure.key ? tDynamic(`earnings.measure.${measure.key}`) : measure.label ?? "";
}

/** A new-guidance row names the later period it guides ("Revenue · Q4 2026"). */
export const measureTitle = (measure: EarningsMeasure) => measure.guidance ? `${measureLabel(measure)} · ${measure.period}` : measureLabel(measure);

const currencyCode = (currency?: string) => currency && /^[A-Z]{3}$/.test(currency) ? currency : undefined;

/** Amounts keep a second decimal so a guidance range stays as stated ("$1.25bn"). */
const AMOUNT = { digits: 2 } as const;

/** One number in the value's unit and currency, never converted; amounts as grids print them. */
function one(value: number, amount: EarningsAmount) {
  const currency = currencyCode(amount.currency);
  switch (amount.unit) {
    case "percent": return `${formatNumber(value, { minimumFractionDigits: 1, maximumFractionDigits: 1 })}%`;
    case "per_share": return currency ? formatNumber(value, { style: "currency", currency, maximumFractionDigits: 2 }) : formatNumber(value, { maximumFractionDigits: 2 });
    default: return formatAmount(value, amount.unit === "currency" ? currency : undefined, AMOUNT);
  }
}

/** "$1.2–1.25bn", "16.0%", "₩115bn" / "1,150억 원" (`@/i18n/format`, like grids). */
export function amountText(amount: EarningsAmount) {
  if (amount.low !== undefined && amount.high !== undefined) {
    return amount.unit === "currency" || amount.unit === "number"
      ? formatAmountRange(amount.low, amount.high, amount.unit === "currency" ? currencyCode(amount.currency) : undefined, AMOUNT)
      : `${one(amount.low, amount)}–${one(amount.high, amount)}`;
  }
  const value = amount.point ?? amount.low ?? amount.high;
  return value === undefined ? "—" : one(value, amount);
}

const BASES: Record<string, "consolidated" | "separate" | "gaap" | "adjusted"> = {
  consolidated: "consolidated", cfs: "consolidated", separate: "separate", ofs: "separate", gaap: "gaap", adjusted: "adjusted", "non-gaap": "adjusted",
};

/** A measure's basis in the reader's words ("연결", "별도"); a basis the app has no word for stays as the source wrote it. */
export function basisLabel(basis: string | undefined) {
  if (!basis) return "";
  const key = BASES[basis.trim().toLowerCase()];
  return key ? tDynamic(`earnings.basis.${key}`) : basis;
}

/**
 * The form a filed step names after its date ("8-K", "fair disclosure"). A
 * Korean disclosure title is never shown in English; one that only repeats the
 * step (잠정실적 for Preliminary results) is left out.
 */
export function chainForm(form: string | undefined, stage: string) {
  if (!form) return "";
  if (/공정공시/u.test(form)) return t("earnings.fairDisclosure");
  const dart = dartReport(form);
  if (dart) return dart.form === tDynamic(`earnings.stage.${stage}`) || dart.form === tDynamic(`filingForm.${stage}`) ? "" : dart.form;
  return /[가-힣]/u.test(form) && !i18n.language.startsWith("ko") ? "" : form;
}

/** The source line under a value: who said it and when. */
export function sourceLine(value: EarningsValue, period?: string) {
  switch (value.kind) {
    case "guidance": return t("earnings.kind.guidance", { date: shortDay(value.factDate) });
    case "consensus": return t("earnings.kind.consensus", { provider: value.provider ?? "" });
    // A connection is named the way Connections names it ("한국투자증권").
    case "broker_estimate": return t("earnings.kind.broker", { provider: connectionText(value.provider ?? "") });
    case "implied": return t("earnings.implied");
    case "reported": {
      if (value.stage === "final") return t(period?.startsWith("FY") ? "earnings.stage.annual" : "earnings.stage.quarterly");
      return value.stage === "preliminary" && value.evidence.some(item => item.tool.startsWith("dart_")) ? t("earnings.stage.preliminary") : t("earnings.kind.release");
    }
  }
}

/** Words first ("Above the range"), then the chip-marked number; never "beat" or "miss". */
export function differenceParts(difference: EarningsDifference | undefined): { words?: string; calc?: EarningsCalc; numberText?: (display: string) => string; muted?: boolean } {
  if (!difference) return {};
  switch (difference.kind) {
    case "range": {
      const words = t(`earnings.diff.${difference.position}`);
      const outside = difference.position === "above" ? "earnings.diff.overTop" : difference.position === "below" ? "earnings.diff.underBottom" : undefined;
      return { words, calc: difference.calc, numberText: outside ? (display) => tDynamic(outside, { value: display }) : undefined };
    }
    case "change": return { calc: difference.calc };
    case "guidance": return { words: t(`earnings.diff.${difference.change}`), calc: difference.calc };
    case "not_comparable": return { words: t("earnings.diff.notComparable", { reason: t(`earnings.diff.reason.${difference.reason}`) }), muted: true };
  }
}

/** Every source on a snapshot, in reading order, so the page numbers them once. */
export function snapshotEvidence(snapshot: EarningsSnapshot, compact = false): AssetEvidence[][] {
  const release = snapshot.chain.find(step => step.stage === "release" || step.stage === "preliminary")?.evidence ?? [];
  return [
    release,
    ...snapshot.measures.flatMap(measure => [...measure.expected.map(value => value.evidence), ...measure.reported.map(value => value.evidence)]),
    ...(compact ? [] : [snapshot.implied?.evidence ?? [], snapshot.reaction?.evidence ?? [], snapshot.tone?.evidence ?? []]),
    ...snapshot.chain.map(step => step.evidence),
    ...snapshot.revisions.map(revision => revision.evidence),
  ];
}

/**
 * A number Rust calculated (percent change, points, the reverse-DCF range):
 * the number is the control and opens its formula and inputs.
 */
export function CalcChip({ calc, children, className, spoken }: { calc: EarningsCalc; children?: ReactNode; className?: string; /** The value as it should be spoken ("plus 12.3 billion won"), when the printed one reads poorly. */ spoken?: string }) {
  useTranslation("chat");
  const [open, setOpen] = useState(false);
  const titleId = useId();
  return <Popover.Root open={open} onOpenChange={setOpen}>
    <Popover.Trigger asChild>
      <button type="button" className={["number-chip earnings-calc gg-num", className].filter(Boolean).join(" ")} data-testid="number-chip" data-kind="calculated" data-status="matched"
        aria-haspopup="dialog" aria-label={i18n.t("chat:numberChip.labelCalculated", { value: spoken ?? (typeof children === "string" ? children : calc.display) })}>{children ?? calc.display}</button>
    </Popover.Trigger>
    <Popover.Portal>
      <Popover.Content role="dialog" aria-modal={false} aria-labelledby={titleId} className="number-chip-popover" data-kind="calculated" side="top" align="start" sideOffset={6}
        collisionPadding={{ top: 112, right: 8, bottom: 8, left: 8 }} onOpenAutoFocus={event => event.preventDefault()}>
        <div className="number-chip-head"><strong id={titleId}>{calc.title}</strong><span className="gg-num">{calc.display}</span></div>
        <div className="number-chip-formula">{calc.formulas.map((line, index) => <p key={index}>{line}</p>)}</div>
        {calc.inputs.length ? <>
          <p className="number-chip-caption">{i18n.t("chat:numberChip.inputs")}</p>
          <dl className="number-chip-inputs">{calc.inputs.map(input => <div key={input.name}><dt>{input.name.replaceAll("_", " ")}</dt><dd className="gg-num">{input.display}</dd></div>)}</dl>
        </> : null}
        <p className="number-chip-caption number-chip-here">{i18n.t("chat:numberChip.calculatedHere")}</p>
      </Popover.Content>
    </Popover.Portal>
  </Popover.Root>;
}
