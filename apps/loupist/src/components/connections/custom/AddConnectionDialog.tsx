import { ConnectionAuth } from "./ConnectionAuth";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "../../../i18n";
import type { CustomConnection, CustomConnectionsBridge } from "../../../contracts/customConnections";
import { Button } from "../../ui/button";
import { Input } from "../../ui/input";
import { Textarea } from "../../ui/textarea";
import { Tabs, TabsList, TabsTrigger } from "../../ui/tabs";
import { Disclosure } from "../../ui/disclosure";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogBody, DialogFooter } from "../../ui/dialog";
import { ToolReview, reviewComplete } from "./ToolReview";

export function AddConnectionDialog({ bridge, loupistId, onClose, onChanged }: { bridge: CustomConnectionsBridge; loupistId: string; onClose: () => void; onChanged: () => void }) {
  const { t } = useTranslation("settings");
  const [mode, setMode] = useState("url");
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");
  const [json, setJson] = useState("");
  const [command, setCommand] = useState("");
  const [args, setArgs] = useState("");
  const [env, setEnv] = useState<Array<{ key: string; value: string }>>([]);
  const [pending, setPending] = useState<CustomConnection[]>([]);
  const [review, setReview] = useState<CustomConnection | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const retry = useRef<(() => Promise<void>) | null>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const [bodyFades, setBodyFades] = useState(false);
  const phase = !review ? (pending.length ? "staged" : mode) : review.auth_state === "sign_in_needed" || review.auth_state === "in_progress" ? "auth" : review.pending ? "pending" : "tools";
  const focusDialogAction = (root: HTMLElement | null) => {
    if (!root) return false;
    const buttons = [...root.querySelectorAll<HTMLButtonElement>("button:not([disabled])")].filter(button => !button.closest("[data-slot='dialog-close']") && button.getAttribute("role") !== "tab");
    const labeled = (label: string) => buttons.find(button => button.textContent?.replace(/\s+/g, " ").trim() === label);
    // Review steps open on their title, never on a permission choice, the way out, or the bare dialog.
    const target = phase === "staged" || phase === "auth" || phase === "tools" ? root.querySelector<HTMLElement>("[data-slot='dialog-title']") : labeled(t("custom.cancel"));
    target?.focus();
    return Boolean(target);
  };
  useEffect(() => {
    const node = bodyRef.current;
    if (!node) return;
    const update = () => setBodyFades(node.scrollHeight - node.clientHeight - node.scrollTop > 8);
    update();
    node.addEventListener("scroll", update, { passive: true });
    const observer = typeof ResizeObserver === "function" ? new ResizeObserver(update) : null;
    observer?.observe(node);
    return () => { node.removeEventListener("scroll", update); observer?.disconnect(); };
  }, [pending, mode, review, error]);
  useEffect(() => {
    if (phase !== "staged" && phase !== "auth" && phase !== "tools") return;
    const root = () => contentRef.current ?? document.querySelector<HTMLElement>(".custom-add");
    let observer: MutationObserver | undefined;
    const frame = requestAnimationFrame(() => {
      if (focusDialogAction(root())) return;
      const node = root();
      if (!node) return;
      observer = new MutationObserver(() => { if (focusDialogAction(node)) observer?.disconnect(); });
      observer.observe(node, { childList: true, subtree: true });
    });
    return () => { cancelAnimationFrame(frame); observer?.disconnect(); };
  }, [phase]);
  async function run(action: () => Promise<void>) {
    retry.current = action;
    setBusy(true); setError(false);
    try { await action(); onChanged(); } catch { setError(true); } finally { setBusy(false); }
  }
  async function confirm(record: CustomConnection) {
    retry.current = () => confirm(record);
    try {
      const connected = await bridge.customConnectionConfirm({ connection_id: record.id, ...(loupistId ? { loupist_id: loupistId } : {}) });
      setPending(previous => previous.filter(item => item.id !== record.id));
      setReview(connected);
    } catch {
      setReview(record);
      throw new Error("Connection could not be confirmed");
    }
  }
  async function submit() {
    await run(async () => {
      if (mode === "url") {
        const record = await bridge.customConnectionAddUrl({ url, name: name.trim() || undefined });
        await confirm(record);
      } else if (mode === "json") {
        const records = await bridge.customConnectionAddJson({ json });
        setPending(records);
      } else {
        const record = await bridge.customConnectionAddCommand({ command, args: args.split("\n").filter(Boolean), env: Object.fromEntries(env.filter(row => row.key.trim()).map(row => [row.key.trim(), row.value])) });
        setEnv([]); setCommand(""); setArgs(""); setPending([record]);
      }
    });
  }
  async function returnToPaste() {
    const staged = [...pending];
    await run(async () => {
      for (const record of staged) await bridge.customConnectionRemove({ connection_id: record.id });
      setPending([]);
    });
  }
  return <Dialog open onOpenChange={open => { if (!open) onClose(); }}><DialogContent ref={contentRef} size="md" className="custom-add" aria-describedby={undefined} onOpenAutoFocus={event => { event.preventDefault(); const root = event.currentTarget as HTMLElement; if (!focusDialogAction(root)) root.querySelector<HTMLElement>("input, textarea")?.focus(); }}>
    <DialogHeader><DialogTitle>{t(review && !review.pending && review.auth_state !== "in_progress" && review.auth_state !== "sign_in_needed" ? "custom.reviewTools" : "custom.addTitle")}</DialogTitle></DialogHeader>
    <div className="custom-scroll">
    <DialogBody ref={bodyRef} className={bodyFades ? "custom-stack custom-body-fade" : "custom-stack"}>
      {error ? <p role="alert" className="custom-status negative">{t("custom.error")} <Button variant="link" disabled={busy} onClick={() => { if (retry.current) void run(retry.current); else setError(false); }}>{t("custom.tryAgain")}</Button></p> : null}
      {!loupistId ? <p className="custom-status warning">{t("custom.chooseLoupist")}</p> : null}
      {review ? <>
        {review.auth_state === "sign_in_needed" || review.auth_state === "in_progress" ? <ConnectionAuth bridge={bridge} connection={review} autoStart onChanged={next => {
          // A finished sign-in confirms for the Loupist that opened Add, so tool approval is possible.
          if (next.auth_state === "signed_in" || next.auth_state === "not_required") void run(() => confirm(next));
          else setReview(next);
        }} /> : review.pending ? <p role="alert" className="custom-status negative">{t("custom.failed")} <Button variant="link" disabled={busy} onClick={() => void run(() => confirm(review))}>{t("custom.tryAgain")}</Button></p> : <ToolReview bridge={bridge} connection={review} loupistId={loupistId} onChanged={setReview} onError={() => { retry.current = null; setError(true); }} />}
      </> : <>
        <Tabs value={mode === "command" ? "" : mode} onValueChange={value => { setMode(value); setPending([]); }}><TabsList aria-label={t("custom.method")}><TabsTrigger value="url"><span>{t("custom.serverUrl")}</span><small>{t("custom.recommended")}</small></TabsTrigger><TabsTrigger value="json">{t("custom.pasteJson")}</TabsTrigger></TabsList></Tabs>
        {pending.length ? <><p className="custom-meta">{t("custom.nothingRuns")}</p><ul className="custom-tools">{pending.map(record => {
          const local = record.transport.kind === "stdio";
          return <li className="custom-tool" key={record.id}><strong>{record.name}</strong><span className="custom-meta">{t(local ? "custom.localCommand" : "custom.server")}</span><p className="custom-meta">{t("custom.envNames")}: {record.env_keys.join(", ") || t("custom.none")}</p><p className="custom-meta">{t("custom.headerNames")}: {record.header_names.join(", ") || t("custom.none")}</p>{local && record.command_line ? <pre className="custom-command">{record.command_line}</pre> : null}{record.transport.kind === "http" ? <pre className="custom-command">{record.transport.url}</pre> : null}{local ? <p className="custom-status warning">{t("custom.commandWarning")}</p> : null}<Button disabled={busy} onClick={() => void run(() => confirm(record))}>{t("custom.add")}</Button></li>;
        })}</ul></> : <>
          {mode === "url" ? <><label className="custom-field">{t("custom.serverUrl")}<Input type="url" placeholder={t("custom.urlPlaceholder", { scheme: "https://" })} value={url} onChange={event => setUrl(event.target.value)} /></label><label className="custom-field">{t("custom.optionalName")}<Input value={name} onChange={event => setName(event.target.value)} /></label></> : null}
          {mode === "json" ? <><label className="custom-field">{t("custom.pasteJson")}<Textarea className="custom-code-input" rows={8} value={json} onChange={event => setJson(event.target.value)} /></label><p className="custom-meta">{t("custom.nothingRuns")}</p></> : null}
          <Disclosure title={t("custom.advanced")}><div className="custom-stack"><Button variant="outline" onClick={() => setMode("command")}>{t("custom.localCommand")}</Button>
            {mode === "command" ? <><p className="custom-status warning">{t("custom.commandWarning")}</p><label className="custom-field">{t("custom.command")}<Input value={command} onChange={event => setCommand(event.target.value)} /></label><label className="custom-field">{t("custom.args")}<Textarea rows={3} value={args} onChange={event => setArgs(event.target.value)} /></label><div className="custom-stack"><span>{t("custom.environment")}</span>{env.map((row, index) => <div className="custom-env-row" key={index}><Input aria-label={t("custom.envKey", { number: index + 1 })} value={row.key} onChange={event => setEnv(rows => rows.map((item, i) => i === index ? { ...item, key: event.target.value } : item))} /><Input type="password" autoComplete="new-password" aria-label={t("custom.envValue", { number: index + 1 })} value={row.value} onChange={event => setEnv(rows => rows.map((item, i) => i === index ? { ...item, value: event.target.value } : item))} /></div>)}<Button variant="outline" onClick={() => setEnv(rows => [...rows, { key: "", value: "" }])}>{t("custom.addVariable")}</Button></div></> : null}
          </div></Disclosure>
        </>}
      </>}
    </DialogBody>
    {bodyFades ? <div className="custom-scroll-fade" aria-hidden="true" /> : null}
    </div>
    <DialogFooter>{review ? <Button disabled={busy || !loupistId || review.pending || review.auth_state === "in_progress" || review.auth_state === "sign_in_needed" || !reviewComplete(review)} onClick={() => { void run(async () => { if (pending.length) setReview(null); else onClose(); }); }}>{t("custom.done")}</Button> : pending.length && mode === "json" ? <Button type="button" variant="outline" disabled={busy} onClick={() => void returnToPaste()}>{t("custom.back")}</Button> : !pending.length ? <Button disabled={busy || (mode === "url" ? !url.trim() : mode === "json" ? !json.trim() : !command.trim())} onClick={() => void submit()}>{t(busy ? "custom.connecting" : mode === "url" ? "custom.connect" : "custom.review")}</Button> : null}</DialogFooter>
  </DialogContent></Dialog>;
}
