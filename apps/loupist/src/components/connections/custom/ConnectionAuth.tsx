import { useEffect, useRef, useState } from "react";
import type { CustomConnection, CustomConnectionsBridge } from "../../../contracts/customConnections";
import { useTranslation } from "../../../i18n";
import { Button } from "../../ui/button";
import { Input } from "../../ui/input";
import { Disclosure } from "../../ui/disclosure";
import { Spinner } from "../../ui/spinner";

export function ConnectionAuth({ bridge, connection, onChanged, autoStart = false, reauthenticate = false }: { bridge: CustomConnectionsBridge; connection: CustomConnection; onChanged: (record: CustomConnection) => void; autoStart?: boolean; reauthenticate?: boolean }) {
  const { t } = useTranslation("settings");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [issuer, setIssuer] = useState("");
  const [clientId, setClientId] = useState("");
  const [secret, setSecret] = useState("");
  const running = useRef(false);
  const revision = useRef(0);
  const inflight = useRef<Promise<unknown>>(undefined);
  const mounted = useRef(true);
  const started = useRef(false);
  const [browserFlow, setBrowserFlow] = useState(false);
  const waiting = browserFlow || connection.auth_state === "in_progress";
  async function signIn(advanced = false) {
    const requestRevision = ++revision.current;
    running.current = true; setBusy(true); setError(false);
    if (!advanced) setBrowserFlow(true);
    const request = { connection_id: connection.id, issuer, client_id: clientId, client_secret: secret || undefined };
    setSecret("");
    try {
      const call = advanced ? bridge.customConnectionAuthorize(request) : bridge.customConnectionSignIn({ connection_id: connection.id });
      inflight.current = call.catch(() => {});
      const next = await call;
      if (mounted.current && revision.current === requestRevision) { onChanged(next); setError(next.auth_state === "sign_in_needed"); }
    } catch { if (mounted.current && revision.current === requestRevision) setError(true); }
    finally { if (revision.current === requestRevision) { running.current = false; if (mounted.current) { setBusy(false); if (!advanced) setBrowserFlow(false); } } }
  }
  useEffect(() => {
    mounted.current = true;
    if (autoStart && !started.current && connection.auth_state === "sign_in_needed") { started.current = true; void signIn(); }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- a request counter, not a DOM node: cleanup invalidates the request that is in flight.
    return () => { mounted.current = false; revision.current++; if (running.current) void bridge.customConnectionSignInCancel({ connection_id: connection.id }).catch(() => {}); };
    // Each connection owns exactly one browser flow; changing display state must not cancel it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bridge, connection.id]);
  async function cancel() {
    revision.current++;
    setBrowserFlow(false);
    try { await bridge.customConnectionSignInCancel({ connection_id: connection.id }); running.current = false; setBusy(false); setError(false); onChanged({ ...connection, auth_state: "sign_in_needed" }); }
    catch { setError(true); }
  }
  async function reopen() {
    // The host keeps one browser flow per connection: end it, let it settle, then start a fresh one.
    revision.current++;
    try { await bridge.customConnectionSignInCancel({ connection_id: connection.id }); }
    catch { setError(true); return; }
    await inflight.current;
    if (mounted.current) void signIn();
  }
  return <div className="custom-stack" data-custom-auth>
    {waiting ? <div className="custom-stack" role="status" data-custom-signing-in><p className="custom-waiting"><Spinner />{t("custom.signingIn", { service: connection.name })}</p><Button variant="link" className="flush-start" onClick={() => void reopen()}>{t("custom.openSignInAgain")}</Button><Button variant="secondary" onClick={() => void cancel()}>{t("custom.cancel")}</Button></div> : <Button variant="outline" onClick={() => void signIn()}>{t(reauthenticate || connection.auth_state === "signed_in" ? "custom.signInAgain" : "custom.signIn")}</Button>}
    {error ? <p role="alert" className="custom-status negative">{t("custom.error")}</p> : null}
    {waiting ? null : <Disclosure title={t("custom.advanced")}><div className="custom-stack"><p className="custom-meta">{t("custom.clientCaption")}</p>
      <label className="custom-field">{t("custom.issuer")}<Input type="url" value={issuer} disabled={waiting || busy} onChange={event => setIssuer(event.target.value)} /></label>
      <label className="custom-field">{t("custom.clientId")}<Input value={clientId} disabled={waiting || busy} onChange={event => setClientId(event.target.value)} /></label>
      <label className="custom-field">{t("custom.clientSecret")}<Input type="password" autoComplete="new-password" value={secret} disabled={waiting || busy} onChange={event => setSecret(event.target.value)} /></label>
      <Button variant="outline" disabled={waiting || busy || !issuer.trim() || !clientId.trim()} onClick={() => void signIn(true)}>{t("custom.authorize")}</Button>
    </div></Disclosure>}
  </div>;
}
