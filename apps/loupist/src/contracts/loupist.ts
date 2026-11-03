
export type LoupistRecoveryAction = "recover_memory";

export type LoupistAvailability =
  | { status: "available" }
  | { status: "workspace_unavailable" }
  /** The folder was made by an earlier build; Rust refuses it (no migration). */
  | { status: "earlier_version" }
  | {
      status: "recovery_required";
      reason: "interrupted_memory_update";
      action: LoupistRecoveryAction;
    };

export type LoupistSummary = {
  id: string;
  name: string;
  philosophy: string;
  instructions?: string;
  avatar?: LoupistAvatarId;
  /** How the Loupist talks; missing reads as balanced. */
  tone?: LoupistTone;
  /** The template it was made from (Rust keeps it); missing for imported folders, shared packs and Loupists made before templates were kept. */
  template?: LoupistTemplateId;
  workspace_path: string;
  record_count: number;
  updated_at: string;
  accent: string;
  enabled_skill_ids: string[];
  last_model_profile_id?: string;
  availability: LoupistAvailability;
};
/** Presentation only: how the Loupist talks, never what evidence it needs. */
export const LOUPIST_TONES = ["careful", "balanced", "direct"] as const;
export type LoupistTone = typeof LOUPIST_TONES[number];
/** Template cards in the order onboarding and New Loupist show them; Rust refuses any other id. */
export const LOUPIST_TEMPLATE_IDS = ["portfolio", "semiconductors", "dividends", "technology", "crypto", "macro", "commodities", "realAssets", "custom"] as const;
export type LoupistTemplateId = typeof LOUPIST_TEMPLATE_IDS[number];
export type LoupistCreateRequest = { name: string; focus?: string; avatar?: LoupistAvatarId; template?: LoupistTemplateId; tone?: LoupistTone };
export type LoupistImportRequest = { workspace_path: string };
export type LoupistReconnectRequest = { loupist_id: string; workspace_path: string };
export type LoupistDeleteRequest = { loupist_id: string };
export type LoupistRecoverRequest = {
  loupist_id: string;
  action: LoupistRecoveryAction;
};

export type AgentSkillSummary = {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  fixed?: boolean;
};

export type LoupistCapabilityRequest = { loupist_id: string; entry_id: string };

export type MarketplaceConnectorConfigureRequest = {
  entry_id: string;
  config: Record<string, string>;
};

export type MarketplaceCredentialRequest = {
  entry_id: string;
};

export type MarketplaceCredentialSaveRequest = {
  entry_id: string;
  secrets: Record<string, string>;
};

export const LOUPIST_PORTRAIT_IDS = ["loupist-01", "loupist-02", "loupist-03", "loupist-04", "loupist-05", "loupist-06", "loupist-07", "loupist-08", "loupist-09", "loupist-10"] as const;
export type LoupistPortraitId = typeof LOUPIST_PORTRAIT_IDS[number];
/** Older profile and shared-pack IDs remain valid; presentation maps them to portraits. */
export const LOUPIST_AVATAR_IDS = [...LOUPIST_PORTRAIT_IDS, "initial", "cpu", "coins", "globe", "line-chart", "landmark", "factory", "compass", "book-open", "sparkles"] as const;
export type LoupistAvatarId = typeof LOUPIST_AVATAR_IDS[number];
export type LoupistProfileUpdateRequest = { loupistId: string; name?: string; focus?: string; instructions?: string; avatar?: LoupistAvatarId; tone?: LoupistTone };
