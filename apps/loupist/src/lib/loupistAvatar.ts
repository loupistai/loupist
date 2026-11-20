import { LOUPIST_PORTRAIT_IDS, type LoupistAvatarId, type LoupistPortraitId } from "../contracts/loupist";

export type LoupistAvatarIdentity = { id?: string; name: string; avatar?: LoupistAvatarId };

const legacyPortraits: Partial<Record<LoupistAvatarId, LoupistPortraitId>> = {
  cpu: "loupist-02", coins: "loupist-03", globe: "loupist-04", "line-chart": "loupist-01",
  landmark: "loupist-06", factory: "loupist-07", compass: "loupist-08", "book-open": "loupist-09", sparkles: "loupist-10",
};

/**
 * The stored avatar id, resolved to one of the ten portrait ids. Each portrait id now
 * names a face colour. Older icon ids keep their fixed mapping; "initial" (no stored
 * choice) hashes the stable profile id, so renaming keeps the colour.
 */
export function loupistPortraitId(loupist: LoupistAvatarIdentity): LoupistPortraitId {
  const avatar = loupist.avatar;
  if (avatar && (LOUPIST_PORTRAIT_IDS as readonly string[]).includes(avatar)) return avatar as LoupistPortraitId;
  if (avatar && legacyPortraits[avatar]) return legacyPortraits[avatar]!;
  let hash = 2166136261;
  for (const character of loupist.id || loupist.name) hash = Math.imul(hash ^ character.codePointAt(0)!, 16777619);
  return LOUPIST_PORTRAIT_IDS[(hash >>> 0) % LOUPIST_PORTRAIT_IDS.length];
}

/** The face colour token for a portrait id: `loupist-03` wears `--gg-color-face-03`. */
export function loupistFaceToken(id: LoupistPortraitId): string {
  return `--gg-color-face-${id.slice(-2)}`;
}

/**
 * The face, in a 48-unit box: a round body, two eyes (the right one behind the loupe's
 * glass), a small smile and the loupe's handle. One drawing serves the component and
 * the image export, so the two cannot drift.
 */
export const LOUPIST_FACE = {
  body: "M24 2.5c12.6 0 21.5 8.7 21.5 21S36.6 45.5 24 45.5 2.5 36.9 2.5 23.5 11.4 2.5 24 2.5z",
  lens: { cx: 30.5, cy: 22, r: 8.6 },
  handle: "M36.9 28.6l4.4 4.6",
  eyes: [{ cx: 15.5, cy: 22 }, { cx: 30.5, cy: 22 }],
  eye: { rx: 3.2, ry: 4.4 },
  mouth: "M19.2 32.4q3.8 2.9 7.6 0",
} as const;

/**
 * A standalone SVG image of the face for canvas drawing (the Map export). Colours are
 * read from the current theme's tokens when the image is made.
 */
export function loupistAvatarSource(loupist: LoupistAvatarIdentity): string {
  const style = typeof document === "undefined" ? null : getComputedStyle(document.documentElement);
  const token = (name: string) => style?.getPropertyValue(name).trim() || "currentColor";
  const body = token(loupistFaceToken(loupistPortraitId(loupist)));
  const ink = token("--gg-color-face-ink");
  const detail = token("--gg-color-face-detail");
  const { lens, eye } = LOUPIST_FACE;
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 48 48" width="48" height="48">`
    + `<path d="${LOUPIST_FACE.body}" fill="${body}"/>`
    + `<circle cx="${lens.cx}" cy="${lens.cy}" r="${lens.r}" fill="${detail}" fill-opacity=".2" stroke="${detail}" stroke-width="2.4"/>`
    + `<path d="${LOUPIST_FACE.handle}" stroke="${detail}" stroke-width="3" stroke-linecap="round" fill="none"/>`
    + LOUPIST_FACE.eyes.map(({ cx, cy }) => `<ellipse cx="${cx}" cy="${cy}" rx="${eye.rx}" ry="${eye.ry}" fill="${ink}"/>`).join("")
    + `<path d="${LOUPIST_FACE.mouth}" stroke="${ink}" stroke-width="2.2" stroke-linecap="round" fill="none"/>`
    + `</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}
