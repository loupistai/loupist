import { useEffect, useState } from "react";
import type { LoupistBridge } from "../types";
import { followedMarkets } from "../chat/marketStatus";

/** Markets of the assets the user follows, re-read when the window regains focus or the Loupist changes. */
export function useFollowedMarkets(bridge: LoupistBridge, loupistId: string | null) {
  const [markets, setMarkets] = useState<string[]>([]);
  useEffect(() => {
    let active = true;
    const read = () => void bridge.watchlistList()
      .then((entries) => { if (active) setMarkets(followedMarkets(entries)); })
      .catch(() => { if (active) setMarkets([]); });
    read();
    window.addEventListener("focus", read);
    return () => { active = false; window.removeEventListener("focus", read); };
  }, [bridge, loupistId]);
  return markets;
}
