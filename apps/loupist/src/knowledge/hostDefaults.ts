import i18n from "@/i18n";

/**
 * Section keys whose English default heading the interface translates: the keys
 * suggested per page kind (`lens_page::suggested_sections`; method learning writes
 * `use-when`, `steps`, `checks` and `limits` with their defaults) and `statement`,
 * the section a principle save records in History (`lens::log_principle`).
 */
const SUGGESTED_KEYS = [
  "beliefs", "focus", "priorities", "avoid",
  "use-when", "steps", "checks", "limits",
  "overview", "facts", "value-chain", "drivers", "risks", "views", "open-questions",
  "claim", "mechanism", "evidence-for", "evidence-against", "would-break",
  "statement",
] as const;

type SuggestedKey = (typeof SUGGESTED_KEYS)[number];

const isSuggestedKey = (key: string): key is SuggestedKey => (SUGGESTED_KEYS as readonly string[]).includes(key);

/** The interface-language default heading of a key the app writes, if it is one. */
export function defaultSectionHeading(key: string): string | undefined {
  return isSuggestedKey(key) ? i18n.t(`knowledge:pageReader.sectionHeading.${key}`) : undefined;
}

const englishHeading = (key: string) => (key[0]!.toUpperCase() + key.slice(1)).replaceAll("-", " ");

/** A heading the host or Bot wrote as the English default for its suggested key shows in the interface language. */
export function displaySectionHeading(key: string, heading: string): string {
  return isSuggestedKey(key) && heading === englishHeading(key) ? i18n.t(`knowledge:pageReader.sectionHeading.${key}`) : heading;
}

/** Old history entries may identify only a section kind. Its current title can belong to a later edit. */
export function historySectionHeading({ key, sectionHeading }: { key?: string; sectionHeading?: string }): string {
  if (sectionHeading?.trim()) return displaySectionHeading(key ?? "", sectionHeading);
  return (key ? defaultSectionHeading(key) : undefined) ?? i18n.t("knowledge:pageReader.unnamedSection");
}
