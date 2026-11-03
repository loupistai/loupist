export type MapExportFooter = { name: string; stats: string; date: string; disclosure: string; avatar?: HTMLImageElement };
const FOOTER_HEIGHT = 64;
// Only the map bitmap and these explicit display fields cross into an export.
// The graph contract supplies holding rings, never quantities or valuations.
export function composeMapExport(source: HTMLCanvasElement, footer: MapExportFooter, video = false, output = document.createElement("canvas")) {
  const width = source.clientWidth || source.width, height = source.clientHeight || source.height;
  const scale = video ? (1080 - FOOTER_HEIGHT) / height : 2;
  const mapHeight = video ? 1080 - FOOTER_HEIGHT : height;
  const logicalWidth = video ? Math.round(width * scale / 2) * 2 : width;
  const pixelScale = video ? 1 : scale;
  const targetWidth = Math.round(logicalWidth * pixelScale), targetHeight = video ? 1080 : Math.round((height + FOOTER_HEIGHT) * pixelScale);
  if (output.width !== targetWidth) output.width = targetWidth;
  if (output.height !== targetHeight) output.height = targetHeight;
  const ctx = output.getContext("2d");
  if (!ctx) throw new Error("Canvas unavailable");
  const css = getComputedStyle(document.documentElement);
  ctx.setTransform(pixelScale, 0, 0, pixelScale, 0, 0);
  ctx.fillStyle = css.getPropertyValue("--gg-color-map-bg").trim(); ctx.fillRect(0, 0, logicalWidth, mapHeight);
  ctx.drawImage(source, 0, 0, logicalWidth, mapHeight);
  ctx.fillStyle = css.getPropertyValue("--gg-color-bg-raised").trim(); ctx.fillRect(0, mapHeight, logicalWidth, FOOTER_HEIGHT);
  ctx.fillStyle = css.getPropertyValue("--gg-color-text-secondary").trim();
  const font = css.getPropertyValue("--gg-font-ui").trim() || "sans-serif";
  ctx.font = `12px ${font}`; ctx.textBaseline = "middle";
  const fit = (text: string, max: number) => { let value = text; while (value.length > 1 && ctx.measureText(value).width > max) value = value.slice(0, -2) + "…"; return value; };
  const third = (logicalWidth - 32) / 3;
  ctx.textAlign = "left";
  ctx.beginPath(); ctx.arc(28, mapHeight + 20, 12, 0, Math.PI * 2); ctx.fillStyle = css.getPropertyValue("--gg-color-accent-soft").trim(); ctx.fill();
  if (footer.avatar) ctx.drawImage(footer.avatar, 19, mapHeight + 11, 18, 18);
  else { ctx.fillStyle = css.getPropertyValue("--gg-color-accent-ink").trim(); ctx.textAlign = "center"; ctx.fillText([...footer.name][0] ?? "", 28, mapHeight + 20); }
  ctx.textAlign = "left"; ctx.fillStyle = css.getPropertyValue("--gg-color-text-secondary").trim();
  ctx.fillText(fit(footer.name, third - 40), 48, mapHeight + 20);
  ctx.textAlign = "center"; ctx.fillText(fit(footer.stats, third - 8), logicalWidth / 2, mapHeight + 20);
  ctx.textAlign = "right"; ctx.fillText(fit(`${footer.date} · Loupist`, third - 8), logicalWidth - 16, mapHeight + 20);
  ctx.textAlign = "center"; ctx.font = `11px ${font}`; ctx.fillText(footer.disclosure, logicalWidth / 2, mapHeight + 47, logicalWidth - 32);
  return output;
}
export function pngBytes(canvas: HTMLCanvasElement) {
  const encoded = canvas.toDataURL("image/png").split(",")[1];
  return Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
}
export function recordingFormat() {
  if (typeof MediaRecorder === "undefined") throw new Error("Recording unavailable");
  for (const mime of ["video/mp4;codecs=avc1.42E01E", "video/mp4", "video/webm;codecs=vp9", "video/webm;codecs=vp8", "video/webm"]) {
    if (MediaRecorder.isTypeSupported(mime)) return { mime, kind: mime.startsWith("video/mp4") ? "mp4" as const : "webm" as const };
  }
  throw new Error("Recording unavailable");
}
