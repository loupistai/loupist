import { useState } from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "../../i18n";
import { formatNumber } from "../../i18n/format";
import { Button } from "../ui/button";
import { changeLine, type ChangeRow, type PassageTarget } from "./worldChanges";

const PAGE = 8;

export function ChangeGroup({ title, rows, onOpen, showAllLabel, heading = true }: {
  title: string;
  rows: readonly ChangeRow[];
  onOpen: (row: ChangeRow) => void;
  /** When set, the control says "Show all {n}" instead of "Show all". */
  showAllLabel?: (count: number) => string;
  heading?: boolean;
}) {
  const [open, setOpen] = useState(false);
  if (!rows.length) return null;
  const shown = open ? rows : rows.slice(0, PAGE);
  return <section aria-label={title}>
    {heading ? <h3>{title} <span>{formatNumber(rows.length)}</span></h3> : null}
    <ul>{shown.map(row => <ChangeRowView key={row.id} row={row} onOpen={onOpen} />)}</ul>
    {rows.length > PAGE && !open && showAllLabel ? <Button type="button" variant="link" onClick={() => setOpen(true)}>{showAllLabel(rows.length)}</Button> : null}
  </section>;
}

function ChangeRowView({ row, onOpen }: { row: ChangeRow; onOpen: (row: ChangeRow) => void }) {
  const { t, i18n } = useTranslation("assets");
  const knowledge = i18n.getFixedT(null, "knowledge") as TFunction<"knowledge">;
  const line = row.group === "lens"
    ? (row.sourceTitle && row.sourceTitle !== row.proposition ? t("sinceRead.section", { page: row.sourceTitle, section: row.proposition }) : row.proposition)
    : changeLine(row, knowledge, date => t("sinceRead.published", { date }), Date.now());
  const title = row.sourceTitle;
  const canOpen = !!(row.passage || row.lensRecordId);
  return <li className="change-feed-row">
    <p>{row.proposition}</p>
    <p>{line}</p>
    {title && canOpen ? <Button type="button" variant="link" onClick={() => onOpen(row)}>{title}</Button> : title ? <p>{title}</p> : null}
  </li>;
}

export type { PassageTarget };
