import { ExternalLinkIcon, PanelRightOpenIcon } from "lucide-react";
import { createContext, useContext, useMemo, useState, type MouseEvent, type ReactNode } from "react";
import { destinationIcon, externalUrl, wantsExternal, type ChatDestination, type Open } from "../../chat/navigation";

type Opener = { open: Open; available: (to: ChatDestination) => boolean };

const OpenContext = createContext<Opener | null>(null);
const always = () => true;

/** One layer of open policy. A nested provider reads its parent with {@link useOpener} and adds to it. */
export function OpenProvider({ open, available = always, children }: { open: Open; available?: (to: ChatDestination) => boolean; children: ReactNode }) {
  const value = useMemo(() => ({ open, available }), [open, available]);
  return <OpenContext.Provider value={value}>{children}</OpenContext.Provider>;
}

export const useOpener = () => useContext(OpenContext);

/** Null outside a provider (one component in a test or preview): entry points hide. */
export const useOpen = (): Open | null => useContext(OpenContext)?.open ?? null;

/** For a control with no failure state of its own (a page or view switch). Null outside a provider. */
export function useGo(): ((to: ChatDestination) => void) | null {
  const open = useOpen();
  return useMemo(() => open ? (to: ChatDestination) => { void open(to).catch(() => undefined); } : null, [open]);
}

const icons = { panel: <PanelRightOpenIcon aria-hidden="true" data-open-icon="panel" />, external: <ExternalLinkIcon aria-hidden="true" data-open-icon="external" /> };

export type OpenProps = {
  /** False outside a provider or when nothing can open the target: render plain text, not a dead control. */
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
  const opener = useOpener();
  const [failed, setFailed] = useState(false);
  const external = externalUrl(to) !== null;
  const available = opener !== null && opener.available(to);
  const activate = (event: MouseEvent<HTMLElement>) => {
    if (!opener) return;
    const target = to.kind === "evidence" ? { ...to, anchor: event.currentTarget } : to;
    setFailed(false);
    opener.open(target, external && wantsExternal(event) ? { external: true } : undefined).catch(() => setFailed(true));
  };
  const kind = destinationIcon(to);
  return {
    available,
    external,
    failed,
    activate,
    icon: kind ? icons[kind] : null,
    props: available ? {
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
