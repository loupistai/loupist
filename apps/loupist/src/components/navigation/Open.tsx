import { ExternalLinkIcon, PanelRightOpenIcon } from "lucide-react";
import { createContext, useCallback, useContext, useMemo, useState, type MouseEvent, type ReactNode } from "react";
import { destinationIcon, externalUrl, wantsExternal, type ChatDestination, type Open, type OpenHow } from "../../chat/navigation";

const OpenContext = createContext<Open | null>(null);

/** One layer of open policy. {@link useOpenLayer} builds one over its parent. */
export function OpenProvider({ open, children }: { open: Open; children: ReactNode }) {
  return <OpenContext.Provider value={open}>{children}</OpenContext.Provider>;
}

/** Null outside a provider (one component in a test or preview): entry points render as plain text. */
export const useOpen = (): Open | null => useContext(OpenContext);

/**
 * What one layer does with a destination before its parent sees it: nothing
 * (the parent opens it as is), a destination (the parent opens that instead),
 * or a function (this layer opens it itself).
 */
export type OpenLayer = (to: ChatDestination, how: OpenHow) => ChatDestination | (() => void) | undefined;

/** A layer of open policy over the parent opener. A system-browser open always passes through. */
export function useOpenLayer(handle: OpenLayer): Open {
  const parent = useOpen();
  return useCallback<Open>(async (to, how = {}) => {
    const result = how.external ? undefined : handle(to, how);
    if (typeof result === "function") { result(); return; }
    await parent?.(result ?? to, how);
  }, [handle, parent]);
}

/** Opens from inside a dialog over the chat dismiss it first; a system-browser open leaves it up. */
export function useOpenDismissing(dismiss: () => void): Open {
  return useOpenLayer(useCallback(() => { dismiss(); return undefined; }, [dismiss]));
}

export function DismissOnOpen({ dismiss, children }: { dismiss: () => void; children: ReactNode }) {
  return <OpenProvider open={useOpenDismissing(dismiss)}>{children}</OpenProvider>;
}

/** For a control with no failure state of its own (a page or view switch). Null outside a provider. */
export function useGo(): ((to: ChatDestination) => void) | null {
  const open = useOpen();
  return useMemo(() => open ? (to: ChatDestination) => { void open(to).catch(() => undefined); } : null, [open]);
}

const icons = { panel: <PanelRightOpenIcon aria-hidden="true" data-open-icon="panel" />, external: <ExternalLinkIcon aria-hidden="true" data-open-icon="external" /> };

export type OpenProps = {
  /** False outside a provider: render plain text, not a dead control. */
  available: boolean;
  /** Spread on the control. Cmd/Ctrl-click and middle-click ask for the system browser. */
  props: {
    onClick?: (event: MouseEvent<HTMLElement>) => void;
    onAuxClick?: (event: MouseEvent<HTMLElement>) => void;
    "data-open-failed"?: true;
  };
  icon: ReactNode;
  failed: boolean;
  /** Opens from a click the leaf handled itself, with the same modifier rule. */
  activate: (event: MouseEvent<HTMLElement>) => void;
  /** The target has a system-browser address, so a modifier click means something. */
  external: boolean;
};

export function useOpenProps(to: ChatDestination): OpenProps {
  const open = useOpen();
  const [failed, setFailed] = useState(false);
  const external = externalUrl(to) !== null;
  const activate = (event: MouseEvent<HTMLElement>) => {
    if (!open) return;
    setFailed(false);
    open(to, { anchor: event.currentTarget, ...(external && wantsExternal(event) ? { external: true } : {}) }).catch(() => setFailed(true));
  };
  const kind = destinationIcon(to);
  return {
    available: open !== null,
    external,
    failed,
    activate,
    icon: kind ? icons[kind] : null,
    props: open ? {
      onClick: activate,
      onAuxClick: (event) => {
        if (event.button !== 1) return;
        event.preventDefault();
        if (external) activate(event);
      },
      ...(failed ? { "data-open-failed": true as const } : {}),
    } : {},
  };
}
