// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { NotifyPolicy } from "../scheduled_tasks/types";

export type BotFocusRequest = { loupist_id: string | null };

export type InboxChanged = { loupistId: string };

export type InboxOpen = { loupistId: string, runId: string | null };

export type NotificationDefaults = {
  crossedLine: NotifyPolicy,
  filings: NotifyPolicy,
  briefing: NotifyPolicy,
  other: NotifyPolicy,
  failures: NotifyPolicy,
};

export type NotificationSettings = {
  quietHours: QuietHours | null,
  dailyCap: number,
  showDetails: boolean,
  defaults: NotificationDefaults,
};

export type NotificationTemplates = {
  result: string,
  privateResult: string,
  digest: string,
  lineCrossed: string,
};

export type QuietHours = { start: string, end: string };

export type ResidentRequest = {
  keepRunning?: boolean | null,
  notifications?: NotificationSettings | null,
};

export type ResidentSettings = {
  keepRunning: boolean,
  notificationPermissionAsked: boolean,
  notifications?: NotificationSettings,
  /** None follows the automatic default; Some preserves the user's choice. */
  keepRunningChoice: boolean | null,
  loginHintDismissed?: boolean,
};

export type TemplatesRequest = { language: string, templates: NotificationTemplates };
