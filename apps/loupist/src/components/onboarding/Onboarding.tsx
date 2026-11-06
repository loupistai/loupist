import { LoupistTemplateFields, templateDraft, type LoupistDraft } from "./LoupistTemplateFields";
import { NoticeCard } from "./Notice";
import { useRef, useState } from "react";
import { useTranslation } from "@/i18n";
import { Button } from "@/components/ui/button";
import type { LoupistCreateRequest, LoupistBridge, ModelCatalog } from "@/types";
import { ConnectStep } from "./ConnectStep";
import "@/styles/onboarding.css";
import "@/styles/briefings.css";

export function Onboarding({ bridge, catalog, onCatalogChange, onCreate, onComplete, onImport, busy, error, hasLoupist }: {
  bridge: LoupistBridge; catalog: ModelCatalog | null; onCatalogChange: (catalog: ModelCatalog) => void;
  onCreate: (request: LoupistCreateRequest) => Promise<string | null>; onComplete: () => void | Promise<void>;
  onImport?: () => void;
  busy: boolean; error: string | null; hasLoupist: boolean;
}) {
  const { t } = useTranslation("onboarding");
  const [noticeChecked, setNoticeChecked] = useState(false);
  const [step, setStep] = useState(1);
  const [connected, setConnected] = useState<string | null>(null);
  const [draft, setDraft] = useState<LoupistDraft>(() => templateDraft("portfolio"));
  const finishing = useRef(false);
  const [saving, setSaving] = useState(false);
  const alreadyConnected = catalog?.providers.some(provider => provider.credential_source !== "missing") || catalog?.models.some(model => model.credential_source !== "missing");
  const next = () => { if (hasLoupist) void finish(); else setStep(3); };
  const finish = async () => {
    if (busy || finishing.current) return;
    finishing.current = true;
    setSaving(true);
    try {
      if (!hasLoupist && !await onCreate({ name: draft.name.trim(), focus: draft.focus.trim() || undefined, avatar: draft.avatar, template: draft.template, tone: draft.tone })) return;
      await onComplete();
    } finally {
      finishing.current = false;
      setSaving(false);
    }
  };
  return <main className="onboarding-window" data-onboarding={step}><div className="onboarding-column">
    {step === 1 ? <section className="onboarding-step" data-step={step}><h1>{t("wizard.welcomeTitle")}</h1><p>{t("wizard.welcomeLoupist")}</p>
      <NoticeCard checked={noticeChecked} onChange={setNoticeChecked} />
      <Button data-onboarding-start disabled={!noticeChecked || saving || busy} onClick={() => alreadyConnected ? next() : setStep(2)}>{t("wizard.start")}</Button>
      {onImport && <Button variant="link" disabled={!noticeChecked || busy} onClick={onImport}>{t("wizard.importLoupist")}</Button>}
    </section> : step === 2 ? <section className="onboarding-step" data-step={step}><ConnectStep bridge={bridge} catalog={catalog} onCatalogChange={onCatalogChange} connected={connected}
      onConnected={name => { setConnected(name); next(); }} onWaitingChange={() => {}} />
    </section> : <section className="onboarding-step" data-step={step}><h1>{t("wizard.createTitle")}</h1><p>{t("wizard.loupistDefinition")}</p>
      <LoupistTemplateFields draft={draft} onChange={setDraft} busy={saving || busy} idPrefix="onboarding" />
      <Button data-onboarding-create disabled={!draft.name.trim() || saving || busy} onClick={() => void finish()}>{t(saving ? "wizard.creating" : "wizard.create")}</Button>
    </section>}
    {error && <p className="onboarding-error" role="alert">{error}</p>}
  </div></main>;
}
