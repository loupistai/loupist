import { useCallback, useEffect, useReducer, useRef } from "react";
import type { LoupistSettingsSection } from "../components/agents/LoupistSettings";
import type { AssetPageTarget } from "../contracts/assets";
import type { PackOpenResult } from "../contracts/sharing";
import { APP_DESTINATIONS, type AppDestinationId, type AppTab } from "./navigation";
import type { WorkspacePanelKind } from "./useWorkspacePanel";

const LAST_VIEW_KEY = "loupist.lastView";

export type AssetPageLayer = AssetPageTarget & { key: number };

/**
 * What the stage shows: one page, and the layers over it. A page change closes
 * Loupist settings and nothing else. An asset page, the Gallery and a pack review
 * stay until they are closed, so closing a review returns to the Gallery, and
 * closing Loupist settings returns to the layer under it.
 */
export type AppSurface = {
  tab: AppTab;
  loupistSettings: { loupistId: string; section?: LoupistSettingsSection; remove: boolean } | null;
  assetPage: AssetPageLayer | null;
  gallery: boolean;
  packReview: { result: PackOpenResult; use: boolean } | null;
};

export type SurfaceAction =
  | { type: "page"; tab: AppTab }
  | { type: "openLoupistSettings"; loupistId: string; section?: LoupistSettingsSection; remove: boolean }
  | { type: "closeLoupistSettings" }
  | { type: "openAssetPage"; page: AssetPageLayer }
  | { type: "closeAssetPage" }
  /** Another Loupist became current: its asset page cannot stay open. */
  | { type: "keepAssetPageFor"; loupistId: string | null }
  | { type: "openGallery" }
  | { type: "closeGallery" }
  | { type: "openPackReview"; result: PackOpenResult; use: boolean }
  | { type: "closePackReview" }
  /** The asset page, the Gallery and the pack review. */
  | { type: "closeLayers" };

export function surfaceReducer(surface: AppSurface, action: SurfaceAction): AppSurface {
  switch (action.type) {
    case "page": return surface.tab === action.tab && !surface.loupistSettings ? surface : { ...surface, tab: action.tab, loupistSettings: null };
    case "openLoupistSettings": return { ...surface, loupistSettings: { loupistId: action.loupistId, section: action.section, remove: action.remove } };
    case "closeLoupistSettings": return surface.loupistSettings ? { ...surface, loupistSettings: null } : surface;
    case "openAssetPage": return { ...surface, loupistSettings: null, assetPage: action.page };
    case "closeAssetPage": return surface.assetPage ? { ...surface, assetPage: null } : surface;
    case "keepAssetPageFor": return surface.assetPage && surface.assetPage.loupistId !== action.loupistId ? { ...surface, assetPage: null } : surface;
    case "openGallery": return surface.gallery ? surface : { ...surface, gallery: true };
    case "closeGallery": return surface.gallery ? { ...surface, gallery: false } : surface;
    case "openPackReview": return { ...surface, packReview: { result: action.result, use: action.use } };
    case "closePackReview": return surface.packReview ? { ...surface, packReview: null } : surface;
    case "closeLayers": return surface.assetPage || surface.gallery || surface.packReview ? { ...surface, assetPage: null, gallery: false, packReview: null } : surface;
  }
}

/** What each part of the shell may show, derived from one surface. */
export function surfaceView(surface: AppSurface, selectedLoupistId: string | null) {
  const loupistSettingsOpen = surface.loupistSettings !== null;
  // An asset page belongs to one Loupist and stays under app Settings.
  const assetPageOpen = Boolean(surface.assetPage && surface.assetPage.loupistId === selectedLoupistId && surface.tab !== "settings");
  /** A layer covers the page: the page beneath is inert. */
  const covered = loupistSettingsOpen || assetPageOpen || surface.gallery || surface.packReview !== null;
  return {
    loupistSettingsOpen,
    assetPageOpen,
    covered,
    /** Chat is the page and nothing covers it: the sidebar marks its thread, and the workspace panel may open. */
    chatVisible: surface.tab === "chat" && !covered,
    /** Only Loupist settings pause the Chat; it keeps streaming under the other layers. */
    chatActive: surface.tab === "chat" && !loupistSettingsOpen,
  };
}

const initialSurface = (): AppSurface => ({
  tab: localStorage.getItem(LAST_VIEW_KEY) === "schedule" ? "schedule" : "chat",
  loupistSettings: null,
  assetPage: null,
  gallery: false,
  packReview: null,
});

/**
 * The shell's page and layers. Every navigation moves the epoch, so a hand-off
 * that resolves after the user went elsewhere can tell it is stale.
 */
export function useAppSurface(workspace: { hydrated: boolean; selectKind: (kind: WorkspacePanelKind) => void }) {
  const [surface, dispatch] = useReducer(surfaceReducer, undefined, initialSurface);
  const navigationEpoch = useRef(0);
  const { hydrated, selectKind } = workspace;
  // Map and Lens were pages before they became panels; a saved one opens as its panel once.
  const legacyPanel = useRef(localStorage.getItem(LAST_VIEW_KEY));
  useEffect(() => {
    if (!hydrated) return;
    const previous = legacyPanel.current;
    legacyPanel.current = null;
    if (previous === "map" || previous === "lens") {
      selectKind(previous === "map" ? "world" : "lens");
      localStorage.setItem(LAST_VIEW_KEY, "chat");
    }
  }, [hydrated, selectKind]);
  const setTab = useCallback((tab: AppTab) => {
    navigationEpoch.current += 1;
    if (tab !== "settings") localStorage.setItem(LAST_VIEW_KEY, tab);
    dispatch({ type: "page", tab });
  }, []);
  /** World or Lens beside the Chat. */
  const showPanel = useCallback((kind: Exclude<WorkspacePanelKind, "materials">) => {
    navigationEpoch.current += 1;
    selectKind(kind);
    localStorage.setItem(LAST_VIEW_KEY, "chat");
    dispatch({ type: "page", tab: "chat" });
  }, [selectKind]);
  const goTo = useCallback((id: AppDestinationId) => {
    const destination = APP_DESTINATIONS.find((item) => item.id === id)!;
    if ("page" in destination.target) setTab(destination.target.page);
    else showPanel(destination.target.panel);
  }, [setTab, showPanel]);
  return { surface, dispatch, navigationEpoch, setTab, showPanel, goTo };
}
