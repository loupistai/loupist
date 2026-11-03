import type { TFunction } from "i18next";
import { resources } from "../../i18n";
/** Open World vocabulary: only curated keys translate; unfamiliar terms stay readable. */
export function relationName(relation: string, t: TFunction<"knowledge">): string {
  const normalized = relation.toUpperCase();
  if (!Object.hasOwn(resources.en.knowledge.relations, normalized)) return relation.toLowerCase().replaceAll("_", " ");
  return t(`relations.${normalized as keyof typeof resources.en.knowledge.relations}`);
}
