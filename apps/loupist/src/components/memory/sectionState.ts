import i18n from "../../i18n";
import { dateValue } from "../../knowledge/presentation";
import { displayWidth } from "../../i18n/localized";

/** A section as `knowledge_lens_read` delivers it. Contested wins over superseded. */
export type SectionFacts = {
  contested: boolean;
  sinceMs?: number;
  claimIds?: string[];
  role?: string;
  asOf?: string;
};

export type SectionStateKind = "current" | "contested" | "superseded";

export function sectionState(section: SectionFacts): SectionStateKind {
  if (section.contested) return "contested";
  if (section.sinceMs != null && (section.claimIds?.length ?? 0) > 0) return "superseded";
  return "current";
}

export function changedSectionCount(sections: readonly SectionFacts[]) {
  return sections.filter(section => sectionState(section) !== "current").length;
}

export function sectionRoleLabel(role: string | undefined) {
  switch (role) {
    case "check": return i18n.t("knowledge:pageReader.role.check");
    case "open_question": return i18n.t("knowledge:pageReader.role.open_question");
    case "refutation": return i18n.t("knowledge:pageReader.role.refutation");
    default: return undefined;
  }
}

/** The chip's room, in Latin characters (`displayWidth`). */
const CHIP_WIDTH = 64;

/**
 * "Superseded · {end date} · {Source}". The Source shortens first so the date
 * stays whole. With either part missing the chip is "Superseded" alone.
 */
export function supersededChipLabel(input: { end?: string; source?: string }, language = i18n.resolvedLanguage || i18n.language) {
  const t = i18n.getFixedT(language, "knowledge");
  const end = input.end?.trim();
  const name = input.source?.trim();
  if (!end || !name) return t("pageReader.superseded");
  const date = dateValue(end);
  let source = name;
  const line = () => t("pageReader.supersededDetail", { date, source });
  let text = line();
  while (displayWidth(text) > CHIP_WIDTH && [...source].length > 2) {
    const chars = [...source];
    source = `${chars.slice(0, Math.max(1, chars.length - 2)).join("").trimEnd()}…`;
    text = line();
  }
  return text;
}
