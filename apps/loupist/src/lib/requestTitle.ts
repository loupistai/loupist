/**
 * The title the host derives from a request (`fallback_chat_title`): its first line with the
 * spacing collapsed, cut to 40 characters with an ellipsis, at a word where it can and never
 * inside a grapheme. A session whose first reply stops or fails is titled this way.
 */
export function requestTitle(prompt: string): string {
  const title = prompt.split("\n", 1)[0]!.trim().replace(/\s+/gu, " ");
  const characters = Array.from(title);
  if (characters.length <= 40) return title;

  // Rust reserves the 40th code point for the ellipsis and keeps whole graphemes.
  const budget = characters.slice(0, 39).join("").length;
  let wordEnd: number | undefined;
  let graphemeEnd = 0;
  for (const { index, segment } of new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(title)) {
    if (index > budget) break;
    graphemeEnd = index;
    if (segment.startsWith(" ")) wordEnd = index;
  }
  return title.slice(0, wordEnd ?? graphemeEnd) + "…";
}
