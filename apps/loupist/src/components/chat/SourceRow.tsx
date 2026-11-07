import { useTranslation } from "@/i18n";
import { formatDate } from "@/i18n/format";
import type { ChatEvidenceRef } from "../../types";
import { evidencePresentation, evidenceTitle, sourceIdentityLabel } from "../../chat/evidence";
import { useOpenProps } from "../navigation/Open";
import { SourceFavicon } from "./SourceFavicon";

export function SourceRow({ evidence, number }: { evidence: ChatEvidenceRef; number: number }) {
  const { t } = useTranslation("chat");
  const link = useOpenProps({ kind: "evidence", evidence });
  const body = <>
    <span className="source-number">{number}</span><span><strong>{evidenceTitle(evidence)}</strong>
      <small className="source-site"><SourceFavicon evidence={evidence} /><span>{sourceIdentityLabel(evidence)} · {evidencePresentation(evidence).label}{evidence.retrieved_at ? ` · ${formatDate(new Date(evidence.retrieved_at), { dateStyle: "medium" })}` : ""}{evidence.excerpt_match ? t("chatMessageCard.citedPassage") : ""}</span></small>
    </span>
  </>;
  return <li>{link.available ? <button type="button" {...link.props}>{body}{link.icon}</button> : <div className="source-row-static">{body}</div>}</li>;
}
