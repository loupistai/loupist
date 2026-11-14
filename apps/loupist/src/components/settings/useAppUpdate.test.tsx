import { act, renderHook } from "@testing-library/react";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { UpdateState } from "../../types";
import { useAppUpdate } from "./useAppUpdate";

const idle: UpdateState = {
  supported: true, current_version: "0.0.1", phase: "idle", offer: null,
  downloaded_bytes: 0, total_bytes: null, last_checked_at_ms: null,
  next_auto_check_at_ms: null, error: null, blockers: [],
};
const deferred = <T,>() => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(done => { resolve = done; });
  return { promise, resolve };
};

describe("update request lifecycle", () => {
  it("does not let an earlier status response erase a newer checked offer", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const initial = deferred<UpdateState>();
    vi.spyOn(bridge, "updateStatus").mockReturnValue(initial.promise);
    const available = { ...idle, offer: { offer_id: "current-offer", version: "0.0.2", notes: null, published_at: null } };
    vi.spyOn(bridge, "updateCheck").mockResolvedValue(available);
    const { result } = renderHook(() => useAppUpdate(bridge));
    await act(() => result.current.checkForUpdates());
    await act(async () => initial.resolve(idle));
    expect(result.current.status?.offer?.offer_id).toBe("current-offer");
  });

  it("exposes busy state before initial status arrives and prevents overlapping actions", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "updateStatus").mockResolvedValue(idle);
    const pending = deferred<UpdateState>();
    const check = vi.spyOn(bridge, "updateCheck").mockReturnValue(pending.promise);
    const install = vi.spyOn(bridge, "updateInstall");
    const { result } = renderHook(() => useAppUpdate(bridge));
    act(() => {
      void result.current.checkForUpdates();
      void result.current.checkForUpdates();
      void result.current.installUpdate("current-offer");
    });
    expect(result.current.phase).toBe("checking");
    expect(check).toHaveBeenCalledTimes(1);
    expect(install).not.toHaveBeenCalled();
    await act(async () => pending.resolve(idle));
    expect(result.current.phase).toBe("idle");
  });

  it("clears a manual check error after a later status recovers", async () => {
    vi.useFakeTimers();
    try {
      const bridge = new MockLoupistBridge({ delay_ms: 0 });
      const failing = { ...idle, error: "Could not check for updates. Loupist will retry." };
      const status = vi.spyOn(bridge, "updateStatus").mockResolvedValue(failing);
      vi.spyOn(bridge, "updateCheck").mockRejectedValue(new Error("update check failed"));
      const { result } = renderHook(() => useAppUpdate(bridge));
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      await act(() => result.current.checkForUpdates());
      expect(result.current.error).toBe("Couldn't check for updates. Check your internet connection and try again.");
      status.mockResolvedValue({ ...idle, last_checked_at_ms: 20, error: null });
      await act(async () => { await vi.advanceTimersByTimeAsync(15_000); });
      expect(result.current.error).toBeNull();
      expect(result.current.status?.last_checked_at_ms).toBe(20);
    } finally {
      vi.useRealTimers();
    }
  });
});


it("retains status identity when background polling returns the same update state", async () => {
  vi.useFakeTimers();
  try {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const status = vi.spyOn(bridge, "updateStatus").mockImplementation(async () => structuredClone(idle));
    const { result, unmount } = renderHook(() => useAppUpdate(bridge));
    await act(async () => {});
    const first = result.current.status;
    await act(async () => vi.advanceTimersByTimeAsync(15_000));
    expect(status).toHaveBeenCalledTimes(2);
    expect(result.current.status).toBe(first);
    unmount();
  } finally { vi.useRealTimers(); }
});
