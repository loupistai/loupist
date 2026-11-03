import i18n from "@/i18n";
import type { Verdict } from "../../contracts/assets";
import { verdictLabel, verdictStep } from "./assetText";
import "@/styles/asset-rows.css";

/**
 * The Loupist's read on a thesis: a four-dot ladder glyph (inline status) and
 * the step's label. "Holding up" never reads as "you hold it".
 */
export function VerdictChip({ verdict, loupist }: { verdict: Verdict; loupist?: string }) {
  const step = verdictStep(verdict);
  const label = verdictLabel(verdict);
  return <span className="verdict-chip" data-verdict={verdict} role="img" aria-label={loupist ? i18n.t("assets:verdict.chipLabel", { loupist, verdict: label, step }) : i18n.t("assets:verdict.chipLabelPlain", { verdict: label, step })}>
    <span className="verdict-ladder" aria-hidden="true">{[1, 2, 3, 4].map(index => <i key={index} data-filled={index <= step || undefined} />)}</span>
    <span aria-hidden="true">{label}</span>
  </span>;
}
