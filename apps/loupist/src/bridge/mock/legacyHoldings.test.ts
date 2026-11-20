import { MockLoupistBridge } from "../mockBridge";

it("models every export state and dismissal, retries, save cancellation and subscriptions", async () => {
  const bridge = new MockLoupistBridge();
  const control = bridge.legacyHoldings;
  control.setStatus({ state: "pending", dismissed: false });
  const handler = vi.fn();
  const stop = bridge.onLegacyHoldingsChanged(handler);
  for (const state of ["none", "pending", "failed", "exported"] as const) {
    for (const dismissed of [false, true]) {
      control.setStatus({ state, dismissed, ...(state === "exported" ? { exportedAtMs: 7, counts: { wallets: 1, snapshots: 2, positions: 3 } } : {}) });
      expect(await bridge.legacyHoldingsStatus()).toMatchObject({ state, dismissed });
      expect(handler).toHaveBeenLastCalledWith(await bridge.legacyHoldingsStatus());
      if (state !== "exported") {
        await expect(bridge.legacyHoldingsReveal()).rejects.toMatchObject({ code: "invalid_request" });
        await expect(bridge.legacyHoldingsSaveCopy()).rejects.toMatchObject({ code: "invalid_request" });
      }
    }
  }
  control.saveCopyResult = false;
  expect(await bridge.legacyHoldingsSaveCopy()).toBe(false);
  control.saveCopyResult = true;
  expect(await bridge.legacyHoldingsSaveCopy()).toBe(true);
  control.setStatus({ state: "pending", dismissed: false });
  control.retryResult = { state: "failed", dismissed: false };
  expect(await bridge.legacyHoldingsRetry()).toEqual(control.retryResult);
  expect(await bridge.legacyHoldingsDismiss()).toEqual({ state: "failed", dismissed: true });
  control.retryResult = { state: "exported", dismissed: false, exportedAtMs: 42, counts: { wallets: 1, snapshots: 2, positions: 3 } };
  expect(await bridge.legacyHoldingsRetry()).toEqual(control.retryResult);
  expect(await bridge.legacyHoldingsDismiss()).toMatchObject({ state: "exported", dismissed: true });
  await bridge.legacyHoldingsReveal();
  handler.mockClear();
  stop();
  control.setStatus({ state: "none", dismissed: false });
  expect(handler).not.toHaveBeenCalled();
});
