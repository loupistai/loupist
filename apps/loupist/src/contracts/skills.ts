import type { ConfigScope, ConfigSnapshot } from "./config";
import type * as Wire from "./generated";
export type ManagedSkill = {
  id: string; name: string; description: string; source: "bundled" | "personal" | "loupist";
  path: string | null; enabled: boolean; fixed: boolean; editable: boolean; shadowed: boolean;
  revision: string; error: string | null;
};
export type SkillCatalog = { skills: ManagedSkill[]; configuration: ConfigSnapshot };
export type SkillDocument = { skill: ManagedSkill; instructions: string; files: string[] };
export type SkillReadRequest = Wire.SkillReadRequest;
export type SkillCreateRequest = Wire.SkillCreateRequest;
export type SkillSaveRequest = SkillReadRequest & { expectedRevision: string; description: string; instructions: string };
export type SkillDeleteRequest = SkillReadRequest & { expectedRevision: string };
export type SkillImportRequest = { scope: ConfigScope; kind: "folder" | "zip" | "github"; url?: string };
