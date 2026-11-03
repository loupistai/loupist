import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { SearchIcon, XIcon } from "lucide-react";
import i18n from "@/i18n";
import { chatTitle } from "../../app/chatTitle";
import { chatSessionKey } from "../../chat/sessionRegistry";
import type { ChatThread } from "../../types";
import { Button } from "../ui/button";
import { LoupistChatItem } from "./LoupistChatItem";
export function ChatListPanel({ threads, activeThreadId, runningThreadKeys, onSelect, onRename, onDelete, onClose }: { threads: ChatThread[]; activeThreadId: string | null; runningThreadKeys: ReadonlySet<string>; onSelect: (thread: ChatThread) => void; onRename: (thread: ChatThread) => void; onDelete: (thread: ChatThread) => void; onClose: () => void }) {
  const panelRef = useRef<HTMLElement>(null);
  const previousFocus = useRef(document.activeElement);
  useEffect(() => () => { const previous = previousFocus.current; if (previous instanceof HTMLElement && previous.isConnected) previous.focus(); }, []);
  const [overlay, setOverlay] = useState(() => window.matchMedia("(max-width: 899px)").matches);
  useLayoutEffect(() => {
    const region = panelRef.current?.closest<HTMLElement>(".chat-tab-panel");
    const query = window.matchMedia("(max-width: 899px)");
    const update = () => {
      const available = region?.getBoundingClientRect().width ?? 0;
      // The list needs 280 px beside a readable 420 px conversation. A docked
      // workspace can use that room even when the desktop window itself is wide.
      setOverlay(query.matches || (available > 0 && available < 700));
    };
    update();
    const observer = new ResizeObserver(update);
    if (region) observer.observe(region);
    query.addEventListener("change", update);
    return () => { observer.disconnect(); query.removeEventListener("change", update); };
  }, []);
  useLayoutEffect(() => {
    if (!overlay) return;
    const chat = panelRef.current?.parentElement?.querySelector<HTMLElement>(".chat-layout");
    if (!chat) return;
    const previous = chat.inert;
    if (chat.contains(document.activeElement)) panelRef.current?.querySelector<HTMLInputElement>("input")?.focus();
    chat.inert = true;
    return () => { chat.inert = previous; };
  }, [overlay]);
  const [query, setQuery] = useState("");
  useEffect(() => { const key = (event: KeyboardEvent) => { if (event.key === "Escape" && !event.defaultPrevented && !panelRef.current?.closest("[inert]")) { event.preventDefault(); onClose(); } }; window.addEventListener("keydown", key); return () => window.removeEventListener("keydown", key); }, [onClose]);
  const containFocus = (event: React.KeyboardEvent<HTMLElement>) => {
    if (event.key !== "Tab" || !overlay) return;
    const controls = [...(panelRef.current?.querySelectorAll<HTMLElement>('button:not([disabled]), input:not([disabled]), [tabindex="0"]') ?? [])];
    const first = controls[0], last = controls[controls.length - 1];
    if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); }
    else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
  };
  const matches = threads.filter(thread => chatTitle(thread).toLocaleLowerCase().includes(query.toLocaleLowerCase()));
  return <><button className="chat-list-scrim" data-overlay={overlay || undefined} aria-label={i18n.t("app:shell.close")} tabIndex={-1} onClick={onClose} /><aside ref={panelRef} onKeyDown={containFocus} className="chat-list-panel" data-overlay={overlay || undefined} aria-label={i18n.t("app:shell.allChats")}><header><strong>{i18n.t("app:shell.allChats")}</strong><Button variant="ghost" size="icon-sm" aria-label={i18n.t("app:shell.close")} onClick={onClose}><XIcon /></Button></header><label className="navigation-chat-search"><SearchIcon /><input autoFocus type="search" aria-label={i18n.t("app:appNavigation.searchChats")} placeholder={i18n.t("app:appNavigation.searchChats")} value={query} onChange={event => setQuery(event.target.value)} /></label><ul>{matches.map(thread => <LoupistChatItem key={thread.id} thread={thread} active={activeThreadId === thread.id} running={runningThreadKeys.has(chatSessionKey(thread.loupist_id, thread.id))} onSelect={thread => { onSelect(thread); if (overlay) onClose(); }} onRename={onRename} onDelete={onDelete} />)}</ul>{!matches.length ? <p>{i18n.t("app:loupistNavigationGroup.noMatchingChats")}</p> : null}</aside></>;
}
