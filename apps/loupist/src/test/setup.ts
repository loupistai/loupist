/// <reference types="vitest/jsdom" />
import "@testing-library/jest-dom/vitest";
import { configure } from "@testing-library/react";

// Streaming-chat tests wait on real timers, so a loaded machine (CI, verify.sh
// alongside cargo builds) needs a wider async window than the 1s default.
configure({ asyncUtilTimeout: 20_000 });

// Node 26 exposes Web Storage globals even without a storage file. Renderer
// tests must use the jsdom window's per-origin storage, never Node's globals.
for (const name of ["localStorage", "sessionStorage"] as const) {
  Object.defineProperty(globalThis, name, {
    configurable: true,
    get: () => jsdom.window[name],
  });
}

// Independent profiles between tests. Remounts inside a test retain their saved state.
beforeEach(() => { localStorage.clear(); sessionStorage.clear(); });

Object.defineProperty(window, "matchMedia", {
  configurable: true,
  value: vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  })),
});

class ResizeObserverMock implements ResizeObserver {
  disconnect = vi.fn();
  observe = vi.fn();
  unobserve = vi.fn();
}

Object.defineProperty(globalThis, "ResizeObserver", {
  configurable: true,
  value: ResizeObserverMock,
});

Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
  configurable: true,
  value: vi.fn(),
});

// jsdom has no 2D context; chart libraries call drawing methods (scale, save, fillText…)
// while laying out, so every method is a no-op and text measures 8 px per character.
Object.defineProperty(HTMLCanvasElement.prototype, "getContext", {
  configurable: true,
  value: vi.fn(() => new Proxy({ font: "", measureText: (text: string) => ({ width: text.length * 8 }) } as Record<string, unknown>, {
    get: (target, key) => (key in target ? target[key as string] : () => undefined),
    set: (target, key, value) => { target[key as string] = value; return true; },
  })),
});

Object.defineProperties(HTMLElement.prototype, {
  hasPointerCapture: {
    configurable: true,
    value: vi.fn().mockReturnValue(false),
  },
  releasePointerCapture: {
    configurable: true,
    value: vi.fn(),
  },
  setPointerCapture: {
    configurable: true,
    value: vi.fn(),
  },
});

Object.defineProperty(navigator, "clipboard", {
  configurable: true,
  value: { writeText: vi.fn().mockResolvedValue(undefined) },
});

Object.defineProperties(URL, {
  createObjectURL: {
    configurable: true,
    value: vi.fn(
      (blob: Blob) => `data:${blob.type || "application/octet-stream"};base64,dGVzdA==`,
    ),
  },
  revokeObjectURL: {
    configurable: true,
    value: vi.fn(),
  },
});
