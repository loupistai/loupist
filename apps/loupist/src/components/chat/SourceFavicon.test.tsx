import { fireEvent, render, waitFor } from "@testing-library/react";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { evidenceFixture } from "../../test/evidenceFixtures";
import { SourceFavicon, SourceFaviconProvider } from "./SourceFavicon";

const png = "data:image/png;base64,iVBORw0KGgo=";

it("shares one origin-only lookup across source entries and falls back when an image fails", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const read = vi.spyOn(bridge, "siteFavicon").mockResolvedValue(png);
  const first = evidenceFixture({ url: "https://example.com/a?private=value#section" });
  const second = evidenceFixture({ url: "https://example.com/b" });
  const { container } = render(<SourceFaviconProvider bridge={bridge}><SourceFavicon evidence={first} /><SourceFavicon evidence={second} /></SourceFaviconProvider>);
  await waitFor(() => expect(container.querySelectorAll("img")).toHaveLength(2));
  expect(read).toHaveBeenCalledExactlyOnceWith("https://example.com");
  fireEvent.error(container.querySelector("img")!);
  expect(container.querySelectorAll("img")).toHaveLength(1);
});

it("keeps missing icons quiet and does not request icons for local or unusable sources", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const read = vi.spyOn(bridge, "siteFavicon").mockRejectedValue(new Error("Offline"));
  const { container } = render(<SourceFaviconProvider bridge={bridge}>
    <SourceFavicon evidence={evidenceFixture()} />
    <SourceFavicon evidence={evidenceFixture({ kind: "local_file" })} />
    <SourceFavicon evidence={evidenceFixture({ url: "https://user:password@example.com" })} />
  </SourceFaviconProvider>);
  await waitFor(() => expect(read).toHaveBeenCalledOnce());
  expect(container.querySelector("img")).toBeNull();
});

it("waits until a source becomes visible before fetching its icon", async () => {
  let notify!: IntersectionObserverCallback;
  const observe = vi.fn();
  const disconnect = vi.fn();
  vi.stubGlobal("IntersectionObserver", class {
    constructor(callback: IntersectionObserverCallback) { notify = callback; }
    observe = observe;
    disconnect = disconnect;
  });
  try {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const read = vi.spyOn(bridge, "siteFavicon").mockResolvedValue(png);
    const mounted = render(<SourceFaviconProvider bridge={bridge}><SourceFavicon evidence={evidenceFixture()} /></SourceFaviconProvider>);
    expect(observe).toHaveBeenCalledOnce();
    expect(read).not.toHaveBeenCalled();
    notify([{ isIntersecting: true } as IntersectionObserverEntry], {} as IntersectionObserver);
    await waitFor(() => expect(mounted.container.querySelector("img")).not.toBeNull());
    expect(disconnect).toHaveBeenCalled();
    mounted.unmount();
  } finally { vi.unstubAllGlobals(); }
});
