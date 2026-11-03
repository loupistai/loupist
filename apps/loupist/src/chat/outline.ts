/** Answers this long, or with this many headings, get an outline (chat spec X16). */
export const OUTLINE_MIN_WORDS = 600;
export const OUTLINE_MIN_HEADINGS = 3;
export const OUTLINE_MAX_ROWS = 8;

export type OutlineHeading = { level: 2 | 3; text: string };

/** Markdown decoration removed, so the row reads like the rendered heading. */
const plain = (text: string) => text
  .replace(/!?\[([^\]]*)\]\([^)]*\)/gu, "$1")
  .replace(/[*_`~]+/gu, "")
  .replace(/\s+#+\s*$/u, "")
  .trim();

/** H2 and H3 headings outside code fences, in order. */
export function answerHeadings(markdown: string): OutlineHeading[] {
  const headings: OutlineHeading[] = [];
  let fenced = false;
  for (const line of markdown.split("\n")) {
    if (/^\s*(```|~~~)/u.test(line)) { fenced = !fenced; continue; }
    if (fenced) continue;
    const match = /^(#{2,3})\s+(.+)$/u.exec(line);
    if (match) {
      const text = plain(match[2]!);
      if (text) headings.push({ level: match[1]!.length as 2 | 3, text });
    }
  }
  return headings;
}

const wordCount = (markdown: string) => markdown.split(/\s+/u).filter(Boolean).length;

/** The outline rows for an answer, or none when it is short and flat. */
export function answerOutline(markdown: string): OutlineHeading[] {
  const headings = answerHeadings(markdown);
  if (!headings.length) return [];
  if (headings.length < OUTLINE_MIN_HEADINGS && wordCount(markdown) <= OUTLINE_MIN_WORDS) return [];
  return headings.slice(0, OUTLINE_MAX_ROWS);
}
