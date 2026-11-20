import { ChevronsUpDownIcon, PlusIcon, Settings2Icon } from "lucide-react";
import i18n, { useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuRadioGroup,
  DropdownMenuRadioItem, DropdownMenuSeparator, DropdownMenuTrigger,
} from "../ui/dropdown-menu";
import { loupistOptionLabel } from "../../lib/loupistFolder";
import type { LoupistSummary } from "../../types";
import { LoupistAvatar } from "./LoupistAvatar";

/**
 * The first thing in the sidebar: the current Loupist's face, name and focus. The row opens
 * the switcher, which lists every Loupist, its settings and New Loupist.
 */
export function LoupistSwitcher({ loupist, loupists, runningLoupists, unreadCounts, iconOnly, onSelect, onCreate, onSettings }: {
  loupist: LoupistSummary;
  loupists: LoupistSummary[];
  runningLoupists: ReadonlySet<string>;
  unreadCounts: Record<string, number>;
  iconOnly: boolean;
  onSelect: (id: string) => void;
  onCreate: () => void;
  onSettings: () => void;
}) {
  useTranslation();
  const name = loupistOptionLabel(loupist, loupists);
  const focus = loupist.philosophy.trim();
  // Unread briefings for the current Loupist show on its Schedule row; the switcher marks the others.
  const otherUnread = loupists.some(item => item.id !== loupist.id && (unreadCounts[item.id] ?? 0) > 0);
  return <nav className="loupist-header-selector" data-testid="loupist-header-selector" aria-label={i18n.t("app:shell.loupists")}>
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button type="button" className="loupist-header-identity" data-testid="loupist-header-identity" data-loupist-id={loupist.id}
          title={i18n.t("app:shell.switchLoupist")} aria-label={iconOnly ? name : undefined}>
          <span className="loupist-header-avatar">
            <LoupistAvatar loupist={loupist} size={32} busy={runningLoupists.has(loupist.id)} />
            {otherUnread ? <span className="loupist-switcher-unread" aria-hidden="true" /> : null}
          </span>
          {iconOnly ? null : <>
            <span className="loupist-identity-text"><strong>{name}</strong>{focus ? <small>{focus}</small> : null}</span>
            <ChevronsUpDownIcon className="loupist-switcher-icon" aria-hidden="true" />
          </>}
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent className="loupist-switcher-menu" align="start" side={iconOnly ? "right" : "bottom"}>
        <DropdownMenuLabel>{i18n.t("app:shell.loupists")}</DropdownMenuLabel>
        <DropdownMenuRadioGroup value={loupist.id}>
          {loupists.map(item => {
            const unread = unreadCounts[item.id] ?? 0;
            return <DropdownMenuRadioItem key={item.id} value={item.id} data-loupist-option={item.id}
              aria-describedby={unread > 0 ? `switcher-unread-${item.id}` : undefined} onSelect={() => onSelect(item.id)}>
              <span className="loupist-header-avatar">
                <LoupistAvatar loupist={item} size={24} busy={runningLoupists.has(item.id)} />
              </span>
              <span className="loupist-switcher-name">{loupistOptionLabel(item, loupists)}</span>
              {unread > 0 ? <><span className="shell-unread" aria-hidden="true">{formatNumber(unread)}</span><span className="sr-only" aria-hidden="true" id={`switcher-unread-${item.id}`}>{i18n.t("app:shell.unreadBriefings", { count: unread })}</span></> : null}
            </DropdownMenuRadioItem>;
          })}
        </DropdownMenuRadioGroup>
        <DropdownMenuSeparator />
        <DropdownMenuItem data-header-loupist-settings onSelect={onSettings}><Settings2Icon aria-hidden="true" />{i18n.t("app:shell.loupistSettings")}</DropdownMenuItem>
        <DropdownMenuItem className="loupist-header-new" data-testid="loupist-header-new" onSelect={onCreate}><PlusIcon aria-hidden="true" />{i18n.t("app:shell.newLoupist")}</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
    {/* Background work stays announced while the switcher is closed. */}
    {loupists.filter(item => runningLoupists.has(item.id)).map(item => <span key={item.id} className="sr-only" role="status" aria-label={i18n.t("app:loupistNavigationGroup.hasActiveSessions", { label: item.name })} />)}
  </nav>;
}
