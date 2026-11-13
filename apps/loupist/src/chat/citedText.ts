/** A piece of a model-written caption or note: plain text, or a citation of one evidence id. */
export type CitedPart = { text: string } | { evidenceId: string; label?: string };

// `[label](#evidence-id)` as in the answer, or a bare `#evidence-id` the model left as text.
const CITATION = /\[([^\]\n]+)\]\(#evidence-([A-Za-z0-9_]+)\)|#evidence-([A-Za-z0-9_]+)/gu;

/**
 * Splits a chart caption or a table note into text and citations, so a link
 * target the model wrote (`#evidence-ev_…`) is drawn as a citation, never as
 * its raw id.
 */
export function citedParts(text: string): CitedPart[] {
  const parts: CitedPart[] = [];
  let last = 0;
  for (const match of text.matchAll(CITATION)) {
    if (match.index > last) parts.push({ text: text.slice(last, match.index) });
    parts.push(match[2] ? { evidenceId: match[2], label: match[1] } : { evidenceId: match[3]! });
    last = match.index + match[0].length;
  }
  if (last < text.length) parts.push({ text: text.slice(last) });
  return parts;
}

/**
 * The same note in a file: a citation keeps its link text, and a bare target
 * becomes its source's name (`name`), so an export never carries a raw id.
 */
export function citedPlainText(text: string, name: (evidenceId: string) => string): string {
  return citedParts(text).map((part) => "text" in part ? part.text : part.label ?? name(part.evidenceId)).join("");
}
