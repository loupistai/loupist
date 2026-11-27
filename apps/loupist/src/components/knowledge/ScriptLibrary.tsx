import { useEffect, useState } from "react";
import { formatDateTime, formatNumber } from "../../i18n/format";
import { useTranslation } from "../../i18n";
import { errorMessage } from "../../lib/errors";
import type { LoupistBridge } from "../../types";
import type { ScheduledTaskListItem } from "../../contracts/scheduled";
import type { PythonStep, ScriptDraft, ScriptSummary, ScriptVersion } from "../../contracts/generated";
import { useGo } from "../navigation/Open";
import { Button } from "../ui/button";
import { folded, oneReading, scriptOutcome, scriptReads, scriptTask } from "./scriptCard";
import { CopyTextButton } from "../ui/copy-text-button";
import { Input } from "../ui/input";
import { Textarea } from "../ui/textarea";
import { PythonReads } from "../chat/PythonReads";

type Props = {
  bridge: LoupistBridge;
  loupistId: string;
  query: string;
  onQueryChange: (query: string) => void;
};

export function ScriptLibrary({ bridge, loupistId, query, onQueryChange }: Props) {
  const { t } = useTranslation("knowledge");
  const [scripts, setScripts] = useState<ScriptSummary[]>();
  const [tasks, setTasks] = useState<ScheduledTaskListItem[]>([]);
  const [selected, setSelected] = useState<ScriptVersion>();
  const [error, setError] = useState<string>();
  const [revision, setRevision] = useState(0);
  const go = useGo();
  useEffect(() => {
    let active = true;
    setError(undefined);
    void bridge.scriptsList({ loupistId, query: query.trim() || undefined })
      .then(async items => {
        const listed = await bridge.scheduledTasksList({ loupist_id: loupistId }).catch(() => [] as ScheduledTaskListItem[]);
        if (active) { setScripts(items); setTasks(listed); }
      })
      .catch((cause: unknown) => { if (active) setError(errorMessage(cause, t("scripts.error"))); });
    return () => { active = false; };
  }, [bridge, loupistId, query, revision, t]);

  const open = async (name: string) => {
    setError(undefined);
    try { setSelected(await bridge.scriptGet({ loupistId, name })); }
    catch (cause) { setError(errorMessage(cause, t("scripts.error"))); }
  };
  const refresh = () => setRevision(revision => revision + 1);
  if (selected) return <ScriptViewer key={selected.summary.name} bridge={bridge} loupistId={loupistId} initial={selected}
    onClose={() => setSelected(undefined)} onChanged={refresh} />;
  return <section className="knowledge-browser research-library">
    <div className="research-library-controls">
      <Input aria-label={t("scripts.search")} placeholder={t("scripts.search")} value={query} maxLength={200} onChange={event => onQueryChange(event.target.value)} />
    </div>
    {error ? <p role="alert">{error}</p> : scripts === undefined ? <p role="status">{t("scripts.loading")}</p> : <>
      {!scripts.length ? <p>{t(query ? "scripts.noMatches" : "scripts.empty")}</p> : null}
      <ul>{scripts.map(script => {
        const { name, version, metadata, usedBy } = script;
        const task = scriptTask(script, tasks);
        const reads = scriptReads(script);
        const summary = folded(metadata.summary) !== folded(metadata.title) ? metadata.summary : undefined;
        const last = task?.trigger?.last;
        const outcome = last ? scriptOutcome(last.outcome) : undefined;
        const tools = reads.extra > 0 ? `${reads.labels.join(", ")} +${formatNumber(reads.extra)}` : reads.labels.join(", ");
        return <li key={name}>
          <Button className="research-library-row" variant="ghost" onClick={() => void open(name)}>
            <span className="memory-rail-copy">
              <strong>{metadata.title}</strong>
              <small>{t("scripts.versionOrigin", { version, origin: t(`scripts.origin.${metadata.origin.kind}`) })}</small>
              {summary ? <span className="source-summary">{summary}</span> : null}
              {reads.labels.length ? <small>{t("scripts.reads", { tools })}</small> : null}
              {!task && usedBy.length ? <small>{t("scripts.usedByTitles", { tasks: usedBy.join(", ") })}</small> : null}
              {last && outcome ? <small>{t("scripts.lastResult", { outcome: t(`scripts.outcome.${outcome}`), reading: oneReading(last.reading), time: formatDateTime(last.at_ms) })}</small>
                : !last && metadata.lastTest ? <small>{t("scripts.tested", { time: formatDateTime(metadata.lastTest.atMs) })}</small> : null}
            </span>
          </Button>
          {task ? go
            ? <Button variant="link" size="sm" onClick={() => go({ kind: "schedule", taskId: task.id })}>{t("scripts.usedByTask", { task: task.title })}</Button>
            : <small>{t("scripts.usedByTask", { task: task.title })}</small> : null}
        </li>;
      })}</ul>
    </>}
  </section>;
}

function scriptDraft({ summary, code }: ScriptVersion): ScriptDraft {
  return { name: summary.name, title: summary.metadata.title, summary: summary.metadata.summary, args: summary.metadata.args, code };
}

function jsonObject(text: string): Record<string, unknown> {
  const object: unknown = JSON.parse(text);
  if (!object || typeof object !== "object" || Array.isArray(object)) throw new Error();
  return object as Record<string, unknown>;
}

function ScriptViewer({ bridge, loupistId, initial, onClose, onChanged }: {
  bridge: LoupistBridge; loupistId: string; initial: ScriptVersion; onClose: () => void; onChanged: () => void;
}) {
  const { t } = useTranslation("knowledge");
  const [script, setScript] = useState(initial);
  const [draft, setDraft] = useState<ScriptDraft>();
  const [args, setArgs] = useState("");
  const [testing, setTesting] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [test, setTest] = useState<PythonStep>();
  const [error, setError] = useState<string>();
  const [pending, setPending] = useState(false);
  const { name, version, metadata, usedBy } = script.summary;
  const latest = script.versions[0] === version;

  const act = async (operation: () => Promise<void>) => {
    setPending(true);
    setError(undefined);
    try { await operation(); }
    catch (cause) { setError(errorMessage(cause, t("scripts.error"))); }
    finally { setPending(false); }
  };
  const select = (item: number) => void act(async () => {
    setScript(await bridge.scriptGet({ loupistId, name, version: item }));
    setTest(undefined);
    setTesting(false);
    setDeleting(false);
  });
  const save = (draft: ScriptDraft) => void act(async () => {
    setScript(await bridge.scriptSave({ loupistId, draft }));
    setDraft(undefined);
    setTest(undefined);
    onChanged();
  });
  const run = () => {
    let input: Record<string, unknown>;
    try { input = jsonObject(args); }
    catch { setError(t("scripts.invalidArgs")); return; }
    void act(async () => {
      setTest(await bridge.scriptTest({ loupistId, name, version, args: input }));
      setScript(await bridge.scriptGet({ loupistId, name, version }));
      onChanged();
    });
  };
  return <section className="script-library-view research-reader-pane">
    <Button size="sm" variant="ghost" onClick={onClose}>{t("scripts.back")}</Button>
    <h3>{metadata.title} · {t("scripts.version", { version })}</h3>
    <p><span className="knowledge-hint">{t("scripts.scriptName")}</span> {name}</p>
    <p>{metadata.summary}</p>
    <small>{formatDateTime(script.summary.createdAtMs)} · {t(`scripts.origin.${metadata.origin.kind}`)}</small>
    {metadata.afterOutsideText ? <p className="reader-notice">{t("scripts.outside")}</p> : null}
    {error ? <p role="alert">{error}</p> : null}
    {draft ? <form onSubmit={event => {
      event.preventDefault();
      try { save({ ...draft, args: jsonObject(args) as ScriptDraft["args"] }); }
      catch { setError(t("scripts.invalidArgs")); }
    }}>
      <label>{t("scripts.title")}<Input maxLength={120} value={draft.title ?? ""} onChange={event => setDraft({ ...draft, title: event.target.value || undefined })} /></label>
      <label>{t("scripts.summary")}<Textarea required maxLength={300} value={draft.summary} onChange={event => setDraft({ ...draft, summary: event.target.value })} /></label>
      <label>{t("scripts.args")}<Textarea value={args} onChange={event => setArgs(event.target.value)} /></label>
      <label>{t("scripts.code")}<Textarea className="script-code-editor" required value={draft.code} onChange={event => setDraft({ ...draft, code: event.target.value })} /></label>
      <div className="script-library-actions">
        <Button disabled={pending} size="sm" type="submit">{t("scripts.saveVersion")}</Button>
        <Button disabled={pending} size="sm" type="button" variant="ghost" onClick={() => setDraft(undefined)}>{t("scripts.cancel")}</Button>
      </div>
    </form> : <>
      <div className="script-library-actions">
        <Button disabled={pending} size="sm" onClick={() => {
          setArgs(JSON.stringify(Object.fromEntries(Object.entries(metadata.args).map(([key, spec]) => [key, spec.default])), null, 2));
          setTesting(true);
          setTest(undefined);
          setDeleting(false);
        }}>{t("scripts.test")}</Button>
        <Button disabled={pending} size="sm" variant="outline" onClick={() => { setTesting(false); setDeleting(false); setDraft(scriptDraft(script)); setArgs(JSON.stringify(metadata.args, null, 2)); }}>{t("scripts.edit")}</Button>
        <CopyTextButton text={script.code} label={t("scripts.code")} />
        <Button disabled={pending} size="sm" variant="outline" onClick={() => void act(async () => {
          await bridge.exportSave({ suggestedName: `${name}.py`, kind: "py", bytes: Array.from(new TextEncoder().encode(script.code)), title: t("scripts.export") });
        })}>{t("scripts.export")}</Button>
        <Button disabled={pending} size="sm" variant="ghost" onClick={() => { setDeleting(true); setTesting(false); }}>{t("scripts.delete")}</Button>
      </div>
      {deleting ? <section aria-label={t("scripts.delete")}>
        <p>{usedBy.length ? t("scripts.pinned", { tasks: usedBy.join(", ") }) : t("scripts.deleteQuestion")}</p>
        {!usedBy.length ? <Button disabled={pending} size="sm" variant="destructive" onClick={() => void act(async () => {
          await bridge.scriptDelete({ loupistId, name });
          onChanged();
          onClose();
        })}>{t("scripts.delete")}</Button> : null}
        <Button disabled={pending} size="sm" variant="ghost" onClick={() => setDeleting(false)}>{t("scripts.cancel")}</Button>
      </section> : null}
      {testing ? <section aria-label={t("scripts.test")}>
        <label>{t("scripts.args")}<Textarea value={args} onChange={event => setArgs(event.target.value)} /></label>
        <Button disabled={pending} size="sm" onClick={run}>{t("scripts.runTest")}</Button>
        {test ? <div role="status"><small>{t("scripts.testStatus", { status: test.status, rounds: test.rounds })}</small><pre tabIndex={0}>{test.stdout || test.stderr}</pre></div> : null}
      </section> : null}
      {!!Object.keys(metadata.args).length ? <section>
        <h4>{t("scripts.args")}</h4>
        <dl>{Object.entries(metadata.args).map(([key, spec]) => <div key={key}><dt>{key} · {JSON.stringify(spec.default)}</dt><dd>{spec.description}</dd></div>)}</dl>
      </section> : null}
      <PythonReads reads={metadata.reads} />
      {usedBy.length ? <section><h4>{t("scripts.usedBy")}</h4><ul>{usedBy.map((task, index) => <li key={index}>{task}</li>)}</ul></section> : null}
      <pre className="chat-command-line" tabIndex={0} aria-label={t("scripts.code")}>{script.code}</pre>
      <section>
        <h4>{t("scripts.versions")}</h4>
        <ol className="script-library-actions">{script.versions.map(item => <li key={item}><Button disabled={pending || item === version} size="sm" variant="ghost" onClick={() => select(item)}>{t("scripts.version", { version: item })}</Button></li>)}</ol>
        {!latest ? <Button disabled={pending} size="sm" variant="outline" onClick={() => save(scriptDraft(script))}>{t("scripts.restore")}</Button> : null}
      </section>
    </>}
  </section>;
}
