import { act, renderHook } from "@testing-library/react";
import { usePacedText } from "./usePacedText";

it("paints successive word-boundary prefixes and catches up to a growing Korean reply", () => {
  vi.useFakeTimers();
  const media = window.matchMedia("(prefers-reduced-motion: no-preference)");
  vi.stubGlobal("matchMedia", () => ({ ...media, matches: true }));
  try {
    const { result, rerender } = renderHook(({ text, streaming }) => usePacedText(text, streaming), {
      initialProps: { text: "", streaming: true },
    });
    const first = "월별 매출을 확인하면 ".repeat(25);
    const full = first + "합계는 405이고 평균은 135입니다.";
    rerender({ text: first, streaming: true });
    act(() => vi.advanceTimersByTime(32));
    const early = result.current;
    expect(early.length).toBeGreaterThan(0);
    expect(early.length).toBeLessThan(first.length);
    rerender({ text: full, streaming: true });
    act(() => vi.advanceTimersByTime(32));
    expect(result.current.startsWith(early)).toBe(true);
    expect(result.current.length).toBeGreaterThan(early.length);
    expect(full.startsWith(result.current)).toBe(true);
    act(() => vi.advanceTimersByTime(112));
    expect(result.current).toBe(full);

    rerender({ text: full + first, streaming: true });
    rerender({ text: "중지 시 보관된 답변", streaming: false });
    expect(result.current).toBe("중지 시 보관된 답변");
    act(() => vi.advanceTimersByTime(200));
    expect(result.current).toBe("중지 시 보관된 답변");
  } finally {
    vi.unstubAllGlobals();
    vi.useRealTimers();
  }
});
