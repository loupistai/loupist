import { LoupistTemplateFields, existingDraft, templateDraft, type LoupistDraft } from "./LoupistTemplateFields";
import { LoupistIdentityFields } from "./LoupistIdentityFields";
import { NoticeCard } from "./Notice";
import { useEffect, useRef, useState } from "react";
import { ChevronLeftIcon } from "lucide-react";
import { useTranslation } from "@/i18n";
import { Button } from "@/components/ui/button";
import type { LoupistCreateRequest, LoupistBridge, LoupistSummary, ModelCatalog } from "@/types";
import { ConnectStep } from "./ConnectStep";
import { loupistPortraitId } from "@/lib/loupistAvatar";
import { errorMessage } from "@/lib/errors";
import "@/styles/onboarding.css";
import "@/styles/briefings.css";

/**
 * Welcome and notice (1), Connect (2), what to research (3), then the name and face (4).
 * A Loupist that already exists skips step 3 but is still named on step 4.
 */
export function Onboarding({ bridge, catalog, onCatalogChange, onCreate, onComplete, onImport, busy, error, hasLoupist, loupist = null, onLoupistUpdated }: {
  bridge: LoupistBridge; catalog: ModelCatalog | null; onCatalogChange: (catalog: ModelCatalog) => void;
  onCreate: (request: LoupistCreateRequest) => Promise<string | null>; onComplete: () => void | Promise<void>;
  onImport?: () => void;
  busy: boolean; error: string | null; hasLoupist: boolean;
  /** The Loupist that exists before onboarding finishes, which the name step renames. */
  loupist?: LoupistSummary | null;
  onLoupistUpdated?: (loupist: LoupistSummary) => void;
}) {
  const { t } = useTranslation("onboarding");
  const [noticeChecked, setNoticeChecked] = useState(false);
  const [step, setStep] = useState(1);
  const [connected, setConnected] = useState<string | null>(null);
  const [waiting, setWaiting] = useState(false);
  const existing = hasLoupist ? loupist : null;
  const [draft, setDraft] = useState<LoupistDraft>(() => existing ? existingDraft(existing) : templateDraft("portfolio"));
  const finishing = useRef(false);
  const [saving, setSaving] = useState(false);
  const [renameError, setRenameError] = useState<string | null>(null);
  const alreadyConnected = catalog?.providers.some(provider => provider.credential_source !== "missing") || catalog?.models.some(model => model.credential_source !== "missing");
  const next = () => {
    if (!hasLoupist) setStep(3);
    else if (existing) { setDraft(existingDraft(existing)); setStep(4); }
    else void finish();
  };
  const back = (to: number) => <Button type="button" variant="ghost" size="sm" className="onboarding-back" data-onboarding-back disabled={saving || busy} onClick={() => setStep(to)}>
    <ChevronLeftIcon aria-hidden="true" />{t("wizard.back")}
  </Button>;
  /** Renames the existing Loupist when the name or colour changed; false keeps the step open. */
  const rename = async (current: LoupistSummary) => {
    const name = draft.name.trim();
    const avatar = draft.avatar !== loupistPortraitId(current) ? draft.avatar : undefined;
    if (name === current.name && !avatar) return true;
    setRenameError(null);
    try {
      onLoupistUpdated?.(await bridge.loupistProfileUpdate({ loupistId: current.id, name, ...(avatar ? { avatar } : {}) }));
      return true;
    } catch (cause) {
      setRenameError(errorMessage(cause, t("wizard.renameFailed")));
      return false;
    }
  };
  const finish = async () => {
    if (busy || finishing.current) return;
    finishing.current = true;
    setSaving(true);
    try {
      if (existing && step === 4) { if (!await rename(existing)) return; }
      else if (!hasLoupist) {
        const created = await onCreate({ name: draft.name.trim(), focus: draft.focus.trim() || undefined, avatar: draft.avatar, template: draft.template, tone: draft.tone });
        if (!created) return;
        // The picked starters are followed by name in the new Loupist. One that does not follow now can still be followed from Following; onboarding goes on.
        for (const name of draft.starters) await bridge.assetFollowName({ loupistId: created, name }).catch(() => undefined);
      }
      await onComplete();
    } finally {
      finishing.current = false;
      setSaving(false);
    }
  };
  // A new step takes focus at its heading, so the keyboard and a screen reader start there, not on a control that went away.
  const root = useRef<HTMLElement>(null);
  const shownStep = useRef(step);
  useEffect(() => {
    if (shownStep.current === step) return;
    shownStep.current = step;
    root.current?.querySelector<HTMLElement>(".onboarding-step h1")?.focus();
  }, [step]);
  const shownError = renameError ?? error;
  return <main ref={root} className="onboarding-window" data-onboarding={step}><div className="onboarding-column">
    {step === 1 ? <section className="onboarding-step" data-step={step}>
      {/* The product name stays in Latin letters in every language. */}
      <p className="onboarding-wordmark">Loupist</p>
      <h1 tabIndex={-1}>{t("wizard.welcomeTitle")}</h1>
      <p className="onboarding-lead">{t("wizard.welcomeLead")}</p>
      <NoticeCard checked={noticeChecked} onChange={setNoticeChecked} />
      <Button data-onboarding-start disabled={!noticeChecked || saving || busy} onClick={() => alreadyConnected ? next() : setStep(2)}>{t("wizard.start")}</Button>
      {onImport && <Button variant="link" className="onboarding-import" disabled={!noticeChecked || busy} onClick={onImport}>{t("wizard.importLoupist")}</Button>}
    </section> : step === 2 ? <section className="onboarding-step" data-step={step}>{back(1)}<ConnectStep bridge={bridge} catalog={catalog} onCatalogChange={onCatalogChange} connected={connected}
      onConnected={name => { setConnected(name); next(); }} onWaitingChange={setWaiting} />
      {/* No plan, key or local model yet: the app still opens, and the composer asks to connect. */}
      {connected ? null : <div className="onboarding-skip">
        <Button type="button" variant="ghost" className="onboarding-skip-button" data-onboarding-skip disabled={waiting || saving || busy} onClick={next}>{t("wizard.skipConnect")}</Button>
        <p>{t("wizard.skipConnectNote")}</p>
      </div>}
    </section> : step === 3 ? <section className="onboarding-step" data-step={step}>{back(alreadyConnected ? 1 : 2)}<h1 tabIndex={-1}>{t("wizard.createTitle")}</h1><p>{t("wizard.createLead")}</p>
      <LoupistTemplateFields draft={draft} onChange={setDraft} busy={saving || busy} idPrefix="onboarding" essentials />
      <Button data-onboarding-next disabled={saving || busy} onClick={() => setStep(4)}>{t("wizard.next")}</Button>
    </section> : <section className="onboarding-step" data-step={step}>{back(existing ? (alreadyConnected ? 1 : 2) : 3)}<h1 tabIndex={-1}>{t("wizard.nameTitle")}</h1><p>{t("wizard.nameLead")}</p>
      <LoupistIdentityFields id={existing?.id} name={draft.name} avatar={draft.avatar} focus={draft.focus} busy={saving || busy} idPrefix="onboarding"
        onChange={identity => { setDraft(current => ({ ...current, ...identity })); setRenameError(null); }} />
      <Button data-onboarding-create disabled={!draft.name.trim() || saving || busy} onClick={() => void finish()}>{t(saving ? (existing ? "wizard.saving" : "wizard.creating") : existing ? "wizard.saveName" : "wizard.create")}</Button>
      {existing ? null : <p className="onboarding-spending" data-background-reading>{t("wizard.backgroundReading")}</p>}
    </section>}
    {shownError && <p className="onboarding-error" role="alert">{shownError}</p>}
  </div></main>;
}
