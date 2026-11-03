import { useEffect, useId, useState } from "react";
import { LoaderCircleIcon } from "lucide-react";
import { useTranslation } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { providerKeyName, providerShortName } from "@/chat/providerNames";
import type { ProviderConnectFailure } from "@/types";
import type { SignInController } from "./signIn";
import "@/styles/connect.css";

/** Copy for a typed connect failure; `warning` rows are recoverable by the user (spec providers §2.1). */
type FailureKey = "wizard.declined" | "wizard.connectError" | "wizard.notEligible" | "wizard.portBusy" | "wizard.timedOut" | "wizard.offline";
export function signInFailureCopy(provider: string, reason: ProviderConnectFailure | null): { key: FailureKey; warning: boolean } {
  switch (reason) {
    case "plan_permission_declined": return { key: "wizard.declined", warning: true };
    case "denied": return provider === "openai" ? { key: "wizard.declined", warning: true } : { key: "wizard.connectError", warning: false };
    case "plan_not_eligible": return { key: "wizard.notEligible", warning: true };
    case "port_in_use": return { key: "wizard.portBusy", warning: true };
    case "timed_out": return { key: "wizard.timedOut", warning: true };
    case "offline": return { key: "wizard.offline", warning: false };
    default: return { key: "wizard.connectError", warning: false };
  }
}

/**
 * The waiting card, device code card and error row of one sign-in. The owner
 * renders the start button; this replaces it while the flow runs.
 */
export function SignInFlow({ flow, onUseApiKey }: { flow: SignInController; onUseApiKey?: () => void }) {
  const { state } = flow;
  if (state.phase === "waiting" && state.device) return <DeviceCard flow={flow} provider={state.provider} code={state.device.userCode} />;
  if (state.phase === "waiting") return <WaitingCard flow={flow} />;
  if (state.phase === "error") return <SignInError flow={flow} onUseApiKey={onUseApiKey} />;
  return null;
}

function WaitingCard({ flow }: { flow: SignInController }) {
  const { t } = useTranslation("onboarding");
  const id = useId();
  const state = flow.state.phase === "waiting" ? flow.state : null;
  const [pasteOpen, setPasteOpen] = useState(false);
  const [value, setValue] = useState("");
  const [pasteError, setPasteError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [copied, setCopied] = useState(false);
  if (!state) return null;
  const name = providerShortName(state.provider) ?? state.provider;
  const submit = async () => {
    if (!value.trim() || submitting) return;
    setSubmitting(true); setPasteError(null);
    const outcome = await flow.submitCode(value);
    setSubmitting(false);
    if (outcome === "invalid") setPasteError(t("wizard.pasteInvalid"));
    else if (outcome === "failed") setPasteError(t("wizard.connectError"));
  };
  const copyLink = async () => {
    if (!state.url) return;
    try { await navigator.clipboard.writeText(state.url); setCopied(true); } catch { setCopied(false); }
  };
  // Only the status line is announced; the actions and the paste form are not live.
  return <div className="signin-card" data-signin="waiting">
    <p className="signin-title" role="status" aria-live="polite"><LoaderCircleIcon className="signin-spinner animate-spin" aria-hidden="true" />{t("wizard.waitingTitle", { provider: name })}</p>
    <p className="signin-body">{t(state.provider === "openai" ? "wizard.waitingBody" : "wizard.waitingBodyOther")}</p>
    <div className="signin-actions">
      <Button type="button" variant="outline" onClick={() => void flow.openAgain().catch(() => {})}>{t("wizard.openAgain")}</Button>
      {!state.opened && state.url ? <Button type="button" variant="outline" data-copy-link onClick={() => void copyLink()}>{t("wizard.copyLink")}</Button> : null}
      <Button type="button" variant="ghost" data-signin-cancel onClick={() => void flow.cancel()}>{t("wizard.cancel")}</Button>
    </div>
    {copied ? <span className="sr-only">{t("wizard.linkCopied")}</span> : null}
    {state.pasteAvailable && state.manual && !pasteOpen ? <button type="button" className="signin-link" data-paste-toggle onClick={() => setPasteOpen(true)}>{t("wizard.pasteToggle")}</button> : null}
    {state.pasteAvailable && state.manual && pasteOpen ? <form className="signin-paste" onSubmit={event => { event.preventDefault(); void submit(); }}>
      <Label htmlFor={`${id}-paste`}>{t("wizard.pasteLabel")}</Label>
      <div className="signin-paste-row">
        <Input id={`${id}-paste`} autoFocus value={value} maxLength={2048} autoComplete="off" spellCheck={false} aria-invalid={pasteError ? true : undefined} aria-describedby={pasteError ? `${id}-paste-error` : undefined} onChange={event => { setValue(event.target.value); setPasteError(null); }} />
        <Button type="submit" disabled={!value.trim() || submitting}>{t("wizard.continue")}</Button>
      </div>
      {pasteError ? <p id={`${id}-paste-error`} className="signin-paste-error" role="alert">{pasteError}</p> : null}
    </form> : null}
  </div>;
}

function DeviceCard({ flow, provider, code }: { flow: SignInController; provider: string; code: string }) {
  const { t } = useTranslation("onboarding");
  const [copied, setCopied] = useState(false);
  useEffect(() => setCopied(false), [code]);
  const copy = async () => {
    try { await navigator.clipboard.writeText(code); setCopied(true); } catch { setCopied(false); }
  };
  return <div className="signin-card" data-signin="device">
    <p className="signin-title" role="status" aria-live="polite"><LoaderCircleIcon className="signin-spinner animate-spin" aria-hidden="true" />{t("wizard.deviceCodeTitle", { provider: providerKeyName(provider, provider) })}</p>
    {/* An output is a status: its label, spelled character by character, is announced. */}
    <output className="signin-device-code gg-num" aria-label={code.split("").join(" ")}>{code}</output>
    <div className="signin-actions">
      <Button type="button" variant="outline" data-copy-code onClick={() => void copy()}>{t("wizard.copyCode")}</Button>
      <Button type="button" variant="ghost" onClick={() => void flow.openAgain().catch(() => {})}>{t("wizard.openPageAgain")}</Button>
      <Button type="button" variant="ghost" data-signin-cancel onClick={() => void flow.cancel()}>{t("wizard.cancel")}</Button>
    </div>
    <span className="sr-only" aria-live="polite">{copied ? t("wizard.codeCopied") : ""}</span>
  </div>;
}

function SignInError({ flow, onUseApiKey }: { flow: SignInController; onUseApiKey?: () => void }) {
  const { t } = useTranslation("onboarding");
  if (flow.state.phase !== "error") return null;
  const { provider, reason, message } = flow.state;
  const copy = signInFailureCopy(provider, reason);
  const text = reason || !message ? t(copy.key) : message;
  return <div className="signin-error" role="alert" data-tone={copy.warning ? "warning" : "negative"} data-reason={reason ?? "other"}>
    <p>{text}</p>
    <div className="signin-error-actions">
      {reason === "plan_not_eligible" && onUseApiKey ? <Button type="button" size="sm" onClick={onUseApiKey}>{t("wizard.useApiKey")}</Button> : null}
      <Button type="button" size="sm" variant="ghost" onClick={() => void flow.start(provider)}>{t("wizard.retry")}</Button>
    </div>
  </div>;
}
