// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { ChatArtifactRef } from "../chat/artifacts";
import type { JsonValue } from "../external";

export type DockByKind = { chart?: WorkspacePlacement, reading?: WorkspacePlacement };

export type DraftAttachment = { id: string, filename: string, mediaType: string, url: string };

/** Unsent text lives in the app profile, outside the agent's workspace. */
export type DraftIdentity = { loupist_id: string, thread_id: string };

export type StoredChatDraft = { prompt: string, context?: { [key in string]: JsonValue } };

export type WorkspaceMemoryLocation = {
  loupistId: string,
  surface: WorkspaceMemorySurface,
  recordId?: string,
  revisionId?: string,
  objectId?: string,
  selection?: WorkspaceMemorySelection,
};

export type WorkspaceMemoryPin = {
  kind: WorkspaceMemoryPinKind,
  loupistId: string,
  recordId?: string,
  revisionId?: string,
  objectId: string,
  spanId?: string,
  label: string,
};

export type WorkspaceMemoryPinKind =
  | "source"
  | "world_node"
  | "world_claim"
  | "lens"
  | "journal"
  | "research";

export type WorkspaceMemorySelection =
  | { "kind": "node", id: string }
  | { "kind": "claim", id: string }
  | {
    "kind": "source",
    sourceId?: string,
    spanId?: string,
    worldRecordId?: string,
    worldRevisionId?: string,
  }
  | { "kind": "lens", recordId: string }
  | { "kind": "research", recordId: string, sectionId?: string }
  | { "kind": "journal", journalId: string }
  | { "kind": "snapshot", pin: WorkspaceMemoryPin };

export type WorkspaceMemorySurface = "home" | "world" | "lens" | "source" | "note";

export type WorkspacePlacement = "side" | "bottom";

export type WorkspaceUiSession = {
  loupist_id: string,
  thread_id: string,
  tabs: Array<WorkspaceUiTab>,
  active_tab_id?: string,
  /** The user closed the panel in this chat; the renderer does not reopen it by itself. */
  dismissed?: boolean,
};

export type WorkspaceUiState = {
  version: number,
  open: boolean,
  placement: WorkspacePlacement,
  /** The user's dock choice per content kind (side-panel.md P7); unset kinds use defaults. */
  dock_by_kind?: DockByKind,
  width: number,
  height: number,
  sessions: Array<WorkspaceUiSession>,
};

export type WorkspaceUiTab =
  | { "kind": "files", id: string, title: string, path: string, filePath?: string }
  | {
    "kind": "memory",
    id: string,
    title: string,
    record_id: string,
    location: WorkspaceMemoryLocation,
  }
  | { "kind": "work", id: string, artifact?: ChatArtifactRef, artifact_thread_id?: string }
  | { "kind": "browser", id: string, title: string, url: string }
  | { "kind": "unsupported" };

export type WorkspaceUiWriteRequest = { state: WorkspaceUiState };
