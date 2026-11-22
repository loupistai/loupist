import { useRef, useState, type ReactElement } from "react";
import i18n, { useTranslation } from "@/i18n";
import { scaleLabel } from "../../chat/numberText";
import { currencyName } from "@/i18n/format";
import type { ChatEvidenceRef, ChatMessage, NumberCheck } from "../../types";
import { calculationCheck, needsChecking } from "../../chat/numberChecks";
import { currencyOfText } from "../../chat/numberText";
import { comparisonSourceTitle, evidenceProvenance } from "../../chat/evidence";
import { Button } from "../ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";
import { useOpenProps } from "../navigation/Open";

const t = i18n.getFixedT(null, "chat");

export function comparisonTitle(check: NumberCheck, evidence?: ChatEvidenceRef) {
  const replay = check.comparisonReplay;
  if (replay?.reason === "no_candidate" && replay.searched.title.trim()) return replay.searched.title;
  if (calculationCheck(check, evidence)) return t("figureWarning.calculation");
  if (!evidence) return t("workspaceEvidence.kind.source");
  return comparisonSourceTitle(evidence);
}

function currencyMismatch(check: NumberCheck) {
  if (check.comparisonReason !== "incompatible_unit") return undefined;
  const source = currencyOfText(check.nearestSourceUnit ?? "", true);
  const answer = currencyOfText(check.text, true);
  if (!source || !answer || source === answer) return undefined;
  return t("numberChip.incompatibleCurrency", { sourceCurrency: currencyName(source), answerCurrency: currencyName(answer) });
}

export function replayWarning(check: NumberCheck) {
  switch (check.comparisonReplay?.reason) {
    case "no_candidate": return t("figureWarning.replay.retainedPassage");
    case "evidence_unavailable": return t("figureWarning.replay.evidenceUnavailable");
    default: return undefined;
  }
}

export function warningReason(check: NumberCheck, calculation: boolean) {
  const replay = replayWarning(check);
  if (replay) return replay;
  if (check.status === "differs") return t("numberChip.differs", { nearest: check.nearest ?? "", shown: check.text });
  const reason = check.comparisonReason;
  if (!reason) return calculation ? t("figureWarning.legacyStep") : t("numberChip.notFound");
  return currencyMismatch(check) ?? (calculation && reason === "no_candidate" ? t("figureWarning.missingStep") : t(`figureWarning.reasons.${reason}`));
}

const unitLabels = {
  "$": "dollars", "£": "pounds", "₩": "won", "€": "euros", "¥": "yenOrYuan",
  shares: "shares", per_share: "perShare", index: "index", "%": "percent",
  pp: "points", bp: "bp", multiple: "multiple",
} as const;

function units(check: NumberCheck) {
  const unit = check.nearestSourceUnit;
  let name: string | undefined;
  if (unit && Object.hasOwn(unitLabels, unit)) {
    name = t(`figureWarning.units.${unitLabels[unit as keyof typeof unitLabels]}`);
  } else if (unit && /^[A-Z]{3}$/u.test(unit)) {
    name = new Intl.DisplayNames([i18n.language], { type: "currency" }).of(unit);
  }
  const scale = check.nearestSourceScale;
  const scaleName = scale === undefined ? undefined : scaleLabel(scale);
  return [name, scaleName].filter(Boolean).join(" · ");
}

function WarningValues({ check, calculation }: { check: NumberCheck; calculation: boolean }) {
  const candidate = check.comparisonReason && check.comparisonReason !== "no_candidate" ? check.comparedValue : undefined;
  const metadata = candidate ? units(check) : "";
  return <dl className="figure-warning-values"><div><dt>{t("figureWarning.answer")}</dt><dd>{check.text}</dd></div>
      {candidate && <div><dt>{calculation ? t("figureWarning.result") : t("figureWarning.source")}</dt>
        <dd>{candidate}</dd>{metadata && <dd className="figure-warning-units">{metadata}</dd>}</div>}</dl>;
}

export function WarningDetails({ check, calculation }: { check: NumberCheck; calculation: boolean }) {
  return <>
    <WarningValues check={check} calculation={calculation} />
    <p className="number-chip-status" data-status="not_found">{warningReason(check, calculation)}</p>
  </>;
}

export function WarningAction({ check, evidence, close }: { check: NumberCheck; evidence: ChatEvidenceRef; close: () => void }) {
  const link = useOpenProps({ kind: "evidence", evidence, step: { label: "", comparison: check } });
  if (check.comparisonReplay?.reason === "evidence_unavailable") return null;
  return <Button type="button" variant="ghost" size="sm" disabled={!link.available} onClick={event => { link.activate(event); close(); }}>
    {calculationCheck(check, evidence) ? t("figureWarning.openStep") : t("figureWarning.openSource")}
  </Button>;
}

export function FigureWarnings({ message, label, children }: { message: Pick<ChatMessage, "number_checks" | "evidence_refs">; label: string; children: ReactElement }) {
  useTranslation("chat");
  const [open, setOpen] = useState(false);
  const following = useRef(false);
  return <Popover open={open} onOpenChange={setOpen}>
    <PopoverTrigger asChild>{children}</PopoverTrigger>
    <PopoverContent aria-label={label} className="figure-warning-list" onCloseAutoFocus={event => {
      if (following.current) event.preventDefault();
      following.current = false;
    }}>
      <p>{t("figureWarning.explanation")}</p>
      <ul>{message.number_checks?.filter(needsChecking).map(check => {
        const evidence = message.evidence_refs?.find(item => item.id === check.evidence_id);
        const calculation = calculationCheck(check, evidence);
        const provenance = evidence?.memory_origin ? evidenceProvenance(evidence) : null;
        const meta = [provenance?.attribution, provenance?.publication?.label].filter(Boolean).join(" · ");
        return <li key={check.link_index}>
          <strong>{comparisonTitle(check, evidence)}</strong>
          {meta ? <small className="number-chip-meta">{meta}</small> : null}
          {provenance?.route ? <small className="number-chip-meta">{provenance.route}</small> : null}
          <WarningValues check={check} calculation={calculation} />
          <span>{replayWarning(check) ?? currencyMismatch(check) ?? t(`figureWarning.short.${check.comparisonReason ?? (check.status === "differs" ? "digits_differ" : "legacy")}`)}</span>
          {evidence && <WarningAction check={check} evidence={evidence} close={() => { following.current = true; setOpen(false); }} />}
        </li>;
      })}</ul>
    </PopoverContent>
  </Popover>;
}
