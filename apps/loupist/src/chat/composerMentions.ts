export type ComposerMentionKind = "skill" | "connection";

export type ComposerPromptPart =
  | { type: "text"; value: string }
  | { type: "mention"; kind: ComposerMentionKind; value: string };

const MENTION_PATTERN = /(^|\s)([@$][\p{L}\p{N}._:@/-]*)/gu;

export const knownComposerMentions = (
  skills: ReadonlyArray<{ id: string }>,
  plugins: ReadonlyArray<{ id: string }>,
): ReadonlySet<string> => {
  const known = new Set<string>();
  for (const skill of skills) {
    known.add(`$${skill.id}`);
  }
  for (const plugin of plugins) {
    known.add(`@${plugin.id}`);
  }
  return known;
};

/** Plain `@entry_id` tokens from the prompt; host intersects with enabled bindings. */
export const extractMentionedConnectionIds = (prompt: string): string[] => {
  const ids: string[] = [];
  const seen = new Set<string>();
  for (const match of prompt.matchAll(MENTION_PATTERN)) {
    const token = match[2] ?? "";
    if (!token.startsWith("@") || token.length < 2) continue;
    const id = token.slice(1);
    if (seen.has(id)) continue;
    seen.add(id);
    ids.push(id);
  }
  return ids;
};

export const splitComposerMentions = (
  prompt: string,
  knownMentions: ReadonlySet<string>,
): ComposerPromptPart[] => {
  const parts: ComposerPromptPart[] = [];
  const pushText = (value: string) => {
    if (!value) return;
    const last = parts.at(-1);
    if (last?.type === "text") {
      last.value += value;
      return;
    }
    parts.push({ type: "text", value });
  };

  let lastIndex = 0;
  for (const match of prompt.matchAll(MENTION_PATTERN)) {
    const lead = match[1] ?? "";
    const token = match[2] ?? "";
    const tokenStart = (match.index ?? 0) + lead.length;
    if (tokenStart > lastIndex) {
      pushText(prompt.slice(lastIndex, tokenStart));
    }
    if (token) {
      if (knownMentions.has(token)) {
        parts.push({
          type: "mention",
          kind: token.startsWith("@") ? "connection" : "skill",
          value: token,
        });
      } else {
        pushText(token);
      }
    }
    lastIndex = tokenStart + token.length;
  }
  if (lastIndex < prompt.length) {
    pushText(prompt.slice(lastIndex));
  }
  return parts;
};
