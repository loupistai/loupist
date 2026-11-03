import { LOUPIST_AVATAR_IDS, LOUPIST_PORTRAIT_IDS } from "../contracts/loupist";
import { loupistAvatarSource, loupistPortraitId } from "./loupistAvatar";

it.each(LOUPIST_PORTRAIT_IDS)("resolves bundled portrait %s", avatar => {
  const profile = { id: "same-profile", name: "Research", avatar };
  expect(loupistPortraitId(profile)).toBe(avatar);
  expect(loupistAvatarSource(profile)).toContain(`${avatar}.png`);
});

it.each(LOUPIST_AVATAR_IDS)("gives persisted avatar %s a bundled portrait", avatar => {
  expect(LOUPIST_PORTRAIT_IDS).toContain(loupistPortraitId({ id: "existing-profile", name: "Research", avatar }));
});

it.each([['cpu', 'loupist-02'], ['compass', 'loupist-08'], ['line-chart', 'loupist-01']] as const)("preserves the fixed portrait for legacy %s", (avatar, expected) => {
  expect(loupistPortraitId({ id: "existing-profile", name: "Research", avatar })).toBe(expected);
});

it("keeps an initial-based profile's portrait stable after renaming and reopening", () => {
  const original = { id: "legacy-profile", name: "Quality", avatar: "initial" as const };
  const portrait = loupistPortraitId(original);
  expect(loupistPortraitId({ ...original, name: "Renamed" })).toBe(portrait);
  expect(loupistPortraitId(JSON.parse(JSON.stringify(original)))).toBe(portrait);
  expect(loupistPortraitId({ id: original.id, name: "Quality" })).toBe(portrait);
});
