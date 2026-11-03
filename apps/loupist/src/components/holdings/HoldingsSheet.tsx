import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from "react";
import { MoreHorizontalIcon, PlusIcon, TriangleAlertIcon } from "lucide-react";
import { useTranslation } from "@/i18n";
import { formatDate, formatDateTime, formatNumber } from "@/i18n/format";
import type { LoupistBridge } from "@/types";
import type { CryptoFilePreview, HoldingsCsvPreview, HoldingsSummary, WalletView } from "@/contracts/userContext";
import { errorMessage } from "@/lib/errors";
import { Button } from "../ui/button";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "../ui/dropdown-menu";
import { Input } from "../ui/input";
import { NativeSelect } from "../ui/native-select";
import { Spinner } from "../ui/spinner";
import { detectNetwork, looksLikeSecret } from "./walletAddress";
import "@/styles/holdings.css";

const MAX_WALLETS = 10;
/** " · " that never starts a line. */
const SEPARATOR = "\u00a0· ";
const day = (ms: number) => formatDate(ms, { year: undefined, month: "short", day: "numeric" });
const moment = (ms: number) => formatDateTime(ms, { year: undefined, month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
const code = (cause: unknown) => cause && typeof cause === "object" && "code" in cause && typeof cause.code === "string" ? cause.code : null;

type Body =
  | { kind: "sources" }
  | { kind: "stocks"; preview: HoldingsCsvPreview }
  | { kind: "crypto"; preview: CryptoFilePreview; file: string };

/** The file's own name, for the steps that talk about it. */
const fileName = (path: string) => path.split(/[\\/]/u).pop() || path;

function useOnline() {
  const [online, setOnline] = useState(() => typeof navigator === "undefined" || navigator.onLine !== false);
  useEffect(() => {
    const update = () => setOnline(navigator.onLine !== false);
    window.addEventListener("online", update); window.addEventListener("offline", update);
    return () => { window.removeEventListener("online", update); window.removeEventListener("offline", update); };
  }, []);
  return online;
}

/**
 * "Your holdings" (finance-crypto-and-templates.md §2.1): stock and exchange
 * files, Korea Investment and read-only wallet addresses. Rust parses files,
 * validates addresses and reads balances; this sheet shows counts, dates and
 * masked addresses, and quantities only in a file preview before import.
 */
export function HoldingsSheet({ bridge, open, onOpenChange, onImported }: {
  bridge: LoupistBridge; open: boolean; onOpenChange: (open: boolean) => void;
  /** Symbols a file import committed, for callers that also follow them. */
  onImported?: (assets: Array<{ symbol: string; market: string; displayName?: string | null }>) => void;
}) {
  const { t } = useTranslation("knowledge");
  const [summary, setSummary] = useState<HoldingsSummary | null>(null);
  const [wallets, setWallets] = useState<WalletView[]>([]);
  const [kisReady, setKisReady] = useState(false);
  const [body, setBody] = useState<Body>({ kind: "sources" });
  const [busy, setBusy] = useState(false);
  const [reading, setReading] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  /** "Add a wallet address" opened from a refused file, not only from its own row. */
  const [adding, setAdding] = useState(false);
  const online = useOnline();
  const bodyRef = useRef<HTMLDivElement>(null);
  // A step that replaces the sheet's body starts on its first action, never on the whole dialog.
  useEffect(() => {
    if (body.kind === "sources") return;
    bodyRef.current?.querySelector<HTMLElement>(".holdings-preview select, .holdings-preview button:not(:disabled)")?.focus();
  }, [body]);

  const reload = useCallback(async () => {
    const [nextSummary, nextWallets] = await Promise.all([bridge.holdingsSummary().catch(() => null), bridge.walletsList().catch(() => [])]);
    setSummary(nextSummary); setWallets(nextWallets);
  }, [bridge]);
  useEffect(() => {
    if (!open) return;
    let active = true;
    setBody({ kind: "sources" }); setError(null); setAdding(false);
    void reload();
    void bridge.marketplaceSnapshot().then(snapshot => {
      if (active) setKisReady(snapshot.connectors.some(connector => connector.entry_id === "koreainvestment.market-data" && connector.readiness === "ready"));
    }).catch(() => {});
    return () => { active = false; };
  }, [open, bridge, reload]);

  const run = async (action: () => Promise<void>) => {
    setBusy(true); setError(null);
    try { await action(); } catch (cause) { setError(errorMessage(cause, t("holdings.error"))); }
    finally { setBusy(false); }
  };
  const pick = () => bridge.pickFile({ title: t("holdings.importFile"), filters: [{ name: "CSV", extensions: ["csv"] }] });
  const importStocks = () => run(async () => { const path = await pick(); if (path) setBody({ kind: "stocks", preview: await bridge.holdingsImportCsv({ path }) }); });
  const importCrypto = () => run(async () => { const path = await pick(); if (path) setBody({ kind: "crypto", preview: await bridge.holdingsImportCryptoFile({ path }), file: fileName(path) }); });
  const importKis = () => run(async () => {
    const snapshot = await bridge.holdingsImportKis();
    onImported?.(snapshot.positions.map(position => ({ symbol: position.symbol, market: position.market, displayName: position.displayName })));
    await reload();
  });
  const commit = (previewId: string) => run(async () => {
    const snapshot = await bridge.holdingsCommitCsv({ previewId });
    onImported?.(snapshot.positions.map(position => ({ symbol: position.symbol, market: position.market, displayName: position.displayName })));
    setBody({ kind: "sources" });
    await reload();
  });
  const refresh = async (wallet?: WalletView) => {
    const ids = wallet ? [wallet.id] : wallets.map(item => item.id);
    setReading(current => new Set([...current, ...ids])); setError(null);
    try { setWallets(await bridge.walletRefresh(wallet ? { walletId: wallet.id } : {})); setSummary(await bridge.holdingsSummary()); }
    catch (cause) { setError(errorMessage(cause, t("holdings.error"))); }
    finally { setReading(current => new Set([...current].filter(id => !ids.includes(id)))); }
  };

  const source = (name: "csv" | "kis" | "crypto_file") => summary?.sources.find(item => item.source === name);
  const status = (name: "csv" | "kis" | "crypto_file") => {
    const row = source(name);
    if (name === "kis" && !kisReady && !row) return t("holdings.notConnected");
    return row ? t("holdings.imported", { date: day(row.importedAtMs), count: row.count, formattedCount: formatNumber(row.count) }) : t("holdings.notImported");
  };

  return <Dialog open={open} onOpenChange={next => { if (!next && body.kind !== "sources") { setBody({ kind: "sources" }); return; } onOpenChange(next); }}>
    <DialogContent size="md" className="holdings-sheet" data-holdings-sheet>
      <DialogHeader>
        <DialogTitle>{t("holdings.title")}</DialogTitle>
        <DialogDescription className="holdings-lead">{t("holdings.lead")}</DialogDescription>
      </DialogHeader>
      <DialogBody className="holdings-body" ref={bodyRef}>
        {body.kind === "stocks" ? <StockPreview preview={body.preview} busy={busy} onCancel={() => setBody({ kind: "sources" })} onImport={() => void commit(body.preview.previewId)} />
          : body.kind === "crypto" ? <CryptoPreview bridge={bridge} preview={body.preview} file={body.file} busy={busy} onPreview={preview => setBody({ kind: "crypto", preview, file: body.file })} onError={setError}
            onCancel={() => setBody({ kind: "sources" })} onImport={() => void commit(body.preview.previewId)}
            onAddWallet={() => { setBody({ kind: "sources" }); setAdding(true); }} onChooseAnother={() => void importCrypto()} />
            : <>
              <section className="holdings-section" aria-labelledby="holdings-stocks">
                <h3 id="holdings-stocks" className="holdings-section-label">{t("holdings.stocks")}</h3>
                <SourceRow name={t("holdings.file")} status={status("csv")} action={<Button variant="outline" size="sm" disabled={busy} onClick={() => void importStocks()}>{t("holdings.importFile")}</Button>} />
                <SourceRow name={t("holdings.kis")} status={status("kis")} helper={kisReady ? undefined : t("holdings.kisHelper")}
                  action={<Button variant="outline" size="sm" disabled={busy || !kisReady} onClick={() => void importKis()}>{t("holdings.import")}</Button>} />
              </section>
              <section className="holdings-section" aria-labelledby="holdings-crypto">
                <h3 id="holdings-crypto" className="holdings-section-label">{t("holdings.crypto")}</h3>
                <SourceRow name={t("holdings.exchangeFile")} status={status("crypto_file")} action={<Button variant="outline" size="sm" disabled={busy} data-holdings-crypto-import onClick={() => void importCrypto()}>{t("holdings.importFile")}</Button>} />
                <h4 className="holdings-subhead">{t("holdings.wallets")}</h4>
                <ul className="holdings-wallets">
                  {wallets.map(wallet => <WalletRow key={wallet.id} wallet={wallet} reading={reading.has(wallet.id)} online={online} bridge={bridge}
                    onRefresh={() => void refresh(wallet)} onChanged={() => void reload()} onError={setError} />)}
                </ul>
                <AddWallet bridge={bridge} count={wallets.length} online={online} open={adding} onOpenChange={setAdding} onAdded={next => { setWallets(next); void bridge.holdingsSummary().then(setSummary).catch(() => {}); }} />
              </section>
              <p className="holdings-note">{t("holdings.walletNote")}</p>
            </>}
        {error ? <p className="holdings-error" role="alert">{error}</p> : null}
      </DialogBody>
      {body.kind === "sources" ? <DialogFooter><Button onClick={() => onOpenChange(false)}>{t("holdings.done")}</Button></DialogFooter> : null}
    </DialogContent>
  </Dialog>;
}

function SourceRow({ name, status, helper, action }: { name: string; status: string; helper?: string; action: ReactNode }) {
  return <div className="holdings-source">
    <div className="holdings-source-text"><span className="holdings-source-name">{name}</span><span className="holdings-meta">{status}</span>
      {helper ? <span className="holdings-helper">{helper}</span> : null}</div>
    {action}
  </div>;
}

function WalletRow({ wallet, reading, online, bridge, onRefresh, onChanged, onError }: {
  wallet: WalletView; reading: boolean; online: boolean; bridge: LoupistBridge;
  onRefresh: () => void; onChanged: () => void; onError: (message: string | null) => void;
}) {
  const { t } = useTranslation("knowledge");
  const [mode, setMode] = useState<"view" | "rename" | "remove">("view");
  const [name, setName] = useState(wallet.name ?? "");
  const label = t("holdings.walletLabel", { network: t(`holdings.networks.${wallet.family}`), ending: wallet.address.slice(-4) });
  const failed = wallet.status === "failed";
  const act = async (action: () => Promise<unknown>) => {
    onError(null);
    try { await action(); setMode("view"); onChanged(); } catch (cause) { onError(errorMessage(cause, t("holdings.error"))); }
  };
  return <li className="holdings-wallet" data-wallet-status={reading ? "reading" : wallet.status}>
    <div className="holdings-wallet-text">
      <span className="holdings-address" aria-label={label}>{wallet.masked}</span>
      {/* Parts are joined with a no-break space before each "·", so a wrapped line never starts with one. */}
      <span className="holdings-meta holdings-wallet-meta">
        {[t(`holdings.networks.${wallet.family}`), wallet.name].filter(Boolean).join(SEPARATOR)}
        {reading ? <>{SEPARATOR}<span className="holdings-reading"><Spinner />{t("holdings.reading")}</span></>
          : failed ? <>{SEPARATOR}<span className="holdings-failed"><TriangleAlertIcon aria-hidden="true" />{wallet.lastGoodReadAtMs ? t("holdings.readFailed", { date: moment(wallet.lastGoodReadAtMs) }) : t("holdings.readFailedNever")}</span></>
            : wallet.lastReadAtMs ? <>{SEPARATOR}<span>{t("holdings.readAt", { date: moment(wallet.lastReadAtMs) })}</span></> : null}
        {wallet.unlistedCount > 0 && !reading ? <>{SEPARATOR}<span>{t("holdings.unlisted", { count: wallet.unlistedCount, formattedCount: formatNumber(wallet.unlistedCount) })}</span></> : null}
      </span>
      {mode === "rename" ? <form className="holdings-inline" onSubmit={event => { event.preventDefault(); void act(() => bridge.walletRename({ walletId: wallet.id, name: name.trim() || null })); }}>
        <Input aria-label={t("holdings.name")} value={name} maxLength={256} autoFocus onChange={event => setName(event.target.value)} />
        <Button type="button" variant="ghost" size="sm" onClick={() => setMode("view")}>{t("holdings.cancel")}</Button>
        <Button type="submit" size="sm">{t("holdings.save")}</Button>
      </form> : null}
      {mode === "remove" ? <div className="holdings-inline" role="group" aria-label={t("holdings.removeConfirm")}>
        <span className="holdings-meta">{t("holdings.removeConfirm")}</span>
        <Button variant="ghost" size="sm" onClick={() => setMode("view")}>{t("holdings.cancel")}</Button>
        <Button variant="destructive" size="sm" data-wallet-remove-confirm onClick={() => void act(() => bridge.walletRemove({ walletId: wallet.id }))}>{t("holdings.remove")}</Button>
      </div> : null}
    </div>
    <div className="holdings-wallet-actions">
      <Button variant="ghost" size="sm" className="holdings-refresh" disabled={reading || !online} title={online ? undefined : t("holdings.offline")}
        aria-label={`${t("holdings.refresh")} · ${label}`} onClick={onRefresh}>{t("holdings.refresh")}</Button>
      <DropdownMenu>
        <DropdownMenuTrigger asChild><Button variant="ghost" size="icon-sm" aria-label={t("holdings.more", { wallet: label })}><MoreHorizontalIcon /></Button></DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="holdings-menu">
          <DropdownMenuItem className="holdings-menu-refresh" disabled={reading || !online} onSelect={onRefresh}>{t("holdings.refresh")}</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => { void navigator.clipboard?.writeText(wallet.address).catch(() => {}); }}>{t("holdings.copyAddress")}</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => { setName(wallet.name ?? ""); setMode("rename"); }}>{t("holdings.rename")}</DropdownMenuItem>
          <DropdownMenuItem variant="destructive" data-wallet-remove onSelect={() => setMode("remove")}>{t("holdings.remove")}</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  </li>;
}

function AddWallet({ bridge, count, online, open, onOpenChange: setOpen, onAdded }: {
  bridge: LoupistBridge; count: number; online: boolean; open: boolean; onOpenChange: (open: boolean) => void; onAdded: (wallets: WalletView[]) => void;
}) {
  const { t } = useTranslation("knowledge");
  const id = useId();
  const [address, setAddress] = useState("");
  const [name, setName] = useState("");
  const [secret, setSecret] = useState(false);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const field = useRef<HTMLInputElement>(null);
  const form = useRef<HTMLFormElement>(null);
  // The whole form, its warning and its buttons, scrolls into the sheet's view.
  useEffect(() => { if (open) form.current?.scrollIntoView?.({ block: "nearest" }); }, [open, secret]);
  if (count >= MAX_WALLETS) return <p className="holdings-meta" data-wallet-limit>{t("holdings.limit")}</p>;
  if (!open) return <Button variant="ghost" size="sm" className="holdings-add" disabled={!online} title={online ? undefined : t("holdings.offline")}
    onClick={() => { setOpen(true); setSecret(false); setError(null); }}><PlusIcon aria-hidden="true" />{t("holdings.addWallet")}</Button>;
  const network = detectNetwork(address);
  const detected = address.trim() ? network ? t("holdings.detected", { network: network === "evm" ? t("holdings.networks.evmDetected") : t(`holdings.networks.${network}`) }) : t("holdings.unsupported") : "";
  const change = (value: string) => {
    // A recovery phrase or key is cleared at once and never kept anywhere.
    if (looksLikeSecret(value)) { setAddress(""); setSecret(true); return; }
    setAddress(value); setSecret(false); setError(null);
  };
  const add = async () => {
    if (!network || adding || !online) return;
    setAdding(true); setError(null);
    try {
      onAdded(await bridge.walletAdd({ address: address.trim(), name: name.trim() || null }));
      setOpen(false); setAddress(""); setName("");
    } catch (cause) {
      if (code(cause) === "secret_refused") { setAddress(""); setSecret(true); }
      else setError(code(cause) === "unsupported_address" ? t("holdings.unsupported") : errorMessage(cause, t("holdings.error")));
    } finally { setAdding(false); }
  };
  return <form ref={form} className="holdings-add-form" data-wallet-form onSubmit={event => { event.preventDefault(); void add(); }}>
    <label className="holdings-field" htmlFor={`${id}-address`}><span>{t("holdings.address")}</span>
      <Input ref={field} id={`${id}-address`} value={address} autoComplete="off" spellCheck={false} autoFocus className="holdings-address-input"
        aria-invalid={secret || undefined} aria-describedby={`${secret ? `${id}-secret ` : ""}${id}-detected ${id}-note`} data-secret={secret || undefined} onChange={event => change(event.target.value)} />
    </label>
    {/* The warning belongs to the Address field: right under it, in the field's error style. */}
    {secret ? <p id={`${id}-secret`} className="holdings-secret" role="alert" data-wallet-secret>{t("holdings.secret")}</p> : null}
    <label className="holdings-field" htmlFor={`${id}-name`}><span>{t("holdings.name")}</span>
      <Input id={`${id}-name`} value={name} maxLength={256} onChange={event => setName(event.target.value)} /></label>
    <p id={`${id}-detected`} className="holdings-meta" aria-live="polite" data-wallet-detected={network ?? (address.trim() ? "unsupported" : undefined)}>{detected}</p>
    <p id={`${id}-note`} className="holdings-meta">{t("holdings.readOnly")}</p>
    {error ? <p className="holdings-error" role="alert">{error}</p> : null}
    <div className="holdings-actions">
      <Button type="button" variant="ghost" onClick={() => { setOpen(false); setAddress(""); setName(""); setSecret(false); }}>{t("holdings.cancel")}</Button>
      <Button type="submit" disabled={!network || adding || !online} title={online ? undefined : t("holdings.offline")}>{adding ? <Spinner /> : null}{t("holdings.addAndRead")}</Button>
    </div>
    {!online ? <p className="holdings-meta">{t("holdings.offline")}</p> : null}
  </form>;
}

function StockPreview({ preview, busy, onCancel, onImport }: { preview: HoldingsCsvPreview; busy: boolean; onCancel: () => void; onImport: () => void }) {
  const { t } = useTranslation("knowledge");
  return <div className="holdings-preview" data-holdings-preview="stocks">
    <p className="holdings-meta">{t("holdings.file")} · {t("holdings.preview.rows", { count: preview.rowCount, formattedCount: formatNumber(preview.rowCount) })}</p>
    <div className="holdings-table"><table><thead><tr><th>{t("holdings.preview.asset")}</th><th>{t("holdings.preview.company")}</th><th>{t("holdings.preview.quantity")}</th></tr></thead>
      <tbody>{preview.positions.map(position => <tr key={`${position.symbol}:${position.market}`}><td>{position.symbol}</td><td>{position.displayName ?? "—"}</td><td className="gg-num">{position.quantity}</td></tr>)}</tbody></table></div>
    {preview.errors.map((issue, index) => <p key={index} className="holdings-error" role="alert">{t("holdings.preview.errorRow", { row: issue.row, message: issue.message })}</p>)}
    <p className="holdings-meta">{t("holdings.preview.quantityNote")}</p>
    <div className="holdings-actions">
      <Button variant="ghost" onClick={onCancel}>{t("holdings.cancel")}</Button>
      <Button disabled={busy || preview.errors.length > 0 || !preview.positions.length} onClick={onImport}>{t("holdings.preview.importAssets", { count: preview.positions.length, formattedCount: formatNumber(preview.positions.length) })}</Button>
    </div>
  </div>;
}

function CryptoPreview({ bridge, preview, file, busy, onPreview, onError, onCancel, onImport, onAddWallet, onChooseAnother }: {
  bridge: LoupistBridge; preview: CryptoFilePreview; file: string; busy: boolean; onPreview: (preview: CryptoFilePreview) => void;
  onError: (message: string | null) => void; onCancel: () => void; onImport: () => void;
  /** A refused file offers the two ways forward: a wallet address, or another file. */
  onAddWallet: () => void; onChooseAnother: () => void;
}) {
  const { t } = useTranslation("knowledge");
  const id = useId();
  const [asset, setAsset] = useState("");
  const [balance, setBalance] = useState("");
  const cancel = <Button variant="ghost" onClick={onCancel}>{t("holdings.cancel")}</Button>;
  if (preview.tradesOnly) return <div className="holdings-preview" data-holdings-preview="trades">
    <p className="holdings-meta" data-holdings-file>{file}</p>
    <p className="holdings-error" role="alert">{t("holdings.preview.tradesOnly")}</p>
    <div className="holdings-actions">
      {cancel}
      <Button variant="outline" disabled={busy} onClick={onChooseAnother}>{t("holdings.preview.chooseAnother")}</Button>
      <Button disabled={busy} data-holdings-add-wallet onClick={onAddWallet}>{t("holdings.addWallet")}</Button>
    </div>
  </div>;
  if (preview.mappingHeaders) {
    const headers = preview.mappingHeaders;
    const choose = async () => {
      onError(null);
      try { onPreview(await bridge.holdingsMapColumns({ previewId: preview.previewId, asset, balance })); }
      catch (cause) { onError(errorMessage(cause, t("holdings.error"))); }
    };
    return <div className="holdings-preview" data-holdings-preview="mapping">
      <p className="holdings-meta" data-holdings-file>{t("holdings.mapping.file", { file })}</p>
      {([["asset", asset, setAsset], ["balance", balance, setBalance]] as const).map(([key, value, set]) => <label key={key} className="holdings-field" htmlFor={`${id}-${key}`}>
        <span>{t(`holdings.mapping.${key}`)}</span>
        <NativeSelect id={`${id}-${key}`} value={value} onChange={event => set(event.target.value)}>
          <option value="">{t("holdings.mapping.choose")}</option>
          {headers.map(header => <option key={header} value={header}>{header}</option>)}
        </NativeSelect>
      </label>)}
      <div className="holdings-actions">{cancel}<Button disabled={!asset || !balance || asset === balance} onClick={() => void choose()}>{t("holdings.mapping.continue")}</Button></div>
    </div>;
  }
  const skipped = (["cash", "zero"] as const).map(reason => {
    const symbols = preview.skipped.filter(item => item.reason === reason).map(item => item.symbol);
    return symbols.length ? t(`holdings.preview.${reason}`, { symbols: symbols.join(", ") }) : null;
  }).filter(Boolean).join(" · ");
  const header = [
    preview.format ? t(`holdings.preview.${preview.format}`) : t("holdings.exchangeFile"),
    t("holdings.preview.rows", { count: preview.rowCount, formattedCount: formatNumber(preview.rowCount) }),
    preview.asOfMs ? t("holdings.preview.asOf", { date: formatDate(preview.asOfMs) }) : null,
  ].filter(Boolean).join(" · ");
  return <div className="holdings-preview" data-holdings-preview="crypto">
    <p className="holdings-meta">{header}</p>
    <div className="holdings-table"><table><thead><tr><th>{t("holdings.preview.asset")}</th><th className="holdings-quantity">{t("holdings.preview.quantity")}</th><th>{t("holdings.preview.from")}</th></tr></thead>
      <tbody>{preview.rows.map(row => <tr key={row.symbol}><td>{row.symbol}</td><td className="gg-num holdings-quantity">{row.quantity}</td><td>{row.from.join(", ")}</td></tr>)}</tbody></table></div>
    {skipped ? <p className="holdings-skipped">{t("holdings.preview.skipped", { items: skipped })}</p> : null}
    {preview.errors.map((issue, index) => <p key={index} className="holdings-error" role="alert">{t("holdings.preview.errorRow", { row: issue.row, message: issue.message })}</p>)}
    <p className="holdings-meta">{t("holdings.preview.quantityNote")}</p>
    <div className="holdings-actions">{cancel}
      <Button disabled={busy || preview.errors.length > 0 || !preview.rows.length} data-holdings-import onClick={onImport}>{t("holdings.preview.importAssets", { count: preview.rows.length, formattedCount: formatNumber(preview.rows.length) })}</Button>
    </div>
  </div>;
}
