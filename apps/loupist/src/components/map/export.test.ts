import { composeMapExport, pngBytes, recordingFormat } from "./export";
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });
it("composites a 2x PNG or 1080-high video with an identity/stats/disclosure footer", () => {
  const ctx = { setTransform: vi.fn(), drawImage: vi.fn(), fillRect: vi.fn(), fillText: vi.fn(), beginPath: vi.fn(), arc: vi.fn(), fill: vi.fn(), measureText: (s: string) => ({ width: s.length * 5 }) };
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(ctx as unknown as CanvasRenderingContext2D);
  const source = document.createElement("canvas"); source.width = 800; source.height = 600;
  const footer = { name: "Research", stats: "40 names · 60 relationships", date: "Oct 28", disclosure: "AI can be wrong. Not a licensed financial adviser." };
  const png = composeMapExport(source, footer); expect([png.width, png.height]).toEqual([1600, 1328]);
  expect(ctx.fillText).toHaveBeenCalledWith(footer.disclosure, 400, 647, 768);
  expect(ctx.drawImage).toHaveBeenCalledWith(source, 0, 0, 800, 600);
  const video = composeMapExport(source, footer, true); expect(video.height).toBe(1080); expect(video.width % 2).toBe(0);
  vi.spyOn(png, "toDataURL").mockReturnValue("data:image/png;base64,AQID"); expect([...pngBytes(png)]).toEqual([1, 2, 3]);
});
it("prefers MP4 and falls back to WebM supported by the recorder", () => {
  const supports = vi.fn((mime: string) => mime.startsWith("video/mp4")); vi.stubGlobal("MediaRecorder", { isTypeSupported: supports });
  expect(recordingFormat().kind).toBe("mp4"); supports.mockImplementation(mime => mime === "video/webm"); expect(recordingFormat().kind).toBe("webm");
  supports.mockReturnValue(false); expect(() => recordingFormat()).toThrow();
});
