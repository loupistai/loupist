import { act, renderHook } from "@testing-library/react";
import { DRAIN_LIMIT_MS, lastSentenceEnd, nextUnitEnd, usePacedText } from "./usePacedText";

type Props = { text: string; live: boolean; halted?: boolean; paused?: boolean; arrived?: boolean };

/** Frames run on fake timers, so the governor's timing is exact. */
function setup(initial: Props, reduced = false) {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "requestAnimationFrame", "cancelAnimationFrame", "performance", "Date"] });
  vi.stubGlobal("matchMedia", (query: string) => ({ matches: reduced && query.includes("reduce"), media: query, addEventListener() {}, removeEventListener() {} }));
  const drained = vi.fn();
  const hook = renderHook((props: Props) => usePacedText(props.text, { ...props, onDrained: drained }), { initialProps: initial });
  return { ...hook, drained, advance: (ms: number) => act(() => { vi.advanceTimersByTime(ms); }) };
}

afterEach(() => { vi.unstubAllGlobals(); vi.useRealTimers(); });

const sentence = "Hanbit Chem has the widest margins of the three companies this year. ";

describe("the reveal governor", () => {
  it("reveals whole words at an even pace about half a second behind, never part of a word", () => {
    const text = sentence.repeat(3);
    const { result, rerender, advance } = setup({ text: "", live: true, arrived: true });
    rerender({ text, live: true, arrived: true });
    const lengths: number[] = [];
    for (let frame = 0; frame < 40; frame += 1) { advance(16); lengths.push(result.current.text.length); }
    for (const length of lengths) if (length) expect(/[\p{L}\p{N}.]$/u.test(text.slice(0, length))).toBe(true);
    expect(lengths[2]).toBeLessThan(lengths[20]!);
    expect(lengths.at(-1)).toBeLessThan(text.length);
    advance(1_500);
    expect(text.startsWith(result.current.text)).toBe(true);
    expect(result.current.text.length).toBeGreaterThanOrEqual(text.trimEnd().length);
  });

  it("spreads a burst after a stall instead of showing it in one frame", () => {
    const { result, rerender, advance } = setup({ text: "", live: true, arrived: true });
    rerender({ text: sentence, live: true, arrived: true });
    advance(1_500);
    const before = result.current.text.length;
    rerender({ text: sentence.repeat(4), live: true, arrived: true });
    advance(16);
    expect(result.current.text.length - before).toBeLessThanOrEqual(40);
  });

  it("holds a word that may still grow until the next character arrives", () => {
    expect(nextUnitEnd("Margin fell", 6, false)).toBeNull();
    expect(nextUnitEnd("Margin fell", 6, true)).toBe(11);
    expect(nextUnitEnd("Margin fell ", 0, false)).toBe(6);
    expect(nextUnitEnd("영업이익률은 낮아졌어요 ", 0, false)).toBeGreaterThan(0);
  });

  it("shows everything at once when the segment mounts with text already received", () => {
    const { result } = setup({ text: sentence, live: true, arrived: false });
    expect(result.current.text).toBe(sentence);
  });

  it("freezes while a stop is requested and shows everything received, with no fade, once it is confirmed", () => {
    const text = sentence.repeat(4);
    const { result, rerender, advance } = setup({ text: "", live: true, arrived: true });
    rerender({ text, live: true, arrived: true });
    advance(100);
    rerender({ text, live: true, arrived: true, paused: true });
    const frozen = result.current.text;
    advance(500);
    expect(result.current.text).toBe(frozen);
    expect(frozen.length).toBeLessThan(text.length);
    rerender({ text, live: false, arrived: true, halted: true });
    expect(result.current).toEqual({ text, animating: false });
  });

  it("drains faster once the host finished, signals the commit once, and never waits more than 400 ms", () => {
    const text = sentence.repeat(8);
    const { result, rerender, advance, drained } = setup({ text: "", live: true, arrived: true });
    rerender({ text, live: true, arrived: true });
    advance(50);
    rerender({ text, live: false, arrived: true });
    expect(drained).not.toHaveBeenCalled();
    expect(result.current.animating).toBe(true);
    advance(DRAIN_LIMIT_MS + 32);
    expect(result.current).toEqual({ text, animating: false });
    expect(drained).toHaveBeenCalledTimes(1);
  });

  it("under reduced motion reveals whole sentences, at most four times a second", () => {
    const text = "First sentence here. Second one is still being writ";
    const { result, rerender, advance } = setup({ text: "", live: true, arrived: true }, true);
    rerender({ text, live: true, arrived: true });
    advance(16);
    expect(result.current.text).toBe("First sentence here.");
    rerender({ text: `${text}ten. Third`, live: true, arrived: true });
    advance(100);
    expect(result.current.text).toBe("First sentence here.");
    advance(200);
    expect(result.current.text).toBe("First sentence here. Second one is still being written.");
    expect(lastSentenceEnd("No end yet", 0)).toBe(-1);
  });
});
