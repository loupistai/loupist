import i18n from "@/i18n";
import type { LensKind } from "../contracts/lens";

/** Title the host gives the default perspective (`PERSPECTIVE_TITLE` in `chat_reads.rs`). */
export const HOST_PERSPECTIVE_TITLE = "Outlook";

const SUGGESTED_KEYS = [
  "beliefs", "focus", "priorities", "avoid",
  "use-when", "steps", "checks", "limits",
  "overview", "facts", "value-chain", "drivers", "risks", "views", "open-questions",
  "claim", "mechanism", "evidence-for", "evidence-against", "would-break",
] as const;

type SuggestedKey = (typeof SUGGESTED_KEYS)[number];

const isSuggestedKey = (key: string): key is SuggestedKey => (SUGGESTED_KEYS as readonly string[]).includes(key);

const englishHeading = (key: string) => (key[0]!.toUpperCase() + key.slice(1)).replaceAll("-", " ");

/** The host-default title in the interface language; a title the user or Bot chose stays as written. */
export function displayPageTitle(kind: LensKind, title: string): string {
  return kind === "perspective" && title === HOST_PERSPECTIVE_TITLE ? i18n.t("knowledge:pageReader.kind.perspective") : title;
}

/** A heading the host or Bot wrote as the English default for its suggested key shows in the interface language. */
export function displaySectionHeading(key: string, heading: string): string {
  return isSuggestedKey(key) && heading === englishHeading(key) ? i18n.t(`knowledge:pageReader.sectionHeading.${key}`) : heading;
}
