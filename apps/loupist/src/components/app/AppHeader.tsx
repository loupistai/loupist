import { Fragment, useEffect, useRef } from "react";
import { ChevronDownIcon, PanelBottomIcon, PanelRightIcon, PlusIcon, Settings2Icon } from "lucide-react";
import i18n, { useTranslation } from "@/i18n";
import { SidebarTrigger } from "@/components/ui/sidebar";
import { Button } from "../ui/button";
import { type AppTab } from "../../app/navigation";
import { modalDialogOpen } from "../../app/shortcuts";
import type { WorkspacePanelKind } from "../../app/useWorkspacePanel";
import type { WorkspacePlacement } from "../../chat/workspace";
import type { LoupistSummary } from "../../types";
import { focusForKeyboard } from "../../lib/inputModality";
import { notePanelOpenedFromHeader } from "../workspace/ChatWorkspacePanel";
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from "../ui/dropdown-menu";
import { loupistOptionLabel } from "../../lib/loupistFolder";
import { LoupistAvatar } from "./LoupistAvatar";
import { MarketStatusChip } from "./MarketStatusChip";
export { LoupistAvatar } from "./LoupistAvatar";

const tabs = ["chat", "map", "lens", "schedule"] as const;
export function AppHeader({ runningLoupists, unreadCounts, loupists, onSelect, onCreate, tab, loupist, onTabChange, onLoupistSettings, markets, workspaceOpen, workspaceKind, workspacePlacement, onWorkspaceKind, onWorkspaceSplit, onWorkspaceToggle }: {
  runningLoupists: ReadonlySet<string>; unreadCounts: Record<string, number>; loupists: LoupistSummary[]; onSelect: (id: string) => void; onCreate: () => void;
  tab: AppTab; loupist: LoupistSummary | null; onTabChange: (tab: AppTab) => void; onLoupistSettings: () => void; markets: readonly string[];
  workspaceOpen: boolean; workspaceKind: WorkspacePanelKind; workspacePlacement: WorkspacePlacement; onWorkspaceKind: (kind: WorkspacePanelKind) => void; onWorkspaceSplit: (placement: WorkspacePlacement) => void; onWorkspaceToggle: () => void;
}) {
  useTranslation();
  const openingPanel = useRef(false);
  const keyboardOpening = useRef(false);
  const panelHint = (kind: WorkspacePanelKind) => i18n.t(kind === "lens" ? "app:shell.lensHint" : kind === "world" ? "app:shell.worldHint" : "app:shell.chatMaterialsHint");
  const panelLabel = (kind: WorkspacePanelKind) => kind === "lens" ? "Lens" : kind === "world" ? "World" : i18n.t("app:shell.chatMaterials");
  const profiles = useRef<HTMLDivElement>(null);
  useEffect(() => { profiles.current?.querySelector<HTMLButtonElement>('[aria-pressed="true"]')?.scrollIntoView?.({ block: "nearest", inline: "nearest" }); }, [loupist?.id]);
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat || event.isComposing || event.altKey || !(event.metaKey || event.ctrlKey) || modalDialogOpen()) return;
      if (event.key === "\\" && (tab === "chat" || tab === "map")) {
        event.preventDefault();
        if (!workspaceOpen) {
          const current = document.activeElement;
          notePanelOpenedFromHeader(current instanceof HTMLElement && current.classList.contains("header-panel-toggle"));
        }
        onWorkspaceToggle();
        return;
      }
      const next = tabs[Number(event.key) - 1];
      if (next) { event.preventDefault(); onTabChange(next); }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [onTabChange, onWorkspaceToggle, tab, workspaceOpen]);
  return <header className="app-header" data-tauri-drag-region="deep">
    <SidebarTrigger className="app-header-sidebar-trigger" />
    {loupist && tab !== "settings" ? <>
      <nav className="loupist-header-selector" aria-label={i18n.t("app:shell.loupists")}>
        <div className="loupist-header-profiles" ref={profiles}>
          {loupists.map(item => <Fragment key={item.id}><button type="button" data-loupist-id={item.id} className="loupist-header-identity" aria-describedby={unreadCounts[item.id] > 0 ? `header-unread-${item.id}` : undefined} title={loupistOptionLabel(item,loupists)} aria-pressed={item.id === loupist.id} onClick={() => onSelect(item.id)} onFocus={event => event.currentTarget.scrollIntoView?.({ block: "nearest", inline: "nearest" })}>
            <span className="loupist-header-avatar">
              <LoupistAvatar loupist={item} />
              {runningLoupists.has(item.id) && <span className="loupist-run-indicator" aria-hidden="true" />}
            </span>
            <strong>{loupistOptionLabel(item,loupists)}</strong>
            {unreadCounts[item.id] > 0 && <><span className="shell-unread" aria-hidden="true">{unreadCounts[item.id]}</span><span className="sr-only" aria-hidden="true" id={`header-unread-${item.id}`}>{i18n.t("app:shell.unreadBriefings", { count: unreadCounts[item.id] })}</span></>}
          </button>{runningLoupists.has(item.id) && <span className="sr-only" role="status" aria-label={i18n.t("app:loupistNavigationGroup.hasActiveSessions", { label: item.name })} />}</Fragment>)}
        </div>
        <Button variant="ghost" size="icon-sm" className="loupist-header-new" aria-label={i18n.t("app:shell.newLoupist")} title={i18n.t("app:shell.newLoupist")} onClick={onCreate}><PlusIcon /></Button>
      </nav>
      <Button variant="ghost" size="icon-sm" data-header-loupist-settings aria-label={i18n.t("app:shell.loupistSettings")} title={i18n.t("app:shell.loupistSettings")} onClick={onLoupistSettings}><Settings2Icon /></Button>
      <div className="header-market-slot"><MarketStatusChip markets={markets} /></div>
      <div className="header-workspace-controls" role="group" aria-label={i18n.t("app:shell.workspaceControls")}>
        <DropdownMenu><DropdownMenuTrigger asChild><Button variant="ghost" size="sm" className="header-panel-toggle" onPointerDown={() => { keyboardOpening.current = false; }} onKeyDown={() => { keyboardOpening.current = true; }} aria-label={`${panelLabel(workspaceKind)} · ${i18n.t("app:shell.panelKind")}`} aria-controls={workspaceOpen ? "workspace-panel" : undefined}><span>{panelLabel(workspaceKind)}</span><ChevronDownIcon /></Button></DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="header-panel-menu" onCloseAutoFocus={event => { if (openingPanel.current) { openingPanel.current = false; event.preventDefault(); if (keyboardOpening.current) requestAnimationFrame(() => focusForKeyboard(document.querySelector<HTMLButtonElement>("[data-workspace-close]"))); } }}>
            <DropdownMenuRadioGroup value={workspaceKind}>
              {(["lens", "world", "materials"] as const).map(kind => <DropdownMenuRadioItem key={kind} data-workspace-kind={kind} value={kind} aria-labelledby={`panel-kind-${kind}`} aria-describedby={`panel-kind-${kind}-hint`} onSelect={() => { openingPanel.current = !workspaceOpen; notePanelOpenedFromHeader(!workspaceOpen && keyboardOpening.current); onWorkspaceKind(kind); }}><span className="header-panel-kind"><strong id={`panel-kind-${kind}`}>{panelLabel(kind)}</strong><small id={`panel-kind-${kind}-hint`}>{panelHint(kind)}</small></span></DropdownMenuRadioItem>)}
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>
        <span className="header-workspace-divider" aria-hidden="true" />
        {(["bottom", "side"] as const).map(placement => <Button key={placement} type="button" variant="ghost" size="icon-sm" className="header-workspace-split" data-workspace-split={placement} aria-label={i18n.t(placement === "bottom" ? "app:shell.splitTopBottom" : "app:shell.splitLeftRight")} title={i18n.t(placement === "bottom" ? "app:shell.splitTopBottom" : "app:shell.splitLeftRight")} aria-pressed={workspaceOpen && workspacePlacement === placement} aria-controls={workspaceOpen ? "workspace-panel" : undefined} onClick={event => { notePanelOpenedFromHeader(event.detail === 0 && !workspaceOpen); onWorkspaceSplit(placement); }}>{placement === "bottom" ? <PanelBottomIcon /> : <PanelRightIcon />}</Button>)}
      </div>
    </> : null}
  </header>;
}
