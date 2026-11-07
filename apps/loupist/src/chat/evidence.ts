import i18n from "@/i18n";
import type { ChatEvidenceRef } from "../types";
import { parseCredentialFreeHttpUrl } from "../lib/credentialFreeUrl";
import { operationName } from "./numberText";

const t = i18n.getFixedT(null, "chat");

/** Host tool name, without a connection prefix (`server__web_fetch`). */
const toolLeaf = (tool: string) => tool.split("__").at(-1) ?? tool;

/**
 * A page the host read from the web. `web_fetch` and other `web_*` page tools count;
 * search previews, finance, files and quote baskets do not, even with a source address.
 * The address is host metadata (`url` only). A title that looks like a link is not one.
 * The tool name is the only check, so a quote excerpt is never parsed on this path.
 */
const isWebsitePage = (evidence: ChatEvidenceRef) => {
  if (evidence.kind !== "original") return false;
  const tool = toolLeaf(evidence.tool_name);
  return tool === "web_fetch" || (tool.startsWith("web_") && tool !== "web_search");
};

/** Favicon and original page for a retrieved website. Other tools keep their own reader. */
export function webEvidenceUrl(evidence: ChatEvidenceRef): string | null {
  if (!isWebsitePage(evidence) || !evidence.url) return null;
  return parseCredentialFreeHttpUrl(evidence.url)?.href ?? null;
}

/**
 * Page a citation opens first: a website the host read, or an original whose text was not
 * retained (a host post's source), where the reader would only say so.
 */
export function citationPageUrl(evidence: ChatEvidenceRef): string | null {
  const notRetained = evidence.kind === "original" && !evidence.excerpt && evidence.url;
  return webEvidenceUrl(evidence) ?? (notRetained ? parseCredentialFreeHttpUrl(evidence.url!)?.href ?? null : null);
}

/** Execution output belongs to the work trace, even when an answer cites it. */
const isExecutionEvidence = (evidence: ChatEvidenceRef) =>
  evidence.kind === "derived" || evidence.tool_name === "dataset_query" || evidence.tool_name === "dataset_describe";

const MEMORY_RECEIPT_TOOLS = new Set(["world_search", "world_read", "lens_search", "lens_read", "conversation_memory"]);

/** A Memory tool receipt is not an external citation. */
const isMemoryReceipt = (evidence: ChatEvidenceRef) =>
  evidence.kind === "saved_context" && MEMORY_RECEIPT_TOOLS.has(evidence.tool_name);

/** Numbered citations are retrieved originals, not search previews or work receipts. */
export const isCitationSource = (evidence: ChatEvidenceRef) =>
  evidence.kind !== "discovery" && !isExecutionEvidence(evidence) && !isMemoryReceipt(evidence);

/** One row per cited subject, in evidence order, excluding Memory receipts and execution output. */
export function citationSources(evidence: readonly ChatEvidenceRef[]): ChatEvidenceRef[] {
  const seen = new Set<string>();
  return evidence.filter((item) => {
    if (!isCitationSource(item)) return false;
    const key = evidenceSubjectKey(item);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

/** Source number shared by the answer citation and its inline sources list. */
export function citationNumber(evidence: ChatEvidenceRef, evidenceRefs: readonly ChatEvidenceRef[]): number | null {
  if (!isCitationSource(evidence)) return null;
  const index = citationSources(evidenceRefs).findIndex((item) => evidenceSubjectKey(item) === evidenceSubjectKey(evidence));
  return index < 0 ? null : index + 1;
}

export function citationLabel(evidence: ChatEvidenceRef, evidenceRefs: readonly ChatEvidenceRef[], memoryTitle?: string): string {
  const title = evidenceTitle(evidence);
  const number = citationNumber(evidence, evidenceRefs);
  if (number !== null) return t("evidence.numberedSource", { number: number, title: title });
  if (memoryTitle) return t("evidence.memoryLabel", { memoryTitle: memoryTitle });
  return t("evidence.resultLabel", { title: title });
}

export function executionEvidenceType(evidence: ChatEvidenceRef): "dataset" | "artifact" | "tool" {
  if (["dataset_query", "dataset_describe", "dataset_import", "chart_query"].includes(evidence.tool_name)) return "dataset";
  if (["artifact_read", "artifact_list"].includes(evidence.tool_name)) return "artifact";
  return "tool";
}

/** A URL fallback is a locator, not a document title. Keep its filename readable. */
export function evidenceTitle(evidence: ChatEvidenceRef): string {
  const memoryTitles: Record<string, string> = { world_read: "World", world_search: t("evidence.worldSearch"), lens_read: "Lens", lens_search: t("evidence.lensSearch") };
  if (evidence.title === evidence.tool_name.replaceAll("_", " ") && memoryTitles[evidence.tool_name]) return memoryTitles[evidence.tool_name];
  // A calculation's reader title is the host's operation name; show it in plain words ("Reverse DCF grid" is jargon).
  if (evidence.tool_name === "calculate") {
    const name = operationName(evidence.title);
    if (name) return name;
  }
  const url = parseCredentialFreeHttpUrl(evidence.title);
  if (!url) return evidence.title;
  const filename = url.pathname.split("/").filter(Boolean).at(-1);
  if (!filename) return url.hostname;
  try { return decodeURIComponent(filename); } catch { return filename; }
}

/** Byte offsets come from the host's exact passage selector, never model prose. */
export function evidenceExcerptParts(evidence: ChatEvidenceRef) {
  const match = evidence.excerpt_match;
  if (!match || !Number.isSafeInteger(match.start) || !Number.isSafeInteger(match.end) || match.start < 0 || match.end <= match.start) return null;
  const bytes = new TextEncoder().encode(evidence.excerpt);
  if (match.end > bytes.length) return null;
  try {
    const decoder = new TextDecoder("utf-8", { fatal: true });
    return {
      before: decoder.decode(bytes.slice(0, match.start)),
      match: decoder.decode(bytes.slice(match.start, match.end)),
      after: decoder.decode(bytes.slice(match.end)),
    };
  } catch { return null; }
}

/** Keep passage coverage useful without exposing storage offsets. */
export const evidenceCoverage = (coverage: string) => /^(?:Host-selected literal passage; )?UTF-8 bytes /.test(coverage)
  ? t("evidence.selectedPassageFromTheRetrievedDocument") : coverage;

const kinds = (): Record<ChatEvidenceRef["kind"], { label: string; description: string }> => ({
  original: { label: t("evidence.retrievedSource"), description: t("evidence.contentReturnedByTheSourceWhenThisAnswerWas") },
  discovery: { label: t("evidence.searchResult"), description: t("evidence.aSearchPreviewTheFullSourceWasNotRead") },
  derived: { label: t("evidence.calculatedResult"), description: t("evidence.outputProducedFromOtherDataCheckItsInputsAnd") },
  local_file: { label: t("evidence.workspaceFile"), description: t("evidence.aSavedCopyOfLocalFileContentReadDuring") },
  saved_context: { label: t("evidence.savedResearch"), description: t("evidence.researchThisLoupistAlreadyHadRetrievedForThisReply") },
});

/** One row per real subject, not per evidence id. */
function evidenceSubjectKey(evidence: ChatEvidenceRef) {
  const url = evidence.url ? parseCredentialFreeHttpUrl(evidence.url)?.href : undefined;
  if (url) return `url:${url}:${evidence.title}`;
  if (evidence.dataset_ref) return `dataset:${evidence.dataset_ref.dataset_id}:${evidence.dataset_ref.digest}`;
  if (evidence.kind === "saved_context" || evidence.kind === "local_file") return `${evidence.kind}:${evidence.tool_name}:${evidence.title}`;
  return `id:${evidence.id}`;
}

export function sourceIdentityLabel(evidence: ChatEvidenceRef) {
  if (evidence.tool_name.startsWith("world")) return "World";
  if (evidence.tool_name.startsWith("lens")) return "Lens";
  if (evidence.kind === "local_file") return t("evidence.files");
  if (evidence.kind === "saved_context") return t("evidence.memory");
  const url = evidence.url ? parseCredentialFreeHttpUrl(evidence.url) : null;
  if (url) return url.hostname.replace(/^www\./, "");
  if (evidence.provider) return evidence.provider;
  if (evidence.tool_name.startsWith("web") || evidence.kind === "original" || evidence.kind === "discovery") return t("evidence.web");
  return evidencePresentation(evidence).label;
}

export function evidencePresentation(evidence: ChatEvidenceRef) {
  if (evidence.tool_name === "dataset_import") return { label: t("evidence.localDataset"), description: t("evidence.dataImportedFromAWorkspaceFileItsContentsHave") };
  if (evidence.tool_name === "dataset_query") return { label: t("evidence.selectedData"), description: t("evidence.rowsOrValuesSelectedFromASavedDatasetFor") };
  if (evidence.tool_name === "dataset_describe") return { label: t("evidence.datasetSummary"), description: t("evidence.structureAndCoverageOfASavedDatasetInspectedFor") };
  if (evidence.kind === "derived") return {
    label: executionEvidenceType(evidence) === "artifact" ? t("evidence.documentData") : executionEvidenceType(evidence) === "dataset" ? t("evidence.datasetResult") : t("evidence.toolResult"),
    description: t("evidence.aSavedAnalysisResultThisIsNotAnOriginal"),
  };
  return kinds()[evidence.kind];
}
