import i18n, { useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import "@/styles/sensitivity.css";
import { TriangleAlertIcon } from "lucide-react";
import { Popover } from "radix-ui";
import { useEffect, useId, useState, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { DialogTitle } from "@/components/ui/dialog";
import type { ChatEvidenceRef, NumberCheck, SensitivityGrid } from "../../types";
import { formatAsOf } from "../../chat/asOf";
import { citationLabel, evidencePresentation, evidenceTitle, sourceIdentityLabel } from "../../chat/evidence";
import { citationNumber, needsChecking } from "../../chat/numberChecks";
import { baseCell, cardDay, cellText, changePrompt, discountRate, growthRate, inputLabel, inputValue, outsideCount, priceText, sensitivityTsv, shaded } from "../../chat/sensitivity";
import { CitationChip } from "./CitationChip";
import { NumberChip, type NumberChipActions } from "./NumberChip";

const t = i18n.getFixedT(null, "chat");
/** A private-use marker that finds the reference value inside its translated sentence. */
const VALUE = "";

/** The source of one number on the card: its numbered citation, opening the evidence. */
function SourceChip({ evidence, evidenceRefs, onOpen }: { evidence: ChatEvidenceRef; evidenceRefs: readonly ChatEvidenceRef[]; onOpen?: NumberChipActions["onOpenEvidence"] }) {
  const number = citationNumber(evidence, evidenceRefs);
  if (number === null || !onOpen) return null;
  return <CitationChip evidence={evidence} label={citationLabel(evidence, evidenceRefs)} title={evidenceTitle(evidence)} source={sourceIdentityLabel(evidence) || evidencePresentation(evidence).label} onOpen={(anchor) => onOpen(evidence, undefined, anchor)}>{number}</CitationChip>;
}

/** A cell reverse_dcf could not solve: "—", with why on hover or focus. */
function NullCell({ spoken }: { spoken: string }) {
  useTranslation("chat");
  const [open, setOpen] = useState(false);
  return <Popover.Root open={open} onOpenChange={setOpen}>
    <Popover.Trigger asChild>
      <button type="button" className="sensitivity-null" aria-label={spoken}
        onPointerEnter={() => setOpen(true)} onPointerLeave={() => setOpen(false)} onFocus={() => setOpen(true)} onBlur={() => setOpen(false)}>—</button>
    </Popover.Trigger>
    <Popover.Portal>
      <Popover.Content role="tooltip" className="sensitivity-note" side="top" align="center" sideOffset={4} onOpenAutoFocus={(event) => event.preventDefault()}>{t("sensitivity.outside")}</Popover.Content>
    </Popover.Portal>
  </Popover.Root>;
}

export type SensitivityCardProps = {
  grid: SensitivityGrid;
  /** The company or asset the grid is about. */
  subject: string;
  /** In an answer: the grid's own result, for "How this was calculated". */
  evidence?: ChatEvidenceRef;
  /** In an answer: the answer's evidence, so inputs and the price show their sources. */
  evidenceRefs?: readonly ChatEvidenceRef[];
  actions?: NumberChipActions;
  /** An as-of answer's date: the card says "Known as of". */
  asOf?: string;
  /** Fills the composer with the rates to change; never sends. */
  onChangeRates?: (prompt: string) => void;
  /** In the results card's dialog the card's title is the dialog's title. */
  variant?: "answer" | "dialog";
  /**
   * Source chips from outside an answer (the results card numbers its own
   * sources): beside the price in the title and beside named inputs.
   */
  sources?: { price?: ReactNode; inputs?: Partial<Record<string, ReactNode>> };
};

/**
 * What the price implies, or value per share, over discount rates × long-run
 * growth (finance-korea-retail-disagreement.md §2.2). Never one number: the
 * base case is outlined, and reverse-DCF cells that need more growth than the
 * reference are shaded. Every value is Rust's own calculation.
 */
export function SensitivityCard({ grid, subject, evidence, evidenceRefs = [], actions = {}, asOf, onChangeRates, variant = "answer", sources }: SensitivityCardProps) {
  useTranslation("chat");
  const titleId = useId();
  const formulasId = useId();
  const [copied, setCopied] = useState(false);
  const [formulasOpen, setFormulasOpen] = useState(false);
  useEffect(() => {
    if (!copied) return undefined;
    const handle = window.setTimeout(() => setCopied(false), 2000);
    return () => window.clearTimeout(handle);
  }, [copied]);

  const reverse = grid.operation === "reverse_dcf";
  const reference = reverse ? grid.reference : undefined;
  const find = (id?: string) => (id ? evidenceRefs.find((item) => item.id === id) : undefined);
  const price = grid.price;
  const title = reverse
    ? price?.date
      ? t("sensitivity.title", { name: subject, price: priceText(price), date: cardDay(price.date) })
      : t("sensitivity.titleUndated", { name: subject, price: price ? priceText(price) : "—" })
    : t("sensitivity.valueTitle");
  const subtitle = reverse ? t("sensitivity.subtitle", { years: formatNumber(grid.forecast_years) }) : subject;
  const priceSource = reverse ? find(price?.evidence_id) : undefined;
  const priceChip = priceSource
    ? <SourceChip evidence={priceSource} evidenceRefs={evidenceRefs} onOpen={actions.onOpenEvidence} />
    : reverse ? sources?.price : undefined;
  // The price, its date and its source stay together on one line ("$41.20 on Oct 21 [1]" / "10월 21일 US$41.20").
  const titleParts = title.split(" · ");
  const titleEnd = reverse && titleParts.length > 1 ? titleParts.pop()! : undefined;
  const [baseRow, baseColumn] = baseCell(grid);
  const outside = outsideCount(grid);
  const discount = t("sensitivity.discount");
  const longRun = t("sensitivity.longRun");
  const needsMore = reference ? (reference.years ? t("sensitivity.needsMore", { years: formatNumber(reference.years) }) : t("sensitivity.needsMoreShort")) : "";
  const inputs = grid.inputs.filter((input) => input.name !== "price");
  // Show step reveals the producing step in the thread; without a thread (the results dialog) the formula opens here.
  const showStep = evidence && actions.onShowStep ? () => actions.onShowStep!(evidence) : undefined;
  const Title = variant === "dialog" ? DialogTitle : "h3";

  const copy = () => { void navigator.clipboard?.writeText(sensitivityTsv(grid)).then(() => setCopied(true), () => undefined); };

  let legend: ReactNode = null;
  if (reference) {
    const value = growthRate(reference.value_pct);
    const values = { value: VALUE, period: reference.label ?? "" };
    // Without a period label the parenthesis keeps only the yearly value.
    const sentence = (reference.years ? t("sensitivity.reference", { ...values, years: formatNumber(reference.years) }) : t("sensitivity.referenceShort", values)).replace(/,\s*\)/u, ")");
    const [before, after = ""] = sentence.split(VALUE);
    const source = find(reference.evidence_id);
    const check: NumberCheck | undefined = source ? { link_index: -1, text: value, evidence_id: source.id, kind: "step", status: reference.status ?? "matched" } : undefined;
    legend = <p className="sensitivity-legend">
      <span className="sensitivity-swatch" aria-hidden="true" />
      <span>{before}{check && source
        ? <NumberChip check={check} evidence={source} evidenceRefs={evidenceRefs} asOf={asOf} actions={actions}>{value}</NumberChip>
        : <span className="gg-num">{value}</span>}{after}</span>
    </p>;
  }

  return <section className="sensitivity-card" data-operation={grid.operation} data-reference={reference ? true : undefined} data-variant={variant} aria-labelledby={variant === "dialog" ? undefined : titleId}>
    <header className="sensitivity-header">
      <div className="sensitivity-heading">
        {/* In the dialog Radix names the title itself; an `id` prop would replace the one the dialog points to. */}
        <Title {...(variant === "dialog" ? {} : { id: titleId })} className="sensitivity-title">
          {titleEnd === undefined ? title : <>{titleParts.join(" · ")} · <span className="sensitivity-title-price">{titleEnd}{priceChip ? <> {priceChip}</> : null}</span></>}
        </Title>
        {asOf ? <p className="sensitivity-meta">{t("grid.knownAsOf", { date: formatAsOf(asOf) })}</p> : null}
        <p className="sensitivity-meta">{subtitle}</p>
      </div>
      <Button type="button" variant="ghost" size="sm" className="sensitivity-copy" onClick={copy}>{copied ? t("grid.copied") : t("sensitivity.copy")}</Button>
    </header>
    <p className="sensitivity-axes" aria-hidden="true">{discount} ↓ · {longRun} →</p>
    {/* Where "Discount rate" first appears, a plain line says what it is. */}
    <p className="sensitivity-hint">{t("sensitivity.discountHint")}</p>
    <div className="sensitivity-scroll" role="region" tabIndex={0} aria-label={t("grid.scroll")}>
      <table className="sensitivity-table">
        <caption className="sr-only">{title}</caption>
        <thead>
          <tr>
            <td className="sensitivity-corner" aria-hidden="true"><span className="sensitivity-axis-label">{longRun} →</span></td>
            {grid.column_values.map((growth, column) => <th key={column} scope="col" className="gg-num"><span className="sr-only">{longRun} </span>{growthRate(growth)}</th>)}
          </tr>
        </thead>
        <tbody>
          {grid.row_values.map((rate, row) => <tr key={row}>
            <th scope="row"><span className={row === 0 ? "sensitivity-axis-label" : "sr-only"}>{discount} </span><span className="gg-num">{discountRate(rate)}</span></th>
            {grid.column_values.map((_, column) => {
              const value = grid.values[row]?.[column] ?? null;
              const above = shaded(grid, row, column);
              const base = row === baseRow && column === baseColumn;
              return <td key={column} className="sensitivity-cell gg-num" data-above={above || undefined} data-base={base || undefined} data-null={value === null || undefined}>
                {value === null
                  ? <NullCell spoken={[t("sensitivity.outside"), base ? t("sensitivity.baseCase") : ""].filter(Boolean).join(", ")} />
                  : <>{cellText(grid, value)}{above ? <span className="sr-only">, {needsMore}</span> : null}{base ? <span className="sr-only">, {t("sensitivity.baseCase")}</span> : null}</>}
              </td>;
            })}
          </tr>)}
        </tbody>
      </table>
    </div>
    {outside ? <p className="sensitivity-caption">{t("sensitivity.outsideCount", { count: outside, formattedCount: formatNumber(outside) })}</p> : null}
    {legend}
    {inputs.length ? <p className="sensitivity-inputs">
      <span className="sensitivity-inputs-label">{t("numberChip.inputs")}</span>
      {inputs.map((input, index) => {
        const source = find(input.evidence_id);
        const flagged = input.status ? needsChecking({ status: input.status }) : false;
        return <span key={input.name} className="sensitivity-input" data-status={input.status}>
          {index ? <span aria-hidden="true"> · </span> : null}
          {/* A share count names itself ("34.6m shares"); a label the calculation was given still shows. */}
          {input.name === "shares" && !input.label ? null : <><span>{inputLabel(input)}</span> </>}<span className="gg-num">{inputValue(grid, input)}</span>
          {flagged ? <TriangleAlertIcon className="sensitivity-input-flag" role="img" aria-label={t("numberChip.notFound")} /> : null}
          {source ? <> <SourceChip evidence={source} evidenceRefs={evidenceRefs} onOpen={actions.onOpenEvidence} /></>
            : sources?.inputs?.[input.name] ? <> {sources.inputs[input.name]}</> : null}
        </span>;
      })}
    </p> : null}
    <div className="sensitivity-foot">
      <span>{t("sensitivity.notForecast")}</span>
      {showStep || grid.formulas.length ? <button type="button" className="sensitivity-how" aria-expanded={showStep ? undefined : formulasOpen} aria-controls={showStep || !formulasOpen ? undefined : formulasId}
        onClick={() => (showStep ? showStep() : setFormulasOpen((open) => !open))}>{t("sensitivity.how")}</button> : null}
      {onChangeRates ? <>
        <span aria-hidden="true">·</span>
        <Button type="button" variant="ghost" size="sm" className="sensitivity-change" aria-label={`${t("sensitivity.change")} · ${subject}`} onClick={() => onChangeRates(changePrompt(grid))}>{t("sensitivity.change")}</Button>
      </> : null}
    </div>
    {formulasOpen && !showStep ? <div id={formulasId} className="sensitivity-formulas">{grid.formulas.map((line, index) => <p key={index}>{line}</p>)}</div> : null}
  </section>;
}
