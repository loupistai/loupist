import { useCallback, useEffect, useId, useMemo, useState } from "react";
import { ArrowLeftIcon } from "lucide-react";
import i18n, { useTranslation } from "@/i18n";
import { formatDate, formatNumber, formatRelative } from "@/i18n/format";
import type { LoupistBridge } from "../../types";
import type { GalleryEntry, GalleryIndex, PackAssetClass, PackOpenResult } from "../../contracts/sharing";
import { errorMessage } from "../../lib/errors";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { NativeSelect } from "../ui/native-select";
import { Spinner } from "../ui/spinner";
import { errorCode, licenseName } from "./sharingText";
import "@/styles/sharing.css";

const CLASSES: PackAssetClass[] = ["stocks", "etfs", "bonds", "crypto", "economy"];
type Sort = "popular" | "recent";
type Filters = { language: string; covers: string; sort: Sort };
const DEFAULT_FILTERS: Filters = { language: "all", covers: "all", sort: "popular" };
/** Gallery filters are remembered per device. */
const FILTERS_KEY = "loupist.gallery.filters";

function readFilters(): Filters {
  try {
    const stored = JSON.parse(localStorage.getItem(FILTERS_KEY) ?? "null") as Partial<Filters> | null;
    return {
      language: typeof stored?.language === "string" ? stored.language : "all",
      covers: typeof stored?.covers === "string" ? stored.covers : "all",
      sort: stored?.sort === "recent" ? "recent" : "popular",
    };
  } catch { return DEFAULT_FILTERS; }
}

function writeFilters(filters: Filters) {
  try { localStorage.setItem(FILTERS_KEY, JSON.stringify(filters)); } catch { /* storage may be unavailable */ }
}

/** "3 days ago", in the largest unit that fits. */
export function ago(atMs: number, nowMs = Date.now()) {
  const seconds = Math.max(0, (nowMs - atMs) / 1000);
  const steps: [Intl.RelativeTimeFormatUnit, number][] = [["year", 31_536_000], ["month", 2_592_000], ["week", 604_800], ["day", 86_400], ["hour", 3_600], ["minute", 60]];
  for (const [unit, size] of steps) if (seconds >= size) return formatRelative(-Math.floor(seconds / size), unit);
  return formatRelative(0, "minute");
}

function languageName(code: string) {
  try { return new Intl.DisplayNames([i18n.language === "en-XA" ? "en" : i18n.language || "en"], { type: "language" }).of(code) ?? code; }
  catch { return code; }
}

/**
 * The community Gallery (W19.6): Loupists listed in the open index, which
 * nobody reviews by hand. Preview and Use both open "Review before adding";
 * nothing is added from here.
 */
export function Gallery({ bridge, onBack, onOpened, onUseLinkOrFile }: {
  bridge: LoupistBridge; onBack: () => void; onOpened: (result: PackOpenResult, use: boolean) => void; onUseLinkOrFile: () => void;
}) {
  const { t } = useTranslation("sharing");
  const titleId = useId();
  const [index, setIndex] = useState<GalleryIndex | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [query, setQuery] = useState("");
  const [filters, setFilters] = useState<Filters>(readFilters);
  const [opening, setOpening] = useState<string | null>(null);
  const [openError, setOpenError] = useState<{ entryId: string; message: string } | null>(null);
  const load = useCallback(async (refresh: boolean) => {
    if (refresh) setRefreshing(true);
    try { setIndex(await bridge.galleryIndex({ refresh })); }
    catch { setIndex(current => current ?? { status: "unavailable", fetchedAtMs: null, entries: [] }); }
    finally { setRefreshing(false); }
  }, [bridge]);
  useEffect(() => { void load(false); }, [load]);
  const update = (next: Partial<Filters>) => setFilters(current => { const merged = { ...current, ...next }; writeFilters(merged); return merged; });

  const entries = index?.entries ?? [];
  const languages = useMemo(() => [...new Set(entries.map(entry => entry.language))].sort(), [entries]);
  const terms = query.trim().toLocaleLowerCase();
  const shown = useMemo(() => entries
    .filter(entry => filters.language === "all" || entry.language === filters.language)
    .filter(entry => filters.covers === "all" || entry.assetClasses.includes(filters.covers as PackAssetClass))
    .filter(entry => !terms || [entry.name, entry.focus, entry.author ?? ""].some(text => text.toLocaleLowerCase().includes(terms)))
    .sort((a, b) => filters.sort === "recent"
      ? Date.parse(b.updatedAt) - Date.parse(a.updatedAt)
      : b.forks - a.forks || b.stars - a.stars || a.name.localeCompare(b.name)), [entries, filters, terms]);
  const filtered = Boolean(terms) || filters.language !== "all" || filters.covers !== "all";

  const open = async (entry: GalleryEntry, use: boolean) => {
    setOpening(`${entry.id}:${use}`); setOpenError(null);
    try {
      const result = await bridge.packOpen({ source: { gallery: entry.id } });
      if (result) onOpened(result, use);
    } catch (cause) {
      const code = errorCode(cause);
      setOpenError({ entryId: entry.id, message: code === "offline" ? t("publishOffline") : code === "rate_limited" ? t("rateLimited") : code === "link_not_found" ? t("linkNotFound") : errorMessage(cause, t("linkNotFound")) });
    } finally { setOpening(null); }
  };

  const unavailable = index?.status === "unavailable" || index?.status === "notConfigured";
  return <section className="gallery-page" aria-labelledby={titleId} data-gallery data-gallery-status={index?.status ?? "loading"}>
    <header className="gallery-header">
      <Button variant="ghost" size="sm" onClick={onBack}><ArrowLeftIcon aria-hidden="true" />{t("back")}</Button>
      <h1 id={titleId}>{t("galleryTitle")}</h1>
      {index?.fetchedAtMs ? <span className="gallery-updated">{t("updated", { relative: ago(index.fetchedAtMs) })}</span> : null}
      <Button variant="outline" size="sm" disabled={!index || refreshing} onClick={() => void load(true)}>{refreshing ? <Spinner /> : null}{t("refresh")}</Button>
    </header>
    <p className="gallery-body">{t("galleryBody")}</p>
    {index?.status === "offline" && index.fetchedAtMs ? <p className="gallery-notice" role="status" data-gallery-offline>{t("galleryOffline", { date: formatDate(index.fetchedAtMs, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" }) })}</p> : null}
    {unavailable ? <div className="gallery-unavailable" data-gallery-unavailable>
      <p>{t("galleryUnavailable")}</p>
      <Button variant="outline" size="sm" onClick={onUseLinkOrFile}>{t("useLinkOrFile")}</Button>
    </div> : <>
      <div className="gallery-filters">
        <Input type="search" className="gallery-search" aria-label={t("search")} placeholder={t("search")} value={query} onChange={event => setQuery(event.target.value)} />
        <label className="gallery-filter"><span>{t("language")}</span>
          <NativeSelect value={filters.language} onChange={event => update({ language: event.target.value })}>
            <option value="all">{t("all")}</option>
            {languages.map(code => <option key={code} value={code}>{languageName(code)}</option>)}
          </NativeSelect>
        </label>
        <label className="gallery-filter"><span>{t("covers")}</span>
          <NativeSelect value={filters.covers} onChange={event => update({ covers: event.target.value })}>
            <option value="all">{t("all")}</option>
            {CLASSES.map(value => <option key={value} value={value}>{t(`class.${value}`)}</option>)}
          </NativeSelect>
        </label>
        <label className="gallery-filter"><span>{t("sort")}</span>
          <NativeSelect value={filters.sort} onChange={event => update({ sort: event.target.value === "recent" ? "recent" : "popular" })}>
            <option value="popular">{t("sortPopular")}</option>
            <option value="recent">{t("sortRecent")}</option>
          </NativeSelect>
        </label>
      </div>
      {!index ? <div className="gallery-grid" role="status" aria-label={t("galleryLoading")}>
        {Array.from({ length: 6 }, (_, key) => <div key={key} className="gallery-card is-skeleton" aria-hidden="true"><div className="gallery-card-image" /><div className="gallery-card-content"><span /><span /><span /></div></div>)}
      </div> : shown.length ? <div className="gallery-grid">
        {shown.map(entry => <GalleryCard key={entry.id} bridge={bridge} entry={entry} busy={opening} error={openError?.entryId === entry.id ? openError.message : null} onOpen={use => void open(entry, use)} />)}
      </div> : <div className="gallery-empty" data-gallery-empty>
        <p>{t("galleryNoMatch")}</p>
        {filtered ? <Button variant="outline" size="sm" onClick={() => { setQuery(""); update({ language: "all", covers: "all" }); }}>{t("clearFilters")}</Button> : null}
      </div>}
    </>}
  </section>;
}

function GalleryCard({ bridge, entry, busy, error, onOpen }: {
  bridge: LoupistBridge; entry: GalleryEntry; busy: string | null; error: string | null; onOpen: (use: boolean) => void;
}) {
  const { t } = useTranslation("sharing");
  const nameId = useId();
  const [image, setImage] = useState<string | null>(null);
  useEffect(() => {
    if (!entry.hasPreview) return;
    let live = true;
    void bridge.galleryImage({ entryId: entry.id }).then(url => { if (live) setImage(url); }).catch(() => undefined);
    return () => { live = false; };
  }, [bridge, entry.id, entry.hasPreview]);
  const meta = [entry.author, languageName(entry.language), licenseName(entry.license)].filter(Boolean).join(" · ");
  return <article className="gallery-card" aria-labelledby={nameId} data-gallery-card={entry.id}>
    {entry.hasPreview ? <div className="gallery-card-image">{image ? <img src={image} alt="" /> : null}</div> : null}
    <div className="gallery-card-content">
      <h2 id={nameId} className="gallery-card-name">{entry.name}</h2>
      <p className="gallery-card-meta">{meta}</p>
      {entry.focus ? <p className="gallery-card-focus">{entry.focus}</p> : null}
      <p className="gallery-card-counts gg-num">{t("cardCounts", { methods: formatNumber(entry.counts.methods), names: formatNumber(entry.counts.names) })}</p>
      {entry.assetClasses.length ? <ul className="gallery-card-chips">{entry.assetClasses.map(value => <li key={value}>{t(`class.${value}`)}</li>)}</ul> : null}
      <p className="gallery-card-social gg-num">{t("cardSocial", { stars: formatNumber(entry.stars), forks: formatNumber(entry.forks), relative: ago(Date.parse(entry.updatedAt)) })}</p>
      {error ? <p className="share-error" role="alert">{error}</p> : null}
      <div className="gallery-card-actions">
        <Button variant="ghost" size="sm" aria-label={t("previewNamed", { name: entry.name })} disabled={busy !== null} onClick={() => onOpen(false)}>{busy === `${entry.id}:false` ? <Spinner /> : null}{t("preview")}</Button>
        <Button variant="outline" size="sm" aria-label={t("useNamed", { name: entry.name })} disabled={busy !== null} onClick={() => onOpen(true)}>{busy === `${entry.id}:true` ? <Spinner /> : null}{t("use")}</Button>
      </div>
    </div>
  </article>;
}
