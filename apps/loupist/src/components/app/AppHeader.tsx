import { useEffect } from "react";
import { ActivityIcon, ArrowLeftIcon, PanelBottomIcon, PanelRightIcon } from "lucide-react";
import i18n, { useTranslation } from "@/i18n";
import { SidebarTrigger, useSidebar } from "@/components/ui/sidebar";
import { Button } from "../ui/button";
import { destinationForKey, type AppDestinationId, type AppTab } from "../../app/navigation";
import { modalDialogOpen } from "../../app/shortcuts";
import type { LoupistSummary } from "../../types";
import type { WorkspacePlacement } from "../../chat/workspace";
import { notePanelOpenedFromHeader } from "../workspace/ChatWorkspacePanel";
import { LoupistAvatar } from "./LoupistAvatar";
import { useRailWidth } from "./AppNavigation";
export { LoupistAvatar } from "./LoupistAvatar";

/**
 * The page header: one 56 px row that says who or what the page is, with its actions at the
 * right: on a conversation, the side panel's two splits (top and bottom, left and right) and the activity sheet. On Bot it is the Loupist's face and name, and "Working" while it has a run (its focus is in the sidebar); elsewhere the page's
 * or the research session's title; a Map or Lens page entered from a conversation leads with the way back to it.
 * The sidebar toggle appears here while the sidebar is an
 * icon rail or a drawer, and in the rail's width range after it expands the rail. It also owns ⌘1–⌘4 and ⌘\.
 */
export function AppHeader({ tab, loupist, running, title, back = null, onNavigate, materials, workspaceOpen, workspacePlacement, onWorkspaceToggle, onWorkspaceSplit }: {
  tab: AppTab;
  loupist: LoupistSummary | null;
  /** The current Loupist has a reply or scheduled run in progress. */
  running: boolean;
  /** The page or session title. Without one, the header shows the Loupist (its Bot). */
  title: string | null;
  /** Returns to the conversation a Map or Lens page was opened from. */
  back?: { label: string; onBack: () => void } | null;
  onNavigate: (destination: AppDestinationId) => void;
  /** The chat's materials panel (map, evidence, outputs), when the page is a conversation. */
  materials: { open: boolean; onToggle: () => void } | null;
  workspaceOpen: boolean;
  /** Where the side panel docks: beside the conversation or under it. */
  workspacePlacement: WorkspacePlacement;
  onWorkspaceToggle: () => void;
  /** Opens the panel docked there, or closes it when it is already open there. */
  onWorkspaceSplit: (placement: WorkspacePlacement) => void;
}) {
  useTranslation();
  const { isMobile, state } = useSidebar();
  // In the rail's width range the toggle stays after it expands the rail, so the same button folds it back.
  const railWidth = useRailWidth();
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat || event.isComposing || event.altKey || !(event.metaKey || event.ctrlKey) || modalDialogOpen()) return;
      if (event.key === "\\" && tab === "chat") {
        event.preventDefault();
        if (!workspaceOpen) {
          const current = document.activeElement;
          notePanelOpenedFromHeader(current instanceof HTMLElement && current.classList.contains("header-panel-toggle"));
        }
        onWorkspaceToggle();
        return;
      }
      const next = destinationForKey(event.key);
      if (next) { event.preventDefault(); onNavigate(next.id); }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [onNavigate, onWorkspaceToggle, tab, workspaceOpen]);
  return <header className="app-header" data-tauri-drag-region="deep">
    {isMobile || railWidth || state === "collapsed" ? <SidebarTrigger className="app-header-sidebar-trigger" aria-label={i18n.t("app:macTitlebarControls.showOrHideSidebar")} /> : null}
    {back ? <Button type="button" variant="ghost" size="sm" className="app-header-back" data-page-back onClick={back.onBack}><ArrowLeftIcon aria-hidden="true" />{back.label}</Button> : null}
    {title !== null ? <strong className="app-header-title" data-tauri-drag-region="deep">{title}</strong>
      : loupist ? <div className="app-header-who" data-busy={running || undefined} data-tauri-drag-region="deep">
        <LoupistAvatar loupist={loupist} size={36} busy={running} />
        <span className="app-header-who-text"><strong>{loupist.name}</strong>
          {running ? <small>{i18n.t("app:shell.working")}</small> : null}</span>
      </div> : null}
    <span className="app-header-space" data-tauri-drag-region="deep" />
    {materials ? (["bottom", "side"] as const).map(placement => <Button key={placement} type="button" variant="ghost" size="icon-sm" className="header-workspace-split" data-workspace-split={placement}
      aria-label={i18n.t(placement === "bottom" ? "app:shell.splitTopBottom" : "app:shell.splitLeftRight")} title={i18n.t(placement === "bottom" ? "app:shell.splitTopBottom" : "app:shell.splitLeftRight")}
      aria-pressed={workspaceOpen && workspacePlacement === placement} aria-controls={workspaceOpen ? "workspace-panel" : undefined}
      onClick={event => { notePanelOpenedFromHeader(event.detail === 0 && !workspaceOpen); onWorkspaceSplit(placement); }}>
      {placement === "bottom" ? <PanelBottomIcon aria-hidden="true" /> : <PanelRightIcon aria-hidden="true" />}</Button>) : null}
    {/* Opens or closes the conversation's activity sheet (ActivitySheet.tsx), which holds the way to the chat's materials. */}
    {materials ? <Button variant="ghost" size="icon-sm" className="header-panel-toggle" data-testid="header-panel-toggle" data-header-activity
      aria-label={i18n.t("chat:activitySheet.open")} title={i18n.t("chat:activitySheet.openHint")}
      onClick={() => window.dispatchEvent(new CustomEvent("loupist:open-activity", { detail: { toggle: true } }))}><ActivityIcon aria-hidden="true" /></Button> : null}
  </header>;
}
