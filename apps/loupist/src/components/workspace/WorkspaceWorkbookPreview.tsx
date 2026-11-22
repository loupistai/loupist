import { formatNumber } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import { useEffect, useState } from "react";
import type { LoupistBridge, WorkbookRead, WorkbookSheet } from "../../types";
import { errorMessage } from "../../lib/errors";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../ui/tabs";
import { FilePreviewTable } from "./FilePreviewTable";

const t = i18n.getFixedT(null, "chat");

function columnLetters(column: number) {
  let letters = "";
  for (let index = column; index > 0; index = Math.floor((index - 1) / 26)) {
    letters = String.fromCharCode(65 + (index - 1) % 26) + letters;
  }
  return letters;
}

function refusal(status: Exclude<WorkbookRead["status"], "ready">, format: string) {
  const messages = {
    unsupported_format: t("workspaceFiles.workbook.unsupportedFormat", { format }),
    encrypted: t("workspaceFiles.workbook.encrypted"),
    invalid_workbook: t("workspaceFiles.workbook.invalidWorkbook"),
    unknown_sheet: t("workspaceFiles.workbook.unknownSheet"),
    unsupported_sheet: t("workspaceFiles.workbook.unsupportedSheet"),
    limit_exceeded: t("workspaceFiles.workbook.limitExceeded"),
    worker_unavailable: t("workspaceFiles.workbook.unavailable"),
    timed_out: t("workspaceFiles.workbook.timedOut"),
  };
  return messages[status];
}

type LoadedSheet = {
  revision: number;
  requestedIndex: number | null;
  response?: WorkbookRead;
  error?: string;
  sheets: WorkbookSheet[];
  selectedIndex: number | null;
};

export function WorkspaceWorkbookPreview({ bridge, loupistId, path, format, revision }: {
  bridge: LoupistBridge; loupistId: string; path: string; format: string; revision: number;
}) {
  useTranslation("chat");
  const [requestedIndex, setRequestedIndex] = useState<number | null>(null);
  const [loaded, setLoaded] = useState<LoadedSheet>();
  useEffect(() => {
    let active = true;
    void bridge.workspaceWorkbookRead({ loupist_id: loupistId, path, sheet_index: requestedIndex }, () => undefined)
      .then(response => {
        if (active) setLoaded(previous => ({
          revision, requestedIndex, response,
          sheets: response.status === "ready" ? response.sheets : previous?.sheets ?? [],
          selectedIndex: response.status === "ready" ? response.selected_sheet : requestedIndex,
        }));
      })
      .catch(cause => {
        if (active) setLoaded(previous => ({
          revision, requestedIndex, sheets: previous?.sheets ?? [], selectedIndex: requestedIndex,
          error: errorMessage(cause, t("workspaceFiles.workbook.couldNotRead")),
        }));
      });
    return () => { active = false; };
  }, [bridge, loupistId, path, requestedIndex, revision]);

  const current = loaded?.revision === revision && loaded.requestedIndex === requestedIndex ? loaded : undefined;
  const projection = current?.response?.status === "ready" ? current.response : undefined;
  const error = current?.error ?? (current?.response && current.response.status !== "ready" ? refusal(current.response.status, format) : null);
  const sheets = loaded?.sheets ?? [];
  const selectedIndex = requestedIndex ?? loaded?.selectedIndex ?? null;
  const selected = selectedIndex === null ? undefined : sheets[selectedIndex];
  const value = selectedIndex === null ? "empty" : String(selectedIndex);
  const sheetTabs = sheets.map((sheet, index) => ({ ...sheet, index }));
  const orderedTabs = [...sheetTabs.filter(sheet => !sheet.hidden), ...sheetTabs.filter(sheet => sheet.hidden)];
  const columnCount = Math.max(0, ...(projection?.rows.map(row => row.length) ?? []));
  const columns = Array.from({ length: columnCount }, (_, offset) => columnLetters((projection?.first_column ?? 1) + offset));
  const rows = projection?.rows.map(row => row.map(cell => {
    const uncached = cell.formula && cell.display === "";
    return {
      text: uncached ? "—" : cell.display,
      numeric: cell.kind === "number",
      error: cell.kind === "error",
      formula: cell.formula ? (uncached ? "uncached" as const : "cached" as const) : undefined,
      tooltip: cell.formula ? t(uncached ? "workspaceFiles.workbook.uncachedFormula" : "workspaceFiles.workbook.cachedFormula") : undefined,
    };
  }));
  const body = !current ? <p className="workspace-file-state reader-loading" role="status">{t("workspaceFiles.workbook.reading")}</p>
    : error ? <div className="workspace-file-state reader-error" role="alert"><p>{error}</p></div>
    : rows?.length ? <FilePreviewTable label={t("workspaceFiles.dataPreviewLabel", { value1: selected?.name ?? path.split("/").at(-1) })}
      columns={columns} rows={rows} firstRow={projection?.first_row ?? 1} />
    : <p className="workspace-file-state">{t(selected ? "workspaceFiles.workbook.emptySheet" : "workspaceFiles.workbook.noVisibleSheets")}</p>;

  if (!sheets.length) return body;
  return <Tabs className="reader-workbook" value={value} onValueChange={tab => setRequestedIndex(Number(tab))} activationMode="manual">
    <TabsList className="reader-workbook-tabs" aria-label={t("workspaceFiles.workbook.sheets")}>
      {orderedTabs.map(sheet => <TabsTrigger key={sheet.index} value={String(sheet.index)} disabled={sheet.hidden || !sheet.worksheet}>
        {sheet.name}{sheet.hidden ? <> <small className="reader-workbook-hidden">{t("workspaceFiles.workbook.hidden")}</small></> : null}
      </TabsTrigger>)}
    </TabsList>
    <p className="reader-table-notice">{t("workspaceFiles.workbook.valuesOnly")}
      {projection?.truncated ? <> {t("workspaceFiles.workbook.truncated", { rows: formatNumber(projection.rows.length), total: formatNumber(projection.rows_total) })}</> : null}
    </p>
    {selectedIndex === null ? <div className="reader-workbook-sheet">{body}</div>
      : <TabsContent className="reader-workbook-sheet" value={value}>{body}</TabsContent>}
  </Tabs>;
}
