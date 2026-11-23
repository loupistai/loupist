import { Fragment, type ReactNode, type RefObject } from "react";

import type { ReadableMark } from "./citedPassages";
import { namesRows } from "./revealTableCell";

/** Only the pipe-row format emitted by filing capture is interpreted. All other source text stays literal. */
export function SavedPassage({ text, preformatted = false, marks = [], markRef, singleTableRow = false }: {
  text: string;
  preformatted?: boolean;
  marks?: readonly ReadableMark[];
  singleTableRow?: boolean;
  markRef?: RefObject<HTMLElement | null>;
}) {
  let marked = false;
  const content = (value: string, offset: number) => {
    const parts: ReactNode[] = [];
    let cursor = 0;
    for (const { start, end, focused } of marks) {
      if (start >= offset + value.length) break;
      const from = Math.max(cursor, start - offset);
      const to = Math.min(value.length, end - offset);
      if (from >= to) continue;
      const ref = marked ? undefined : markRef;
      marked = true;
      parts.push(value.slice(cursor, from), <mark key={from} ref={ref} className="evidence-match" data-cited-focus={focused} aria-current={focused}>{value.slice(from, to)}</mark>);
      cursor = to;
    }
    return parts.length ? [...parts, value.slice(cursor)] : value;
  };
  const Plain = preformatted ? "pre" : Fragment;
  const blocks: ReactNode[] = [];
  let cursor = 0;
  // An isolated row stays literal unless a cited projection starts with that row.
  const table = singleTableRow && text.startsWith("|") ? /^\|[^\n]*\|(?=\n|$)(?:\n\|[^\n]*\|(?=\n|$))*/gm
    : /^\|[^\n]*\|(?=\n)(?:\n\|[^\n]*\|(?=\n|$))+/gm;
  for (const block of text.matchAll(table)) {
    const begin = block.index;
    if (cursor < begin) blocks.push(<Plain key={cursor}>{content(text.slice(cursor, begin), cursor)}</Plain>);
    const rows = block[0].split("\n");
    const header = /^\|(?:\s*:?-{3,}:?\s*\|)+$/.test(rows[1] ?? "");
    const cellsOf = (row: string) => [...row.matchAll(/\|((?:\\\||[^|])*)(?=\|)/g)];
    // The Source reader keeps a first column that names the rows in view while the figures scroll.
    const labelled = namesRows((header ? rows.slice(2) : rows).map(row => cellsOf(row)[0]?.[1] ?? ""), cellsOf(rows[0] ?? "").length);
    let offset = begin;
    const rendered = rows.flatMap((row, index) => {
      if (header && index === 1) { offset += row.length + 1; return []; }
      const rowStart = offset;
      offset += row.length + 1;
      const Cell = header && index === 0 ? "th" : "td";
      const cells = cellsOf(row);
      return [<tr key={rowStart}>{cells.map((cell, column) => {
        // Hidden syntax preserves Range.toString() offsets into the exact saved passage for quotes.
        const tail = column < cells.length - 1 ? "" : "|" + (index < rows.length - 1 ? "\n" : "")
          + (header && index === 0 ? rows[1] + (rows.length > 2 ? "\n" : "") : "");
        const value = cell[1]!;
        const cellStart = rowStart + cell.index + 1;
        const shown = value.split(/(\\\|)/g).map((part, i, parts) =>
          part === "\\|" ? <Fragment key={i}><span hidden>\</span>|</Fragment>
            : <Fragment key={i}>{content(part, cellStart + parts.slice(0, i).join("").length)}</Fragment>);
        return <Cell key={column} scope={Cell === "th" ? "col" : undefined}><span hidden>|</span>{labelled && column === 0 ? <span className="saved-passage-label">{shown}</span> : shown}{tail && <span hidden>{tail}</span>}</Cell>;
      })}</tr>];
    });
    blocks.push(<div key={`table-${begin}`} className="saved-passage-table"><table role={header ? undefined : "presentation"} data-aligned={!header || undefined} data-row-labels={labelled || undefined}>
      {header && <thead>{rendered[0]}</thead>}<tbody>{header ? rendered.slice(1) : rendered}</tbody>
    </table></div>);
    cursor = begin + block[0].length;
  }
  if (cursor < text.length || !blocks.length) blocks.push(<Plain key={cursor}>{content(text.slice(cursor), cursor)}</Plain>);
  return <>{blocks}</>;
}
