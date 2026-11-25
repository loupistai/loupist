import { Disclosure } from "../ui/disclosure";
import { formatNumber } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import { useEffect, useState } from "react";
import { FileTextIcon } from "lucide-react";
import { Button } from "../ui/button";
import { errorMessage } from "../../lib/errors";
import type { ChatActivityDetail, LoupistBridge } from "../../types";
import { readerValue } from "../workspace/readerFormatting";
import { PythonReads } from "./PythonReads";
import { CopyTextButton } from "../ui/copy-text-button";

const t = i18n.getFixedT(null, "chat");

type Props = {
  bridge: LoupistBridge; loupistId: string; threadId: string; detailId: string;
  active?: boolean;
  /** Host output revision. When present, replace the one-second poll. */
  outputRevision?: number;
};
function inputFields(input: string) {
  try {
    const value: unknown = JSON.parse(input);
    return value && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;
  } catch { return null; }
}

export function ChatActivityDetailView(props: Props) {
  useTranslation("chat");
  return <ActivityReader key={JSON.stringify([props.loupistId, props.threadId, props.detailId])} {...props} />;
}

function ActivityReader({ bridge, loupistId, threadId, detailId, active: visible = true, outputRevision }: Props) {
  useTranslation("chat");
  const [detail, setDetail] = useState<ChatActivityDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [revision, setRevision] = useState(0);
  const [replay, setReplay] = useState<{ matches: boolean; stdout: string; stderr: string }>();
  const [replayError, setReplayError] = useState<string>();
  const [replaying, setReplaying] = useState(false);
  useEffect(() => {
    if (!visible) return;
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    setError(null);
    const read = async () => {
      try {
        const next = await bridge.chatActivityRead({ loupist_id: loupistId, thread_id: threadId, detail_id: detailId });
        if (!active) return;
        if (next.id !== detailId) throw new Error(t("chatActivityDetailView.thisActivityIsNotAvailableInThisChat"));
        setDetail(next); setError(null);
        if (next.status === "running" && outputRevision === undefined) timer = setTimeout(() => void read(), 1000);
      } catch (cause) { if (active) setError(errorMessage(cause, t("chatActivityDetailView.couldNotOpenThisActivity"))); }
    };
    void read();
    return () => { active = false; clearTimeout(timer); };
  }, [bridge, loupistId, threadId, detailId, revision, visible, outputRevision]);

  const fields = detail ? inputFields(detail.input) : null;
  const program = detail?.python?.code ?? (detail?.tool_name === "run_python" ? (typeof fields?.code === "string" ? fields.code : !fields ? detail.input : null) : null);
  const facts = fields ? Object.entries(fields).filter(([key]) => ["working_directory", "cwd", "timeout", "timeout_seconds"].includes(key)) : [];
  const writtenContent = detail?.tool_name === "write" && typeof fields?.content === "string" ? fields.content : null;
  const replacement = detail?.tool_name === "edit" && typeof fields?.oldText === "string" && typeof fields?.newText === "string"
    ? { before: fields.oldText, after: fields.newText } : null;
  const editCount = Array.isArray(fields?.edits) ? fields.edits.length : 0;

  const repeat = async () => {
    if (!detail?.step_id || !detail.python_message_id) return;
    setReplaying(true);
    setReplayError(undefined);
    try { setReplay(await bridge.pythonStepReplay({ chatId: threadId, messageId: detail.python_message_id, stepId: detail.step_id })); }
    catch (cause) { setReplayError(errorMessage(cause, t("python.error"))); }
    finally { setReplaying(false); }
  };

  return <section className="chat-command" aria-label={t("chatActivityDetailView.activityDetails")}>
    {error ? <div className="reader-error" role="alert"><p>{error}</p><Button type="button" size="sm" variant="outline" onClick={() => setRevision((current) => current + 1)}>{t("chatActivityDetailView.tryAgain")}</Button></div> : null}
    {!detail ? !error && <p role="status">{t("chatActivityDetailView.openingActivity")}</p> : <>
      {detail.status === "failed" ? <p role="alert">{detail.output ? t("chatActivityDetailView.thisStepFailedTheOutputIsBelow") : t("chatActivityDetailView.thisStepFailedBeforeItReturnedOutput")}</p> : null}
      {detail.status === "stopped" ? <p className="reader-notice">{t("chatActivityDetailView.thisStepWasStoppedBeforeItCompletedAnyFiles")}</p> : null}
      {detail.python ? <><h3>{t("python.showMath")}</h3><p>{t("python.rounds", { count: detail.python.rounds })}</p></> : null}
      {program ? <pre className="chat-command-line" tabIndex={0}>{program}</pre> : null}
      {writtenContent !== null ? <Disclosure title={<>{t("chatActivityDetailView.writtenContent")}</>}><pre tabIndex={0}>{writtenContent}</pre></Disclosure> : null}
      {replacement ? <Disclosure title={<>{t("chatActivityDetailView.requestedChange")}</>}><h3>{t("chatActivityDetailView.before")}</h3><pre tabIndex={0}>{replacement.before}</pre><h3>{t("chatActivityDetailView.after")}</h3><pre tabIndex={0}>{replacement.after}</pre></Disclosure> : null}
      {editCount ? <p className="reader-notice">{t("chatActivityDetailView.replacements", { count: editCount, formattedCount: formatNumber(editCount) })}</p> : null}
      {detail.python ? <>
        <PythonReads reads={detail.python.reads} evidence={detail.python_sources} />
        <h4>{t("python.printed")}</h4>
        {detail.python.stdout ? <pre tabIndex={0}>{detail.python.stdout}</pre> : null}
        {detail.python.stderr ? <pre tabIndex={0}>{detail.python.stderr}</pre> : null}
        {detail.python_message_id && detail.step_id ? <Button size="sm" variant="outline" disabled={replaying} onClick={() => void repeat()}>{t(replaying ? "python.replaying" : "python.replay")}</Button> : null}
        {replayError ? <p role="alert">{replayError}</p> : null}
        {replay ? <div role="status"><p>{t(replay.matches ? "python.same" : "python.changed")}</p>
          {!replay.matches ? <pre tabIndex={0}>{replay.stdout || replay.stderr}</pre> : null}
        </div> : null}
      </> : null}
      {!detail.python && detail.output ? <CopyTextButton text={detail.output} label={t("chatActivityDetailView.outputLabel")} /> : null}
      {!detail.python ? detail.output ? <pre tabIndex={0} aria-label={t("chatActivityDetailView.commandOutput")} data-failed={detail.status === "failed"}>{detail.output}</pre> : <p className="reader-empty-output">{detail.status === "running" ? t("chatActivityDetailView.waitingForOutput") : detail.status === "succeeded" ? t("chatActivityDetailView.completedWithoutTextOutput") : t("chatActivityDetailView.noOutputWasRecorded")}</p> : null}
      {detail.truncated ? <p className="reader-notice">{t("chatActivityDetailView.thisIsALimitedPreviewOfTheSavedInput")}</p> : null}
      {detail.file_path ? <div className="reader-activity-file"><FileTextIcon aria-hidden="true" /><span title={detail.file_path}>{detail.file_path}</span></div> : null}
      {facts.length ? <Disclosure className="reader-technical-details" title={<>{t("chatActivityDetailView.details")}</>}><dl className="reader-facts">{facts.map(([key, value]) => <div key={key}><dt>{key === "timeout" || key === "timeout_seconds" ? t("chatActivityDetailView.timeLimitSeconds") : t("chatActivityDetailView.folder")}</dt><dd>{readerValue(value)}</dd></div>)}</dl></Disclosure> : null}
    </>}
  </section>;
}
