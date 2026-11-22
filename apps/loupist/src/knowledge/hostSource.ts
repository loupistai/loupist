import i18n from "@/i18n";
import { knownActivityLabel } from "../chat/progressLabels";
import type { SourceTitleKey } from "@/contracts/knowledge";

const t = i18n.getFixedT(null, "knowledge");

/**
 * A Source the host wrote ("What you follow") keeps its English title for the
 * World; the interface shows it in its own language. Every other Source keeps
 * the title it was saved with.
 */
export function sourceTitle(source: { title: string; titleKey?: SourceTitleKey }): string {
  return source.titleKey ? t(`hostSource.${source.titleKey}.title`) : source.title;
}

/** The origin line of a host-written Source, in the interface language. */
export function sourceOrigin(source: { originLabel?: string; titleKey?: SourceTitleKey }): string | undefined {
  if (source.titleKey) return t(`hostSource.${source.titleKey}.origin`);
  if (source.originLabel?.startsWith("tool:")) return knownActivityLabel(source.originLabel.slice(5), "done") ?? t("sourceWorkspace.sourceSaved");
  return source.originLabel;
}

export const compactOrigin = (value: string | undefined) => {
  if (!value) return "";
  if (value === "Pasted text") return i18n.t("knowledge:sourceWorkspace.pastedText");
  try { const url = new URL(value); if (["https:", "http:"].includes(url.protocol)) return url.hostname.replace(/^www\./, ""); } catch { /* Named or local source. */ }
  return value;
};
