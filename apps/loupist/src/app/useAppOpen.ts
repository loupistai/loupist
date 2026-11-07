import i18n from "@/i18n";
import { useCallback, useRef, useState } from "react";
import type { AssetPageTarget } from "../contracts/assets";
import type { ChatArtifactRef, ChatEvidenceRef, KnowledgeLocation } from "../types";
import { evidenceTitle } from "../chat/evidence";
import { evidenceTarget, externalUrl, type ChatDestination, type Open, type OpenHow } from "../chat/navigation";
import type { EvidenceStepLink } from "../chat/workspace";
import { lensMemoryLocation } from "../knowledge/location";
import { displayHost } from "../lib/displayHost";
import { EMPTY_CHAT_THREAD_ID } from "./emptyChat";
import type { SettingsSection } from "./navigation";

/** The shell as the router sees it. Every field is read when a leaf is clicked, never when it renders. */
export type AppOpenShell = {
  openExternalUrl: (url: string) => Promise<void>;
  loupistId: string | null;
  /** The visible chat's thread, which the panel belongs to. */
  threadId: string | null;
  panel: {
    openEvidence: (loupistId: string, threadId: string, evidence: ChatEvidenceRef, step?: EvidenceStepLink) => void;
    openBrowser: (loupistId: string, threadId: string, url?: string) => void;
    openMemory: (loupistId: string, threadId: string, location: KnowledgeLocation, title: string) => void;
    openArtifact: (loupistId: string, threadId: string, artifact: ChatArtifactRef) => void;
    openFiles: (loupistId: string, threadId: string, path?: string) => void;
  };
  /** Shows another chat of the selected Loupist, on the Chat tab. */
  showThread: (loupistId: string, threadId: string) => void;
  openKnowledgePage: (location: KnowledgeLocation) => void;
  openAssetPage: (target: AssetPageTarget) => void;
  openScheduledTask: (taskId: string) => void;
  showBriefing: (loupistId: string, runId: string) => void;
  openSettings: (section: SettingsSection) => void;
};

export type PanelAnnouncement = { text: string; key: number };

/** Opens beside a chat other than the visible one (a briefing's own thread), which is shown first. */
export type OpenIn = (threadId: string | null, to: ChatDestination, how?: OpenHow) => Promise<void>;

async function route(shell: AppOpenShell, threadId: string | null, to: ChatDestination, how: OpenHow | undefined, announce: (title: string) => void): Promise<void> {
  const external = how?.external ? externalUrl(to) : null;
  if (external) { await shell.openExternalUrl(external); return; }
  const { loupistId } = shell;
  // The panel belongs to a chat: the one asked for, shown first, or the visible one.
  const beside = (own?: string) => {
    const id = threadId ?? own ?? shell.threadId;
    if (loupistId && threadId) shell.showThread(loupistId, threadId);
    return loupistId && id ? { loupistId, threadId: id } : null;
  };
  switch (to.kind) {
    case "evidence": {
      // A step, a quote, Memory or an asset page resolves in the answer's TurnProvider;
      // outside one the card's fallback stands in.
      const target = evidenceTarget(to.evidence, undefined, loupistId ?? "").kind;
      if (target !== "browser" && target !== "reader") { if (to.fallback) await route(shell, threadId, to.fallback, how, announce); return; }
      const chat = beside();
      if (!chat) return;
      shell.panel.openEvidence(chat.loupistId, chat.threadId, to.evidence, to.step);
      announce(evidenceTitle(to.evidence));
      return;
    }
    case "link": {
      const url = externalUrl(to);
      if (!loupistId || !url) return;
      // A browser tab opens before the first chat exists too.
      const chat = beside() ?? { loupistId, threadId: EMPTY_CHAT_THREAD_ID };
      shell.panel.openBrowser(chat.loupistId, chat.threadId, url);
      announce(displayHost(url));
      return;
    }
    case "lens": {
      if (!loupistId) return;
      const { location, title } = lensMemoryLocation(loupistId, to.recordId);
      await route(shell, threadId, { kind: "memory", location, title }, how, announce);
      return;
    }
    case "memory": {
      if (to.location.surface === "world") { shell.openKnowledgePage(to.location); return; }
      const chat = beside();
      if (!chat) return;
      shell.panel.openMemory(chat.loupistId, chat.threadId, to.location, to.title);
      announce(to.title);
      return;
    }
    case "page":
      shell.openKnowledgePage(to.location);
      return;
    case "artifact": {
      const chat = beside(to.threadId);
      if (!chat) return;
      shell.panel.openArtifact(chat.loupistId, chat.threadId, to.artifact);
      announce(to.artifact.title);
      return;
    }
    case "file": {
      const chat = beside();
      if (!chat) return;
      shell.panel.openFiles(chat.loupistId, chat.threadId, to.path);
      announce(to.path?.split("/").at(-1) || i18n.t("chat:workspace.files"));
      return;
    }
    case "asset":
      shell.openAssetPage(to.target);
      return;
    case "schedule":
      shell.openScheduledTask(to.taskId);
      return;
    case "chat":
      if (loupistId) shell.showThread(loupistId, to.threadId);
      return;
    case "briefing":
      if (loupistId) shell.showBriefing(loupistId, to.runId);
      return;
    case "settings":
      shell.openSettings(to.section);
      return;
  }
}

/**
 * The shell's one opener. `open` routes a destination to the panel beside the
 * visible chat or to the view that owns it; `openIn` routes it beside another
 * chat of the same Loupist. Panel tabs open without moving focus, so one polite
 * region (`announcement`) says what opened.
 */
export function useAppOpen(shell: AppOpenShell): { open: Open; openIn: OpenIn; announcement: PanelAnnouncement | null } {
  const latest = useRef(shell);
  latest.current = shell;
  const [announcement, setAnnouncement] = useState<PanelAnnouncement | null>(null);
  const openIn = useCallback<OpenIn>((threadId, to, how) => route(latest.current, threadId, to, how, (title) =>
    setAnnouncement((current) => ({ text: i18n.t("chat:navigation.openedInPanel", { title }), key: (current?.key ?? 0) + 1 }))), []);
  const open = useCallback<Open>((to, how) => openIn(null, to, how), [openIn]);
  return { open, openIn, announcement };
}
