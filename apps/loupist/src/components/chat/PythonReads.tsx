import type { ReadSummary } from "../../contracts/generated";
import { useTranslation } from "../../i18n";
import type { ChatEvidenceRef } from "../../types";
import { parseCredentialFreeHttpUrl } from "../../lib/credentialFreeUrl";
import { useGo } from "../navigation/Open";
import { humanizeToolName } from "../connections/custom/toolTitle";

export function PythonReads({ reads, evidence = [] }: { reads: readonly ReadSummary[]; evidence?: readonly ChatEvidenceRef[] }) {
  const { t } = useTranslation("chat");
  const go = useGo();
  if (!reads.length) return null;
  return <section aria-label={t("python.reads")}>
    <h4>{t("python.reads")}</h4>
    <ol className="activity-step-members">
      {reads.map((read, index) => {
        const title = humanizeToolName(read.tool);
        const source = evidence.find(source => read.citationUrl === `#evidence-${source.id}`);
        const url = read.citationUrl ? parseCredentialFreeHttpUrl(read.citationUrl) : null;
        return <li key={index}>
          {source && go ? <button type="button" className="activity-step-expand" onClick={() => go({ kind: "evidence", evidence: source })}>{title}</button>
            : url ? <a href={url.href} target="_blank" rel="noopener noreferrer">{title}</a> : title}
          <small>{[read.provider, read.asOf, read.status === "ok" ? null : read.status].filter(Boolean).join(" · ")}</small>
        </li>;
      })}
    </ol>
  </section>;
}
