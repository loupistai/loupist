import i18n from "@/i18n";
import { formatNumber } from "@/i18n/format";
import { knowledgeLocationFromReference } from "../knowledge/location";
import type { ChatEvidenceRef, ChatMessage, KnowledgeReference } from "../types";
import { citationSources, evidenceProvenance, evidenceTitle, isUserProvided, sourceIdentityLabel } from "./evidence";
import { evidenceLinks } from "./evidenceLinkOrder";
import { memoryReadForEvidence } from "./memoryActivity";
import type { ChatDestination } from "./navigation";

export type AnswerSource = {
  evidence: ChatEvidenceRef;
  saved?: { reference: KnowledgeReference; title: string; citations: number };
};

/** Keep the existing source ordinals, then add the saved Sources actually linked in the answer. */
export function answerSources(message: ChatMessage): AnswerSource[] {
  const refs = message.evidence_refs ?? [];
  const reads = new Map<string, { evidence: ChatEvidenceRef; reference: KnowledgeReference; title: string }>();
  for (const evidence of refs) {
    const read = memoryReadForEvidence(evidence, message.memory_refs);
    if (read?.knowledge?.kind === "source") {
      reads.set(evidence.id, { evidence, reference: read.knowledge, title: read.title });
    }
  }
  const ordinary = reads.size ? refs.filter(ref => !reads.has(ref.id)) : refs;
  const sources: AnswerSource[] = citationSources(ordinary).map(evidence => ({ evidence }));
  if (!reads.size) return sources;

  const saved = new Map<string, NonNullable<AnswerSource["saved"]>>();
  for (const { id } of evidenceLinks(message.content)) {
    const read = reads.get(id);
    if (!read) continue;
    const { evidence, reference, title } = read;
    const key = JSON.stringify([reference.recordId, reference.revisionId, reference.objectId]);
    const prior = saved.get(key);
    if (prior) {
      prior.citations++;
    } else {
      const source = { reference, title, citations: 1 };
      saved.set(key, source);
      sources.push({ evidence, saved: source });
    }
  }
  return sources;
}

export function answerSourceOrigin({ evidence, saved }: AnswerSource) {
  return saved && !evidence.memory_origin && !isUserProvided(evidence) ? i18n.t("chat:workspaceEvidence.kind.savedSource") : sourceIdentityLabel(evidence);
}

/** The answer footer and Activity open the same saved revision and passage. */
export function answerSourceRow(source: AnswerSource, loupistId: string) {
  const { evidence, saved } = source;
  const title = saved?.title ?? evidenceTitle(evidence);
  const to: ChatDestination = saved
    ? { kind: "memory", title, location: knowledgeLocationFromReference(loupistId, saved.reference) }
    : { kind: "evidence", evidence };
  const citations = saved ? i18n.t("chat:activitySheet.read.citations", { count: saved.citations, formattedCount: formatNumber(saved.citations) }) : null;
  return { title, to, citations, provenance: evidenceProvenance(evidence, answerSourceOrigin(source)) };
}
