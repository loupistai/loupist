import { createContext, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { GlobeIcon } from "lucide-react";
import type { ChatEvidenceRef, LoupistBridge } from "../../types";
import { webEvidenceUrl } from "../../chat/evidence";

type LoadIcon = (origin: string) => Promise<string | null>;
const IconContext = createContext<LoadIcon | null>(null);

export function SourceFaviconProvider({ bridge, children }: { bridge: LoupistBridge; children: ReactNode }) {
  const load = useMemo<LoadIcon>(() => {
    const cache = new Map<string, Promise<string | null>>();
    return (origin) => {
      let pending = cache.get(origin);
      if (!pending) {
        pending = bridge.siteFavicon(origin).catch(() => null);
        if (cache.size >= 128) cache.delete(cache.keys().next().value!);
        cache.set(origin, pending);
      }
      return pending;
    };
  }, [bridge]);
  return <IconContext.Provider value={load}>{children}</IconContext.Provider>;
}

export function SourceFavicon({ evidence }: { evidence: ChatEvidenceRef }) {
  const url = webEvidenceUrl(evidence);
  if (!url) return null;
  const origin = new URL(url).origin;
  return <SiteIcon key={origin} origin={origin} />;
}

function SiteIcon({ origin }: { origin: string }) {
  const load = useContext(IconContext);
  const root = useRef<HTMLSpanElement>(null);
  const [icon, setIcon] = useState<string | null>(null);
  useEffect(() => {
    if (!load) return;
    let active = true;
    const fetch = () => { void load(origin).then((value) => { if (active) setIcon(value); }); };
    const observer = typeof IntersectionObserver === "undefined" ? null : new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) {
        observer?.disconnect();
        fetch();
      }
    });
    if (observer && root.current) observer.observe(root.current);
    else fetch();
    return () => { active = false; observer?.disconnect(); };
  }, [load, origin]);
  return <span ref={root} className="source-favicon" aria-hidden="true">
    {icon ? <img src={icon} alt="" width={16} height={16} decoding="async" onError={() => setIcon(null)} /> : <GlobeIcon />}
  </span>;
}
