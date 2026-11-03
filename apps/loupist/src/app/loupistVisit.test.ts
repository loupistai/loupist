import { act, renderHook } from "@testing-library/react";
import { lastSeenAt, recordVisit, useLoupistVisit } from "./loupistVisit";

const MINUTE = 60_000;
const START = new Date(2026, 9, 31, 9, 0).getTime();

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(START); localStorage.clear(); });
afterEach(() => { vi.useRealTimers(); localStorage.clear(); });

const later = (minutes: number) => act(() => { vi.setSystemTime(Date.now() + minutes * MINUTE); });

describe("Loupist visits", () => {
  it("counts only visits of at least five minutes", () => {
    expect(recordVisit("loupist-a", START, START + 4 * MINUTE)).toBe(false);
    expect(lastSeenAt("loupist-a")).toBeUndefined();
    expect(recordVisit("loupist-a", START, START + 5 * MINUTE)).toBe(true);
    expect(lastSeenAt("loupist-a")).toBe(START + 5 * MINUTE);
    expect(lastSeenAt("loupist-b")).toBeUndefined();
  });

  it("treats unreadable storage as a first visit", () => {
    localStorage.setItem("loupist.loupistVisit.v1", "{not json");
    expect(lastSeenAt("loupist-a")).toBeUndefined();
    localStorage.setItem("loupist.loupistVisit.v1", JSON.stringify({ "loupist-a": { lastSeenAtMs: "yesterday" } }));
    expect(lastSeenAt("loupist-a")).toBeUndefined();
  });

  it("records the visit when the user leaves the Loupist, never after a quick look", () => {
    const view = renderHook(({ loupistId, present }) => useLoupistVisit(loupistId, present), { initialProps: { loupistId: "loupist-a" as string | null, present: true } });
    later(2);
    view.rerender({ loupistId: "loupist-b", present: true });
    expect(lastSeenAt("loupist-a")).toBeUndefined();
    later(6);
    view.rerender({ loupistId: "loupist-b", present: false });
    expect(lastSeenAt("loupist-b")).toBe(START + 8 * MINUTE);
    view.unmount();
  });

  it("ends the visit when the window is away for more than five minutes", () => {
    const view = renderHook(() => useLoupistVisit("loupist-a", true));
    later(6);
    const leftAt = Date.now();
    act(() => { window.dispatchEvent(new Event("blur")); });
    later(3);
    act(() => { window.dispatchEvent(new Event("focus")); });
    expect(lastSeenAt("loupist-a")).toBeUndefined();
    act(() => { window.dispatchEvent(new Event("blur")); });
    later(6);
    act(() => { window.dispatchEvent(new Event("focus")); });
    expect(lastSeenAt("loupist-a")).toBe(leftAt + 3 * MINUTE);
    later(1);
    view.unmount();
    expect(lastSeenAt("loupist-a")).toBe(leftAt + 3 * MINUTE);
  });
});
