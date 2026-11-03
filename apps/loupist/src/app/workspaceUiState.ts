import i18n from "@/i18n";
import type { KnowledgeSelection, WorkspaceUiState, WorkspaceUiTab } from "../types";
import { EVIDENCE_TAB_ID, knowledgeEntry, pushEvidenceEntry, workWorkspaceTab, type ChatWorkspaceSession, type ChatWorkspaceTab, type EvidenceWorkspaceTab } from "../chat/workspace";
import { chatSessionKey } from "../chat/sessionRegistry";
import { parseCredentialFreeHttpUrl } from "../lib/credentialFreeUrl";
import { EMPTY_CHAT_THREAD_ID } from "./emptyChat";

export type DockKind = "chart" | "reading";
export type WorkspaceLayout = Pick<WorkspaceUiState, "open" | "placement" | "width" | "height"> & {
  /** The user's dock choice per content kind; unset kinds use the defaults (charts bottom, reading side). */
  dockByKind: Partial<Record<DockKind, WorkspaceUiState["placement"]>>;
};
export type WorkspaceSessions = Record<string, ChatWorkspaceSession>;

export const defaultWorkspaceLayout = (): WorkspaceLayout => ({ open: false, placement: "side", width: 460, height: 420, dockByKind: {} });
const byteLength = (value: string) => new TextEncoder().encode(value).length;
const shortText = (value: string, limit = 512) => {
  let result = "";
  const normalized = value.replace(/[\p{Cc}\s]+/gu, " ").trim() || i18n.t("app:workspaceUiState.untitled");
  for (const character of normalized) {
    if (byteLength(result + character) > limit) break;
    result += character;
  }
  return result;
};
const fits = (value: string | undefined, limit = 256) => value === undefined || byteLength(value) <= limit;
const persistablePath = (value: string | undefined) => value === undefined || (
  fits(value, 4096) && !value.startsWith("/") && !/[\\:\p{Cc}]/u.test(value) && !value.split("/").includes("..")
);
const boundedSize = (value: number, fallback: number, min: number, max: number) =>
  Number.isFinite(value) ? Math.round(Math.max(min, Math.min(max, value))) : fallback;

const referenceSelection = (selection: KnowledgeSelection | undefined): KnowledgeSelection | undefined => {
  if (!selection) return undefined;
  switch (selection.kind) {
    case "node": case "claim": return { kind: selection.kind, id: selection.id };
    case "source": return { kind: "source", sourceId: selection.sourceId, spanId: selection.spanId, worldRecordId: selection.worldRecordId, worldRevisionId: selection.worldRevisionId };
    case "lens": return { kind: "lens", recordId: selection.recordId };
    case "research": return { kind: "research", recordId: selection.recordId, sectionId: selection.sectionId };
    case "journal": return { kind: "journal", journalId: selection.journalId };
    case "snapshot": {
      const pin = selection.pin;
      return { kind: "snapshot", pin: { kind: pin.kind, loupistId: pin.loupistId, recordId: pin.recordId, revisionId: pin.revisionId, objectId: pin.objectId, spanId: pin.spanId, label: shortText(pin.label) } };
    }
  }
};

const referenceStringsFit = (value: unknown): boolean => {
  if (!value || typeof value !== "object") return true;
  return Object.entries(value).every(([key, item]) => typeof item === "string"
    ? fits(item, key === "label" ? 512 : 256)
    : referenceStringsFit(item));
};

/** Explicit projection prevents new reader/runtime fields from entering durable UI state. */
const tabReference = (tab: ChatWorkspaceTab, loupistId: string): WorkspaceUiTab | undefined => {
  if (!tab.id || !fits(tab.id, 8192)) return undefined;
  const id = tab.id;
  switch (tab.kind) {
    // Evidence bytes belong to the persisted Chat message: a citation is reopened from
    // the message, never from a renderer copy. Only a saved Memory reader is a reference.
    case "evidence": {
      const entry = tab.entries[tab.index];
      if (entry?.kind !== "knowledge") return undefined;
      const { loupistId: owner, surface, recordId, revisionId, objectId } = entry.location;
      const selection = referenceSelection(entry.location.selection);
      const location = { loupistId: owner, surface, recordId, revisionId, objectId, selection };
      const record = objectId ?? recordId ?? surface;
      if (owner !== loupistId || !fits(record) || !referenceStringsFit(location)) return undefined;
      return { id, kind: "memory", title: shortText(entry.title), record_id: record, location };
    }
    case "files": return persistablePath(tab.path) && persistablePath(tab.filePath)
      ? { id, kind: "files", title: shortText(tab.title), path: tab.path, filePath: tab.filePath } : undefined;
    case "browser": return fits(tab.url, 4096) && (!tab.url || parseCredentialFreeHttpUrl(tab.url))
      ? { id, kind: "browser", title: shortText(tab.title), url: tab.url } : undefined;
    case "work": {
      const artifact = tab.artifact;
      if (!artifact || !tab.artifactThreadId) return { id, kind: "work" };
      if (!fits(artifact.artifact_id) || !fits(artifact.digest) || !fits(tab.artifactThreadId)) return { id, kind: "work" };
      return { id, kind: "work", artifact_thread_id: tab.artifactThreadId,
        artifact: { artifact_id: artifact.artifact_id, revision: artifact.revision, kind: artifact.kind, title: shortText(artifact.title, 200), digest: artifact.digest } };
    }
  }
};

const retainActiveTab = (tabs: WorkspaceUiTab[], activeId: string | undefined) => {
  const kept = tabs.slice(-20);
  const active = tabs.find((tab) => tab.id === activeId);
  if (active && !kept.includes(active)) kept[0] = active;
  return kept;
};

export function workspaceUiSnapshot(layout: WorkspaceLayout, sessions: WorkspaceSessions): WorkspaceUiState {
  const { dockByKind, ...geometry } = layout;
  const state: WorkspaceUiState = {
    version: 1,
    ...geometry,
    ...(dockByKind.chart || dockByKind.reading ? { dock_by_kind: { ...dockByKind } } : {}),
    width: boundedSize(layout.width, 460, 360, 1280),
    height: boundedSize(layout.height, 420, 260, 720),
    sessions: [],
  };
  for (const [key, session] of Object.entries(sessions)) {
    const separator = key.indexOf(":");
    if (separator < 0) continue;
    let loupistId: string, threadId: string;
    try { loupistId = decodeURIComponent(key.slice(0, separator)); threadId = decodeURIComponent(key.slice(separator + 1)); }
    catch { continue; }
    if (!loupistId || !threadId || !fits(loupistId) || !fits(threadId) || threadId === EMPTY_CHAT_THREAD_ID) continue;
    const tabs = retainActiveTab(session.tabs.flatMap((tab) => {
      const reference = tabReference(tab, loupistId);
      return reference ? [reference] : [];
    }), session.active_tab_id);
    if (!tabs.length && !session.dismissed) continue;
    state.sessions.push({ loupist_id: loupistId, thread_id: threadId, tabs,
      active_tab_id: tabs.some((tab) => tab.id === session.active_tab_id) ? session.active_tab_id : tabs[0]?.id,
      ...(session.dismissed ? { dismissed: true } : {}) });
  }
  state.sessions = state.sessions.slice(-100);
  // Bound the wire payload too; the host independently enforces the same cap.
  while (byteLength(JSON.stringify(state)) > 512 * 1024 && state.sessions.length) state.sessions.shift();
  return state;
}

export function restoreWorkspaceUiState(state: WorkspaceUiState | null): { layout: WorkspaceLayout; sessions: WorkspaceSessions } {
  if (!state || state.version !== 1) return { layout: defaultWorkspaceLayout(), sessions: {} };
  const sessions: WorkspaceSessions = {};
  for (const session of state.sessions) {
    const tabs: ChatWorkspaceTab[] = [];
    let evidence: EvidenceWorkspaceTab | undefined;
    let activeId = session.active_tab_id;
    for (const tab of session.tabs) {
      if (tab.kind === "files") tabs.push(tab);
      else if (tab.kind === "work") tabs.push(workWorkspaceTab(tab.artifact, tab.artifact_thread_id));
      else if (tab.kind === "browser") tabs.push({ ...tab, loading: false });
      else if (tab.kind === "memory") {
        // A saved reader reference becomes the Evidence tab's current entry.
        evidence = pushEvidenceEntry(evidence, knowledgeEntry(tab.location, tab.title));
        const at = tabs.findIndex((item) => item.kind === "evidence");
        if (at < 0) tabs.push(evidence);
        else tabs[at] = evidence;
        if (activeId === tab.id) activeId = EVIDENCE_TAB_ID;
      }
    }
    const restored: ChatWorkspaceSession = {
      tabs,
      active_tab_id: tabs.some((tab) => tab.id === activeId) ? activeId : tabs[0]?.id,
    };
    if (session.dismissed) restored.dismissed = true;
    sessions[chatSessionKey(session.loupist_id, session.thread_id)] = restored;
  }
  return { layout: { open: state.open, placement: state.placement, width: state.width, height: state.height, dockByKind: { ...state.dock_by_kind } }, sessions };
}
