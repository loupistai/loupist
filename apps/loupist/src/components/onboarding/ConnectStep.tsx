import { useCallback, useEffect, useState } from "react";
import { CheckCircleIcon } from "lucide-react";
import { useTranslation } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Disclosure } from "@/components/ui/disclosure";
import { ClaudeExtraNote, ConnectSheet, connectedName, type ConnectSheetTarget } from "@/components/settings/ConnectSheet";
import { ProviderWays } from "@/components/settings/ProviderWays";
import { SignInFlow } from "@/components/settings/SignInFlow";
import { useSignIn } from "@/components/settings/signIn";
import type { LoupistBridge, ModelCatalog } from "@/types";
import "@/styles/connect.css";

/**
 * Onboarding step 2 (providers spec §2.1): one primary ChatGPT sign-in, and
 * every other way behind a disclosure. Sign-in rows run in place; key, local
 * and custom rows open the shared connect sheet.
 */
export function ConnectStep({ bridge, catalog, onCatalogChange, connected, onConnected, onWaitingChange }: {
  bridge: LoupistBridge;
  catalog: ModelCatalog | null;
  onCatalogChange: (catalog: ModelCatalog) => void;
  /** The connected provider's name while the step holds before moving on. */
  connected: string | null;
  onConnected: (label: string) => void;
  onWaitingChange: (waiting: boolean) => void;
}) {
  const { t } = useTranslation("onboarding");
  const [otherOpen, setOtherOpen] = useState(false);
  const [sheet, setSheet] = useState<ConnectSheetTarget | null>(null);
  /** Claude's billing note comes before its browser opens. */
  const [claudeConfirm, setClaudeConfirm] = useState(false);
  const accept = useCallback((provider: string, next: ModelCatalog) => {
    onCatalogChange(next);
    onConnected(connectedName(next, provider));
  }, [onCatalogChange, onConnected]);
  const flow = useSignIn(bridge, accept);
  useEffect(() => { onWaitingChange(flow.busy); }, [flow.busy, onWaitingChange]);
  // Escape cancels a waiting sign-in, the same as Cancel; an open sheet handles its own Escape.
  useEffect(() => {
    if (!flow.busy || sheet) return;
    const onKey = (event: KeyboardEvent) => { if (event.key === "Escape" && !event.defaultPrevented) { event.preventDefault(); void flow.cancel(); } };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [flow, sheet]);
  const signIn = (provider: string) => {
    setClaudeConfirm(false);
    if (provider === "anthropic") { flow.dismiss(); setClaudeConfirm(true); return; }
    void flow.start(provider);
  };
  const locked = flow.busy || Boolean(connected);
  return <>
    <h1>{t("wizard.connectTitle")}</h1>
    <p>{t("wizard.connectBody")}</p>
    {connected ? <div className="onboarding-success" role="status"><CheckCircleIcon aria-hidden="true" />{t("wizard.connected", { provider: connected })}</div>
      : flow.state.phase === "waiting" ? <SignInFlow flow={flow} />
        : claudeConfirm ? <div className="signin-card" data-signin="confirm">
          <ClaudeExtraNote />
          <div className="signin-actions">
            <Button type="button" data-claude-continue onClick={() => { setClaudeConfirm(false); void flow.start("anthropic"); }}>{t("wizard.continue")}</Button>
            <Button type="button" variant="ghost" onClick={() => setClaudeConfirm(false)}>{t("wizard.cancel")}</Button>
          </div>
        </div>
          : <div className="connect-primary-block">
            <Button className="onboarding-primary" data-onboarding-connect onClick={() => signIn("openai")}>{t("wizard.chatgpt")}</Button>
            <p className="connect-meta onboarding-connect-note">{t("wizard.chatgptNote")}</p>
          </div>}
    {flow.state.phase === "error" ? <SignInFlow flow={flow} onUseApiKey={() => { flow.dismiss(); setSheet({ kind: "provider", provider: "openai", method: "api_key" }); }} /> : null}
    <Disclosure className="onboarding-other" title={t("wizard.other")} open={otherOpen || Boolean(sheet)} onOpenChange={setOtherOpen}>
      <ProviderWays catalog={catalog} includePrimary={false} includeMore={false} disabled={locked} actions={{
        onSignIn: signIn,
        onKey: provider => setSheet({ kind: "provider", provider, method: "api_key" }),
        onUpstage: () => setSheet({ kind: "upstage" }),
        onLocal: () => setSheet({ kind: "local" }),
        onCustom: () => setSheet({ kind: "custom" }),
      }} />
      <p className="connect-meta connect-key-stays">{t("wizard.keyStays")}</p>
    </Disclosure>
    <ConnectSheet target={sheet} catalog={catalog} bridge={bridge} onCatalogChange={onCatalogChange}
      onConnected={(_provider, label) => onConnected(label)} onClose={() => setSheet(null)} />
  </>;
}
