import type { ChatArtifactRef } from "./chat";
import type { KnowledgeLocation } from "./knowledge";
import type * as Wire from "./generated";

export type WorkspacePathRequest = { loupist_id: string; path?: string };
export type WorkspaceEntry = { path: string; name: string; kind: "file" | "directory"; size_bytes: number | null };
export type WorkspaceDirectory = { path: string; entries: WorkspaceEntry[]; truncated: boolean };
export type WorkspaceFile = { path: string; size_bytes: number; content: string | null; media_type: string; truncated: boolean; binary: boolean; content_sha256: string | null; image_data_url?: string | null };
export type ChatActivityRequest = Wire.ChatActivityRequest;
export type ChatActivityDetail = Wire.ChatActivityDetail;

/** Small reader references only; runtime handles, selections and loaded bodies stay in memory. */
export type WorkspaceUiTab =
  | { id: string; kind: "files"; title: string; path: string; filePath?: string }
  /** The Evidence tab's current saved-Memory reader. Citations are never persisted. */
  | { id: string; kind: "memory"; title: string; record_id: string; location: Omit<KnowledgeLocation, "view"> }
  /** The Work tab; an open item carries the chat that made it. */
  | { id: string; kind: "work"; artifact?: Omit<ChatArtifactRef, "selected_range">; artifact_thread_id?: string }
  | { id: string; kind: "browser"; title: string; url: string };

export type WorkspaceUiState = {
  version: 1;
  open: boolean;
  placement: "side" | "bottom";
  /** Where the panel docks for each content kind, once the user chose (side-panel.md P7). */
  dock_by_kind?: { chart?: "side" | "bottom"; reading?: "side" | "bottom" };
  width: number;
  height: number;
  sessions: {
    loupist_id: string;
    thread_id: string;
    tabs: WorkspaceUiTab[];
    active_tab_id?: string;
    /** The user closed the panel in this chat; the panel does not open by itself here. */
    dismissed?: boolean;
  }[];
};

/** Renderer-owned attachment metadata and bytes for a device-local unsent draft. */
export type DraftAttachment = Wire.DraftAttachment;

export type ComposerChip = {
  key: string;
  kind: "skill" | "connection" | "file" | "quote";
  /** Skill or connection id, file path, or the quoted message's id. */
  id: string;
  label: string;
  /** A quote's full selected text. */
  text?: string;
};

export type ComposerInlineReference = {
  key: string;
  kind: string;
  text: string;
  start: number;
  end: number;
};

/** Inert composer presentation data. Chat admission checks each referenced object again on send. */
export type DraftContext = {
  pins: import("./knowledge").KnowledgePin[];
  chips: ComposerChip[];
  inlineReferences: ComposerInlineReference[];
  asOf?: string;
  artifact?: ChatArtifactRef;
};
export type StoredChatDraft = { prompt: string; context?: DraftContext };
