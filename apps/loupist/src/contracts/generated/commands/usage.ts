// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { ChatCostEstimate } from "../domain/chat";
import type { CredentialKind } from "../providers/status";
import type { UsageSurface } from "../runs/log";

export type UsageAccount = {
  provider: string,
  credentialKind: CredentialKind | null,
  totals: UsageTotals,
};

export type UsageByLoupist = { loupistId: string | null, name: string | null, totals: UsageTotals };

export type UsageBySurface = { surface: UsageSurface, totals: UsageTotals };

export type UsageChecks = { evaluations: number, reads: number, met: number };

export type UsageDay = { date: string, you: number, background: number, world: number };

export type UsageProgram = {
  loupistId: string,
  taskId: string,
  title: string | null,
  runs: number,
  totals: UsageTotals,
  checks?: UsageChecks,
};

export type UsageSummary = {
  fromMs: number,
  toMs: number,
  totals: UsageTotals,
  accounts: Array<UsageAccount>,
  days: Array<UsageDay>,
  surfaces: Array<UsageBySurface>,
  loupists: Array<UsageByLoupist>,
  programs: Array<UsageProgram>,
};

export type UsageSummaryRequest = { fromMs: number, toMs: number, timeZone: string };

export type UsageTotals = {
  runs: number,
  unknownRuns: number,
  requests: number,
  inputTokens: number,
  cachedInputTokens: number,
  outputTokens: number,
  costEstimate: ChatCostEstimate | null,
};
