/** "Last looked" per asset page, per device (spec §3.5): written on leaving after ≥ 10 s. */
const KEY = "loupist.assetVisit.v1";
export const MIN_ASSET_VISIT_MS = 10_000;

function read(): Record<string, number> {
  try {
    const value = JSON.parse(localStorage.getItem(KEY) ?? "{}") as unknown;
    return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, number> : {};
  } catch {
    return {};
  }
}

export function lastAssetVisit(loupistId: string, assetId: string): number | undefined {
  const value = read()[`${loupistId}:${assetId}`];
  return typeof value === "number" && Number.isFinite(value) ? value : undefined;
}

export function recordAssetVisit(loupistId: string, assetId: string, enteredAtMs: number, nowMs = Date.now()) {
  if (nowMs - enteredAtMs < MIN_ASSET_VISIT_MS) return;
  try {
    localStorage.setItem(KEY, JSON.stringify({ ...read(), [`${loupistId}:${assetId}`]: nowMs }));
  } catch {
    // Storage may be unavailable; the page then opens as a first look.
  }
}

/** Range choice for "What changed", remembered for this session. */
let sessionRange: "since" | "7" | "30" | undefined;
export const rememberedRange = () => sessionRange;
export const rememberRange = (range: "since" | "7" | "30") => { sessionRange = range; };
