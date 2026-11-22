import type { CitedPassage } from "../../contracts/knowledge";

export type TextRange = { start: number; end: number };
export type ReadableMark = TextRange & { focused?: true };
type ReadablePassage = TextRange & { marks: ReadableMark[] };

/** Translate only the requested byte boundaries in one pass over the original saved window. */
export function readableCitedPassages(text: string, passages: readonly CitedPassage[] | undefined, focusedMark?: TextRange): ReadablePassage[] {
  if (!passages?.length) return [];
  const offsets = new Map<number, number>();
  const requested = new Set(passages.flatMap(({ start, end, marks }) => [start, end, ...marks.flatMap(mark => [mark.start, mark.end])]));
  if (focusedMark) {
    requested.add(focusedMark.start);
    requested.add(focusedMark.end);
  }
  let bytes = 0;
  let units = 0;
  for (const character of text) {
    if (requested.delete(bytes)) offsets.set(bytes, units);
    if (!requested.size) break;
    const codePoint = character.codePointAt(0)!;
    bytes += codePoint < 0x80 ? 1 : codePoint < 0x800 ? 2 : codePoint < 0x10000 ? 3 : 4;
    units += character.length;
  }
  if (requested.has(bytes)) offsets.set(bytes, units);
  const convert = (range: TextRange) => {
    const start = offsets.get(range.start), end = offsets.get(range.end);
    return start !== undefined && end !== undefined && start < end ? { start, end } : null;
  };
  const result: ReadablePassage[] = [];
  for (const passage of passages) {
    const range = convert(passage);
    // A bad window must not hide the saved text. A bad mark leaves its text unmarked.
    if (!range || (result.length && range.start < result.at(-1)!.end)) return [];
    const insidePassage = (mark: TextRange) => {
      const match = convert(mark);
      return match && match.start >= range.start && match.end <= range.end
        ? { start: match.start - range.start, end: match.end - range.start } : null;
    };
    const focus = focusedMark && insidePassage(focusedMark);
    const marks = passage.marks.flatMap<ReadableMark>(mark => {
      const local = insidePassage(mark);
      if (!local) return [];
      if (!focus || local.end <= focus.start || local.start >= focus.end) return [local];
      // A broader mark may cover the same words. Leave its non-overlapping text marked.
      return [
        ...(local.start < focus.start ? [{ start: local.start, end: focus.start }] : []),
        ...(local.end > focus.end ? [{ start: focus.end, end: local.end }] : []),
      ];
    });
    if (focus) marks.push({ ...focus, focused: true });
    marks.sort((a, b) => a.start - b.start || b.end - a.end);
    result.push({ ...range, marks });
  }
  return result;
}
