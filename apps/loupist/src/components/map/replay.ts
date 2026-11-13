import { useEffect, useMemo, useState } from "react";
import type { KnowledgeMapEdge, KnowledgeMapNode } from "../../contracts/knowledge";
export const DAY_MS = 86_400_000;
export const REPLAY_DURATION_MS = 20_000;
export function presentAt(item: { firstSeenAtMs?: number }, time: number) { return item.firstSeenAtMs == null || item.firstSeenAtMs <= time; }
export function replayVisible<N extends KnowledgeMapNode>(nodes: N[], edges: KnowledgeMapEdge[], time: number) {
  const visible = nodes.filter(node => presentAt(node, time));
  const ids = new Set(visible.map(node => node.id));
  return { nodes: visible, edges: edges.filter(edge => presentAt(edge, time) && ids.has(edge.source) && ids.has(edge.target)) };
}
/**
 * What replay shows at `time`, kept stable between first appearances: the visible set only
 * changes when time crosses a node's or edge's first-seen moment, so a frame inside the same
 * step reuses the last result and the canvas keeps its layout input.
 */
export function useReplayVisible<N extends KnowledgeMapNode>(nodes: N[], edges: KnowledgeMapEdge[], time: number | undefined) {
  const moments = useMemo(() => [...new Set([...nodes, ...edges].flatMap(item => item.firstSeenAtMs == null ? [] : [item.firstSeenAtMs]))].sort((a, b) => a - b), [nodes, edges]);
  const step = time === undefined ? undefined : lastAtOrBefore(moments, time);
  return useMemo(() => step === undefined ? { nodes, edges } : replayVisible(nodes, edges, step), [nodes, edges, step]);
}
/** The latest moment at or before `time`, or -Infinity before the first one. */
function lastAtOrBefore(sorted: number[], time: number) {
  let low = 0, high = sorted.length;
  while (low < high) { const mid = (low + high) >> 1; if (sorted[mid]! <= time) low = mid + 1; else high = mid; }
  return low ? sorted[low - 1]! : -Infinity;
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
      // Reduced motion advances a day at a time: frames inside the same day keep the state, so nothing re-renders.
      setState(s => {
        const time = replayTime(start, s.end, progress, reduced), playing = progress < 1;
        return time === s.time && playing === s.playing ? s : { ...s, time, playing };
      });
      if (progress < 1) frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `state.time` is read once when playback starts; the frame loop advances it.
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
