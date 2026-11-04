import { useEffect, useId, useMemo, useRef, useState, type ReactNode } from "react";
import { ArrowLeftIcon, ChevronRightIcon, ExternalLinkIcon } from "lucide-react";
import i18n, { useTranslation } from "@/i18n";
import { formatDate, formatNumber } from "@/i18n/format";
import type { LoupistBridge, LoupistSummary } from "../../types";
import { LOUPIST_TONES, type LoupistTone } from "../../contracts/loupist";
import type { PackAddResult, PackFlag, PackItemView, PackManifestView, PackOpenResult, PackPreview } from "../../contracts/sharing";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { NativeSelect } from "../ui/native-select";
import { RadioGroup, RadioGroupItem } from "../ui/radio-group";
import { Spinner } from "../ui/spinner";
import { Switch } from "../ui/switch";
import { PackMarkdown } from "./PackMarkdown";
import { revealFocused } from "../../lib/revealFocused";
import { licenseName, licenseSummary } from "./sharingText";
import "@/styles/sharing.css";

type Sections = { perspective: boolean; methods: boolean; principles: boolean; map: boolean };

/**
 * "Review before adding" (W19.4, W19.7): everything a shared Loupist would
 * bring, with its warnings, before one Add. Nothing lands until Add; methods
 * and principles then arrive as suggestions the user accepts.
 */
export function PackReview({ bridge, result, loupists, currentLoupistId, focusAdd, onBack, onAdded }: {
  bridge: LoupistBridge; result: PackOpenResult; loupists: readonly LoupistSummary[]; currentLoupistId?: string | null;
  /** Use from a Gallery card: "A new Loupist" is chosen and Add has focus. */
  focusAdd?: boolean;
  onBack: () => void; onAdded: (added: PackAddResult) => void;
}) {
  const { t } = useTranslation("sharing");
  const titleId = useId();
  return <section className="pack-review" aria-labelledby={titleId} data-pack-review>
    <header className="pack-review-header">
      <Button variant="ghost" size="sm" onClick={onBack}><ArrowLeftIcon aria-hidden="true" />{t("back")}</Button>
      <h1 id={titleId}>{t("reviewTitle")}</h1>
    </header>
    {/* The header's Back is the one way out of a rejected pack. */}
    {result.kind === "rejected" ? <div className="pack-review-rejected" role="alert" data-pack-rejected={result.reason}>
      {result.sourceLabel ? <p className="pack-review-source">{result.sourceLabel}</p> : null}
      <h2>{t("invalidTitle")}</h2>
      <p>{t(`invalid.${result.reason}`)}</p>
      <p>{t("nothingAdded")}</p>
    </div> : <ReviewBody bridge={bridge} preview={result.preview} loupists={loupists} currentLoupistId={currentLoupistId} focusAdd={focusAdd} onAdded={onAdded} />}
  </section>;
}

function ReviewBody({ bridge, preview, loupists, currentLoupistId, focusAdd, onAdded }: {
  bridge: LoupistBridge; preview: PackPreview; loupists: readonly LoupistSummary[]; currentLoupistId?: string | null; focusAdd?: boolean; onAdded: (added: PackAddResult) => void;
}) {
  const { t } = useTranslation("sharing");
  const ids = { name: useId(), tone: useId(), mode: useId(), target: useId() };
  const { manifest } = preview;
  const [mode, setMode] = useState<"new" | "merge">("new");
  const [target, setTarget] = useState(() => (loupists.find(item => item.id === currentLoupistId) ?? loupists[0])?.id ?? "");
  const [name, setName] = useState(preview.profile.name);
  const [tone, setTone] = useState<LoupistTone>(preview.profile.tone);
  const [sections, setSections] = useState<Sections>({ perspective: Boolean(preview.perspective), methods: preview.methods.length > 0, principles: preview.principles.length > 0, map: preview.world.names > 0 });
  const [adding, setAdding] = useState<{ done: number; total: number } | null>(null);
  const [failed, setFailed] = useState(false);
  const addButton = useRef<HTMLButtonElement>(null);
  useEffect(() => { if (focusAdd) addButton.current?.focus(); }, [focusAdd]);
  const merge = mode === "merge";
  const effective: Sections = { ...sections, perspective: sections.perspective && !merge };
  const flagged = preview.flaggedCount;
  const targetName = loupists.find(item => item.id === target)?.name ?? "";
  const modeLocked = Boolean(adding) || Boolean(preview.blocked);
  const canAdd = !preview.blocked && !adding && (merge ? Boolean(target) : Boolean(name.trim())) && Object.values(effective).some(Boolean);

  const add = async () => {
    if (!canAdd) return;
    setFailed(false); setAdding({ done: 0, total: 0 });
    try {
      const added = await bridge.packAdd({ stagingId: preview.stagingId, mode: merge ? { merge: { loupistId: target } } : { new: { name: name.trim(), tone } }, sections: effective }, event => setAdding(event));
      onAdded(added);
    } catch {
      setFailed(true); setAdding(null);
    }
  };
  const basedOn = manifest.origin.map(origin => origin.name).join(", ");
  const date = Number.isNaN(Date.parse(manifest.createdAt)) ? null : formatDate(Date.parse(manifest.createdAt), { year: undefined, month: "short", day: "numeric" });
  const meta = [manifest.author, t("version", { version: formatNumber(manifest.version) }), date].filter(Boolean).join(" · ");
  const hosts = preview.world.sourceHosts.map(host => `${host.host} (${formatNumber(host.count)})`).concat(preview.world.moreHosts ? [`+${formatNumber(preview.world.moreHosts)}`] : []).join(", ");
  const tracks = [preview.world.kinds.join(", "), preview.world.relations.join(", ")].filter(Boolean).join(" · ");

  return <div className="pack-review-grid">
    <div className="pack-review-main" onFocus={revealFocused}>
    <div className="pack-review-summary">
      <p className="pack-review-name">{manifest.name}</p>
      <p className="pack-review-meta">{meta}</p>
      {preview.sourceUrl && preview.sourceLabel ? <Button variant="link" size="sm" className="pack-review-link" onClick={() => void bridge.openExternalUrl(preview.sourceUrl!)}>{preview.sourceLabel}<ExternalLinkIcon aria-hidden="true" /></Button> : null}
      {basedOn ? <p className="pack-review-meta">{t("basedOn", { names: basedOn })}</p> : null}
      <div className="pack-review-license" data-pack-license>
        <Button variant="link" size="sm" className="pack-review-link" onClick={() => void bridge.openExternalUrl(manifest.licenseUrl)}>{licenseName(manifest.license)}<ExternalLinkIcon aria-hidden="true" /></Button>
        {/* Items under a license that does not allow sharing stay out of every later share. */}
        <p className={manifest.redistributable ? "pack-review-meta" : "pack-review-restricted"} data-pack-not-redistributable={!manifest.redistributable || undefined}>
          {manifest.redistributable ? licenseSummary(manifest.license) : t("notRedistributable")}
        </p>
      </div>
      {manifest.credits.length ? <Credits credits={manifest.credits} onOpen={url => void bridge.openExternalUrl(url)} /> : null}
      <h2 className="share-label">{t("safety")}</h2>
      <p className="pack-review-safety">{t("safetyNoAuthority")}</p>
      {preview.blocked ? <p className="pack-review-blocked" role="alert" data-pack-blocked>{t("safetyBlocked", { reason: t(`reason.${preview.blocked}`) })}</p>
        : <p className={flagged ? "pack-review-flags is-warning" : "pack-review-flags"}>{flagged ? t("needsLook", { count: flagged, formattedCount: formatNumber(flagged) }) : t("clean")}</p>}
    </div>
    <div className="pack-review-sections">
      <section aria-labelledby={`${ids.name}-h`} className="pack-review-section" data-disabled={merge || undefined}>
        <h2 id={`${ids.name}-h`} className="share-label">{t("profile")}</h2>
        {/* Said once: under Perspective when the pack has one, else here. */}
        {merge && !preview.perspective ? <p className="pack-review-note">{t("mergeNote")}</p> : null}
        <div className="pack-review-profile">
          <label className="share-field" htmlFor={ids.name}><span>{i18n.t("onboarding:wizard.name")}</span><Input id={ids.name} value={merge ? preview.profile.name : name} disabled={merge || Boolean(adding)} maxLength={80} onChange={event => setName(event.target.value)} /></label>
          <label className="share-field" htmlFor={ids.tone}><span>{i18n.t("app:loupistProfile.tone")}</span>
            <NativeSelect id={ids.tone} value={tone} disabled={merge || Boolean(adding)} onChange={event => setTone(event.target.value as LoupistTone)}>
              {LOUPIST_TONES.map(value => <option key={value} value={value}>{i18n.t(`onboarding:tone.${value}.label`)}</option>)}
            </NativeSelect>
          </label>
        </div>
        {preview.profile.focus ? <p className="pack-review-focus"><span>{i18n.t("onboarding:wizard.focus")}</span> “{preview.profile.focus}”</p> : null}
        <Flags flags={preview.profileFindings} />
      </section>
      {preview.perspective ? <ReviewSection label={t("perspective")} checked={effective.perspective} disabled={merge || Boolean(adding)} note={merge ? t("mergeNote") : null} onChange={value => setSections(current => ({ ...current, perspective: value }))}>
        <Perspective item={preview.perspective} />
      </ReviewSection> : null}
      {preview.methods.length ? <ReviewSection label={t("methods")} count={preview.methods.length} checked={effective.methods} disabled={Boolean(adding)} onChange={value => setSections(current => ({ ...current, methods: value }))}>
        <ItemRows items={preview.methods} />
      </ReviewSection> : null}
      {preview.principles.length ? <ReviewSection label={t("principles")} count={preview.principles.length} checked={effective.principles} disabled={Boolean(adding)} onChange={value => setSections(current => ({ ...current, principles: value }))}>
        <ItemRows items={preview.principles} />
      </ReviewSection> : null}
      {preview.world.names ? <ReviewSection label={t("map")} detail={<span className="gg-num">{t("mapCounts", { names: formatNumber(preview.world.names), links: formatNumber(preview.world.links), sources: formatNumber(preview.world.sources) })}</span>} checked={effective.map} disabled={Boolean(adding)} onChange={value => setSections(current => ({ ...current, map: value }))}>
        {hosts ? <p className="pack-review-meta">{t("mapSources", { hosts })}</p> : null}
        {tracks ? <p className="pack-review-meta">{t("mapTracks", { kinds: tracks })}</p> : null}
        <MapFindings world={preview.world} />
      </ReviewSection> : null}
    </div>
    </div>
    <div className="pack-review-add" data-pack-add>
      <div className="pack-review-mode">
        <p id={`${ids.mode}-label`} className="share-label pack-review-mode-label">{t("addAs")}</p>
        {/* A pack the Gallery removed can't be added in any way, so the choice is off with Add. */}
        <RadioGroup className="pack-review-options" aria-labelledby={`${ids.mode}-label`} value={mode} disabled={modeLocked} onValueChange={value => setMode(value as "new" | "merge")}>
          <div className="share-radio-row"><RadioGroupItem id={`${ids.mode}-new`} value="new" /><label htmlFor={`${ids.mode}-new`}>{t("asNew")}</label></div>
          {loupists.length ? <div className="share-radio-row pack-review-merge">
            <RadioGroupItem value="merge" aria-label={t("asMerge", { name: targetName })} />
            <NativeSelect aria-label={t("addAs")} value={target} disabled={!merge || modeLocked} onChange={event => setTarget(event.target.value)}>
              {loupists.map(item => <option key={item.id} value={item.id}>{t("asMerge", { name: item.name })}</option>)}
            </NativeSelect>
          </div> : null}
        </RadioGroup>
      </div>
      <Button ref={addButton} size="lg" className="share-primary" disabled={!canAdd} data-pack-add-button onClick={() => void add()}>{t("add")}</Button>
      {adding ? <p className="share-status" role="status"><Spinner className="share-spinner" />{t("adding", { done: formatNumber(adding.done), total: formatNumber(adding.total) })}</p>
        : failed ? <p className="share-error" role="alert">{t("addFailed")}</p>
        : <p className="pack-review-note">{t("addNote")}</p>}
    </div>
  </div>;
}

/** Attribution for included material, as the pack names it: each origin (linked when it has a link), its author, version and license. */
function Credits({ credits, onOpen }: { credits: PackManifestView["credits"]; onOpen: (url: string) => void }) {
  const { t } = useTranslation("sharing");
  const id = useId();
  return <section className="pack-review-credits" aria-labelledby={id} data-pack-credits>
    <h2 id={id} className="share-label">{t("credits")}</h2>
    <ul>
      {credits.map((credit, index) => <li key={`${credit.name}-${index}`}>
        {credit.url ? <Button variant="link" size="sm" className="pack-review-link" title={credit.url} onClick={() => onOpen(credit.url!)}>{credit.name}<ExternalLinkIcon aria-hidden="true" /></Button>
          : <span className="pack-review-credit-name">{credit.name}</span>}
        <span className="pack-review-meta">{[credit.author, t("version", { version: formatNumber(credit.version) }), credit.license ? licenseName(credit.license) : null].filter(Boolean).join(" · ")}</span>
      </li>)}
    </ul>
  </section>;
}

function ReviewSection({ label, count, detail, note, checked, disabled, onChange, children }: {
  label: string; count?: number; detail?: ReactNode; note?: string | null; checked: boolean; disabled: boolean; onChange: (checked: boolean) => void; children: ReactNode;
}) {
  const id = useId();
  return <section className="pack-review-section" aria-labelledby={id} data-off={!checked || undefined}>
    <div className="pack-review-section-head">
      <h2 id={id} className="share-label">{label}{count !== undefined ? <span className="gg-num"> {formatNumber(count)}</span> : null}</h2>
      {detail ? <span id={`${id}-detail`} className="pack-review-meta">{detail}</span> : null}
      <Switch aria-labelledby={id} aria-describedby={detail ? `${id}-detail` : undefined} checked={checked} disabled={disabled} onCheckedChange={onChange} />
    </div>
    {note ? <p className="pack-review-note">{note}</p> : null}
    {children}
  </section>;
}

function Perspective({ item }: { item: PackItemView }) {
  const [all, setAll] = useState(false);
  const lines = item.markdown.split("\n").length;
  const highlight = useMemo(() => new Set(item.findings.flatMap(finding => finding.lines)), [item]);
  return <div className="pack-review-perspective">
    <Findings item={item} />
    <PackMarkdown markdown={item.markdown} highlight={highlight} maxLines={all ? undefined : 6} />
    {!all && lines > 6 ? <Button variant="link" size="sm" onClick={() => setAll(true)}>{i18n.t("chat:chatRail.showAll")}</Button> : null}
  </div>;
}

function Findings({ item }: { item: PackItemView }) {
  return <Flags flags={item.findings.map(finding => finding.flag)} />;
}

function Flags({ flags }: { flags: readonly PackFlag[] }) {
  const { t } = useTranslation("sharing");
  const unique = [...new Set(flags)];
  return unique.length ? <ul className="pack-flags">{unique.map(flag => <li key={flag}>{t(`flag.${flag}`)}</li>)}</ul> : null;
}

/** Names, relationships and Sources whose text was flagged, each with its reasons in text. */
function MapFindings({ world }: { world: PackPreview["world"] }) {
  const { t } = useTranslation("sharing");
  if (!world.findings.length) return null;
  const more = world.flagged - world.findings.length;
  return <>
    <ul className="pack-map-findings" data-pack-map-findings>
      {world.findings.map((finding, index) => <li key={`${finding.kind}-${index}`}>
        <span className="pack-review-item-title">{finding.label}</span>
        <span className="pack-review-item-flags">{[...new Set(finding.flags)].map(flag => t(`flag.${flag}`)).join(" · ")}</span>
      </li>)}
    </ul>
    {more > 0 ? <p className="pack-review-meta">{t("mapFindingsMore", { count: more, formattedCount: formatNumber(more) })}</p> : null}
  </>;
}

function ItemRows({ items }: { items: readonly PackItemView[] }) {
  const { t } = useTranslation("sharing");
  const [open, setOpen] = useState<string | null>(null);
  return <ul className="pack-review-items">
    {items.map(item => {
      const expanded = open === item.key;
      const flags = [...new Set(item.findings.map(finding => finding.flag))];
      const panelId = `pack-item-${item.key}`;
      return <li key={item.key} data-flagged={flags.length ? true : undefined}>
        <button type="button" className="pack-review-item" aria-expanded={expanded} aria-controls={panelId} onClick={() => setOpen(expanded ? null : item.key)}>
          <ChevronRightIcon aria-hidden="true" />
          <span className="pack-review-item-title">{item.title}</span>
          {flags.length ? <span className="pack-review-item-flags">{flags.map(flag => t(`flag.${flag}`)).join(" · ")}</span> : null}
        </button>
        {expanded ? <div id={panelId} className="pack-review-item-body">
          <PackMarkdown markdown={item.markdown} highlight={new Set(item.findings.flatMap(finding => finding.lines))} />
        </div> : null}
      </li>;
    })}
  </ul>;
}
