import i18n from "../i18n/index";
import englishSettings from "../i18n/locales/en/settings.json";
import type { AgentSkillSummary } from "../types";

type BundledSkillId = keyof typeof englishSettings.bundledSkills;
type BundledSkill = Omit<AgentSkillSummary, "enabled">;

/** Map (`world`), Lens, Journal and Visualize are always on; the other bundled Skills can be toggled. */
const FIXED_SKILL_IDS: ReadonlySet<string> = new Set(["world", "lens", "journal", "visualize"]);

const BUNDLED_SKILLS: BundledSkill[] = (Object.keys(englishSettings.bundledSkills) as BundledSkillId[]).map((id) => ({
  id,
  ...englishSettings.bundledSkills[id],
  fixed: FIXED_SKILL_IDS.has(id),
}));

export const AGENT_SKILLS: ReadonlyArray<BundledSkill> = BUNDLED_SKILLS.filter((skill) => !skill.fixed);
export const FIXED_AGENT_SKILLS: ReadonlyArray<BundledSkill> = BUNDLED_SKILLS.filter((skill) => skill.fixed);
export const DEFAULT_AGENT_SKILL_IDS = AGENT_SKILLS.map((skill) => skill.id);
/** Tools for making and installing Skills: both Skills lists keep them last, under "Advanced". */
export const SKILL_TOOLING_IDS: ReadonlySet<string> = new Set(["skill-creator"]);

/** Localize bundled UI metadata only; installed Skill content keeps its own language. */
type SkillIdentity = Pick<AgentSkillSummary, "id" | "name" | "description"> & { source?: string };

/** The bundled Skill this summary is, unless an installed Skill shadows its id. */
function bundledId(skill: SkillIdentity): BundledSkillId | null {
  const builtin = BUNDLED_SKILLS.find((candidate) => candidate.id === skill.id);
  if (!builtin || (skill.source !== undefined ? skill.source !== "bundled" : skill.name !== builtin.name || skill.description !== builtin.description)) return null;
  return builtin.id as BundledSkillId;
}

export function skillPresentation(skill: SkillIdentity) {
  const id = bundledId(skill);
  if (!id) return { name: skill.name, description: skill.description };
  return {
    name: i18n.t(`settings:bundledSkills.${id}.name`),
    description: i18n.t(`settings:bundledSkills.${id}.description`),
  };
}

/** A list row's line: a bundled Skill's short people-facing blurb (the "/" menu's), else its description. Details keep the full text. */
export function skillBlurb(skill: SkillIdentity): string {
  const id = bundledId(skill);
  return id && i18n.exists(`chat:skillBlurb.${id}`) ? i18n.t(`chat:skillBlurb.${id}`) : skillPresentation(skill).description;
}
