import { useEffect, useId, useRef, useState } from "react";
import type { LoupistBridge, Principle, PrincipleCreateRequest } from "../../types";
import { useTranslation } from "../../i18n";
import { formatNumber } from "../../i18n/format";
import { Button } from "../ui/button";
import { commandErrorCode } from "@/lib/errors";

export function principleErrorCode(error: unknown) {
  return commandErrorCode(error) ?? "";
}

export function PrincipleForm({ bridge, loupistId, principle, initialStatement = "", origin = "settings", originChatId, originMessageId, onSaved, onCancel }: {
  bridge: LoupistBridge; loupistId: string; principle?: Principle; initialStatement?: string;
  origin?: PrincipleCreateRequest["origin"]; originChatId?: string; originMessageId?: string;
  onSaved: (principle: Principle) => void; onCancel: () => void;
}) {
  const { t } = useTranslation("knowledge");
  const id = useId();
  const [current, setCurrent] = useState(principle);
  const [statement, setStatement] = useState(principle?.statement ?? initialStatement);
  const [why, setWhy] = useState(principle?.why ?? "");
  const [example, setExample] = useState(principle?.example ?? "");
  const [pending, setPending] = useState(false);
  const [error, setFailure] = useState("");
  const busy = useRef(false);
  const formRef = useRef<HTMLFormElement>(null);
  useEffect(() => {
    const form = formRef.current;
    if (!form) return;
    // The sticky action bar covers a field the browser still treats as visible, so autofocus does not scroll. scroll-margin-bottom applies once we ask.
    const reveal = () => form.querySelector<HTMLElement>("textarea:focus, input:focus")?.scrollIntoView({ block: "nearest" });
    reveal();
    const panel = form.closest("[role=tabpanel]");
    const observer = new ResizeObserver(reveal);
    if (panel) observer.observe(panel);
    return () => observer.disconnect();
  }, []);
  const count = [...statement].length;
  const valid = [...statement.trim()].length > 0 && count <= 280 && [...why].length <= 500 && [...example].length <= 500;
  const conflict = error === "conflict" || error === "revision_conflict";
  async function save() {
    if (!valid || busy.current || conflict) return;
    busy.current = true; setPending(true); setFailure("");
    try {
      const fields = { statement: statement.trim(), why: why.trim(), example: example.trim() };
      const saved = current
        ? await bridge.principleUpdate({ loupistId, recordId: current.recordId, expectedRevisionId: current.revisionId, ...fields })
        : await bridge.principleCreate({ loupistId, origin, originChatId, originMessageId, ...fields });
      onSaved(saved);
    } catch (cause) { setFailure(principleErrorCode(cause) || "failed"); }
    finally { busy.current = false; setPending(false); }
  }
  async function reload() {
    if (!current || busy.current) return;
    busy.current = true; setPending(true);
    try {
      const fresh = (await bridge.principleList({ loupistId, includeArchived: true })).find(item => item.recordId === current.recordId);
      if (!fresh) throw new Error("Missing principle");
      setCurrent(fresh); setStatement(fresh.statement); setWhy(fresh.why ?? ""); setExample(fresh.example ?? ""); setFailure("");
    } catch { setFailure("revision_conflict"); }
    finally { busy.current = false; setPending(false); }
  }
  return <form ref={formRef} className="principle-form" data-testid="principle-form" aria-label={t(current ? "principles.edit" : "principles.add")} onFocus={event => { if (event.target instanceof HTMLElement) event.target.scrollIntoView({ block: "nearest" }); }} onSubmit={event => { event.preventDefault(); void save(); }}>
    <label htmlFor={`${id}-statement`}>{t("principles.statement")}</label>
    <textarea id={`${id}-statement`} rows={2} value={statement} onChange={event => setStatement([...event.target.value].slice(0, 280).join(""))} disabled={pending} autoFocus aria-describedby={count >= 240 ? `${id}-count` : undefined} />
    {count >= 240 ? <small id={`${id}-count`}>{t("principles.counter", { current: formatNumber(count), max: formatNumber(280) })}</small> : null}
    <label htmlFor={`${id}-why`}>{t("principles.why")}</label>
    <textarea id={`${id}-why`} rows={2} value={why} onChange={event => setWhy([...event.target.value].slice(0, 500).join(""))} disabled={pending} placeholder={t("principles.whyPlaceholder")} />
    <label htmlFor={`${id}-example`}>{t("principles.example")}</label>
    <textarea id={`${id}-example`} rows={2} value={example} onChange={event => setExample([...event.target.value].slice(0, 500).join(""))} disabled={pending} placeholder={t("principles.examplePlaceholder")} />
    <div className="principle-actions"><Button type="submit" disabled={!valid || pending || conflict}>{t("principles.save")}</Button><Button type="button" variant="ghost" disabled={pending} onClick={onCancel}>{t("principles.cancel")}</Button></div>
    {error ? <div role="alert"><p>{t(conflict ? "principles.conflict" : error === "duplicate_principle" ? "principles.duplicate" : "principles.error")}</p>{conflict ? <Button type="button" variant="ghost" disabled={pending} onClick={() => void reload()}>{t("principles.reload")}</Button> : error !== "duplicate_principle" ? <Button type="button" variant="ghost" disabled={pending || !valid} onClick={() => void save()}>{t("principles.retry")}</Button> : null}</div> : null}
  </form>;
}
