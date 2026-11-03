import { fireEvent, render, renderHook, screen } from "@testing-library/react";
import { useFileDropGuard } from "./useFileDropGuard";

it("prevents file drops outside upload zones from navigating the webview", () => {
  renderHook(() => useFileDropGuard());
  // During dragover browsers expose file types but protect the file contents.
  expect(fireEvent.dragOver(document.body, { dataTransfer: { types: ["Files"], files: [] } })).toBe(false);
  expect(fireEvent.drop(document.body, {
    dataTransfer: { types: ["Files"], files: [new File(["source"], "source.txt")] },
  })).toBe(false);
});

it("lets the target upload zone receive the exact dropped file", () => {
  renderHook(() => useFileDropGuard());
  const received = vi.fn();
  render(<div role="region" aria-label="Upload source" onDrop={(event) => received(event.dataTransfer.files[0])} />);
  const file = new File([new Uint8Array([0x00, 0xff])], "source.bin");
  expect(fireEvent.drop(screen.getByRole("region", { name: "Upload source" }), {
    dataTransfer: { types: ["Files"], files: [file] },
  })).toBe(false);
  expect(received).toHaveBeenCalledExactlyOnceWith(file);
});

it.each(["text/plain", "text/uri-list"])("preserves native %s drops", (type) => {
  renderHook(() => useFileDropGuard());
  const received = vi.fn();
  render(<textarea aria-label="Text destination" onDrop={received} />);
  const destination = screen.getByRole("textbox", { name: "Text destination" });
  const dataTransfer = { types: [type], files: [], getData: () => type === "text/plain" ? "Research note" : "https://example.com" };
  expect(fireEvent.dragOver(destination, { dataTransfer })).toBe(true);
  expect(fireEvent.drop(destination, { dataTransfer })).toBe(true);
  expect(received).toHaveBeenCalledOnce();
});

it("removes the guard when the app unmounts", () => {
  const { unmount } = renderHook(() => useFileDropGuard());
  unmount();
  expect(fireEvent.dragOver(document.body, { dataTransfer: { types: ["Files"], files: [] } })).toBe(true);
  expect(fireEvent.drop(document.body, { dataTransfer: { types: ["Files"], files: [] } })).toBe(true);
});
