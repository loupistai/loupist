import { act, render, renderHook, screen, waitFor } from "@testing-library/react";
import { MockLoupistBridge } from "../bridge/mockBridge";
import { FX_DISPLAY_FIXTURES as fixtures } from "../bridge/mock/fxDisplay";
import type { FxDisplay, FxDisplayRequest } from "../types";
import { useConfiguration } from "./useConfiguration";
import { DisplayFxProvider, useDisplayFx, useSharedDisplayFx, type DisplayFx } from "./useDisplayFx";

const QUALITY = "loupist-quality";
const VALUE = "loupist-value";
const global = { kind: "global" } as const;

async function setCurrency(bridge: MockLoupistBridge, code: string | null) {
  const current = await bridge.configRead({ scope: global });
  await bridge.configUpdate({ scope: global, expectedRevision: current.revision,
    set: code ? { appearance: { display_currency: code } } : {}, unset: code ? [] : [["appearance", "display_currency"]] });
}

/** A bridge whose reads and subscriptions are counted, and whose reads can be held. */
function recording(bridge: MockLoupistBridge) {
  const reads: FxDisplayRequest[] = [];
  const held: Array<{ request: FxDisplayRequest; release: () => void }> = [];
  let hold = false;
  const read = bridge.fxDisplay.bind(bridge);
  vi.spyOn(bridge, "fxDisplay").mockImplementation(async (request) => {
    reads.push(request);
    if (hold) await new Promise<void>((release) => held.push({ request, release }));
    return read(request);
  });
  const subscribe = vi.spyOn(bridge, "onFxChanged");
  return { reads, held, subscribe, hold: (value: boolean) => { hold = value; } };
}

function useShell(bridge: MockLoupistBridge, loupistId: string | null) {
  const configuration = useConfiguration(bridge, global);
  return { configuration, fx: useDisplayFx(bridge, loupistId, configuration) };
}

const stateOf = (fx: DisplayFx) => fx.status === "read" ? fx.display.state === "ready" ? `ready:${fx.display.currency}:${fx.display.behind ?? "fresh"}` : `${fx.display.state}:${fx.display.currency}` : fx.status;

afterEach(() => vi.restoreAllMocks());

it("serves two consumers from one subscription and one read, and turns a cold wait into rates without refocus", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await setCurrency(bridge, "KRW");
  const record = recording(bridge);
  function Consumer({ name }: { name: string }) {
    return <p data-testid={name}>{stateOf(useSharedDisplayFx())}</p>;
  }
  function Shell() {
    const { fx } = useShell(bridge, QUALITY);
    return <DisplayFxProvider value={fx}><Consumer name="settings" /><Consumer name="page" /></DisplayFxProvider>;
  }
  const view = render(<Shell />);
  await waitFor(() => expect(screen.getByTestId("page")).toHaveTextContent("waiting:KRW"));
  expect(screen.getByTestId("settings")).toHaveTextContent("waiting:KRW");
  expect(record.subscribe).toHaveBeenCalledTimes(1);
  expect(record.reads).toEqual([{ loupistId: QUALITY }]);

  // The cold read completes: the event alone brings the rates, read once for both.
  act(() => bridge.fxDisplayControl.publish(fixtures.ready));
  await waitFor(() => expect(screen.getByTestId("page")).toHaveTextContent("ready:KRW:fresh"));
  expect(screen.getByTestId("settings")).toHaveTextContent("ready:KRW:fresh");
  expect(record.reads).toHaveLength(2);
  view.unmount();
});

/**
 * A bridge whose fx-changed listener attaches only when the test says so, as Tauri's `listen`
 * does; like `subscribe` in tauriBridge.ts, the readiness callback runs even after disposal.
 */
function delayedListener(bridge: MockLoupistBridge) {
  const attach: Array<() => void> = [];
  const subscribe = bridge.onFxChanged.bind(bridge);
  vi.spyOn(bridge, "onFxChanged").mockImplementation((handler, onListening) => {
    let stop: (() => void) | undefined;
    let disposed = false;
    attach.push(() => { if (!disposed) stop = subscribe(handler); onListening?.(); });
    return () => { disposed = true; stop?.(); };
  });
  return attach;
}

it("reads only once the listener is attached, so a cold completion before that is not missed", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await setCurrency(bridge, "KRW");
  const attach = delayedListener(bridge);
  const record = recording(bridge);
  const hook = renderHook(() => useShell(bridge, QUALITY));
  await waitFor(() => expect(hook.result.current.configuration.snapshot).not.toBeNull());
  // The host finishes the cold read while the listener is still attaching: that hint is lost.
  act(() => bridge.fxDisplayControl.publish(fixtures.ready));
  expect(record.reads).toHaveLength(0);
  expect(hook.result.current.fx.status).toBe("reading");
  act(() => attach[0]!());
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("ready:KRW:fresh"));
  expect(record.reads).toHaveLength(1);
});

it("ignores a listener that attaches after the shell unmounted", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await setCurrency(bridge, "KRW");
  const attach = delayedListener(bridge);
  const record = recording(bridge);
  const hook = renderHook(() => useShell(bridge, QUALITY));
  await waitFor(() => expect(hook.result.current.configuration.snapshot).not.toBeNull());
  hook.unmount();
  act(() => attach.forEach(run => run()));
  act(() => bridge.fxDisplayControl.publish(fixtures.ready));
  expect(record.reads).toHaveLength(0);
});

it("starts in a window left on As listed when another window chose a currency and rates arrive", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const other = renderHook(() => useShell(bridge, QUALITY));
  await waitFor(() => expect(other.result.current.configuration.snapshot).not.toBeNull());
  expect(other.result.current.fx.status).toBe("off");
  // Another window commits KRW; its window-local configuration event never reaches this one.
  await setCurrency(bridge, "KRW");
  act(() => bridge.fxDisplayControl.publish(fixtures.ready));
  await waitFor(() => expect(stateOf(other.result.current.fx)).toBe("ready:KRW:fresh"));
});

it("follows a currency written in another window when the host sends its hint, with no rate change", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  bridge.fxDisplayControl.publish(fixtures.ready);
  const hook = renderHook(() => useShell(bridge, QUALITY));
  await waitFor(() => expect(hook.result.current.configuration.snapshot).not.toBeNull());
  // Another window's write: the host sends its hint for the write itself.
  await act(() => setCurrency(bridge, "KRW"));
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("ready:KRW:fresh"));
  // A reset elsewhere: the reread finds no preference, and the committed configuration ends the figure.
  await act(() => setCurrency(bridge, null));
  await waitFor(() => expect(hook.result.current.fx.status).toBe("off"));
});

it("never reloads configuration for a hint while no Loupist is open", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const hook = renderHook(() => useShell(bridge, null));
  await waitFor(() => expect(hook.result.current.configuration.snapshot).not.toBeNull());
  const reads = vi.spyOn(bridge, "configRead");
  act(() => bridge.fxDisplayControl.invalidate());
  expect(reads).not.toHaveBeenCalled();
});

it("rereads on a new revision and on a same-revision change of state", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await setCurrency(bridge, "KRW");
  bridge.fxDisplayControl.publish(fixtures.ready);
  const record = recording(bridge);
  const hook = renderHook(() => useShell(bridge, QUALITY));
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("ready:KRW:fresh"));

  act(() => bridge.fxDisplayControl.publish(fixtures.behindOffline));
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("ready:KRW:offline"));
  act(() => bridge.fxDisplayControl.publish(fixtures.tooOld));
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("unavailable:KRW"));
  const next = { ...fixtures.ready, revision: "ecb:2026-11-17", date: "2026-11-17", rates: { ...fixtures.ready.rates, USD: 1340.02 } } satisfies FxDisplay;
  act(() => bridge.fxDisplayControl.publish(next));
  await waitFor(() => expect(hook.result.current.fx).toMatchObject({ status: "read", display: { revision: "ecb:2026-11-17" } }));
  expect(record.reads).toHaveLength(4);
});

it("joins hints that arrive during a read into one more read, and loses none", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await setCurrency(bridge, "KRW");
  const record = recording(bridge);
  const hook = renderHook(() => useShell(bridge, QUALITY));
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("waiting:KRW"));
  record.hold(true);
  act(() => bridge.fxDisplayControl.publish(fixtures.behindOffline));
  expect(record.reads).toHaveLength(2);
  // Two more hints while that read is held: one joined reread, after it.
  act(() => { bridge.fxDisplayControl.publish(fixtures.ready); bridge.fxDisplayControl.publish(fixtures.ready); });
  expect(record.reads).toHaveLength(2);
  record.hold(false);
  await act(async () => { record.held.shift()!.release(); });
  await waitFor(() => expect(record.reads).toHaveLength(3));
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("ready:KRW:fresh"));
  expect(record.reads).toHaveLength(3);
});

it("drops a slow reply after the Loupist changes, including A, B and back to A", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await setCurrency(bridge, "KRW");
  bridge.fxDisplayControl.publish(fixtures.ready);
  await bridge.loupistCapabilityDisable({ loupist_id: VALUE, entry_id: "openbb.platform" });
  const record = recording(bridge);
  record.hold(true);
  const hook = renderHook(({ id }) => useShell(bridge, id), { initialProps: { id: QUALITY } });
  await waitFor(() => expect(record.held).toHaveLength(1));
  hook.rerender({ id: VALUE });
  await waitFor(() => expect(record.held).toHaveLength(2));
  hook.rerender({ id: QUALITY });
  await waitFor(() => expect(record.held).toHaveLength(3));
  expect(record.held.map(item => item.request.loupistId)).toEqual([QUALITY, VALUE, QUALITY]);
  // The first A and B replies arrive late: neither paints the current A.
  bridge.fxDisplayControl.publish(fixtures.tooOld);
  record.hold(false);
  await act(async () => { record.held[0]!.release(); record.held[1]!.release(); });
  expect(hook.result.current.fx.status).toBe("reading");
  // The current A's read is the one that paints, with whatever the host now says.
  await act(async () => { record.held[2]!.release(); });
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("unavailable:KRW"));
});

it("never paints a reply for another preference: it reloads the committed configuration first", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await setCurrency(bridge, "KRW");
  bridge.fxDisplayControl.publish(fixtures.ready);
  const record = recording(bridge);
  const renders: DisplayFx[] = [];
  const hook = renderHook(() => { const shell = useShell(bridge, QUALITY); renders.push(shell.fx); return shell; });
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("ready:KRW:fresh"));
  // Another window chose yen; this window learns of it only through the host's hint for that
  // write (its own configuration event is window-local, and it has not been refocused).
  await act(() => setCurrency(bridge, "JPY"));
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("waiting:JPY"));
  expect(renders.filter(fx => fx.status === "read" && fx.display.currency !== fx.currency)).toEqual([]);
  expect(hook.result.current.configuration.snapshot?.effective.appearance.display_currency).toBe("JPY");
  // The initial read, the hint's read (another currency: reload, no paint), then the read for yen.
  expect(record.reads.map(read => read.loupistId)).toEqual([QUALITY, QUALITY, QUALITY]);
});

it("reads once more when the reload after a mismatched reply confirms this window's currency", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await setCurrency(bridge, "KRW");
  bridge.fxDisplayControl.publish(fixtures.ready);
  // The read caught another window's brief yen choice; by the reload, KRW is committed again.
  const read = vi.spyOn(bridge, "fxDisplay").mockResolvedValueOnce({ state: "waiting", currency: "JPY" });
  const hook = renderHook(() => useShell(bridge, QUALITY));
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("ready:KRW:fresh"));
  expect(read).toHaveBeenCalledTimes(2);
});

it("offers Retry when reconciling a mismatched reply fails, and stops retrying a reply that keeps disagreeing", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await setCurrency(bridge, "KRW");
  bridge.fxDisplayControl.publish(fixtures.ready);
  const hook = renderHook(() => useShell(bridge, QUALITY));
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("ready:KRW:fresh"));
  const read = vi.spyOn(bridge, "fxDisplay").mockResolvedValueOnce({ state: "waiting", currency: "JPY" });
  vi.spyOn(bridge, "configRead").mockRejectedValueOnce(new Error("disk unavailable"));
  act(() => bridge.fxDisplayControl.invalidate());
  await waitFor(() => expect(hook.result.current.fx).toMatchObject({ status: "failed", currency: "KRW" }));
  act(() => hook.result.current.fx.retry());
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("ready:KRW:fresh"));

  // A host that keeps answering for another currency than the committed one: a bounded number of reads.
  read.mockResolvedValue({ state: "waiting", currency: "JPY" });
  read.mockClear();
  act(() => bridge.fxDisplayControl.invalidate());
  await waitFor(() => expect(hook.result.current.fx.status).toBe("failed"));
  await act(() => new Promise(resolve => setTimeout(resolve, 50)));
  expect(read.mock.calls.length).toBeLessThanOrEqual(3);
});

/** A ready table whose usability ends `ms` from now, as the host states it in `usable_until`. */
const usableFor = (ms: number): FxDisplay => ({ ...fixtures.ready, usable_until: new Date(Date.now() + ms).toISOString() });
const pause = (ms: number) => act(() => new Promise(resolve => setTimeout(resolve, ms)));

it("hides the figure at the host's usable_until and rereads once, with no polling", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await setCurrency(bridge, "KRW");
  bridge.fxDisplayControl.publish(usableFor(120));
  const record = recording(bridge);
  const hook = renderHook(() => useShell(bridge, QUALITY));
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("ready:KRW:fresh"));
  expect(record.reads).toHaveLength(1);
  // At the deadline the figure goes and one reread asks the host, which now answers too_old.
  await waitFor(() => expect(hook.result.current.fx).toMatchObject({ status: "read", display: { state: "unavailable", reason: "too_old", lastDate: fixtures.ready.date } }));
  expect(record.reads).toHaveLength(2);
  await pause(150);
  expect(record.reads).toHaveLength(2);
  // A new table replaces the expired one.
  act(() => bridge.fxDisplayControl.publish(usableFor(60_000)));
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("ready:KRW:fresh"));
});

it("keeps an expired table hidden after one reread when the host's clock still calls it usable", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await setCurrency(bridge, "KRW");
  const table = usableFor(120);
  // The host's clock runs a little behind this window's: past the deadline it still answers ready.
  const read = vi.spyOn(bridge, "fxDisplay").mockResolvedValue(table);
  const renders: DisplayFx[] = [];
  const hook = renderHook(() => { const shell = useShell(bridge, QUALITY); renders.push(shell.fx); return shell; });
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("ready:KRW:fresh"));
  await waitFor(() => expect(read).toHaveBeenCalledTimes(2));
  await pause(150);
  expect(hook.result.current.fx.status).toBe("reading");
  expect(read).toHaveBeenCalledTimes(2);
  const deadline = Date.parse((table as Extract<FxDisplay, { state: "ready" }>).usable_until!);
  expect(Date.now()).toBeGreaterThan(deadline);
});

it("clears the usable_until timer on unmount and on a newer reply", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await setCurrency(bridge, "KRW");
  bridge.fxDisplayControl.publish(usableFor(100));
  const record = recording(bridge);
  const hook = renderHook(() => useShell(bridge, QUALITY));
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("ready:KRW:fresh"));
  // A newer table without a deadline arrives first: its reply clears the old timer.
  act(() => bridge.fxDisplayControl.publish(fixtures.ready));
  await waitFor(() => expect(record.reads).toHaveLength(2));
  await pause(150);
  expect(stateOf(hook.result.current.fx)).toBe("ready:KRW:fresh");
  expect(record.reads).toHaveLength(2);

  bridge.fxDisplayControl.publish(usableFor(100));
  await waitFor(() => expect(record.reads).toHaveLength(3));
  hook.unmount();
  await pause(150);
  expect(record.reads).toHaveLength(3);
});

it("rereads when the window becomes visible again, and hides a table whose deadline passed during sleep", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await setCurrency(bridge, "KRW");
  bridge.fxDisplayControl.publish(usableFor(60 * 60_000));
  const record = recording(bridge);
  const hook = renderHook(() => useShell(bridge, QUALITY));
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("ready:KRW:fresh"));
  const visible = () => act(() => {
    Object.defineProperty(document, "visibilityState", { configurable: true, get: () => "visible" });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  visible();
  await waitFor(() => expect(record.reads).toHaveLength(2));
  expect(stateOf(hook.result.current.fx)).toBe("ready:KRW:fresh");
  // The machine slept past the deadline; its timer has not fired yet.
  const now = Date.now();
  const clock = vi.spyOn(Date, "now").mockReturnValue(now + 2 * 60 * 60_000);
  record.hold(true);
  visible();
  expect(hook.result.current.fx.status).toBe("reading");
  clock.mockRestore();
  Reflect.deleteProperty(document, "visibilityState");
});

it("shows nothing at once on reset, needs no read without a Loupist, and detaches on unmount", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await setCurrency(bridge, "KRW");
  bridge.fxDisplayControl.publish(fixtures.ready);
  const record = recording(bridge);
  const hook = renderHook(({ id }) => useShell(bridge, id), { initialProps: { id: QUALITY as string | null } });
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("ready:KRW:fresh"));
  await act(async () => { await hook.result.current.configuration.update({}, [["appearance", "display_currency"]]); });
  expect(hook.result.current.fx.status).toBe("off");
  // The host's hint for the reset arrives while this window still holds KRW: one read, which
  // finds no preference. After that a rate publication reads nothing while unset.
  expect(record.reads).toHaveLength(2);
  act(() => bridge.fxDisplayControl.publish(fixtures.ready));
  expect(record.reads).toHaveLength(2);

  // Saved with no Loupist open: no Loupist id is invented, so nothing is read.
  hook.rerender({ id: null });
  await act(async () => { await hook.result.current.configuration.update({ appearance: { display_currency: "KRW" } }); });
  expect(hook.result.current.fx).toMatchObject({ status: "no_loupist", currency: "KRW" });
  expect(record.reads).toHaveLength(2);

  hook.unmount();
  act(() => bridge.fxDisplayControl.publish(fixtures.ready));
  expect(record.reads).toHaveLength(2);
  expect(record.subscribe.mock.results.length).toBe(1);
});

it("reports a failed read without inventing rates, and retries it", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await setCurrency(bridge, "KRW");
  bridge.fxDisplayControl.publish(fixtures.ready);
  const read = bridge.fxDisplay.bind(bridge);
  const spy = vi.spyOn(bridge, "fxDisplay").mockRejectedValueOnce(new Error("bridge closed"));
  const hook = renderHook(() => useShell(bridge, QUALITY));
  await waitFor(() => expect(hook.result.current.fx).toMatchObject({ status: "failed", currency: "KRW" }));
  spy.mockImplementation(read);
  act(() => hook.result.current.fx.retry());
  await waitFor(() => expect(stateOf(hook.result.current.fx)).toBe("ready:KRW:fresh"));
});
