import type { TFunction } from "i18next";
import { resources } from "../../i18n";

type ReviewedRelation = keyof typeof resources.en.knowledge.relations;
type CustomRelation = keyof typeof resources.en.knowledge.customRelations;

/**
 * A relation's stored identity: ASCII SCREAMING_SNAKE, as the host's `normalize_relation`
 * writes it. Hand-written names ("depends_on", "dependsOn", "files with") reach the same key;
 * a name with no ASCII letter or digit is `RELATES_TO`, as the host stores it.
 */
export function relationKey(relation: string): string {
  const key = relation.trim()
    .replace(/([a-z0-9])(?=[A-Z])/g, "$1_")
    .replace(/[^A-Za-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "")
    .toUpperCase();
  return key || "RELATES_TO";
}

/**
 * A relation in the interface language, for every surface that prints one. The reviewed
 * types (`relations`) come first, then the bundled table of common open relations
 * (`customRelations`); any other name reads as plain lowercase English
 * ("raised prices for"), the form of the reviewed English labels, in every language. Nothing calls a model, and the stored name is
 * unchanged: model-facing reads keep the canonical English.
 */
export function relationName(relation: string, t: TFunction<"knowledge">): string {
  const key = relationKey(relation);
  if (Object.hasOwn(resources.en.knowledge.relations, key)) return t(`relations.${key as ReviewedRelation}`);
  if (Object.hasOwn(resources.en.knowledge.customRelations, key)) return t(`customRelations.${key as CustomRelation}`);
  return key.toLowerCase().replaceAll("_", " ");
}
