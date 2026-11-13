const tableRow = (line: string) => line.trimStart().startsWith("|");

/**
 * A table is held whole until a line that is not a table row follows it (a
 * blank line, or text), so it never arrives row by row and its columns never
 * re-flow. Returns the text before the held table, and whether one is held.
 */
function withoutOpenTable(text: string): { text: string; held: boolean } {
  const lines = text.split("\n");
  let last = lines.length - 1;
  // The last line is still being written; when it is empty, the line before it is the one that closed.
  if (!lines[last]!.trim()) {
    if (last === 0 || !lines[last - 1]!.trim()) return { text, held: false };
    last -= 1;
  }
  if (!tableRow(lines[last]!)) return { text, held: false };
  let first = last;
  while (first > 0 && tableRow(lines[first - 1]!)) first -= 1;
  return { text: lines.slice(0, first).join("\n"), held: true };
}

/** A heading is shown whole once its line ends, with one fade, never word by word. */
function withoutOpenHeading(text: string): string {
  const lineStart = text.lastIndexOf("\n") + 1;
  return /^\s{0,3}#{1,6}(?:\s|$)/u.test(text.slice(lineStart)) ? text.slice(0, lineStart) : text;
}

/** `[`, `[1]`, `[text](` and `[12.3](#evidence-…` without their close would render as text or as a link to nowhere. */
function withoutOpenLink(text: string): string {
  const lineStart = text.lastIndexOf("\n") + 1;
  const line = text.slice(lineStart);
  const open = line.search(/!?\[[^\]]*(?:\]\([^)]*|\])?$/u);
  return open < 0 ? text : text.slice(0, lineStart + open);
}

function withoutOpenMath(text: string): string {
  const runs = text.match(/(?<!\\)\$\$/gu)?.length ?? 0;
  return runs % 2 === 0 ? text : text.slice(0, text.lastIndexOf("$$"));
}

/** A marker run that just opened (`**`, `` ` ``, `#`) with nothing after it yet. */
const withoutOpeningMarkers = (text: string) => text.replace(/(?<=^|\s)[*_`#~]+$/u, "");

/** A list or quote marker alone on the last line renders an empty item or quote until its content starts. */
const withoutEmptyListItem = (text: string) => text.replace(/(?<=^|\n)[ \t]*(?:[-*+>]|\d+[.)])[ \t]*$/u, "");

const fenceLine = /^\s*(?:`{3,}|~{3,})/u;
const insideFence = (text: string) => text.split("\n").filter((line) => fenceLine.test(line)).length % 2 === 1;

/**
 * The longest prefix of `text` whose rendering no longer suffix can shrink,
 * and whether a table is held back behind it (`pending`, for its ruled
 * placeholder). Returns the input unchanged when not streaming. For a growing
 * input the rendered text is monotonic: never shorter than the previous
 * frame. Unclosed bold and code fences are left to Streamdown, which closes
 * the one and marks the other's block incomplete; lines inside a fence append.
 */
export function stableMarkdown(text: string, streaming: boolean): { text: string; pending: "table" | null } {
  if (!streaming) return { text, pending: null };
  const fenced = insideFence(text);
  const table = fenced ? { text, held: false } : withoutOpenTable(text);
  let kept = fenced ? text : withoutOpenHeading(table.text);
  for (const step of [withoutOpenLink, withoutOpenMath, withoutOpeningMarkers, withoutEmptyListItem]) kept = step(kept);
  kept = kept.replace(/[ \t]+$/u, "");
  return { text: kept, pending: table.held ? "table" : null };
}
