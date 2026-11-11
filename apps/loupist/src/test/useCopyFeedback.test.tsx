import { act, renderHook } from "@testing-library/react";
import { useCopyFeedback } from "../hooks/useCopyFeedback";

afterEach(() => vi.useRealTimers());

it("restarts feedback after each successful copy and exposes failures", async () => {
  vi.useFakeTimers();
  const write = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue(undefined);
  const { result } = renderHook(() => useCopyFeedback("answer"));
  await act(async () => { await result.current.copy("First"); });
  expect(result.current.state).toBe("copied");
  await act(async () => { await vi.advanceTimersByTimeAsync(1_500); await result.current.copy("Again"); });
  await act(async () => { await vi.advanceTimersByTimeAsync(1_000); });
  expect(result.current.state).toBe("copied");
  await act(async () => { await vi.advanceTimersByTimeAsync(1_000); });
  expect(result.current.state).toBe("idle");
  write.mockRejectedValueOnce(new Error("Clipboard unavailable"));
  await act(async () => { expect(await result.current.copy("Try")).toBe(false); });
  expect(result.current.state).toBe("failed");
});

it("ignores stale clipboard completions after content changes or a newer attempt", async () => {
  let resolveOld!: () => void;
  vi.spyOn(navigator.clipboard, "writeText").mockImplementationOnce(() => new Promise<void>(resolve => { resolveOld = resolve; })).mockResolvedValue(undefined);
  const { result, rerender, unmount } = renderHook(({ key }) => useCopyFeedback(key), { initialProps: { key: "first" } });
  let old!: Promise<boolean>;
  act(() => { old = result.current.copy("Old"); });
  rerender({ key: "second" });
  await act(async () => { resolveOld(); expect(await old).toBe(false); });
  expect(result.current.state).toBe("idle");
  await act(async () => { await result.current.copy("Current"); });
  expect(result.current.state).toBe("copied");
  unmount();
});
