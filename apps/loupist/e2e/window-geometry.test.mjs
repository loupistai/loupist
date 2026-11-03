import assert from "node:assert/strict";
import test from "node:test";
import { runInNewContext } from "node:vm";
import { desktopWindowSize, minimumWindowSize, setWindowSize, withWindowSize } from "./window-geometry.mjs";

function fixture({ initial = { x: 80, y: 40, ...minimumWindowSize }, staleViewports = [], constrain, viewportFor } = {}) {
  let rect = { ...initial };
  let reads = 0;
  const browser = {
    getWindowRect: async () => ({ ...rect }),
    getWindowSize: async () => ({ width: rect.width, height: rect.height }),
    setWindowSize: async (width, height) => { rect = { ...rect, ...(constrain?.({ width, height }) ?? { width, height }) }; },
    setWindowRect: async (x, y, width, height) => { rect = { x, y, width, height }; },
    execute: async (script) => {
      reads += 1;
      const viewport = staleViewports.shift() ?? viewportFor?.(rect) ?? { width: rect.width - 16, height: rect.height - 39 };
      return { ...runInNewContext(`(${script})()`, {
        window: { innerWidth: viewport.width, innerHeight: viewport.height },
        document: { documentElement: { clientWidth: viewport.width, clientHeight: 8 } },
      }) };
    },
    pause: async () => {},
    waitUntil: async (condition) => {
      for (let attempt = 0; attempt < 5; attempt += 1) {
        if (await condition()) return;
      }
      throw new Error("Timed out");
    },
  };
  return { browser, rect: () => rect, reads: () => reads };
}

test("desktop preparation measures the viewport even before document content fills it", async () => {
  const { browser } = fixture();
  const geometry = await setWindowSize(browser);
  assert.deepEqual(geometry, {
    window: desktopWindowSize,
    viewport: { width: 1264, height: 781 },
    units: "logical CSS pixels",
  });
});

test("resize waits for a stale WebView to apply the new CSS viewport", async () => {
  const source = fixture({ staleViewports: [{ width: 684, height: 521 }, { width: 684, height: 521 }] });
  const geometry = await setWindowSize(source.browser);
  assert.equal(geometry.viewport.width, 1264);
  assert.equal(source.reads(), 3);
});

test("a physical-pixel response cannot qualify a half-size CSS viewport", async () => {
  const source = fixture({ viewportFor: (rect) => ({ width: rect.width / 2, height: rect.height / 2 }) });
  await assert.rejects(setWindowSize(source.browser), /"viewport":\{"width":640,"height":410\}/);
});

test("a display-capped window fails with the actual geometry", async () => {
  const source = fixture({ constrain: () => ({ width: 1200, height: 780 }) });
  await assert.rejects(setWindowSize(source.browser), /"window":\{"width":1200,"height":780\}/);
});

test("a temporary minimum-window check restores the desktop size and position", async () => {
  const original = { x: 80, y: 40, ...desktopWindowSize };
  const source = fixture({ initial: original });
  const result = await withWindowSize(source.browser, minimumWindowSize, async (geometry) => {
    assert.deepEqual(geometry.window, minimumWindowSize);
    assert.deepEqual(source.rect(), { ...original, ...minimumWindowSize });
    return "observed";
  });
  assert.equal(result, "observed");
  assert.deepEqual(source.rect(), original);
});

test("a failed layout assertion still restores the desktop window", async () => {
  const original = { x: 80, y: 40, ...desktopWindowSize };
  const source = fixture({ initial: original });
  const failure = new Error("Layout clipped");
  await assert.rejects(withWindowSize(source.browser, minimumWindowSize, async () => { throw failure; }),
    (error) => error === failure);
  assert.deepEqual(source.rect(), original);
});

test("a failed narrow resize restores the desktop before running any layout checks", async () => {
  const original = { x: 80, y: 40, ...desktopWindowSize };
  const source = fixture({ initial: original, viewportFor: (rect) => rect.width === 700
    ? { width: 350, height: 280 } : { width: rect.width, height: rect.height } });
  let observed = false;
  await assert.rejects(withWindowSize(source.browser, minimumWindowSize, async () => { observed = true; }),
    /Native window size could not be verified/);
  assert.equal(observed, false);
  assert.deepEqual(source.rect(), original);
});
