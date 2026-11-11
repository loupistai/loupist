import i18n, { useTranslation } from "@/i18n";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type FormEvent,
} from "react";
import type {
  BrowserBounds,
  BrowserTabEvent,
  BrowserUrlError,
  LoupistBridge,
} from "../../types";
import type {
  BrowserWorkspaceTab,
  ChatWorkspaceSession,
  ChatWorkspaceTab,
} from "../../chat/workspace";
import { commandErrorCode, errorMessage } from "../../lib/errors";
import { parseCredentialFreeHttpUrl } from "../../lib/credentialFreeUrl";

const t = i18n.getFixedT(null, "chat");

const browserUrlMessage = (reason: BrowserUrlError) => {
  switch (reason) {
    case "too_long": return t("useNativeBrowserTabs.addressTooLong");
    case "credentials": return t("useNativeBrowserTabs.addressContainsSignInDetails");
    case "unsupported_protocol": return t("useNativeBrowserTabs.onlyWebAddresses");
    default: return t("useNativeBrowserTabs.invalidWebAddress");
  }
};

const browserFailureMessage = (cause: unknown, fallback: string) => {
  switch (commandErrorCode(cause)) {
    case "browser_url_too_long": return browserUrlMessage("too_long");
    case "browser_url_invalid": return browserUrlMessage("invalid");
    case "browser_url_credentials": return browserUrlMessage("credentials");
    case "browser_url_unsupported_protocol": return browserUrlMessage("unsupported_protocol");
    default: return fallback;
  }
};

type Options = {
  bridge: LoupistBridge;
  loupistId: string;
  threadId: string;
  open: boolean;
  session: ChatWorkspaceSession;
  activeTab?: BrowserWorkspaceTab;
  menuOpen: boolean;
  placement: "side" | "bottom";
  width: number;
  height: number;
  onUpdateTab: (
    tabId: string,
    update: (tab: ChatWorkspaceTab) => ChatWorkspaceTab,
  ) => void;
  onOpenLink: (url: string) => void;
};

const browserBounds = (element: HTMLElement | null): BrowserBounds => {
  const rect = element?.getBoundingClientRect();
  return {
    x: Math.max(0, Math.round(rect?.left ?? 0)),
    y: Math.max(0, Math.round(rect?.top ?? 0)),
    width: Math.max(1, Math.round(rect?.width ?? 1)),
    height: Math.max(1, Math.round(rect?.height ?? 1)),
  };
};

const normalizeBrowserAddress = (raw: string) => {
  const trimmed = raw.trim();
  if (!trimmed || /\s/.test(trimmed)) {
    throw new Error(t("useNativeBrowserTabs.enterAWebAddressNotASearchQuery"));
  }
  const hasScheme = /^[a-z][a-z\d+.-]*:/i.test(trimmed);
  const hostWithPort = /^(?:localhost|[^/?#:\s]+\.[^/?#:\s]+|\[[\da-f:.]+\]):\d+(?:[/?#]|$)/i.test(trimmed);
  const candidate = hasScheme && !hostWithPort ? trimmed : `https://${trimmed}`;
  let url: URL;
  try { url = new URL(candidate); }
  catch { throw new Error(browserUrlMessage("invalid")); }
  if (!["http:", "https:"].includes(url.protocol)) throw new Error(browserUrlMessage("unsupported_protocol"));
  if (url.username || url.password) throw new Error(browserUrlMessage("credentials"));
  if (new TextEncoder().encode(url.href).byteLength > 8 * 1024) throw new Error(browserUrlMessage("too_long"));
  const parsed = parseCredentialFreeHttpUrl(candidate);
  if (!parsed) {
    throw new Error(browserUrlMessage("invalid"));
  }
  return parsed.href;
};

export function useNativeBrowserTabs({
  bridge,
  loupistId,
  threadId,
  open,
  session,
  activeTab,
  menuOpen,
  placement,
  width,
  height,
  onUpdateTab,
  onOpenLink,
}: Options) {
  useTranslation("chat");
  const [address, setAddress] = useState("");
  const viewportRef = useRef<HTMLDivElement>(null);
  const lastBoundsRef = useRef<BrowserBounds>({ x: 0, y: 0, width: 1, height: 1 });
  const openingTabsRef = useRef(new Set<string>());
  const mountedRef = useRef(true);
  const scopeRef = useRef({ loupistId, threadId, epoch: 0 });
  if (scopeRef.current.loupistId !== loupistId || scopeRef.current.threadId !== threadId) {
    scopeRef.current = { loupistId, threadId, epoch: scopeRef.current.epoch + 1 };
  }
  const managedBrowserIdsRef = useRef(new Set<string>());
  const sentBoundsRef = useRef(new Map<string, string>());
  const boundsFrameRef = useRef<number | null>(null);
  const boundsSyncingRef = useRef(false);
  const boundsDirtyRef = useRef(false);
  const tabsRef = useRef(session.tabs);
  tabsRef.current = session.tabs;
  const surfaceRef = useRef({
    open,
    activeTabId: activeTab?.id,
    menuOpen,
  });
  surfaceRef.current = {
    open,
    activeTabId: activeTab?.id,
    menuOpen,
  };

  useLayoutEffect(() => {
    mountedRef.current = true;
    return () => { mountedRef.current = false; };
  }, []);

  useEffect(() => {
    setAddress(activeTab?.url ?? "");
  }, [activeTab?.id, activeTab?.url]);

  const updateFromEvent = useCallback(
    (localTabId: string, event: BrowserTabEvent) => {
      if (event.type === "open_requested") {
        if (parseCredentialFreeHttpUrl(event.url)) onOpenLink(event.url);
        return;
      }
      onUpdateTab(localTabId, (tab) => {
        if (tab.kind !== "browser") return tab;
        if (event.type === "load_started") {
          if (!parseCredentialFreeHttpUrl(event.url)) return tab;
          return { ...tab, url: event.url, loading: true, error: undefined };
        }
        if (event.type === "load_finished") {
          if (!parseCredentialFreeHttpUrl(event.url)) return tab;
          return { ...tab, url: event.url, loading: false };
        }
        if (event.type === "title_changed") {
          return { ...tab, title: event.title || tab.title };
        }
        if (event.type === "navigation_blocked") {
          return { ...tab, loading: false, error: browserUrlMessage(event.reason) };
        }
        return {
          ...tab,
          loading: false,
          error: t("useNativeBrowserTabs.downloadsAreNotAvailableInTheInAppBrowser"),
        };
      });
    },
    [onOpenLink, onUpdateTab],
  );

  useEffect(() => {
    if (!open || !activeTab?.url || activeTab.native_id || activeTab.error) return;
    const epoch = scopeRef.current.epoch;
    const openingKey = `${epoch}:${activeTab.id}`;
    if (openingTabsRef.current.has(openingKey)) return;
    openingTabsRef.current.add(openingKey);
    const localTabId = activeTab.id;
    const stillOwned = () => mountedRef.current && scopeRef.current.epoch === epoch &&
      tabsRef.current.some((tab) => tab.id === localTabId && tab.kind === "browser");
    const bounds = browserBounds(viewportRef.current);
    lastBoundsRef.current = bounds;
    void bridge
      .browserTabOpen(
        { url: activeTab.url, bounds, visible: false },
        (event) => { if (stillOwned()) updateFromEvent(localTabId, event); },
      )
      .then((native) => {
        if (!stillOwned()) {
          // Close a late child whose tab or conversation has already gone away.
          void bridge.browserTabClose(native.tab_id).catch(() => undefined);
          return;
        }
        onUpdateTab(localTabId, (tab) =>
          tab.kind === "browser"
            ? {
                ...tab,
                native_id: native.tab_id,
                url: native.url,
                title: native.title,
                loading: native.loading,
                error: undefined,
              }
            : tab,
        );
      })
      .catch((cause) => {
        if (!stillOwned()) return;
        onUpdateTab(localTabId, (tab) =>
          tab.kind === "browser"
            ? {
                ...tab,
                loading: false,
                error: browserFailureMessage(cause, t("useNativeBrowserTabs.couldNotOpenThisWebPage")),
              }
            : tab,
        );
      })
      .finally(() => openingTabsRef.current.delete(openingKey));
  }, [
    activeTab,
    bridge,
    loupistId,
    menuOpen,
    onUpdateTab,
    open,
    threadId,
    updateFromEvent,
  ]);

  const flushBounds = useCallback(async () => {
    if (boundsSyncingRef.current) return;
    boundsSyncingRef.current = true;
    try {
      while (boundsDirtyRef.current) {
        boundsDirtyRef.current = false;
        const bounds = viewportRef.current
          ? browserBounds(viewportRef.current)
          : lastBoundsRef.current;
        lastBoundsRef.current = bounds;

        const currentTabs = tabsRef.current.flatMap((tab) =>
          tab.kind === "browser" && tab.native_id ? [tab] : [],
        );
        const currentIds = new Set(currentTabs.map((tab) => tab.native_id!));
        const removedIds = [...managedBrowserIdsRef.current].filter(
          (tabId) => !currentIds.has(tabId),
        );
        managedBrowserIdsRef.current = currentIds;

        const surface = surfaceRef.current;
        const requests = [
          ...currentTabs.map((tab) => ({
            tab_id: tab.native_id!,
            bounds,
            visible:
              surface.open &&
              tab.id === surface.activeTabId &&
              !surface.menuOpen,
          })),
          ...removedIds.map((tab_id) => ({
            tab_id,
            bounds,
            visible: false,
          })),
        ];

        await Promise.all(
          requests.map(async (request) => {
            const key = [
              request.bounds.x,
              request.bounds.y,
              request.bounds.width,
              request.bounds.height,
              request.visible ? 1 : 0,
            ].join(":");
            if (sentBoundsRef.current.get(request.tab_id) === key) return;
            sentBoundsRef.current.set(request.tab_id, key);
            try {
              await bridge.browserTabSetBounds(request);
            } catch {
              if (sentBoundsRef.current.get(request.tab_id) === key) {
                sentBoundsRef.current.delete(request.tab_id);
              }
            }
          }),
        );

        for (const tabId of removedIds) sentBoundsRef.current.delete(tabId);
      }
    } finally {
      boundsSyncingRef.current = false;
    }
  }, [bridge]);

  const scheduleBounds = useCallback(() => {
    boundsDirtyRef.current = true;
    if (boundsFrameRef.current !== null) return;
    boundsFrameRef.current = window.requestAnimationFrame(() => {
      boundsFrameRef.current = null;
      void flushBounds();
    });
  }, [flushBounds]);

  useLayoutEffect(() => {
    scheduleBounds();
  }, [
    activeTab?.id,
    loupistId,
    height,
    menuOpen,
    open,
    placement,
    scheduleBounds,
    session.tabs,
    threadId,
    width,
  ]);

  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    const observer = new ResizeObserver(scheduleBounds);
    observer.observe(viewport);
    window.addEventListener("resize", scheduleBounds);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", scheduleBounds);
    };
  }, [activeTab?.id, scheduleBounds]);

  useEffect(
    () => () => {
      if (boundsFrameRef.current !== null) {
        window.cancelAnimationFrame(boundsFrameRef.current);
        boundsFrameRef.current = null;
      }
      surfaceRef.current = { ...surfaceRef.current, open: false };
      boundsDirtyRef.current = true;
      void flushBounds();
    },
    [flushBounds],
  );

  const navigateTo = async (url: string) => {
    if (!activeTab) return;
    if (openingTabsRef.current.has(`${scopeRef.current.epoch}:${activeTab.id}`)) return;
    try {
      onUpdateTab(activeTab.id, (tab) =>
        tab.kind === "browser"
          ? {
              ...tab,
              url,
              title: new URL(url).hostname,
              loading: true,
              error: undefined,
            }
          : tab,
      );
      if (activeTab.native_id) {
        await bridge.browserTabNavigate(activeTab.native_id, url);
      }
    } catch (cause) {
      onUpdateTab(activeTab.id, (tab) =>
        tab.kind === "browser"
          ? {
              ...tab,
              loading: false,
              error: browserFailureMessage(cause, t("useNativeBrowserTabs.couldNotOpenThisWebPage")),
            }
          : tab,
      );
    }
  };

  const navigate = async (event: FormEvent) => {
    event.preventDefault();
    if (!activeTab) return;
    try {
      await navigateTo(normalizeBrowserAddress(address));
    } catch (cause) {
      onUpdateTab(activeTab.id, (tab) => tab.kind === "browser"
        ? { ...tab, loading: false, error: errorMessage(cause, t("useNativeBrowserTabs.invalidWebAddress")) } : tab);
    }
  };

  const retry = async () => {
    if (!activeTab?.url) return;
    setAddress(activeTab.url);
    await navigateTo(activeTab.url);
  };

  const openExternal = async (url: string) => {
    if (!activeTab) return;
    try {
      await bridge.openExternalUrl(url);
    } catch (cause) {
      onUpdateTab(activeTab.id, (tab) => tab.kind === "browser"
        ? { ...tab, error: browserFailureMessage(cause, t("useNativeBrowserTabs.couldNotOpenTheSystemBrowser")) } : tab);
    }
  };

  const performAction = async (action: "back" | "forward" | "reload") => {
    if (!activeTab?.native_id) return;
    try {
      if (action === "reload") await bridge.browserTabReload(activeTab.native_id);
      else await bridge.browserTabHistory(activeTab.native_id, action);
    } catch (cause) {
      onUpdateTab(activeTab.id, (tab) =>
        tab.kind === "browser"
          ? { ...tab, error: browserFailureMessage(cause, t("useNativeBrowserTabs.browserNavigationFailed")) }
          : tab,
      );
    }
  };

  return {
    address,
    setAddress,
    viewportRef,
    navigate,
    retry,
    openExternal,
    performAction,
  };
}
