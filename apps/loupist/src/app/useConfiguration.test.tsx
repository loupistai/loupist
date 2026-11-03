import { act, renderHook, waitFor } from "@testing-library/react";
import { MockLoupistBridge } from "../bridge/mockBridge";
import type { ConfigScope } from "../types";
import { useConfiguration } from "./useConfiguration";

it("refreshes other configuration consumers after a pending save while retaining the sender's diagnostic", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const update = bridge.configUpdate.bind(bridge);
  vi.spyOn(bridge, "configUpdate").mockImplementationOnce(async (request) => {
    await update(request);
    throw { code: "config_apply_pending", message: "Configuration was saved, but connection reconciliation is blocked." };
  });
  const { result } = renderHook(() => ({
    sender: useConfiguration(bridge, { kind: "global" }),
    observer: useConfiguration(bridge, { kind: "global" }),
  }));
  await waitFor(() => {
    expect(result.current.sender.snapshot).not.toBeNull();
    expect(result.current.observer.snapshot).not.toBeNull();
  });
  await act(async () => {
    expect(await result.current.sender.update({ pi: { retry: { max_retries: 5 } } })).toBe(false);
  });
  await waitFor(() => expect(result.current.observer.snapshot?.effective.pi.retry.max_retries).toBe(5));
  expect(result.current.sender.error).toContain("Configuration was saved");
  expect(result.current.sender.snapshot?.revision).toBe(result.current.observer.snapshot?.revision);
  expect(result.current.observer.error).toBeNull();
  await act(async () => {
    expect(await result.current.observer.update({ appearance: { theme: "dark" } })).toBe(true);
  });
  expect((await bridge.configRead({ scope: { kind: "global" } })).effective.appearance.theme).toBe("dark");
});

it.each(["scope change", "unmount"])("announces a pending save after its sender leaves through %s", async (departure) => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const update = bridge.configUpdate.bind(bridge);
  let release: () => void = () => {};
  const pending = new Promise<void>((resolve) => { release = resolve; });
  vi.spyOn(bridge, "configUpdate").mockImplementationOnce(async (request) => {
    await update(request);
    await pending;
    throw { code: "config_apply_pending", message: "Configuration was saved, but connection reconciliation is blocked." };
  });
  const sender = renderHook(({ scope }: { scope: ConfigScope }) => useConfiguration(bridge, scope), {
    initialProps: { scope: { kind: "global" } },
  });
  const observer = renderHook(() => useConfiguration(bridge, { kind: "global" }));
  await waitFor(() => {
    expect(sender.result.current.snapshot).not.toBeNull();
    expect(observer.result.current.snapshot).not.toBeNull();
  });
  let outcome: Promise<boolean>;
  act(() => { outcome = sender.result.current.update({ pi: { retry: { max_retries: 5 } } }); });
  if (departure === "scope change") sender.rerender({ scope: { kind: "project", loupistId: "loupist-quality" } });
  else sender.unmount();
  await act(async () => {
    release();
    expect(await outcome).toBe(false);
  });
  await waitFor(() => expect(observer.result.current.snapshot?.effective.pi.retry.max_retries).toBe(5));
  if (departure === "scope change") {
    expect(sender.result.current.snapshot?.scope).toEqual({ kind: "project", loupistId: "loupist-quality" });
    expect(sender.result.current.error).toBeNull();
  }
});
