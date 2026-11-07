const tableRow = (line: string) => line.trimStart().startsWith("|");
const separatorRow = /^\s*\|?\s*:?-+:?\s*(?:\|\s*:?-+:?\s*)*\|?\s*$/u;

/**
 * A trailing table block renders as raw pipes until its separator row exists,
 * and a row's cells pop in one by one until its newline arrives. Hold back the
 * whole block in the first case and the unterminated row in the second.
 */
function withoutIncompleteTable(text: string): string {
  const lines = text.split("\n");
  let last = lines.length - 1;
  while (last >= 0 && !lines[last]!.trim()) last -= 1;
  if (last < 0 || !tableRow(lines[last]!)) return text;
  let first = last;
  while (first > 0 && tableRow(lines[first - 1]!)) first -= 1;
  const block = lines.slice(first, last + 1);
  const terminated = last < lines.length - 1;
  const complete = block.length >= 2 && separatorRow.test(block[1]!) && (terminated || block.length >= 3);
  if (!complete) return lines.slice(0, first).join("\n");
  return terminated ? text : `${lines.slice(0, last).join("\n")}\n`;
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

/** A list or quote marker alone on the last line renders as text until its content starts. */
const withoutEmptyListItem = (text: string) => text.replace(/(?<=^|\n)[ \t]*(?:[-*+>]|\d+[.)])[ \t]*$/u, "");

const fenceLine = /^\s*(?:`{3,}|~{3,})/u;

function caretAllowed(text: string): boolean {
  const lines = text.split("\n");
  if (lines.filter((line) => fenceLine.test(line)).length % 2 === 1) return false;
  const lastLine = [...lines].reverse().find((line) => line.trim())?.trimStart() ?? "";
  return Boolean(lastLine) && !tableRow(lastLine) && !lastLine.startsWith("#");
}

/**
 * The longest prefix of `text` whose rendering no longer suffix can shrink,
 * and whether a caret may follow it. Returns the input unchanged when not
 * streaming. For a growing input the rendered text is monotonic: never
 * shorter than the previous frame. Unclosed bold and code fences are left to
 * Streamdown, which closes the one and draws a skeleton for the other.
 */
export function stableMarkdown(text: string, streaming: boolean): { text: string; caret: boolean } {
  if (!streaming) return { text, caret: false };
  let kept = text;
  for (const step of [withoutIncompleteTable, withoutOpenLink, withoutOpenMath, withoutOpeningMarkers, withoutEmptyListItem]) kept = step(kept);
  kept = kept.replace(/[ \t]+$/u, "");
  return { text: kept, caret: caretAllowed(kept) };
}
