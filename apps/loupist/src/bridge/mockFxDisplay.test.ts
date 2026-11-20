import { FX_DISPLAY_FIXTURES as fixtures } from "./mock/fxDisplay";
import { MockLoupistBridge } from "./mockBridge";

it("rereads the current global currency and the requested Loupist's grant after invalidation", async () => {
  const bridge = new MockLoupistBridge();
  const on = { loupistId: "loupist-quality" };
  const off = { loupistId: "loupist-value" };
  const global = { kind: "global" } as const;
  expect(await bridge.fxDisplay(on)).toEqual(fixtures.unset);
  expect(bridge.fxDisplayControl.rateReads).toBe(0);
  await expect(bridge.fxDisplay({ loupistId: "unknown" })).rejects.toMatchObject({
    code: "loupist_storage_unavailable",
  });
  await bridge.loupistCapabilityDisable({ loupist_id: off.loupistId, entry_id: "openbb.platform" });
  const initial = await bridge.configRead({ scope: global });
  const set = await bridge.configUpdate({
    scope: global, expectedRevision: initial.revision,
    set: { appearance: { display_currency: "KRW" } }, unset: [],
  });
  expect(await bridge.fxDisplay(on)).toEqual(fixtures.waiting);
  expect(await bridge.fxDisplay(off)).toEqual(fixtures.connectionOff);
  expect(bridge.fxDisplayControl.rateReads).toBe(1);

  const events = vi.fn();
  const unsubscribe = bridge.onFxChanged(events);
  bridge.fxDisplayControl.publish(fixtures.ready);
  expect(events).toHaveBeenCalledExactlyOnceWith({ state: "ready", revision: "ecb:2026-11-16" });
  expect(await bridge.fxDisplay(on)).toEqual(fixtures.ready);
  expect(await bridge.fxDisplay(off)).toEqual(fixtures.behindConnectionOff);

  unsubscribe();
  for (const fixture of [fixtures.offline, fixtures.providerFailed, fixtures.noRate, fixtures.tooOld,
    fixtures.behindOffline, fixtures.behindProviderFailed]) {
    bridge.fxDisplayControl.publish(fixture);
    expect(await bridge.fxDisplay(on)).toEqual(fixture);
  }
  expect(events).toHaveBeenCalledTimes(1);

  const changed = await bridge.configUpdate({
    scope: global, expectedRevision: set.revision,
    set: { appearance: { display_currency: "JPY" } }, unset: [],
  });
  expect(await bridge.fxDisplay(on)).toEqual({ state: "waiting", currency: "JPY" });
  await bridge.configUpdate({
    scope: global, expectedRevision: changed.revision, set: {}, unset: [["appearance", "display_currency"]],
  });
  expect(await bridge.fxDisplay(on)).toEqual(fixtures.unset);
});

it("broadcasts preference and grant changes to both listeners without reading rates", async () => {
  const bridge = new MockLoupistBridge();
  const first = vi.fn();
  const second = vi.fn();
  bridge.onFxChanged(first);
  const stop = bridge.onFxChanged(second);
  const binding = { loupist_id: "loupist-quality", entry_id: "openbb.platform" };
  const scope = { kind: "global" } as const;
  let config = await bridge.configRead({ scope });
  const setCurrency = async (currency?: string) => {
    config = await bridge.configUpdate({
      scope, expectedRevision: config.revision,
      set: currency ? { appearance: { display_currency: currency } } : {},
      unset: currency ? [] : [["appearance", "display_currency"]],
    });
  };
  await bridge.loupistCapabilityDisable(binding);
  await bridge.loupistCapabilityDisable(binding);
  await bridge.loupistCapabilityEnable(binding);
  await bridge.loupistCapabilityEnable(binding);
  await setCurrency("KRW");
  await setCurrency("KRW");
  await setCurrency("JPY");
  await setCurrency();
  await setCurrency();
  expect(first.mock.calls).toEqual(Array.from({ length: 5 }, () => [{ state: "invalidated" }]));
  expect(second.mock.calls).toEqual(first.mock.calls);
  expect(bridge.fxDisplayControl.rateReads).toBe(0);
  stop();
  bridge.fxDisplayControl.invalidate();
  expect(first).toHaveBeenCalledTimes(6);
  expect(second).toHaveBeenCalledTimes(5);
});

it("lets a listener read a published deadline and then the expired reply", async () => {
  const bridge = new MockLoupistBridge();
  const scope = { kind: "global" } as const;
  const config = await bridge.configRead({ scope });
  await bridge.configUpdate({
    scope, expectedRevision: config.revision,
    set: { appearance: { display_currency: "KRW" } }, unset: [],
  });
  const fixture = { ...fixtures.ready, usable_until: "2026-11-23T23:00:00Z" };
  const deadline = Date.parse(fixture.usable_until);
  const now = vi.spyOn(Date, "now").mockReturnValue(deadline - 1);
  try {
    const events = vi.fn();
    bridge.onFxChanged(events, () => bridge.fxDisplayControl.publish(fixture));
    expect(events).toHaveBeenCalledExactlyOnceWith({ state: "ready", revision: fixture.revision });
    const request = { loupistId: "loupist-quality" };
    expect(await bridge.fxDisplay(request)).toEqual(fixture);
    now.mockReturnValue(deadline);
    expect(await bridge.fxDisplay(request)).toEqual({
      state: "unavailable", currency: fixture.currency, reason: "too_old", lastDate: fixture.date,
    });
    expect(events).toHaveBeenCalledTimes(1);
  } finally {
    now.mockRestore();
  }
});
