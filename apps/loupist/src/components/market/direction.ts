export type PriceDirection = "up" | "down" | "flat";

/** Direction of a change as it is shown: a change that rounds to zero at `digits` decimals is flat. */
export function priceDirection(value: number, digits = 2): PriceDirection {
  if (!Number.isFinite(value)) return "flat";
  const scale = 10 ** digits;
  const rounded = Math.round(Math.abs(value) * scale);
  if (rounded === 0) return "flat";
  return value > 0 ? "up" : "down";
}

/** Never colour alone: every change carries one of these beside its sign. */
export const DIRECTION_GLYPH: Record<PriceDirection, string> = { up: "▲", down: "▼", flat: "■" };
