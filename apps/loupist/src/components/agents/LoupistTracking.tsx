import type { LoupistBridge } from "../../contracts/bridge";
import type { ExtensionType, SchemaLabels, WorldSchemaView } from "../../contracts/knowledge";
import { Button } from "@/components/ui/button";
import { useTranslation } from "../../i18n";
import { XIcon } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import "../../styles/map.css";

function schemaLabel(labels: SchemaLabels, language: string) {
  return language.toLowerCase().startsWith("ko") && labels.ko ? labels.ko : labels.en;
}

// English shows `en`. Any other language shows that language's blurb, or nothing.
function schemaBlurb(blurbs: SchemaLabels, language: string): string | null {
  const lang = language.toLowerCase();
  if (lang.startsWith("en")) return blurbs.en || null;
  if (lang.startsWith("ko")) return blurbs.ko || null;
  return null;
}

function relationLine(items: Array<{ labels: SchemaLabels }>, language: string) {
  return items.map(item => schemaLabel(item.labels, language)).join(" · ");
}

export function LoupistTracking({ bridge, loupistId }: { bridge: LoupistBridge; loupistId: string }) {
  const { t, i18n } = useTranslation("app");
  const language = i18n.resolvedLanguage ?? i18n.language ?? "en";
  const [view, setView] = useState<WorldSchemaView | null>(null);
  const [trackedLoupist, setTrackedLoupist] = useState(loupistId);
  const [loadError, setLoadError] = useState(false);
  const [actionError, setActionError] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [reload, setReload] = useState(0);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const pendingFocus = useRef<string | null>(null);
  const mounted = useRef(true);
  const loupistRef = useRef(loupistId);
  loupistRef.current = loupistId;
  if (trackedLoupist !== loupistId) {
    setTrackedLoupist(loupistId);
    setView(null);
    setLoadError(false);
    setActionError(false);
    setNotice(null);
    setBusy(false);
  }

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  useEffect(() => {
    let active = true;
    setLoadError(false);
    void bridge.worldSchemaGet({ loupistId }).then(next => {
      if (active) setView(next);
    }).catch(() => { if (active) setLoadError(true); });
    return () => { active = false; };
  }, [bridge, loupistId, reload]);

  useEffect(() => {
    return bridge.onWorldSchemaChanged(event => { if (event.loupistId === loupistId) setReload(value => value + 1); });
  }, [bridge, loupistId]);

  useLayoutEffect(() => {
    const target = pendingFocus.current;
    if (!target) return;
    pendingFocus.current = null;
    const button = target === "heading" ? null : document.getElementById(`loupist-tracking-remove-${target}`);
    (button instanceof HTMLButtonElement ? button : headingRef.current)?.focus();
  }, [view, notice]);

  async function commit(action: () => Promise<WorldSchemaView>, focus: string, message: string | null) {
    if (busy) return;
    const requestedLoupist = loupistId;
    setBusy(true);
    setActionError(false);
    setNotice(null);
    try {
      const next = await action();
      if (!mounted.current || loupistRef.current !== requestedLoupist) return;
      pendingFocus.current = focus;
      setView(next);
      setNotice(message);
    } catch {
      if (mounted.current && loupistRef.current === requestedLoupist) setActionError(true);
    } finally {
      if (mounted.current && loupistRef.current === requestedLoupist) setBusy(false);
    }
  }

  const extension = view?.extension;
  const proposal = view?.proposal ?? null;
  const types = extension?.types ?? [];
  const relations = extension?.relations ?? [];
  const hasExtension = types.length > 0 || relations.length > 0;
  const preparing = proposal?.status === "preparing";
  const pending = proposal?.status === "pending" ? proposal : null;
  const showEmpty = Boolean(view) && !loadError && !hasExtension && !preparing && !pending;

  const removeType = (name: string) => {
    const index = types.findIndex(type => type.name === name);
    const nextFocus = types[index + 1]?.name ?? "heading";
    void commit(() => bridge.worldSchemaRemove({ loupistId, kind: "type", name }), nextFocus, t("tracking.removed"));
  };
  const accept = () => {
    if (!pending) return;
    const list = [...pending.types, ...pending.relations].map(item => schemaLabel(item.labels, language)).join(", ");
    void commit(() => bridge.worldSchemaApply({ loupistId, proposalId: pending.proposalId }), "heading", t("tracking.added", { list }));
  };
  const dismiss = () => {
    if (!pending) return;
    void commit(() => bridge.worldSchemaDismiss({ loupistId, proposalId: pending.proposalId }), "heading", null);
  };

  return <section className="loupist-tracking" aria-labelledby="loupist-tracking-heading">
    <h3 id="loupist-tracking-heading" tabIndex={-1} ref={headingRef}>{t("tracking.heading")}</h3>
    {notice ? <p className="learning-meta" role="status">{notice}</p> : null}
    <p className="learning-meta">{t("tracking.core")}</p>
    {loadError ? <p role="alert">{t("tracking.error")} <Button type="button" variant="link" disabled={busy} onClick={() => { setLoadError(false); setView(null); setReload(value => value + 1); }}>{t("tracking.retry")}</Button></p> : null}
    {actionError ? <p role="alert">{t("tracking.actionError")}</p> : null}
    {view && !loadError ? <>
      {hasExtension ? <>
        <p className="loupist-tracking-intro">{t("tracking.intro")}</p>
        {types.length ? <ul className="loupist-tracking-list">{types.map(type => <TrackingTypeRow key={type.name} type={type} language={language} busy={busy} onRemove={removeType} />)}</ul> : null}
        {relations.length ? <p className="learning-meta">{t("tracking.relations", { list: relationLine(relations, language) })}</p> : null}
      </> : null}
      {preparing ? <p className="learning-meta" role="status">{t("tracking.preparing")}</p> : null}
      {pending ? <div className="loupist-tracking-suggestion">
        <strong>{t(pending.kind === "seed" ? "tracking.suggestionFromFocus" : "tracking.suggestion")}</strong>
        {pending.types.length ? <ul className="loupist-tracking-list">{pending.types.map(type => <TrackingTypeRow key={type.name} type={type} language={language} busy={busy} />)}</ul> : null}
        {pending.relations.length ? <p className="learning-meta">{t("tracking.relations", { list: relationLine(pending.relations, language) })}</p> : null}
        <p className="learning-meta">{t("tracking.appliesNext")}</p>
        <div className="loupist-tracking-actions">
          <Button type="button" size="sm" disabled={busy} onClick={accept}>{t("tracking.add")}</Button>
          <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={dismiss}>{t("tracking.dismiss")}</Button>
        </div>
      </div> : null}
      {showEmpty ? <p className="learning-meta">{t("tracking.empty")}</p> : null}
    </> : null}
  </section>;
}

function TrackingTypeRow({ type, language, busy, onRemove }: { type: ExtensionType; language: string; busy: boolean; onRemove?: (name: string) => void }) {
  const { t } = useTranslation("app");
  const label = schemaLabel(type.labels, language);
  const blurb = schemaBlurb(type.blurbs, language);
  return <li>
    <i className={`map-swatch is-${type.category}`} aria-hidden="true" />
    <div className="loupist-tracking-copy">
      <strong>{label}</strong>
      {blurb ? <span className="loupist-tracking-desc">{blurb}</span> : null}
    </div>
    {onRemove ? <Button type="button" id={`loupist-tracking-remove-${type.name}`} variant="ghost" size="icon-sm" aria-label={t("tracking.remove", { label })} disabled={busy} onClick={() => onRemove(type.name)}><XIcon aria-hidden="true" /></Button> : null}
  </li>;
}
