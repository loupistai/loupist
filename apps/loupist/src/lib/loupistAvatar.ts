import { LOUPIST_PORTRAIT_IDS, type LoupistAvatarId, type LoupistPortraitId } from "../contracts/loupist";

export type LoupistAvatarIdentity = { id?: string; name: string; avatar?: LoupistAvatarId };

const legacyPortraits: Partial<Record<LoupistAvatarId, LoupistPortraitId>> = {
  cpu: "loupist-02", coins: "loupist-03", globe: "loupist-04", "line-chart": "loupist-01",
  landmark: "loupist-06", factory: "loupist-07", compass: "loupist-08", "book-open": "loupist-09", sparkles: "loupist-10",
};

/** Initials used no stored image. Hash the stable profile ID so renaming keeps its portrait. */
export function loupistPortraitId(loupist: LoupistAvatarIdentity): LoupistPortraitId {
  const avatar = loupist.avatar;
  if (avatar && (LOUPIST_PORTRAIT_IDS as readonly string[]).includes(avatar)) return avatar as LoupistPortraitId;
  if (avatar && legacyPortraits[avatar]) return legacyPortraits[avatar]!;
  let hash = 2166136261;
  for (const character of loupist.id || loupist.name) hash = Math.imul(hash ^ character.codePointAt(0)!, 16777619);
  return LOUPIST_PORTRAIT_IDS[(hash >>> 0) % LOUPIST_PORTRAIT_IDS.length];
}

const portraits = import.meta.glob<string>("../assets/avatars/loupist-*.png", { eager: true, query: "?url", import: "default" });

export function loupistAvatarSource(loupist: LoupistAvatarIdentity): string {
  const id = loupistPortraitId(loupist);
  return portraits[`../assets/avatars/${id}.png`];
}
