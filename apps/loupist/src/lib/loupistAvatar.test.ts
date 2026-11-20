import { LOUPIST_AVATAR_IDS, LOUPIST_PORTRAIT_IDS } from "../contracts/loupist";
import { loupistAvatarSource, loupistFaceToken, loupistPortraitId } from "./loupistAvatar";

it("gives each of the ten stored portrait ids its own face colour", () => {
  const tokens = LOUPIST_PORTRAIT_IDS.map(avatar => loupistFaceToken(loupistPortraitId({ id: "same-profile", name: "Research", avatar })));
  expect(new Set(tokens).size).toBe(10);
  expect(tokens[2]).toBe("--gg-color-face-03");
});

it("draws the export image in the current theme's face colour", () => {
  document.documentElement.style.setProperty("--gg-color-face-07", "#123456");
  try {
    const source = decodeURIComponent(loupistAvatarSource({ id: "same-profile", name: "Research", avatar: "loupist-07" }));
    expect(source).toMatch(/^data:image\/svg\+xml/);
    expect(source).toContain('fill="#123456"');
  } finally { document.documentElement.style.removeProperty("--gg-color-face-07"); }
});

it.each(LOUPIST_AVATAR_IDS)("gives persisted avatar %s a face colour", avatar => {
  expect(LOUPIST_PORTRAIT_IDS).toContain(loupistPortraitId({ id: "existing-profile", name: "Research", avatar }));
});

it.each([['cpu', 'loupist-02'], ['compass', 'loupist-08'], ['line-chart', 'loupist-01']] as const)("preserves the fixed colour for legacy %s", (avatar, expected) => {
  expect(loupistPortraitId({ id: "existing-profile", name: "Research", avatar })).toBe(expected);
});

it("keeps an initial-based profile's colour stable after renaming and reopening", () => {
  const original = { id: "legacy-profile", name: "Quality", avatar: "initial" as const };
  const portrait = loupistPortraitId(original);
  expect(loupistPortraitId({ ...original, name: "Renamed" })).toBe(portrait);
  expect(loupistPortraitId(JSON.parse(JSON.stringify(original)))).toBe(portrait);
  expect(loupistPortraitId({ id: original.id, name: "Quality" })).toBe(portrait);
});
