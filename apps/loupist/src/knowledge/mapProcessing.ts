import type { KnowledgeCatalog } from "../types";

export const isMapProcessing = (status: string | undefined) => status === "queued" || status === "running";

export function processingSourceCount(catalog: KnowledgeCatalog | null) {
  return catalog?.sources.reduce((total, source) =>
    total + source.objects.filter(object => isMapProcessing(object.processing?.status)).length, 0) ?? 0;
}
