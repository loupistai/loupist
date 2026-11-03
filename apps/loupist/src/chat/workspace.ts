import i18n from "@/i18n";
import type { ChatArtifactRef, ChatEvidenceRef, KnowledgeLocation } from "../types";
import { evidenceTitle } from "./evidence";

const t = i18n.getFixedT(null, "chat");

export type WorkspacePlacement = "side" | "bottom";

/**
 * The Work tab: the Loupist's charts and documents across its chats. With an item
 * open it shows that item in place; `artifactThreadId` is the chat that made it,
 * which may differ from the chat the panel belongs to.
 */
export type WorkWorkspaceTab = {
  id: typeof WORK_TAB_ID;
  kind: "work";
  artifact?: ChatArtifactRef;
  artifactThreadId?: string;
};
export const WORK_TAB_ID = "work";

export type BrowserWorkspaceTab = {
  id: string;
  kind: "browser";
  native_id?: string;
  url: string;
  title: string;
  loading: boolean;
  error?: string;
  /** Citation that opened this tab. In memory only, so a redirect of `url` keeps the passage. */
  evidence_key?: string;
};

export type FilesWorkspaceTab = { id: string; kind: "files"; title: string; path: string; filePath?: string };
export const filesWorkspaceTab = (filePath?: string): FilesWorkspaceTab => ({ id: "files", kind: "files", title: t("workspace.files"), path: filePath?.split("/").slice(0, -1).join("/") ?? "", filePath });

/** Where an Evidence entry came from: its step, and for a data number the matched cell to mark. */
export type EvidenceStepLink = { label: string; itemId?: string; cell?: { pointer: string; key: string; value: string } };

/** One thing a person reads in the Evidence tab: a retained citation or a saved Memory reader. */
export type EvidenceEntry =
  | { key: string; kind: "evidence"; title: string; evidence: ChatEvidenceRef; step?: EvidenceStepLink }
  | { key: string; kind: "knowledge"; title: string; location: KnowledgeLocation };

/** The single reader per chat. History lives in memory; evidence bytes are never persisted. */
export type EvidenceWorkspaceTab = { id: typeof EVIDENCE_TAB_ID; kind: "evidence"; entries: EvidenceEntry[]; index: number };

export const EVIDENCE_TAB_ID = "evidence";
export const EVIDENCE_HISTORY_LIMIT = 50;
/** The chat mini map is a fixed tab outside the session tabs. */
export const CHAT_MAP_TAB_ID = "chat-map";

/** Panel content is what a person reads or keeps; computation output stays beside its step. */
export const isPanelEvidence = (evidence: ChatEvidenceRef) =>
  evidence.kind === "original" || evidence.kind === "discovery" || evidence.kind === "local_file";

export const evidenceEntry = (evidence: ChatEvidenceRef, step?: EvidenceStepLink): EvidenceEntry => ({
  key: `evidence:${evidence.id}`,
  kind: "evidence",
  title: evidenceTitle(evidence),
  evidence,
  step,
});

const knowledgeEntryKey = (location: KnowledgeLocation) => {
  const objectId = location.objectId ?? location.recordId ?? location.surface;
  return `knowledge:${JSON.stringify([location.loupistId, location.surface, location.recordId, location.revisionId, objectId, location.selection])}`;
};

export const knowledgeEntry = (location: KnowledgeLocation, title: string): EvidenceEntry => ({
  key: knowledgeEntryKey(location),
  kind: "knowledge",
  title,
  location,
});

export const emptyEvidenceTab = (): EvidenceWorkspaceTab => ({ id: EVIDENCE_TAB_ID, kind: "evidence", entries: [], index: -1 });

export const currentEvidenceEntry = (tab: EvidenceWorkspaceTab | undefined): EvidenceEntry | undefined =>
  tab?.entries[tab.index];

/** The cited result the Evidence reader shows when it is the panel's active tab. */
export function shownEvidenceId(session: ChatWorkspaceSession): string | undefined {
  const tab = session.tabs.find((item) => item.id === session.active_tab_id);
  const entry = tab?.kind === "evidence" ? currentEvidenceEntry(tab) : undefined;
  return entry?.kind === "evidence" ? entry.evidence.id : undefined;
}

/** A reopened entry keeps its reader state and adopts new view requests such as a proposal review. */
const refreshEntry = (previous: EvidenceEntry, next: EvidenceEntry): EvidenceEntry => {
  if (previous.kind === "knowledge" && next.kind === "knowledge") {
    if (!next.location.view) return previous;
    return { ...previous, title: next.title, location: { ...next.location, view: { ...previous.location.view, ...next.location.view } } };
  }
  return next.kind === "evidence" && previous.kind === "evidence" && !previous.step && next.step ? next : previous;
};

/**
 * Opening the current entry does nothing; opening one already in the stack moves there
 * without duplicating it; a new entry drops forward history and keeps the newest 50.
 */
export function pushEvidenceEntry(tab: EvidenceWorkspaceTab | undefined, entry: EvidenceEntry): EvidenceWorkspaceTab {
  const base = tab ?? emptyEvidenceTab();
  const existing = base.entries.findIndex((item) => item.key === entry.key);
  if (existing >= 0) {
    const refreshed = refreshEntry(base.entries[existing]!, entry);
    if (existing === base.index && refreshed === base.entries[existing]) return base;
    const entries = refreshed === base.entries[existing] ? base.entries : base.entries.map((item, index) => index === existing ? refreshed : item);
    return { ...base, entries, index: existing };
  }
  const entries = [...base.entries.slice(0, base.index + 1), entry].slice(-EVIDENCE_HISTORY_LIMIT);
  return { ...base, entries, index: entries.length - 1 };
}

export const moveEvidenceHistory = (tab: EvidenceWorkspaceTab, delta: -1 | 1): EvidenceWorkspaceTab => {
  const index = tab.index + delta;
  return index < 0 || index >= tab.entries.length ? tab : { ...tab, index };
};

export const updateCurrentEvidenceEntry = (
  tab: EvidenceWorkspaceTab,
  update: (entry: EvidenceEntry) => EvidenceEntry,
): EvidenceWorkspaceTab => {
  const current = tab.entries[tab.index];
  if (!current) return tab;
  const next = update(current);
  return next === current ? tab : { ...tab, entries: tab.entries.map((entry, index) => index === tab.index ? next : entry) };
};

export type ChatWorkspaceTab =
  | EvidenceWorkspaceTab
  | FilesWorkspaceTab
  | WorkWorkspaceTab
  | BrowserWorkspaceTab;

export type ChatWorkspaceSession = {
  tabs: ChatWorkspaceTab[];
  active_tab_id?: string;
  /** The user closed the panel in this chat; nothing reopens it until they open it again. */
  dismissed?: boolean;
  /** Tabs that received content while the user was reading another one. In memory only. */
  badges?: string[];
};

export const workspaceTabButtonId = (tabId: string) => `workspace-tab-${tabId}`;

export const workspaceTabPanelId = (tabId: string) =>
  `workspace-tabpanel-${tabId}`;

export const workspaceTabTitle = (tab: ChatWorkspaceTab) =>
  tab.kind === "work" ? t("workspaceTabBar.work")
    : tab.kind === "files" ? t("workspace.files")
      : tab.kind === "evidence" ? t("workspaceTabBar.evidence") : tab.title;

export const emptyWorkspaceSession = (): ChatWorkspaceSession => ({ tabs: [] });

const withoutBadge = (badges: string[] | undefined, tabId: string) => {
  if (!badges?.includes(tabId)) return badges;
  const next = badges.filter((id) => id !== tabId);
  return next.length ? next : undefined;
};

/** Remember which citation opened the browser tab on screen. A later redirect keeps this binding. */
export function bindActiveBrowserPassage(session: ChatWorkspaceSession, evidenceKey: string): ChatWorkspaceSession {
  const active = session.tabs.find((tab) => tab.id === session.active_tab_id);
  if (!active || active.kind !== "browser" || active.evidence_key === evidenceKey) return session;
  return {
    ...session,
    tabs: session.tabs.map((tab) => tab.id === active.id && tab.kind === "browser" ? { ...tab, evidence_key: evidenceKey } : tab),
  };
}

/** Remember a passage in the Evidence tab without leaving the tab the person is on. */
export const stashEvidenceEntry = (session: ChatWorkspaceSession, entry: EvidenceEntry): ChatWorkspaceSession => {
  const next = pushEvidenceEntry(evidenceWorkspaceTab(session), entry);
  const existing = session.tabs.findIndex((tab) => tab.id === EVIDENCE_TAB_ID);
  if (existing >= 0 && session.tabs[existing] === next) return session;
  const tabs = [...session.tabs];
  if (existing === -1) tabs.push(next);
  else tabs[existing] = next;
  return { ...session, tabs };
};

export const activateWorkspaceTab = (
  session: ChatWorkspaceSession,
  tab: ChatWorkspaceTab,
): ChatWorkspaceSession => {
  const existing = session.tabs.findIndex((item) => item.id === tab.id);
  const tabs = [...session.tabs];
  if (existing === -1) tabs.push(tab);
  else tabs[existing] = tab;
  return { ...session, tabs, active_tab_id: tab.id, badges: withoutBadge(session.badges, tab.id) };
};

/** Content arrived for a tab the user is not reading: keep their tab and mark the new one. */
export const badgeWorkspaceTab = (
  session: ChatWorkspaceSession,
  tab: ChatWorkspaceTab,
): ChatWorkspaceSession => {
  const existing = session.tabs.findIndex((item) => item.id === tab.id);
  const tabs = [...session.tabs];
  if (existing === -1) tabs.push(tab);
  else tabs[existing] = tab;
  const badges = session.badges?.includes(tab.id) ? session.badges : [...(session.badges ?? []), tab.id];
  return { ...session, tabs, active_tab_id: session.active_tab_id ?? tab.id, badges: session.active_tab_id ? badges : withoutBadge(badges, tab.id) };
};

export const selectWorkspaceTab = (session: ChatWorkspaceSession, tabId: string): ChatWorkspaceSession =>
  ({ ...session, active_tab_id: tabId, badges: withoutBadge(session.badges, tabId) });

/** The Work tab with one item open; without an item it lists the Loupist's work. */
export const workWorkspaceTab = (artifact?: ChatArtifactRef, artifactThreadId?: string): WorkWorkspaceTab =>
  artifact && artifactThreadId ? { id: WORK_TAB_ID, kind: "work", artifact, artifactThreadId } : { id: WORK_TAB_ID, kind: "work" };

export const workTab = (session: ChatWorkspaceSession | undefined): WorkWorkspaceTab | undefined =>
  session?.tabs.find((tab): tab is WorkWorkspaceTab => tab.kind === "work");

export const browserWorkspaceTab = (url = ""): BrowserWorkspaceTab => {
  let title = t("workspace.newTab");
  if (url) {
    try {
      title = new URL(url).hostname || t("workspace.webPage");
    } catch {
      title = t("workspace.webPage");
    }
  }
  return {
    id: `browser:${crypto.randomUUID()}`,
    kind: "browser",
    url,
    title,
    loading: Boolean(url),
  };
};

export const evidenceWorkspaceTab = (session: ChatWorkspaceSession | undefined): EvidenceWorkspaceTab | undefined =>
  session?.tabs.find((tab): tab is EvidenceWorkspaceTab => tab.kind === "evidence");
