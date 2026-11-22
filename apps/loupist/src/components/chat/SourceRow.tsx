import { useTranslation } from "@/i18n";
import { answerSourceRow, type AnswerSource } from "../../chat/answerSources";
import { evidencePresentation } from "../../chat/evidence";
import { useOpenProps } from "../navigation/Open";
import { SourceFavicon } from "./SourceFavicon";

export function SourceRow({ source, number, loupistId }: { source: AnswerSource; number: number; loupistId: string }) {
  const { t } = useTranslation("chat");
  const { evidence } = source;
  const { title, to, citations, provenance } = answerSourceRow(source, loupistId);
  const link = useOpenProps(to);
  const detail = evidence.memory_origin ? (provenance.attribution ? citations : null) : citations ?? evidencePresentation(evidence).label;
  const meta = [provenance.attribution, detail, provenance.dateLabel].filter(Boolean).join(" · ");
  const body = <>
    <span className="source-number">{number}</span><span><strong>{title}</strong>
      {meta ? <small className="source-site"><SourceFavicon evidence={evidence} /><span>{meta}{evidence.excerpt_match ? t("chatMessageCard.citedPassage") : ""}</span></small> : null}
      {provenance.route ? <small className="source-site">{provenance.route}</small> : null}
    </span>
  </>;
  return <li>{link.available ? <button type="button" {...link.props}>{body}{link.icon}</button> : <div className="source-row-static">{body}</div>}</li>;
}
