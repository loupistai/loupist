import { templateDraft } from "./LoupistTemplateFields";
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
  const finishing = useRef(false);
  const [saving, setSaving] = useState(false);
  const alreadyConnected = catalog?.providers.some(provider => provider.credential_source !== "missing") || catalog?.models.some(model => model.credential_source !== "missing");
  const finish = async () => {
    if (busy || finishing.current) return;
    finishing.current = true;
    setSaving(true);
    try {
      if (!hasLoupist && !await onCreate(templateDraft("portfolio"))) return;
      await onComplete();
    } finally {
      finishing.current = false;
      setSaving(false);
    }
  };
  return <main className="onboarding-window" data-onboarding={step}><div className="onboarding-column">
    {step === 1 ? <section className="onboarding-step"><h1>{t("wizard.welcomeTitle")}</h1><p>{t("wizard.welcomeLoupist")}</p>
      <NoticeCard checked={noticeChecked} onChange={setNoticeChecked} />
      <Button data-onboarding-start disabled={!noticeChecked || saving || busy} onClick={() => alreadyConnected ? void finish() : setStep(2)}>{t("wizard.start")}</Button>
      {onImport && <Button variant="link" disabled={!noticeChecked || busy} onClick={onImport}>{t("wizard.importLoupist")}</Button>}
    </section> : <section className="onboarding-step"><ConnectStep bridge={bridge} catalog={catalog} onCatalogChange={onCatalogChange} connected={connected}
      onConnected={name => { setConnected(name); void finish(); }} onWaitingChange={() => {}} />
      {connected && error && <Button disabled={saving} onClick={() => void finish()}>{t("wizard.start")}</Button>}
    </section>}
    {error && <p role="alert">{error}</p>}
  </div></main>;
}
