import { useTranslation } from "@/i18n";
import type { ChatEvidenceRef } from "../../types";
import { evidencePresentation, evidenceTitle, sourceIdentityLabel } from "../../chat/evidence";
import { evidenceDate } from "../../chat/numberChecks";
import { useOpenProps } from "../navigation/Open";
import { SourceFavicon } from "./SourceFavicon";

export function SourceRow({ evidence, number }: { evidence: ChatEvidenceRef; number: number }) {
  const { t } = useTranslation("chat");
  const link = useOpenProps({ kind: "evidence", evidence });
  // The same date its citation card shows: when it was published, else when it was read.
  const date = evidenceDate(evidence);
  const body = <>
    <span className="source-number">{number}</span><span><strong>{evidenceTitle(evidence)}</strong>
      <small className="source-site"><SourceFavicon evidence={evidence} /><span>{sourceIdentityLabel(evidence)} · {evidencePresentation(evidence).label}{date ? ` · ${date}` : ""}{evidence.excerpt_match ? t("chatMessageCard.citedPassage") : ""}</span></small>
    </span>
  </>;
  return <li>{link.available ? <button type="button" {...link.props}>{body}{link.icon}</button> : <div className="source-row-static">{body}</div>}</li>;
}
