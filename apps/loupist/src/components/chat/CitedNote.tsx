import { useTranslation } from "@/i18n";
import type { ChatEvidenceRef } from "../../types";
import { citationLabel, citationNumber, evidencePresentation, evidenceTitle, sourceIdentityLabel } from "../../chat/evidence";
import { citedParts } from "../../chat/citedText";
import { CitationChip, UnavailableCitation } from "./CitationChip";
import { useTurn } from "./TurnProvider";

/**
 * A chart caption or a table note the model wrote, with each `#evidence-…`
 * target drawn as the answer draws a citation: its number when the source is
 * one of the answer's numbered sources, else the source's name. A target this
 * view cannot resolve reads "source" and opens nothing; the raw id never shows.
 */
export function CitedNote({ text, evidenceRefs }: { text: string; evidenceRefs?: readonly ChatEvidenceRef[] }) {
  const { t } = useTranslation("chat");
  const turn = useTurn();
  const parts = citedParts(text);
  if (parts.every((part) => "text" in part)) return <>{text}</>;
  const known = evidenceRefs?.length ? evidenceRefs : turn?.evidenceRefs ?? [];
  // Numbers follow the answer's own order, so a caption's 1 is the answer's 1.
  const numbering = turn?.evidenceRefs?.length ? turn.evidenceRefs : known;
  return <>{parts.map((part, index) => {
    if ("text" in part) return <span key={index}>{part.text}</span>;
    const evidence = known.find((item) => item.id === part.evidenceId);
    if (!evidence) return <UnavailableCitation key={index}>{part.label ?? t("citedNote.source")}</UnavailableCitation>;
    const number = citationNumber(evidence, numbering);
    const name = sourceIdentityLabel(evidence) || evidencePresentation(evidence).label;
    return <CitationChip key={index} evidence={evidence} label={citationLabel(evidence, numbering)} title={evidenceTitle(evidence)} source={name}
      to={{ kind: "evidence", evidence }}>{part.label ?? (number !== null ? String(number) : name)}</CitationChip>;
  })}</>;
}
