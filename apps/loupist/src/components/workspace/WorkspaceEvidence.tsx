import { formatNumber } from "@/i18n/format";
import { formatDateTime } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import { useEffect, useRef, useState } from "react";
import { ExternalLinkIcon, FileTextIcon } from "lucide-react";
import type { ChatEvidenceRef } from "../../types";
import { evidenceCoverage, evidenceExcerptParts, evidencePresentation, evidenceTitle } from "../../chat/evidence";
import { useMemoryPreview } from "../../chat/memoryPreview";
import { parseCredentialFreeHttpUrl } from "../../lib/credentialFreeUrl";
import { Button } from "../ui/button";

const t = i18n.getFixedT(null, "chat");

/**
 * The matched data cell in a retained structured excerpt (`"value": "452312000000"`),
 * from a number check's field. Only the printed key and value are searched for;
 * nothing is recomputed.
 */
export function evidenceCellParts(excerpt: string, cell: { key: string; value: string } | undefined) {
  if (!cell) return null;
  const key = JSON.stringify(cell.key);
  for (const printed of [JSON.stringify(cell.value), cell.value]) {
    const needle = `${key}: ${printed}`;
    const at = excerpt.indexOf(needle);
    if (at >= 0) return { before: excerpt.slice(0, at), match: needle, after: excerpt.slice(at + needle.length) };
  }
  return null;
}

const readableTime = (value: string) => {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : formatDateTime(date);
};

/**
 * The retained copy of a citation, opened at its host-selected passage. The live page is
 * one click further (`onOpenLink` opens or reuses a browser tab in the panel).
 */
export function WorkspaceEvidence({ evidence, step, onRevealStep, onOpenLink }: {
  evidence: ChatEvidenceRef;
  step?: { label: string; itemId?: string; cell?: { pointer: string; key: string; value: string } };
  onRevealStep?: (itemId: string) => void;
  onOpenLink: (url: string) => void;
}) {
  useTranslation("chat");
  const reader = useRef<HTMLElement>(null);
  const match = useRef<HTMLElement>(null);
  const [awayFromPassage, setAwayFromPassage] = useState(false);
  const memoryText = useMemoryPreview(evidence);
  const sampleText = useMemoryPreview(evidence, evidence.model_delivery?.excerpt ?? "");
  // A number's "Open in Evidence" marks its data cell; a passage keeps its host-selected match.
  const excerptParts = memoryText === null ? evidenceExcerptParts(evidence) ?? evidenceCellParts(evidence.excerpt, step?.cell) : null;
  useEffect(() => {
    setAwayFromPassage(false);
    reader.current?.focus({ preventScroll: true });
    if (match.current) match.current.scrollIntoView({ block: "center" });
    else if (reader.current) reader.current.scrollTop = 0;
  }, [evidence.id, step?.cell?.pointer]);
  // "Jump to cited passage" is only useful once the reader has scrolled the passage away.
  const trackPassage = () => {
    const frame = reader.current?.getBoundingClientRect();
    const passage = match.current?.getBoundingClientRect();
    if (!frame || !passage) return;
    setAwayFromPassage(passage.bottom < frame.top || passage.top > frame.bottom);
  };
  const view = evidencePresentation(evidence);
  const title = evidenceTitle(evidence);
  const sourceUrl = evidence.url ? parseCredentialFreeHttpUrl(evidence.url) : null;
  const notRetained = !excerptParts && memoryText === null && !evidence.excerpt && (evidence.kind === "original" || evidence.kind === "local_file");
  return <article ref={reader} tabIndex={-1} className="workspace-evidence" aria-label={t("workspaceEvidence.sourceLabel", { title: title })} onScroll={excerptParts ? trackPassage : undefined}>
    <header>
      <div className="evidence-kind"><FileTextIcon aria-hidden="true" />{view.label}</div>
      <h2 title={evidence.title}>{title}</h2>
      <p>{view.description}</p>
    </header>
    {/* Sticky, so the live page and the way back to the passage stay one click away while reading. */}
    <div className="evidence-actions">
      {excerptParts && awayFromPassage ? <Button type="button" variant="ghost" size="sm" onClick={() => match.current?.scrollIntoView({ block: "center" })}>{t("workspaceEvidence.jumpToCitedPassage")}</Button> : null}
      {/* The status row above names the host; the button keeps the full address in its tooltip. */}
      {sourceUrl ? <Button type="button" variant="outline" size="sm" title={sourceUrl.href} onClick={() => onOpenLink(sourceUrl.href)}><ExternalLinkIcon />{t("workspaceEvidence.openLivePage")}</Button> : null}
    </div>
    {notRetained ? <div className="evidence-notices" role="alert"><p>{t("chatWorkspacePanel.sourceNotRetained")}</p></div> : null}
    <dl className="evidence-metadata">
      {step?.label ? <><dt>{t("workspaceEvidence.createdIn")}</dt><dd>{step.itemId && onRevealStep ? <Button type="button" variant="ghost" size="sm" onClick={() => onRevealStep(step.itemId!)}>{step.label}</Button> : step.label}</dd></> : null}
      {evidence.provider ? <><dt>{t("workspaceEvidence.provider")}</dt><dd>{evidence.provider}</dd></> : null}
      <dt>{t("workspaceEvidence.retrieved")}</dt><dd><time dateTime={evidence.retrieved_at}>{readableTime(evidence.retrieved_at)}</time></dd>
      {evidence.published_at ? <><dt>{t("workspaceEvidence.published")}</dt><dd>{readableTime(evidence.published_at)}</dd></> : null}
      {evidence.as_of ? <><dt>{t("workspaceEvidence.dataAsOf")}</dt><dd>{evidence.as_of}</dd></> : null}
      {evidence.coverage ? <><dt>{t("workspaceEvidence.coverage")}</dt><dd>{excerptParts ? t("workspaceEvidence.selectedPassageFromTheRetrievedDocument") : evidenceCoverage(evidence.coverage)}</dd></> : null}
    </dl>
    {evidence.warnings.length > 0 ? <div className="evidence-notices" role="note">{evidence.warnings.map((warning, index) => <p key={index}>{warning}</p>)}</div> : null}
    {evidence.model_delivery ? <p className="evidence-notices">{t("workspaceEvidence.theFullResultWasSavedLoupistReceivedASmaller")}</p> : null}
    {notRetained ? null : <section className="evidence-excerpt" data-quote-source={`evidence:${evidence.id}`}><h3>{evidence.kind === "discovery" ? t("workspaceEvidence.searchPreview") : t("workspaceEvidence.savedExcerpt")}</h3>
      <pre>{excerptParts ? <>{excerptParts.before}<mark ref={match} key={evidence.id} className="evidence-match">{excerptParts.match}</mark>{excerptParts.after}</> : (memoryText ?? evidence.excerpt) || t("workspaceEvidence.noTextExcerptWasReturnedForThisResult")}</pre>
      {evidence.excerpt_truncated ? <p className="evidence-truncation">{t(sourceUrl ? "workspaceEvidence.partialOriginal" : evidence.dataset_ref ? "workspaceEvidence.partialDataset" : "workspaceEvidence.partialRetained")}</p> : null}
    </section>}
    {evidence.model_delivery ? <details className="evidence-receipt"><summary>{t("workspaceEvidence.sampleReceivedByLoupist")}</summary>
      <pre>{sampleText ?? evidence.model_delivery.excerpt}</pre>
      {evidence.model_delivery.excerpt_truncated ? <p className="evidence-truncation">{t("workspaceEvidence.thisPreviewAlsoHasADisplayLimit")}</p> : null}
    </details> : null}
    {evidence.dataset_ref || evidence.upstream_evidence_ids?.length ? <details className="evidence-receipt"><summary>{t("workspaceEvidence.relatedData")}</summary><dl>
      {evidence.dataset_ref ? <><dt>{t("workspaceEvidence.dataset")}</dt><dd>{t("workspaceEvidence.datasetVersion", { version: evidence.dataset_ref.revision })}</dd></> : null}
      {evidence.upstream_evidence_ids?.length ? <><dt>{t("workspaceEvidence.inputResults")}</dt><dd>{t("workspaceEvidence.savedResultCount", { count: evidence.upstream_evidence_ids.length, formattedCount: formatNumber(evidence.upstream_evidence_ids.length) })}</dd></> : null}
    </dl></details> : null}
  </article>;
}
