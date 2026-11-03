import i18n from "@/i18n";
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
  return source.titleKey ? t(`hostSource.${source.titleKey}.origin`) : source.originLabel;
}
