import { formatNumber } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ArrowLeftIcon, ChevronRightIcon, FileIcon, FileSpreadsheetIcon, FolderIcon, FolderOpenIcon, RefreshCwIcon, SearchIcon, XIcon } from "lucide-react";
import { Button } from "../ui/button";
import { SafeMessageResponse } from "../chat/SafeMessageResponse";
import { errorMessage } from "../../lib/errors";
import type { LoupistBridge, WorkspaceDirectory, WorkspaceFile } from "../../types";
import { delimitedPreview, PREVIEW_COLUMNS, PREVIEW_ROWS } from "./delimitedPreview";

const t = i18n.getFixedT(null, "chat");

type Props = {
  bridge: LoupistBridge; loupistId: string; rootPath?: string; path: string; filePath?: string;
  onNavigate: (path: string, filePath?: string) => void;
  onAsk: (file: WorkspaceFile) => void;
  onOpenLink: (url: string) => void;
};
const parentPath = (path: string) => path.split("/").slice(0, -1).join("/");
const bytes = (size: number) => size < 1024 ? t("workspaceFiles.bytes", { size: size }) : size < 1048576 ? t("workspaceFiles.kilobytes", { value1: formatNumber(size / 1024, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) }) : t("workspaceFiles.megabytes", { value1: formatNumber(size / 1048576, { minimumFractionDigits: 1, maximumFractionDigits: 1 }) });
const isDelimited = (file: WorkspaceFile) => /\.(csv|tsv)$/i.test(file.path) || /^(text\/(csv|tab-separated-values)|application\/csv)(;|$)/i.test(file.media_type);
const fileType = (file: WorkspaceFile) => file.path.split("/").at(-1)?.match(/\.([^.]+)$/)?.[1].toUpperCase() ?? t("workspaceFiles.file");
const rasterPreview = (file: WorkspaceFile) => !file.truncated && file.image_data_url && /^data:image\/(png|jpeg|gif|webp);base64,[A-Za-z0-9+/]+={0,2}$/.test(file.image_data_url) ? file.image_data_url : null;

type LoadedItem = { key: string; value: WorkspaceDirectory | WorkspaceFile };

export function WorkspaceFiles({ bridge, loupistId, rootPath, path, filePath, onNavigate, onAsk, onOpenLink }: Props) {
  useTranslation("chat");
  const selection = JSON.stringify([loupistId, filePath ? "file" : "directory", filePath ?? path]);
  const [loaded, setLoaded] = useState<LoadedItem | null>(null);
  const [failure, setFailure] = useState<{ key: string; message: string } | null>(null);
  const [loadingKey, setLoadingKey] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  const [sourceViews, setSourceViews] = useState<Record<string, boolean>>({});
  const [headerViews, setHeaderViews] = useState<Record<string, boolean>>({});
  const [filters, setFilters] = useState<Record<string, string>>({});
  const requestRef = useRef(0);
  const contentRef = useRef<HTMLDivElement>(null);
  const scrollPositions = useRef(new Map<string, number>());
  const returnEntries = useRef(new Map<string, string>());
  const value = loaded?.key === selection ? loaded.value : null;
  const directory = value && "entries" in value ? value : null;
  const file = value && "content" in value ? value : null;
  const error = failure?.key === selection ? failure.message : null;
  const loading = loadingKey === selection;
  const source = sourceViews[selection] ?? false;
  const filter = filters[selection] ?? "";

  useEffect(() => {
    const request = ++requestRef.current;
    setLoadingKey(selection); setFailure(null);
    const read = filePath ? bridge.workspaceRead({ loupist_id: loupistId, path: filePath }) : bridge.workspaceList({ loupist_id: loupistId, path });
    void read.then((next) => {
      if (request === requestRef.current) setLoaded({ key: selection, value: next });
    }).catch((cause) => {
      if (request === requestRef.current) setFailure({ key: selection, message: errorMessage(cause, t("workspaceFiles.couldNotReadThisWorkspaceItem")) });
    }).finally(() => { if (request === requestRef.current) setLoadingKey(null); });
    return () => { requestRef.current += 1; };
  }, [bridge, loupistId, path, filePath, revision, selection]);

  useLayoutEffect(() => {
    if (contentRef.current) contentRef.current.scrollTop = scrollPositions.current.get(selection) ?? 0;
    if (directory && contentRef.current) {
      const entry = [...contentRef.current.querySelectorAll<HTMLButtonElement>("[data-file-path]")].find((item) => item.dataset.filePath === returnEntries.current.get(selection));
      entry?.focus({ preventScroll: true });
    }
  }, [selection, value]);

  const reveal = async () => {
    const request = requestRef.current;
    try { await bridge.workspaceReveal({ loupist_id: loupistId, path: filePath ?? path }); }
    catch (cause) {
      if (request === requestRef.current) setFailure({ key: selection, message: errorMessage(cause, t("workspaceFiles.couldNotRevealThisItem")) });
    }
  };
  const rememberScroll = () => {
    scrollPositions.current.set(selection, contentRef.current?.scrollTop ?? 0);
    if (scrollPositions.current.size > 50) scrollPositions.current.delete(scrollPositions.current.keys().next().value!);
  };
  const markdown = file && /\.(md|markdown)$/i.test(file.path);
  const browserFile = file && !file.binary && /\.(html?|svg)$/i.test(file.path);
  const tabular = file && isDelimited(file);
  const entries = directory?.entries.filter((entry) => entry.name.toLowerCase().includes(filter.toLowerCase())) ?? [];
  const crumbs = (filePath ?? path).split("/").filter((part) => part && part !== ".");

  return <section className="workspace-files reader-files" aria-label={t("workspaceFiles.workspaceFiles")}>
    <div className="workspace-file-toolbar">
      <Button type="button" variant="ghost" size="icon-sm" aria-label={t("workspaceFiles.parentFolder")} disabled={!filePath && (!path || path === ".")} onClick={() => onNavigate(filePath ? parentPath(filePath) : parentPath(path))}><ArrowLeftIcon /></Button>
      <nav className="reader-breadcrumbs" aria-label={t("workspaceFiles.fileLocation")}>
        {crumbs.length ? <button type="button" onClick={() => onNavigate("")}>{t("workspaceFiles.files")}</button> : <span aria-current="page">{t("workspaceFiles.files")}</span>}
        {crumbs.map((part, index) => <span className="reader-breadcrumb-part" key={`${index}-${part}`}><ChevronRightIcon aria-hidden="true" />
          {index === crumbs.length - 1 ? <span aria-current="page" title={part}>{part}</span> : <button type="button" title={part} onClick={() => onNavigate(crumbs.slice(0, index + 1).join("/"))}>{part}</button>}
        </span>)}
      </nav>
      <Button type="button" variant="ghost" size="icon-sm" aria-label={t("workspaceFiles.refreshFiles")} disabled={loading} onClick={() => setRevision((current) => current + 1)}><RefreshCwIcon className={loading ? "reader-refreshing" : undefined} /></Button>
      <Button type="button" variant="ghost" size="icon-sm" aria-label={t("workspaceFiles.showInFileManager")} onClick={() => void reveal()}><FolderOpenIcon /></Button>
    </div>
    {rootPath ? <p className="workspace-file-root" title={rootPath}><span>{t("workspaceFiles.localWorkspace")}</span><span>{rootPath}</span></p> : null}
    {loading ? <p className={`workspace-file-state reader-loading${value ? " is-refresh" : ""}`} role="status">{value ? t("workspaceFiles.refreshing") : t("workspaceFiles.opening")}</p> : null}
    {error ? <div className="workspace-file-state reader-error" role="alert"><p>{error}</p>{value ? <p>{t("workspaceFiles.theLastLoadedPreviewIsStillAvailableBelow")}</p> : null}<Button type="button" variant="outline" size="sm" onClick={() => setRevision((current) => current + 1)}>{t("workspaceFiles.tryAgain")}</Button></div> : null}
    {directory ? <>
      <div className="reader-file-filter"><SearchIcon aria-hidden="true" /><input aria-label={t("workspaceFiles.filterFiles")} placeholder={t("workspaceFiles.findAFileInThisFolder")} value={filter} onChange={(event) => setFilters((current) => ({ ...current, [selection]: event.target.value }))} />{filter && <Button type="button" variant="ghost" size="icon-sm" aria-label={t("workspaceFiles.clearFileFilter")} onClick={() => setFilters((current) => ({ ...current, [selection]: "" }))}><XIcon /></Button>}<span>{t("workspaceFiles.items", { count: entries.length, formattedCount: formatNumber(entries.length) })}</span></div>
      <div className="workspace-file-list" ref={contentRef} onScroll={rememberScroll}>
        {entries.length ? entries.map((entry) => <button type="button" key={entry.path} data-file-path={entry.path} className="workspace-file-entry" onClick={() => { returnEntries.current.set(selection, entry.path); if (returnEntries.current.size > 50) returnEntries.current.delete(returnEntries.current.keys().next().value!); entry.kind === "directory" ? onNavigate(entry.path) : onNavigate(path, entry.path); }}>
          {entry.kind === "directory" ? <FolderIcon /> : /\.(csv|tsv|xlsx)$/i.test(entry.name) ? <FileSpreadsheetIcon /> : <FileIcon />}<span>{entry.name}</span><small>{entry.size_bytes == null ? "" : bytes(entry.size_bytes)}</small>{entry.kind === "directory" ? <ChevronRightIcon className="reader-directory-arrow" /> : null}
        </button>) : <div className="workspace-file-state reader-empty"><p>{filter ? t("workspaceFiles.noFilesMatchThisSearch") : t("workspaceFiles.thisFolderIsEmpty")}</p>{filter ? <Button type="button" variant="ghost" size="sm" onClick={() => setFilters((current) => ({ ...current, [selection]: "" }))}>{t("workspaceFiles.clearSearch")}</Button> : <p>{t("workspaceFiles.researchFilesSavedInThisWorkspaceWillAppearHere")}</p>}</div>}
        {directory.truncated ? <p className="workspace-file-state">{t("workspaceFiles.showingALimitedListOpenASubfolderToSee")}</p> : null}
      </div>
    </> : null}
    {file ? <>
      <div className="workspace-file-actions"><span>{fileType(file)} · {bytes(file.size_bytes)}{file.truncated ? t("workspaceFiles.previewTruncated") : ""}</span>
        {(markdown || tabular) && !file.binary ? <div className="reader-view-switch" role="group" aria-label={t("workspaceFiles.fileView")}>
          <button type="button" aria-pressed={!source} onClick={() => setSourceViews((current) => ({ ...current, [selection]: false }))}>{tabular ? t("workspaceFiles.table") : t("workspaceFiles.preview")}</button>
          <button type="button" aria-pressed={source} onClick={() => setSourceViews((current) => ({ ...current, [selection]: true }))}>{t("workspaceFiles.source")}</button>
        </div> : null}
        <Button type="button" variant="outline" size="sm" disabled={loading} onClick={() => onAsk(file)}>{t("workspaceFiles.askAboutThis")}</Button>
      </div>
      {browserFile ? <div className="reader-browser-file-note" role="note">
        <div><strong>{t("workspaceFiles.sourceType", { type: fileType(file) })}</strong><p>{fileType(file) === "SVG" ? t("workspaceFiles.viewImage") : t("workspaceFiles.viewPage")}</p></div>
        <Button type="button" variant="outline" size="sm" onClick={() => void reveal()}>{t("workspaceFiles.showFile")}</Button>
      </div> : null}
      <div className={`workspace-file-content${tabular && !source ? " reader-table-content" : ""}`} ref={contentRef} onScroll={rememberScroll}>
        {rasterPreview(file) ? <WorkspaceImage key={`${selection}:${file.content_sha256}`} file={file} src={rasterPreview(file)!} onReveal={() => void reveal()} />
          : file.binary || file.content === null ? <div className="reader-empty"><p>{t("workspaceFiles.noTextPreviewForThisFileUseShowIn")}</p><Button type="button" variant="outline" size="sm" onClick={() => void reveal()}>{t("workspaceFiles.openInFileManager")}</Button></div>
          : tabular && !source ? <DelimitedFilePreview file={file} header={headerViews[selection] ?? !/header=absent/i.test(file.media_type)} onHeaderChange={(header) => setHeaderViews((current) => ({ ...current, [selection]: header }))} />
          : markdown && !source ? <SafeMessageResponse text={file.content} isAnimating={false} onOpenLink={onOpenLink} workspacePath={rootPath} workspaceBasePath={parentPath(file.path)} onOpenFile={(next) => onNavigate(parentPath(next), next)} />
          : <pre className="reader-source" data-quote-source={`file:${file.path}`}><code>{file.content}</code></pre>}
      </div>
    </> : null}
  </section>;
}

function WorkspaceImage({ file, src, onReveal }: { file: WorkspaceFile; src: string; onReveal: () => void }) {
  useTranslation("chat");
  const [actualSize, setActualSize] = useState(false);
  const [failed, setFailed] = useState(false);
  if (failed) return <div className="reader-empty" role="status"><p>{t("workspaceFiles.couldNotDisplayThisImage")}</p><Button type="button" variant="outline" size="sm" onClick={onReveal}>{t("workspaceFiles.openInFileManager")}</Button></div>;
  return <div className="reader-image-preview">
    <div className="reader-image-controls"><Button type="button" variant="outline" size="sm" aria-pressed={actualSize} onClick={() => setActualSize((current) => !current)}>{actualSize ? t("workspaceFiles.fitToPanel") : t("workspaceFiles.actualSize")}</Button></div>
    <div className={`reader-image-canvas${actualSize ? " is-actual-size" : ""}`}><img src={src} alt={file.path.split("/").at(-1) ?? t("workspaceFiles.workspaceImage")} onError={() => setFailed(true)} /></div>
  </div>;
}

function DelimitedFilePreview({ file, header, onHeaderChange }: { file: WorkspaceFile; header: boolean; onHeaderChange: (header: boolean) => void }) {
  useTranslation("chat");
  const delimiter = /\.tsv$/i.test(file.path) || file.media_type.startsWith("text/tab-separated-values") ? "\t" : ",";
  const data = useMemo(() => delimitedPreview(file.content ?? "", delimiter, file.truncated), [file.content, delimiter, file.truncated]);
  if (data.error) return <div className="reader-empty" role="status"><p>{t("workspaceFiles.couldNotCreateATablePreview")}</p><p>{data.error}</p></div>;
  if (!data.rowCount) return <p className="workspace-file-state">{data.incompleteLastRow ? t("workspaceFiles.theLoadedTextEndsBeforeTheFirstCompleteRecord") : t("workspaceFiles.thisFileHasNoDataRows")}</p>;
  const rowCount = Math.max(0, data.rowCount - Number(header));
  const columnCount = Math.min(data.columnCount, PREVIEW_COLUMNS);
  const rows = data.rows.slice(header ? 1 : 0, (header ? 1 : 0) + PREVIEW_ROWS);
  const columns = Array.from({ length: columnCount }, (_, index) => header ? data.rows[0]?.[index] || t("workspaceFiles.columnLabel", { value1: index + 1 }) : t("workspaceFiles.columnLabel", { value1: index + 1 }));
  return <div className="reader-csv-preview">
    <div className="reader-table-summary"><span>{t(file.truncated ? "workspaceFiles.truncatedTableSummary" : "workspaceFiles.tableSummary", { rows: t("workspaceFiles.rows", { count: rowCount, formattedCount: formatNumber(rowCount) }), columns: t("workspaceFiles.columns", { count: data.columnCount, formattedCount: formatNumber(data.columnCount) }) })}</span><label><input type="checkbox" checked={header} onChange={(event) => onHeaderChange(event.target.checked)} />{t("workspaceFiles.firstRowIsHeader")}</label></div>
    {(rowCount > PREVIEW_ROWS || data.columnCount > PREVIEW_COLUMNS || data.incompleteLastRow) ? <p className="reader-table-notice">{rowCount > PREVIEW_ROWS || data.columnCount > PREVIEW_COLUMNS ? t("workspaceFiles.previewLimit", { value1: PREVIEW_ROWS, value2: PREVIEW_COLUMNS }) : ""}{data.incompleteLastRow ? t("workspaceFiles.theUnfinishedFinalRecordIsOmittedFromThisPreview") : ""}{t("workspaceFiles.openSourceToInspectTheLoadedText")}</p> : null}
    <div className="reader-data-scroll"><table className="reader-data-table" aria-label={t("workspaceFiles.dataPreviewLabel", { value1: file.path.split("/").at(-1) })}><thead><tr><th scope="col" className="reader-row-number">#</th>{columns.map((column, index) => <th scope="col" key={index} title={column}>{column}</th>)}</tr></thead>
      <tbody>{rows.map((row, index) => <tr key={index}><th scope="row" className="reader-row-number">{index + 1}</th>{columns.map((_, column) => <td key={column} title={row[column] ?? ""} className={/^-?\d[\d.,eE+%-]*$/.test(row[column] ?? "") ? "is-numeric" : undefined}>{row[column] ?? ""}</td>)}</tr>)}</tbody>
    </table></div>
    {!rows.length ? <p className="workspace-file-state">{t("workspaceFiles.onlyColumnNamesArePresentThisFileHasNo")}</p> : null}
  </div>;
}
