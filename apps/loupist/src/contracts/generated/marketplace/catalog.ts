// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { JsonValue } from "../external";
import type { KrxCoverage } from "../finance_credentials";

export type LoupistCapabilityBindingDto = {
  entry_id: string,
  enabled: boolean,
  granted_permissions: Array<string>,
  available: boolean,
};

export type LoupistCapabilityRequest = { loupist_id: string, entry_id: string };

export type MarketplaceAuthenticationPolicy = "ON_INSTALL" | "ON_USE";

export type MarketplaceAuthorDto = { name: string, email?: string, url?: string };

export type MarketplaceCatalogDto = { schema_version: string, entries: Array<MarketplaceEntryDto> };

export type MarketplaceConfigState = "not_required" | "missing" | "valid";

export type MarketplaceConnectorConfigureRequest = {
  entry_id: string,
  config: { [key in string]: string },
};

export type MarketplaceConnectorReadiness = "ready" | "needs_configuration" | "runtime_unavailable";

export type MarketplaceConnectorStatusDto = {
  entry_id: string,
  config: { [key in string]: string },
  config_state: MarketplaceConfigState,
  credentials: Array<MarketplaceCredentialStatusDto>,
  readiness: MarketplaceConnectorReadiness,
};

export type MarketplaceCredentialRequest = { entry_id: string };

export type MarketplaceCredentialSaveRequest = {
  entry_id: string,
  secrets: { [key in string]: string },
};

export type MarketplaceCredentialStatusDto = {
  entry_id: string,
  credential_id: string,
  stored: boolean,
  active: boolean,
  pending: boolean,
  verification: MarketplaceCredentialVerification,
  verified_at: number | null,
  last_error: string | null,
  krx_coverage: KrxCoverage | null,
};

export type MarketplaceCredentialVerification =
  | "never"
  | "verified"
  | "rejected"
  | "temporarily_unavailable";

export type MarketplaceEntryDto = {
  id: string,
  plugin: string,
  name: string,
  summary: string,
  publisher: string,
  data_authority: string,
  kind: MarketplaceEntryKind,
  free_state: MarketplaceFreeState,
  trust: MarketplaceTrust,
  runtime: MarketplaceRuntimeDto,
  release_stage: MarketplaceReleaseStage,
  featured: boolean,
  markets: Array<string>,
  asset_classes: Array<string>,
  capabilities: Array<string>,
  freshness: Array<string>,
  attribution: string,
  terms_url: string | null,
  permissions: MarketplacePermissionsDto,
  setup: MarketplaceSetupDto | null,
};

export type MarketplaceEntryKind = "data_source";

export type MarketplaceFreeState = "keyless" | "free_account" | "paid";

export type MarketplaceHealth = "ready" | "needs_configuration" | "error";

export type MarketplaceInstallationPolicy = "INSTALLED_BY_DEFAULT" | "AVAILABLE" | "NOT_AVAILABLE";

export type MarketplaceInstalledDto = {
  entry_id: string,
  configured: boolean,
  health: MarketplaceHealth,
};

export type MarketplacePermissionsDto = {
  network_hosts: Array<string>,
  credential_kinds: Array<string>,
};

export type MarketplacePluginDto = {
  name: string,
  version: string,
  description: string,
  author: MarketplaceAuthorDto,
  interface: MarketplacePluginInterfaceDto,
  policy: MarketplacePluginPolicyDto,
  category: string,
};

export type MarketplacePluginInterfaceDto = {
  displayName: string,
  shortDescription: string,
  category: string,
  capabilities: Array<string>,
};

export type MarketplacePluginPolicyDto = {
  installation: MarketplaceInstallationPolicy,
  authentication?: MarketplaceAuthenticationPolicy,
};

export type MarketplaceReleaseStage = "available" | "preview";

export type MarketplaceRuntimeDto = {
  kind: MarketplaceRuntimeKind,
  server_id: string | null,
  provider_ids: Array<string>,
  credential_mapping: { [key in string]: string },
  config_mapping: { [key in string]: string },
  verification_probe: MarketplaceVerificationProbeDto | null,
};

export type MarketplaceRuntimeKind = "native" | "mcp";

export type MarketplaceSetupDto = {
  config_fields: Array<MarketplaceSetupFieldDto>,
  credential_fields: Array<MarketplaceSetupFieldDto>,
  credential_scope_fields: Array<string>,
};

export type MarketplaceSetupFieldDto = {
  id: string,
  kind: MarketplaceSetupFieldKind,
  options: Array<string>,
  label: string,
  required: boolean,
  min_length: number,
  max_length: number,
  help_url: string | null,
};

export type MarketplaceSetupFieldKind = "api_key" | "email" | "select";

export type MarketplaceSnapshotDto = {
  schema_version: string,
  plugins: Array<MarketplacePluginDto>,
  catalog: MarketplaceCatalogDto,
  installed: Array<MarketplaceInstalledDto>,
  connectors: Array<MarketplaceConnectorStatusDto>,
};

export type MarketplaceTrust = "first_party" | "reviewed_community";

export type MarketplaceVerificationProbeDto = { tool_name: string, arguments: JsonValue };
