import { Disclosure } from "../ui/disclosure";
import { useEffect, useState } from "react";
import { XIcon, LoaderCircleIcon } from "lucide-react";
import { useTranslation } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { LoupistBridge } from "@/types";
import type { HoldingsCsvPreview, WatchlistAddRequest } from "@/contracts/userContext";
import { errorMessage } from "@/lib/errors";
import { HoldingsSheet } from "../holdings/HoldingsSheet";

export function FollowStep({ bridge, chips, onChipsChange, onContinue, onBusyChange }: {
  bridge: LoupistBridge; chips: WatchlistAddRequest[];
  onChipsChange: (chips: WatchlistAddRequest[]) => void; onContinue: () => void; onBusyChange: (busy: boolean) => void;
}) {
  const { t } = useTranslation("onboarding");
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => { onBusyChange(busy); return () => onBusyChange(false); }, [busy, onBusyChange]);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<HoldingsCsvPreview | null>(null);
  const [imported, setImported] = useState(false);
  const [kisReady, setKisReady] = useState(false);
  const [holdingsOpen, setHoldingsOpen] = useState(false);
  useEffect(() => { let active = true; void bridge.marketplaceSnapshot().then(snapshot => {
    if (active) setKisReady(snapshot.connectors.some(connector => connector.entry_id === "koreainvestment.market-data" && connector.readiness === "ready"));
  }).catch(() => {}); return () => { active = false; }; }, [bridge]);
  const addInput = (): WatchlistAddRequest[] => {
    const next = [...chips];
    for (const text of input.split(",")) {
      const [symbol, market = "UNKNOWN"] = text.trim().toUpperCase().split(":");
      if (!symbol || next.some(chip => chip.symbol === symbol && chip.market === market)) continue;
      if (next.length >= 30) { setError(t("wizard.limit")); break; }
      next.push({ symbol, market: market || "UNKNOWN" });
    }
    onChipsChange(next); setInput(""); return next;
  };
  const importSymbols = async (snapshot: { positions: ReadonlyArray<{ symbol: string; market: string; displayName: string | null }> }) => {
    // Imported holdings all belong on the watchlist, even beyond the manual chip limit.
    const next = [...chips];
    const entries = snapshot.positions.map(position => ({ symbol: position.symbol, market: position.market, displayName: position.displayName }));
    for (const entry of entries) {
      if (!next.some(chip => chip.symbol === entry.symbol && chip.market === entry.market)) next.push(entry);
    }
    // The holdings are already committed. Retain every symbol if a watchlist write
    // fails so Continue can retry without submitting the one-use preview twice.
    onChipsChange(next); setPreview(null);
    for (const entry of entries) await bridge.watchlistAdd(entry);
    setImported(true);
  };
  const runImport = async (action: () => Promise<void>) => {
    setBusy(true); setError(null);
    try { await action(); } catch (cause) { setError(errorMessage(cause, t("wizard.importError"))); }
    finally { setBusy(false); }
  };
  // Imported holdings and earlier visits already saved their chips, so removal must reach the watchlist too.
  const remove = async (chip: typeof chips[number]) => {
    setBusy(true); setError(null);
    try { await bridge.watchlistRemove({ symbol: chip.symbol, market: chip.market }); onChipsChange(chips.filter(item => item !== chip)); }
    catch (cause) { setError(errorMessage(cause, t("wizard.saveError"))); }
    finally { setBusy(false); }
  };
  const proceed = async () => {
    setBusy(true); setError(null);
    try { for (const chip of addInput()) await bridge.watchlistAdd(chip); onContinue(); }
    catch (cause) { setError(errorMessage(cause, t("wizard.saveError"))); }
    finally { setBusy(false); }
  };
  return <>
    <h1>{t("wizard.followTitle")}</h1><p>{t("wizard.followBody")}</p>
    <div className="form-field"><Label htmlFor="onboarding-symbols">{t("wizard.symbols")}</Label>
      <Input id="onboarding-symbols" value={input} disabled={busy} placeholder={t("wizard.symbolPlaceholder")} onChange={event => setInput(event.target.value)}
        onKeyDown={event => { if ((event.key === "Enter" || event.key === ",") && !event.nativeEvent.isComposing && event.keyCode !== 229) { event.preventDefault(); addInput(); } }} /></div>
    {chips.length > 0 && <div className="onboarding-chips">{chips.map(chip => <span className="onboarding-chip" key={`${chip.symbol}:${chip.market}`}>
      {chip.symbol}{chip.market !== "UNKNOWN" ? ` · ${chip.market}` : ""}<button type="button" disabled={busy} aria-label={t("wizard.remove", { symbol: chip.symbol })}
        onClick={() => void remove(chip)}><XIcon size={14} aria-hidden="true" /></button></span>)}</div>}
    {/* What the import reads and where holdings stay is said before the section opens. */}
    <Disclosure className="onboarding-other" data-onboarding-holdings title={<>{t("wizard.import")}</>} description={t("wizard.importCaption")}>
      <Button variant="ghost" disabled={busy} onClick={() => void runImport(async () => {
        const path = await bridge.pickFile({ title: t("wizard.csv"), filters: [{ name: "CSV", extensions: ["csv"] }] });
        if (path) { setPreview(await bridge.holdingsImportCsv({ path })); setImported(false); }
      })}>{t("wizard.csv")}</Button>
      {kisReady && <Button variant="ghost" disabled={busy} onClick={() => void runImport(async () => importSymbols(await bridge.holdingsImportKis()))}>{t("wizard.kis")}</Button>}
      <Button variant="ghost" disabled={busy} data-onboarding-crypto onClick={() => setHoldingsOpen(true)}>{t("wizard.cryptoHoldings")}</Button>
    </Disclosure>
    {/* Exchange files and wallet addresses are imported in the holdings sheet; imported assets join the chips. */}
    <HoldingsSheet bridge={bridge} open={holdingsOpen} onOpenChange={setHoldingsOpen}
      onImported={assets => void runImport(async () => importSymbols({ positions: assets.map(asset => ({ ...asset, displayName: asset.displayName ?? null })) }))} />
    {preview && <><div className="onboarding-holdings"><table><thead><tr><th>{t("wizard.symbol")}</th><th>{t("wizard.company")}</th><th>{t("wizard.quantity")}</th></tr></thead>
      <tbody>{preview.positions.map((position, index) => <tr key={index}><td>{position.symbol}</td><td>{position.displayName ?? "—"}</td><td className="gg-num">{position.quantity}</td></tr>)}</tbody></table></div>
      {preview.errors.map((issue, index) => <p role="alert" key={index}>{issue.row}: {issue.message}</p>)}
      <p className="onboarding-holdings-note">{t("wizard.holdingsNote")}</p>
      <Button disabled={busy || preview.errors.length > 0 || !preview.positions.length} onClick={() => void runImport(async () => importSymbols(await bridge.holdingsCommitCsv({ previewId: preview.previewId })))}>{t("wizard.confirmImport")}</Button>
      <Button variant="ghost" disabled={busy} onClick={() => setPreview(null)}>{t("wizard.cancel")}</Button></>}
    {imported && <><div className="onboarding-success" role="status">{t("wizard.imported")}</div><p className="onboarding-holdings-note">{t("wizard.holdingsNote")}</p></>}
    {error && <div role="alert" className="onboarding-error">{error}</div>}
    <div className="onboarding-actions-row"><Button className="onboarding-primary" data-onboarding-continue disabled={busy || !!preview} onClick={() => void proceed()}>{busy && <LoaderCircleIcon className="animate-spin" />}{t("wizard.continue")}</Button>
      <Button data-onboarding-skip variant="ghost" disabled={busy} onClick={onContinue}>{t("wizard.skip")}</Button></div>
  </>;
}
