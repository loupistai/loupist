import i18n from "../i18n/index";
import englishSettings from "../i18n/locales/en/settings.json";
import type { AgentSkillSummary } from "../types";

type BundledSkillId = keyof typeof englishSettings.bundledSkills;
type BundledSkill = Omit<AgentSkillSummary, "enabled">;

/** World, Lens, Journal and Visualize are always on; the other bundled Skills can be toggled. */
const FIXED_SKILL_IDS: ReadonlySet<string> = new Set(["world", "lens", "journal", "visualize"]);

const BUNDLED_SKILLS: BundledSkill[] = (Object.keys(englishSettings.bundledSkills) as BundledSkillId[]).map((id) => ({
  id,
  ...englishSettings.bundledSkills[id],
  fixed: FIXED_SKILL_IDS.has(id),
}));

export const AGENT_SKILLS: ReadonlyArray<BundledSkill> = BUNDLED_SKILLS.filter((skill) => !skill.fixed);
export const FIXED_AGENT_SKILLS: ReadonlyArray<BundledSkill> = BUNDLED_SKILLS.filter((skill) => skill.fixed);
export const DEFAULT_AGENT_SKILL_IDS = AGENT_SKILLS.map((skill) => skill.id);

/** Localize bundled UI metadata only; installed Skill content keeps its own language. */
export function skillPresentation(skill: Pick<AgentSkillSummary, "id" | "name" | "description"> & { source?: string }) {
  const builtin = BUNDLED_SKILLS.find((candidate) => candidate.id === skill.id);
  if (!builtin || (skill.source !== undefined ? skill.source !== "bundled" : skill.name !== builtin.name || skill.description !== builtin.description)) {
    return { name: skill.name, description: skill.description };
  }
  const id = builtin.id as BundledSkillId;
  return {
    name: id === "world" || id === "lens" ? builtin.name : i18n.t(`settings:bundledSkills.${id}.name`),
    description: i18n.t(`settings:bundledSkills.${id}.description`),
  };
}
