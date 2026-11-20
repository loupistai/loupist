import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import i18n, { useTranslation } from "@/i18n";
import { useSidebar } from "@/components/ui/sidebar";

/** The labelled sidebar's default width (`--gg-size-sidebar`). */
export const SIDEBAR_DEFAULT = 248;
/** The narrowest labelled sidebar: the face, a name and "Research sessions" with its plus still fit in English and Korean. */
export const SIDEBAR_MIN = 208;
/** The icon rail's width; a drag that ends left of `SIDEBAR_SNAP` becomes the rail. */
export const SIDEBAR_RAIL = 76;
export const SIDEBAR_SNAP = 140;
const SIDEBAR_CEILING = 420;
/** Arrow keys move along a 20 px grid from the minimum, which passes through the default (208, 228, 248…). */
const STEP = 20;
const stepFrom = (width: number, direction: 1 | -1) => {
  const offset = (width - SIDEBAR_MIN) / STEP;
  return SIDEBAR_MIN + STEP * (direction > 0 ? Math.floor(offset) + 1 : Math.ceil(offset) - 1);
};
const STORAGE_KEY = "loupist.sidebarWidth";

/** About a third of the window, never below the minimum. CSS applies the same limit as the window changes. */
export const sidebarMax = (windowWidth: number) => Math.max(SIDEBAR_MIN, Math.min(SIDEBAR_CEILING, Math.floor(windowWidth / 3)));
export const clampSidebarWidth = (width: number, windowWidth: number) => Math.round(Math.min(sidebarMax(windowWidth), Math.max(SIDEBAR_MIN, width)));

/** The stored width, like the last view and conversation: a per-machine window preference. */
export function readSidebarWidth(): number {
  try {
    const value = Number(localStorage.getItem(STORAGE_KEY));
    return Number.isFinite(value) && value >= SIDEBAR_MIN ? Math.min(SIDEBAR_CEILING, Math.round(value)) : SIDEBAR_DEFAULT;
  } catch { return SIDEBAR_DEFAULT; }
}
const storeSidebarWidth = (width: number) => {
  try {
    if (width === SIDEBAR_DEFAULT) localStorage.removeItem(STORAGE_KEY);
    else localStorage.setItem(STORAGE_KEY, String(width));
  } catch { /* A private store keeps the width for this session only. */ }
};
/** One custom property on the root: the sidebar, its gap and the dialogs' centring read it; React does not render per frame. */
const applySidebarWidth = (width: number) => document.documentElement.style.setProperty("--loupist-sidebar-width", `${width}px`);

/**
 * The sidebar's right edge. Dragging resizes the labelled sidebar between the minimum and about
 * a third of the window; ending left of the snap point folds it into the icon rail, and dragging
 * out of the rail restores it. Double-click returns to the default width. It never toggles on a
 * plain click: ⌘B, the View menu and the page header's toggle do that.
 */
export function SidebarResizeHandle() {
  useTranslation();
  const { open, setOpen, isMobile } = useSidebar();
  const [width, setWidth] = useState(readSidebarWidth);
  const live = useRef(width);
  const handle = useRef<HTMLDivElement>(null);
  const dragging = useRef<number | null>(null);
  useEffect(() => { applySidebarWidth(width); live.current = width; }, [width]);
  const max = sidebarMax(window.innerWidth);
  const commit = useCallback((next: number) => { setWidth(next); storeSidebarWidth(next); }, []);

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    dragging.current = event.pointerId;
    event.currentTarget.setPointerCapture?.(event.pointerId);
    document.documentElement.dataset.sidebarResizing = "";
  };
  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    if (dragging.current !== event.pointerId) return;
    const left = handle.current?.closest("[data-slot='sidebar-container']")?.getBoundingClientRect().left ?? 0;
    const x = event.clientX - left;
    if (x < SIDEBAR_SNAP) { if (open) setOpen(false); return; }
    if (!open) setOpen(true);
    const next = clampSidebarWidth(x, window.innerWidth);
    if (next === live.current) return;
    live.current = next;
    applySidebarWidth(next);
    handle.current?.setAttribute("aria-valuenow", String(next));
  };
  const onPointerUp = (event: PointerEvent<HTMLDivElement>) => {
    if (dragging.current !== event.pointerId) return;
    dragging.current = null;
    delete document.documentElement.dataset.sidebarResizing;
    commit(live.current);
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const current = open ? live.current : SIDEBAR_RAIL;
    let next: number | null = null;
    if (event.key === "ArrowRight") next = open ? stepFrom(current, 1) : SIDEBAR_MIN;
    else if (event.key === "ArrowLeft") next = current <= SIDEBAR_MIN ? SIDEBAR_RAIL : stepFrom(current, -1);
    else if (event.key === "Home") next = SIDEBAR_RAIL;
    else if (event.key === "End") next = max;
    if (next === null) return;
    event.preventDefault();
    if (next === SIDEBAR_RAIL) { setOpen(false); return; }
    if (!open) setOpen(true);
    commit(clampSidebarWidth(next, window.innerWidth));
  };
  if (isMobile) return null;
  const value = open ? Math.min(width, max) : SIDEBAR_RAIL;
  return <div ref={handle} className="sidebar-resize-handle" data-sidebar-resize role="separator" aria-orientation="vertical" tabIndex={0}
    aria-label={i18n.t("app:shell.resizeSidebar")} title={i18n.t("app:shell.resizeSidebarHint")}
    aria-valuemin={SIDEBAR_RAIL} aria-valuemax={max} aria-valuenow={value}
    onPointerDown={onPointerDown} onPointerMove={onPointerMove} onPointerUp={onPointerUp} onPointerCancel={onPointerUp}
    onDoubleClick={() => { if (!open) setOpen(true); commit(SIDEBAR_DEFAULT); }} onKeyDown={onKeyDown} />;
}
