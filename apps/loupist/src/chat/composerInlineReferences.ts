import type { ComposerInlineReference } from "../contracts/workspace";
export type { ComposerInlineReference } from "../contracts/workspace";

export function validInlineReferences(prompt: string, references: readonly ComposerInlineReference[]) {
  let end = 0;
  return [...references].sort((a, b) => a.start - b.start).filter(reference => {
    const valid = reference.start >= end && reference.end > reference.start && reference.end <= prompt.length
      && prompt.slice(reference.start, reference.end) === reference.text;
    if (valid) end = reference.end;
    return valid;
  });
}

/** Rebase one ordinary textarea edit; editing inside a reference detaches its structured context. */
export function rebaseInlineReferences(previous: string, next: string, references: readonly ComposerInlineReference[]) {
  if (previous === next) return [...references];
  let start = 0;
  while (start < previous.length && start < next.length && previous[start] === next[start]) start++;
  let oldEnd = previous.length, newEnd = next.length;
  while (oldEnd > start && newEnd > start && previous[oldEnd - 1] === next[newEnd - 1]) { oldEnd--; newEnd--; }
  const delta = newEnd - oldEnd;
  return references.flatMap(reference => {
    if (reference.end <= start) return [reference];
    if (reference.start >= oldEnd) return [{ ...reference, start: reference.start + delta, end: reference.end + delta }];
    return [];
  });
}

/** Only an exact, still-owned editor range can substitute serialization text. */
export function serializeInlineReferences(prompt: string, references: readonly ComposerInlineReference[], textFor: (key: string) => string | undefined) {
  let result = "", cursor = 0;
  for (const reference of validInlineReferences(prompt, references)) {
    result += prompt.slice(cursor, reference.start) + (textFor(reference.key) ?? reference.text);
    cursor = reference.end;
  }
  return result + prompt.slice(cursor);
}
