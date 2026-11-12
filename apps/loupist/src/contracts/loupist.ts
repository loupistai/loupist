import type * as Wire from "./generated";

export type LoupistAvailability = Wire.LoupistAvailability;

export type LoupistSummary = {
  id: string;
  name: string;
  philosophy: string;
  instructions: string;
  avatar: LoupistAvatarId;
  /** How the Loupist talks. */
  tone: LoupistTone;
  /** The template it was made from (Rust keeps it); missing for imported folders, shared packs and Loupists made before templates were kept. */
  template?: LoupistTemplateId;
  workspace_path: string;
  record_count: number;
  updated_at: string;
  accent: string;
  enabled_skill_ids: string[];
  last_model_profile_id: string | null;
  availability: LoupistAvailability;
};
/** Presentation only: how the Loupist talks, never what evidence it needs. */
export const LOUPIST_TONES = ["careful", "balanced", "direct"] as const;
export type LoupistTone = Wire.LoupistTone;
/** Template cards in the order onboarding and New Loupist show them; Rust refuses any other id. */
export const LOUPIST_TEMPLATE_IDS = ["portfolio", "semiconductors", "dividends", "technology", "crypto", "macro", "commodities", "realAssets", "custom"] as const;
export type LoupistTemplateId = typeof LOUPIST_TEMPLATE_IDS[number];
export type LoupistCreateRequest = { name: string; focus?: string; avatar?: LoupistAvatarId; template?: LoupistTemplateId; tone?: LoupistTone };
export type LoupistImportRequest = Wire.LoupistImportRequest;
export type LoupistReconnectRequest = Wire.LoupistReconnectRequest;
export type LoupistDeleteRequest = Wire.LoupistDeleteRequest;

export type AgentSkillSummary = Wire.AgentSkillSummary;

export type LoupistCapabilityRequest = Wire.LoupistCapabilityRequest;

export type MarketplaceConnectorConfigureRequest = Wire.MarketplaceConnectorConfigureRequest;

export type MarketplaceCredentialRequest = Wire.MarketplaceCredentialRequest;

export type MarketplaceCredentialSaveRequest = Wire.MarketplaceCredentialSaveRequest;

export const LOUPIST_PORTRAIT_IDS = ["loupist-01", "loupist-02", "loupist-03", "loupist-04", "loupist-05", "loupist-06", "loupist-07", "loupist-08", "loupist-09", "loupist-10"] as const;
export type LoupistPortraitId = typeof LOUPIST_PORTRAIT_IDS[number];
/** Older profile and shared-pack IDs remain valid; presentation maps them to portraits. */
export const LOUPIST_AVATAR_IDS = [...LOUPIST_PORTRAIT_IDS, "initial", "cpu", "coins", "globe", "line-chart", "landmark", "factory", "compass", "book-open", "sparkles"] as const;
export type LoupistAvatarId = typeof LOUPIST_AVATAR_IDS[number];
export type LoupistProfileUpdateRequest = { loupistId: string; name?: string; focus?: string; instructions?: string; avatar?: LoupistAvatarId; tone?: LoupistTone };
