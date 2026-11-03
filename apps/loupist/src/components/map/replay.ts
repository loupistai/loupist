import { useEffect, useState } from "react";
import type { KnowledgeMapEdge, KnowledgeMapNode } from "../../contracts/knowledge";
export const DAY_MS = 86_400_000;
export const REPLAY_DURATION_MS = 20_000;
export function presentAt(item: { firstSeenAtMs?: number }, time: number) { return item.firstSeenAtMs == null || item.firstSeenAtMs <= time; }
export function replayVisible<N extends KnowledgeMapNode>(nodes: N[], edges: KnowledgeMapEdge[], time: number) {
  const visible = nodes.filter(node => presentAt(node, time));
  const ids = new Set(visible.map(node => node.id));
  return { nodes: visible, edges: edges.filter(edge => presentAt(edge, time) && ids.has(edge.source) && ids.has(edge.target)) };
}
export function replayStart(nodes: KnowledgeMapNode[], edges: KnowledgeMapEdge[], now: number) {
  return Math.min(now, ...nodes.map(n => n.firstSeenAtMs ?? now), ...edges.map(e => e.firstSeenAtMs ?? now));
}
export function replayTime(start: number, end: number, progress: number, reduced: boolean) {
  if (progress >= 1) return end;
  const elapsed = Math.max(0, progress) * (end - start);
  return start + (reduced ? Math.floor(elapsed / DAY_MS) * DAY_MS : elapsed);
}
export function useReducedMapMotion() {
  const [reduced, setReduced] = useState(() => window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  useEffect(() => { const media = window.matchMedia("(prefers-reduced-motion: reduce)"); const change = () => setReduced(media.matches); media.addEventListener?.("change", change); return () => media.removeEventListener?.("change", change); }, []);
  return reduced;
}
export function useMapReplay(start: number, scope: string) {
  const reduced = useReducedMapMotion();
  const [state, setState] = useState({ active: false, playing: false, time: start, end: Date.now(), speed: 1 });
  useEffect(() => setState(s => ({ ...s, active: false, playing: false })), [scope]);
  useEffect(() => {
    if (!state.active || !state.playing) return;
    const began = performance.now(), initial = state.time;
    let frame = 0;
    const tick = () => {
      const progress = (initial - start) / Math.max(1, state.end - start) + (performance.now() - began) * state.speed / REPLAY_DURATION_MS;
      setState(s => ({ ...s, time: replayTime(start, s.end, progress, reduced), playing: progress < 1 }));
      if (progress < 1) frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [state.active, state.playing, state.speed, state.end, start, reduced]);
  return { ...state, start, reduced, setState,
    open: () => setState(s => ({ ...s, active: true, playing: true, time: start, end: Date.now() })),
    close: () => setState(s => ({ ...s, active: false, playing: false })),
    toggle: () => setState(s => ({ ...s, playing: !s.playing, time: s.time >= s.end ? start : s.time })),
    seek: (time: number) => setState(s => ({ ...s, playing: false, time: Math.max(start, Math.min(s.end, time)) })),
    setSpeed: (speed: number) => setState(s => ({ ...s, speed })),
  };
}
export type MapReplay = ReturnType<typeof useMapReplay>;
