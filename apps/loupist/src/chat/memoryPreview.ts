import i18n, { useTranslation } from "@/i18n";
import { parsePartialJson } from "ai";
import { useEffect, useState } from "react";
import type { ChatEvidenceRef } from "../types";

const t = i18n.getFixedT(null, "chat");

const object = (value: unknown): Record<string, unknown> | undefined =>
  value !== null && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : undefined;
const text = (value: unknown) => typeof value === "string" ? value : "";
const rows = (value: unknown): Record<string, unknown>[] => Array.isArray(value)
  ? value.map(object).filter((item): item is Record<string, unknown> => item !== undefined) : [];
const isMemoryTool = (evidence: ChatEvidenceRef) => evidence.kind === "saved_context" && ["world_search", "world_read", "lens_search", "lens_read"].includes(evidence.tool_name);
const isEnvelope = (excerpt: string) => /^\s*\{\s*"(?:sourceTargets|recordId|revisionId|capped|hits|readRef)"\s*:/.test(excerpt);
const incomplete = () => t("memoryPreview.thisSavedPreviewIsIncompleteOpenTheRelatedRecord");

/** Present known Memory envelopes, without changing retained evidence or source text. */
export async function memoryPreview(evidence: ChatEvidenceRef, excerpt = evidence.excerpt): Promise<string | null> {
  if (!isMemoryTool(evidence)) return null;
  let value: Record<string, unknown> | undefined;
  let partial = false;
  try { value = object(JSON.parse(excerpt)); } catch {
    if (!isEnvelope(excerpt)) return null;
    // The SDK restores JSON syntax only; the UI still marks retained text as partial.
    value = object((await parsePartialJson(excerpt)).value);
    partial = true;
  }
  if (!value) return partial ? incomplete() : null;
  const preview = (content: string) => partial ? [content, incomplete()].filter(Boolean).join("\n\n") : content;
  if (Array.isArray(value.hits)) {
    const hits = rows(value.hits).map((hit) => [text(hit.title), text(hit.snippet)].filter(Boolean).join("\n"));
    return preview([hits.join("\n\n") || (partial ? "" : t("memoryPreview.noMatchingSavedRecords")), value.hasMore ? t("memoryPreview.moreMatchesAreAvailableInMemory") : ""].filter(Boolean).join("\n\n"));
  }
  if (Array.isArray(value.nodes) && (Array.isArray(value.edges) || partial)) {
    const entities = rows(value.nodes).map((node) => [text(node.name), text(node.description)].filter(Boolean).join(" — "));
    const claims = rows(value.edges).map((edge) => {
      const claim = object(edge.claim);
      return claim ? [text(claim.proposition), text(claim.status), text(claim.attribution)].filter(Boolean).join("\n") : "";
    }).filter(Boolean);
    return preview([entities.length ? t("memoryPreview.entities", { value1: entities.join("\n") }) : "", claims.length ? t("memoryPreview.relationships", { value1: claims.join("\n\n") }) : ""].filter(Boolean).join("\n\n") || (partial ? "" : t("memoryPreview.noEntitiesOrRelationshipsInThisSavedView")));
  }
  if (typeof value.readRef === "string" && typeof value.text === "string") {
    return preview(await memoryPreview(evidence, value.text) ?? value.text);
  }
  return partial ? incomplete() : null;
}

export function useMemoryPreview(evidence: ChatEvidenceRef, excerpt = evidence.excerpt): string | null {
  useTranslation("chat");
  const key = JSON.stringify([evidence.kind, evidence.tool_name, excerpt, i18n.language]);
  const candidate = isMemoryTool(evidence) && excerpt.trimStart().startsWith("{");
  const [result, setResult] = useState<{ key: string; text: string | null }>();
  useEffect(() => {
    if (!candidate) return;
    let active = true;
    void memoryPreview(evidence, excerpt).then((text) => {
      if (active) setResult({ key, text });
    }).catch(() => { if (active) setResult({ key, text: isMemoryTool(evidence) && isEnvelope(excerpt) ? incomplete() : null }); });
    return () => { active = false; };
  }, [key, candidate]);
  return !candidate ? null : result?.key === key ? result.text : t("memoryPreview.openingSavedPreview");
}
