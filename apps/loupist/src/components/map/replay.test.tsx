import { act, fireEvent, render, renderHook, screen } from "@testing-library/react";
import { MapReplayBar } from "./MapReplayBar";
import { DAY_MS, presentAt, replayTime, replayVisible, useMapReplay, useReplayVisible } from "./replay";
import userEvent from "@testing-library/user-event";
import type { KnowledgeMapNode } from "../../contracts/knowledge";
import { frozenLayout } from "./layout";
const node = (id: string, firstSeenAtMs?: number): KnowledgeMapNode => ({ id, label: id, kind: "entity", degree: 1, watched: false, held: false, firstSeenAtMs });
it("shows exactly timestamp <= t and undated nodes; edges wait for their timestamp and both endpoints", () => {
  const nodes = [node("undated"), node("zero", 0), node("earlier", 10), node("boundary", 20), node("later", 21)];
  const edges = [{ id: "early", kind: "claim" as const, source: "undated", target: "boundary" }, { id: "future-end", kind: "claim" as const, source: "boundary", target: "later" }, { id: "future-edge", kind: "claim" as const, source: "zero", target: "earlier", firstSeenAtMs: 21 }];
  const positions = frozenLayout("replay-test", "1", nodes, edges);
  const visible = replayVisible(positions, edges, 20);
  expect(visible.nodes.map(n => n.id)).toEqual(["undated", "zero", "earlier", "boundary"]);
  expect(visible.edges.map(e => e.id)).toEqual(["early"]);
  for (const n of visible.nodes) expect(n).toBe(positions.find(p => p.id === n.id));
  expect(replayVisible(positions, edges, 9).nodes.map(n => n.id)).toEqual(["undated", "zero"]);
  expect(presentAt(node("epoch", 0), -1)).toBe(false);
});
it("keeps the replayed set between first appearances so frames inside one step reuse it", () => {
  const nodes = [node("undated"), node("early", 10), node("late", 30)];
  const edges = [{ id: "link", kind: "claim" as const, source: "early", target: "late" }];
  const { result, rerender } = renderHook(({ time }: { time?: number }) => useReplayVisible(nodes, edges, time), { initialProps: { time: 11 } as { time?: number } });
  const first = result.current;
  rerender({ time: 29 });
  expect(result.current).toBe(first);
  expect(first.nodes.map(n => n.id)).toEqual(["undated", "early"]);
  rerender({ time: 30 });
  expect(result.current.nodes.map(n => n.id)).toEqual(["undated", "early", "late"]);
  expect(result.current.edges.map(e => e.id)).toEqual(["link"]);
  rerender({ time: undefined });
  expect(result.current).toEqual({ nodes, edges });
});
it("steps reduced playback by whole days and always reaches now", () => {
  expect(replayTime(0, 10 * DAY_MS, .15, true)).toBe(DAY_MS);
  expect(replayTime(0, 10 * DAY_MS, .15, false)).toBe(1.5 * DAY_MS);
  expect(replayTime(123, 10 * DAY_MS + 456, 1, true)).toBe(10 * DAY_MS + 456);
});
it("pauses on scrub, exposes the current date, supports Space and day stepping, and exits to live", () => {
  const start = Date.UTC(2026, 0, 1);
  const { result } = renderHook(() => useMapReplay(start, "loupist"));
  act(() => { result.current.open(); result.current.seek(start + DAY_MS * 3); });
  const view = render(<MapReplayBar replay={result.current} />);
  const slider = screen.getByRole("slider");
  expect(slider).toHaveAttribute("aria-valuetext", "Jan 4, 2026");
  fireEvent.keyDown(slider, { key: "ArrowRight" });
  expect(result.current.time).toBe(start + 4 * DAY_MS); expect(result.current.playing).toBe(false);
  view.rerender(<MapReplayBar replay={result.current} />);
  fireEvent.keyDown(slider, { key: " " }); expect(result.current.playing).toBe(true);
  view.rerender(<MapReplayBar replay={result.current} />);
  fireEvent.change(slider, { target: { value: start + 2 * DAY_MS } }); expect(result.current.playing).toBe(false);
  view.rerender(<MapReplayBar replay={result.current} />);
  fireEvent.click(screen.getByRole("button", { name: "Exit replay" })); expect(result.current.active).toBe(false);
});
it("finishes playback in 20 seconds divided by speed", () => {
  const speed = 4;
  vi.useFakeTimers();
  vi.setSystemTime(new Date(Date.UTC(2026, 9, 28)));
  const start = Date.UTC(2026, 9, 1);
  const { result, unmount } = renderHook(() => useMapReplay(start, `speed-${speed}`));
  act(() => { result.current.open(); result.current.setSpeed(speed); });
  act(() => vi.advanceTimersByTime(20_000 / speed - 20));
  expect(result.current.playing).toBe(true);
  act(() => vi.advanceTimersByTime(50));
  expect(result.current.playing).toBe(false);
  expect(result.current.time).toBe(result.current.end);
  unmount(); vi.useRealTimers();
});


it("lets Space activate the focused replay button without also toggling playback", async () => {
  const user = userEvent.setup();
  const replay = { active: true, reduced: false, setState: vi.fn(), playing: false, time: 0, start: 0, end: DAY_MS, speed: 1, open: vi.fn(), toggle: vi.fn(), seek: vi.fn(), setSpeed: vi.fn(), close: vi.fn() };
  render(<MapReplayBar replay={replay} />);
  expect(screen.getByRole("button", { name: "Play" })).toHaveFocus();
  await user.keyboard("{ArrowRight}{ArrowLeft}");
  expect(replay.seek).toHaveBeenNthCalledWith(1, DAY_MS);
  expect(replay.seek).toHaveBeenNthCalledWith(2, -DAY_MS);
  await user.keyboard(" ");
  expect(replay.toggle).toHaveBeenCalledTimes(1);
  replay.toggle.mockClear();
  screen.getByRole("button", { name: "4×" }).focus();
  await user.keyboard(" ");
  expect(replay.setSpeed).toHaveBeenCalledWith(4);
  screen.getByRole("button", { name: "Exit replay" }).focus();
  await user.keyboard(" ");
  expect(replay.close).toHaveBeenCalledTimes(1);
  expect(replay.toggle).not.toHaveBeenCalled();
});
