import { plainInline } from "./sharingText";

/**
 * Shared text is data: read-only Markdown without HTML or images, links as
 * text with their domain, one row per source line so findings can mark the
 * exact lines they matched.
 */
export function PackMarkdown({ markdown, highlight, maxLines }: { markdown: string; highlight?: ReadonlySet<number>; maxLines?: number }) {
  const lines = markdown.split("\n");
  const shown = maxLines ? lines.slice(0, maxLines) : lines;
  return <div className="pack-markdown">
    {shown.map((line, index) => {
      const heading = /^#{1,6}\s+/.exec(line);
      const text = plainInline(heading ? line.slice(heading[0].length) : line);
      return <p key={index} className={heading ? "pack-md-line is-heading" : !line.trim() ? "pack-md-line is-blank" : "pack-md-line"} data-flagged={highlight?.has(index + 1) || undefined}>{text}</p>;
    })}
  </div>;
}
