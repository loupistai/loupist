export type CustomToolApproval = "allow_once" | "always" | "deny";
export type CustomToolEligibility = "read_only" | "not_read_only" | "financial_action";
export type CustomConnectionTransport =
  | { kind: "http"; url: string }
  | { kind: "stdio"; command: string; args: string[] };

export interface CustomToolCard {
  name: string;
  /** Untrusted server text. Label as the server's own description in review. */
  server_description: string | null;
  input_schema: Record<string, unknown>;
  annotations: Record<string, unknown> | null;
  definition_hash: string;
  definition_changed: boolean;
  eligibility: CustomToolEligibility;
  approval: CustomToolApproval | null;
}

export interface CustomConnection {
  id: string;
  kind: "custom";
  name: string;
  transport: CustomConnectionTransport;
  command_line: string | null;
  dont_save_to_memory: boolean;
  pending: boolean;
  enabled: boolean;
  needs_review: boolean;
  auth_state: "not_required" | "signed_in" | "sign_in_needed" | "in_progress";
  tools: CustomToolCard[];
  env_keys: string[];
  header_names: string[];
  last_error: string | null;
}

export interface CustomConnectionAddUrl {
  url: string;
  name?: string;
  /** Write-only secrets; the host returns header names only. */
  headers?: Record<string, string>;
}
export interface CustomConnectionAddCommand {
  command: string;
  args: string[];
  env: Record<string, string>;
  name?: string;
}
export interface CustomConnectionIdentity { connection_id: string }
export interface CustomToolIdentity extends CustomConnectionIdentity { loupist_id: string; tool_name: string }
export interface CustomToolApprove extends CustomToolIdentity { definition_hash: string; choice: CustomToolApproval }
export interface CustomConnectionAuthorize extends CustomConnectionIdentity {
  issuer: string;
  client_id: string;
  /** Write-only secret; never returned by the host. */
  client_secret?: string;
}

export interface CustomConnectionsBridge {
  customConnectionAddUrl(request: CustomConnectionAddUrl): Promise<CustomConnection>;
  customConnectionAddJson(request: { json: string }): Promise<CustomConnection[]>;
  /** Stages only. Show command_line verbatim before calling confirm. */
  customConnectionAddCommand(request: CustomConnectionAddCommand): Promise<CustomConnection>;
  /** `loupist_id` enables the connection for the Loupist that opened Add, before tool review. */
  customConnectionConfirm(request: CustomConnectionIdentity & { loupist_id?: string }): Promise<CustomConnection>;
  customConnectionList(request?: { loupist_id?: string }): Promise<CustomConnection[]>;
  customConnectionRemove(request: CustomConnectionIdentity): Promise<void>;
  customConnectionSetMemory(request: CustomConnectionIdentity & { dont_save_to_memory: boolean }): Promise<CustomConnection>;
  customConnectionSetEnabled(request: CustomConnectionIdentity & { loupist_id: string; enabled: boolean }): Promise<CustomConnection>;
  customConnectionAuthorize(request: CustomConnectionAuthorize): Promise<CustomConnection>;
  customConnectionSignIn(request: CustomConnectionIdentity): Promise<CustomConnection>;
  customConnectionSignInCancel(request: CustomConnectionIdentity): Promise<void>;
  customToolApprove(request: CustomToolApprove): Promise<CustomConnection>;
}
