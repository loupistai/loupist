import { useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import type { SourceDataCell, SourceDataTable } from "../../contracts/knowledge";
import { Disclosure } from "../ui/disclosure";
import { namesRows, revealTableCell } from "./revealTableCell";

/** Group the integer digits while preserving the sign and every fractional digit. */
function decimal(value: string) {
  return value.replace(/^[+−-]?\d+(?=(?:\.\d*)?$)/u, digits => digits.replace(/\B(?=(?:\d{3})+$)/gu, ","));
}

export function SavedSourceData({ table, original, focus }: { table: SourceDataTable; original: string; focus: boolean }) {
  const { t } = useTranslation("knowledge");
  const [showOriginal, setShowOriginal] = useState(false);
  const mark = useRef<HTMLElement>(null);
  const focused = focus ? table.focusedCell : undefined;
  useLayoutEffect(() => {
    if (!mark.current) return;
    mark.current.scrollIntoView?.({ block: "nearest", inline: "nearest" });
    revealTableCell(mark.current);
  }, [focused?.row, focused?.column]);

  const cellText = (cell: SourceDataCell) => {
    switch (cell.kind) {
      case "number": return decimal(cell.value);
      case "boolean": return t(cell.value ? "sourceWorkspace.dataTrue" : "sourceWorkspace.dataFalse");
      case "text": return cell.value;
      case "empty": return "";
    }
  };

  const labelled = namesRows(table.rows.map(row => row[0]?.kind === "text" ? row[0].value : ""), table.columns.length);
  return <>
    <div className="saved-passage-table saved-source-data" tabIndex={0}>
      <table data-row-labels={labelled || undefined}>
        <caption>{t("sourceWorkspace.shownDataRows", { shown: formatNumber(table.rows.length), total: formatNumber(table.totalRows) })}</caption>
        <thead><tr>{table.columns.map((column, index) => <th scope="col" key={index}>{column}</th>)}</tr></thead>
        <tbody>{table.rows.map((row, rowIndex) => <tr key={table.rowOffset + rowIndex}>
          {row.map((cell, column) => {
            const value = cellText(cell);
            const shown = focused?.row === rowIndex && focused.column === column
              ? <mark ref={mark} className="evidence-match" data-cited-focus aria-current="true">{value}</mark>
              : value;
            return <td key={column}>{labelled && column === 0 ? <span className="saved-passage-label">{shown}</span> : shown}</td>;
          })}
        </tr>)}</tbody>
      </table>
    </div>
    <Disclosure title={t(showOriginal ? "sourceWorkspace.hideOriginalRecord" : "sourceWorkspace.showOriginalRecord")} open={showOriginal} onOpenChange={setShowOriginal}>
      {showOriginal ? <pre className="knowledge-passage">{original}</pre> : null}
    </Disclosure>
  </>;
}
