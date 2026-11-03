import { formatNumber } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import { useState } from "react";
import { CheckIcon, ClipboardIcon, ChevronLeftIcon, ChevronRightIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { ChartDataset } from "../../types";
import { datasetCsv, exportFileName, utf8Bytes } from "../../chat/exportChat";

const t = i18n.getFixedT(null, "chat");

const PAGE_SIZE = 25;
const cellText = (value: unknown) => value == null ? "" : String(value);
const quotedCell = (value: unknown) => {
  const text = cellText(value);
  return /[\t\n\r"]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text;
};

/** Tab-separated rows with a header line, ready to paste into a spreadsheet. */
export const datasetTsv = (dataset: ChartDataset, rows: readonly unknown[][] = dataset.rows) => [
  dataset.columns.map((column) => column.label), ...rows,
].map((row) => row.map(quotedCell).join("\t")).join("\n");

export function ArtifactData({ dataset, title, onExport }: {
  dataset: ChartDataset;
  title?: string;
  /** Saves through the native dialog (Rust `export_save`); absent where no bridge is available. */
  onExport?: (request: { suggestedName: string; bytes: number[]; kind: "csv" }) => Promise<string | null>;
}) {
  useTranslation("chat");
  const [page, setPage] = useState(0);
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const start = page * PAGE_SIZE;
  const rows = dataset.rows.slice(start, start + PAGE_SIZE);
  const receipts = dataset.lineage.kind === "from_result"
    ? [dataset.lineage.receipt] : dataset.lineage.kind === "agent_authored" ? dataset.lineage.upstream_receipts : [];
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(datasetTsv(dataset, rows));
      setCopied(true);
      setError(null);
    } catch {
      setError(t("artifactData.couldNotCopyTheseRowsTryAgain"));
    }
  };
  const changePage = (next: number) => { setPage(next); setCopied(false); setError(null); };
  const saveCsv = async () => {
    if (!onExport) return;
    try {
      await onExport({ suggestedName: `${exportFileName(title ?? "data")}.csv`, kind: "csv", bytes: utf8Bytes(datasetCsv(dataset.columns, dataset.rows)) });
      setError(null);
    } catch {
      setError(t("artifactData.couldNotSave"));
    }
  };
  return (
    <section className="artifact-data" aria-label={t("artifactData.chartData")}>
      <div className="artifact-data-summary">
        <strong>{t("artifactData.dimensions", { rows: t("artifactData.rows", { count: dataset.rows.length, formattedCount: formatNumber(dataset.rows.length) }), columns: t("artifactData.columns", { count: dataset.columns.length, formattedCount: formatNumber(dataset.columns.length) }) })}</strong>
        <p>{dataset.lineage.kind === "from_dataset" ? t("artifactData.datasetVersion", { revision: dataset.lineage.revision }) : dataset.lineage.kind === "from_result" ? t("artifactData.dataFromASavedResult") : t("artifactData.dataPreparedByLoupist")}
          {receipts.length ? ` · ${[...new Set(receipts.map((receipt) => receipt.provider || t("artifactData.savedResult")))].join(", ")}` : dataset.lineage.kind === "from_dataset" ? t("artifactData.savedInputVersion") : t("artifactData.noLinkedSource")}</p>
        {receipts.some((receipt) => receipt.warnings.length) ? (
          <details><summary>{t("artifactData.dataNotes")}</summary>{receipts.flatMap((receipt) => receipt.warnings).map((warning, index) => <p key={index}>{warning}</p>)}</details>
        ) : null}
      </div>
      <div className="artifact-data-scroll" tabIndex={0} role="region" aria-label={t("artifactData.dataRows")}>
        <table>
          <thead><tr><th scope="col">{t("artifactData.row")}</th>{dataset.columns.map((column) => <th scope="col" key={column.id}>{column.label}</th>)}</tr></thead>
          <tbody>{rows.map((row, index) => <tr key={start + index}>
            <th scope="row">{start + index + 1}</th>
            {dataset.columns.map((column, columnIndex) => <td key={column.id} data-kind={column.kind} title={row[columnIndex] == null ? undefined : cellText(row[columnIndex])}>{row[columnIndex] == null ? "—" : cellText(row[columnIndex])}</td>)}
          </tr>)}</tbody>
        </table>
      </div>
      {error ? <p role="alert">{error}</p> : null}
      <footer className="artifact-data-pagination">
        <Button size="sm" variant="ghost" onClick={() => void copy()}>{copied ? <CheckIcon /> : <ClipboardIcon />}{copied ? t("artifactData.copied") : t("artifactData.copyRows")}</Button>
        {onExport ? <Button size="sm" variant="ghost" onClick={() => void saveCsv()}>{t("artifactData.saveCsv")}</Button> : null}
        <span aria-live="polite">{t("artifactData.pageRange", { start: formatNumber(rows.length ? start + 1 : 0), end: formatNumber(start + rows.length), total: formatNumber(dataset.rows.length) }) }</span>
        <Button size="icon-sm" variant="ghost" aria-label={t("artifactData.previousDataRows")} disabled={page === 0} onClick={() => changePage(page - 1)}><ChevronLeftIcon /></Button>
        <Button size="icon-sm" variant="ghost" aria-label={t("artifactData.nextDataRows")} disabled={start + rows.length >= dataset.rows.length} onClick={() => changePage(page + 1)}><ChevronRightIcon /></Button>
      </footer>
    </section>
  );
}
