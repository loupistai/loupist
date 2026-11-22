import { Disclosure } from "../ui/disclosure";
import { formatNumber } from "@/i18n/format";
import { formatDateTime } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import { useEffect, useId, useRef } from "react";
import { ChevronDownIcon, DatabaseIcon, FileTextIcon, WrenchIcon } from "lucide-react";
import { comparisonPassage } from "../../chat/numberChecks";
import type { ChatEvidenceRef, NumberCheck } from "../../types";
import { evidenceWarning, evidenceCoverage, evidencePresentation, evidenceProvenance, evidenceTitle, executionEvidenceType } from "../../chat/evidence";
import { CopyTextButton } from "../ui/copy-text-button";
import { providerName } from "@/lib/marketProviderName";

const t = i18n.getFixedT(null, "chat");

/** Retained intermediate output stays beside the steps that produced it. */
export function ChatToolResult({ evidence, expanded, onToggle, focusRequest, comparison }: {
  evidence: ChatEvidenceRef;
  expanded: boolean;
  onToggle: () => void;
  focusRequest?: number;
  comparison?: NumberCheck;
}) {
  useTranslation("chat");
  const button = useRef<HTMLButtonElement>(null);
  const mark = useRef<HTMLElement>(null);
  const passage = expanded ? comparisonPassage(evidence.excerpt, comparison) : null;
  const detailsId = useId();
  const type = executionEvidenceType(evidence);
  const Icon = { dataset: DatabaseIcon, artifact: FileTextIcon, tool: WrenchIcon }[type];
  useEffect(() => {
    if (focusRequest === undefined) return;
    button.current?.focus({ preventScroll: true });
    (mark.current ?? button.current)?.scrollIntoView?.({ block: "nearest" });
  }, [focusRequest, expanded]);
  const origin = evidenceProvenance(evidence, providerName(evidence.provider) ?? "");
  return <div className="chat-tool-result" data-result-type={type}>
    <button ref={button} type="button" className="chat-result-toggle" aria-label={t("chatToolResult.inspectResult", { value1: evidenceTitle(evidence) })}
      aria-expanded={expanded} aria-controls={detailsId} onClick={onToggle}>
      <Icon aria-hidden="true" /><span>{evidenceTitle(evidence)}</span><small>{evidencePresentation(evidence).label}</small>
      <ChevronDownIcon className={`chat-progress-chevron ${expanded ? "open" : ""}`} aria-hidden="true" />
    </button>
    {expanded ? <div id={detailsId} className="chat-execution-detail chat-tool-output" aria-label={t("chatToolResult.toolResult")}>
      <div className="reader-section-heading"><h3>{t("chatToolResult.output")}</h3>{evidence.excerpt ? <CopyTextButton text={evidence.excerpt} label={t("chatToolResult.outputLabel")} /> : null}</div>
      {passage && <pre aria-label={t("figureWarning.result")}>{passage.before}<mark ref={mark}>{passage.match}</mark>{passage.after}</pre>}
      <pre tabIndex={0} aria-label={t("chatToolResult.resultOutput")}>{evidence.excerpt || t("chatToolResult.noTextOutputWasRetained")}</pre>
      {evidence.excerpt_truncated ? <p className="reader-notice">{t(evidence.dataset_ref ? "chatToolResult.limitedDataset" : "chatToolResult.limitedRetained")}</p> : null}
      {evidence.warnings.map((warning, index) => <p key={index} className="reader-notice">{evidenceWarning(evidence.tool_name, warning)}</p>)}
      {evidence.model_delivery ? <Disclosure title={<>{t("chatToolResult.sampleReceivedByLoupist")}</>}><pre>{evidence.model_delivery.excerpt}</pre>{evidence.model_delivery.excerpt_truncated ? <p className="reader-notice">{t("chatToolResult.thisSampleAlsoHasADisplayLimit")}</p> : null}</Disclosure> : null}
      <Disclosure className="reader-technical-details" title={<>{t("chatToolResult.resultDetails")}</>}><dl className="reader-facts">
        {!evidence.memory_origin ? <div><dt>{t("chatToolResult.recorded")}</dt><dd><time dateTime={evidence.retrieved_at}>{formatDateTime(new Date(evidence.retrieved_at))}</time></dd></div> : null}
        {origin.attribution ? <div><dt>{t("chatToolResult.provider")}</dt><dd>{origin.attribution}</dd></div> : null}
        {evidence.as_of ? <div><dt>{t("chatToolResult.dataAsOf")}</dt><dd>{evidence.as_of}</dd></div> : null}
        {origin.publication ? <div><dt>{t(origin.publication.kind === "updated" ? "chatToolResult.updated" : "chatToolResult.published")}</dt>
          <dd>{evidence.memory_origin ? origin.publication.date : evidence.published_at}</dd></div> : null}
        {evidence.coverage ? <div><dt>{t("chatToolResult.coverage")}</dt><dd>{evidenceCoverage(evidence.coverage)}</dd></div> : null}
        {evidence.upstream_evidence_ids?.length ? <div><dt>{t("chatToolResult.inputResults")}</dt><dd>{t("chatToolResult.savedResultCount", { count: evidence.upstream_evidence_ids.length, formattedCount: formatNumber(evidence.upstream_evidence_ids.length) })}</dd></div> : null}
        {evidence.dataset_ref ? <div><dt>{t("chatToolResult.dataset")}</dt><dd>{t("chatToolResult.datasetVersion", { version: evidence.dataset_ref.revision })}</dd></div> : null}
      </dl>{origin.route ? <p className="reader-notice">{origin.route}</p> : null}</Disclosure>
    </div> : null}
  </div>;
}
