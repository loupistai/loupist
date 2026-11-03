import { Disclosure } from "../ui/disclosure";
import { formatNumber } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import { useEffect, useState } from "react";
import { FileTextIcon } from "lucide-react";
import { Button } from "../ui/button";
import { errorMessage } from "../../lib/errors";
import type { ChatActivityDetail, LoupistBridge } from "../../types";
import { readerValue } from "../workspace/readerFormatting";
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
  const command = detail?.tool_name === "bash" ? (typeof fields?.command === "string" ? fields.command : !fields ? detail.input : null) : null;
  const facts = fields ? Object.entries(fields).filter(([key]) => ["working_directory", "cwd", "timeout"].includes(key)) : [];
  const writtenContent = detail?.tool_name === "write" && typeof fields?.content === "string" ? fields.content : null;
  const replacement = detail?.tool_name === "edit" && typeof fields?.oldText === "string" && typeof fields?.newText === "string"
    ? { before: fields.oldText, after: fields.newText } : null;
  const editCount = Array.isArray(fields?.edits) ? fields.edits.length : 0;

  return <section className="chat-command" aria-label={t("chatActivityDetailView.activityDetails")}>
    {error ? <div className="reader-error" role="alert"><p>{error}</p><Button type="button" size="sm" variant="outline" onClick={() => setRevision((current) => current + 1)}>{t("chatActivityDetailView.tryAgain")}</Button></div> : null}
    {!detail ? !error && <p role="status">{t("chatActivityDetailView.openingActivity")}</p> : <>
      {detail.status === "failed" ? <p role="alert">{detail.output ? t("chatActivityDetailView.thisStepFailedTheOutputIsBelow") : t("chatActivityDetailView.thisStepFailedBeforeItReturnedOutput")}</p> : null}
      {detail.status === "stopped" ? <p className="reader-notice">{t("chatActivityDetailView.thisStepWasStoppedBeforeItCompletedAnyFiles")}</p> : null}
      {command ? <p className="chat-command-line"><code>{command}</code></p> : null}
      {writtenContent !== null ? <Disclosure title={<>{t("chatActivityDetailView.writtenContent")}</>}><pre tabIndex={0}>{writtenContent}</pre></Disclosure> : null}
      {replacement ? <Disclosure title={<>{t("chatActivityDetailView.requestedChange")}</>}><h3>{t("chatActivityDetailView.before")}</h3><pre tabIndex={0}>{replacement.before}</pre><h3>{t("chatActivityDetailView.after")}</h3><pre tabIndex={0}>{replacement.after}</pre></Disclosure> : null}
      {editCount ? <p className="reader-notice">{t("chatActivityDetailView.replacements", { count: editCount, formattedCount: formatNumber(editCount) })}</p> : null}
      {detail.output ? <CopyTextButton text={detail.output} label={t("chatActivityDetailView.outputLabel")} /> : null}
      {detail.output ? <pre tabIndex={0} aria-label={t("chatActivityDetailView.commandOutput")} data-failed={detail.status === "failed"}>{detail.output}</pre> : <p className="reader-empty-output">{detail.status === "running" ? t("chatActivityDetailView.waitingForOutput") : detail.status === "succeeded" ? t("chatActivityDetailView.completedWithoutTextOutput") : t("chatActivityDetailView.noOutputWasRecorded")}</p>}
      {detail.truncated ? <p className="reader-notice">{t("chatActivityDetailView.thisIsALimitedPreviewOfTheSavedInput")}</p> : null}
      {detail.file_path ? <div className="reader-activity-file"><FileTextIcon aria-hidden="true" /><span title={detail.file_path}>{detail.file_path}</span></div> : null}
      {facts.length ? <Disclosure className="reader-technical-details" title={<>{t("chatActivityDetailView.details")}</>}><dl className="reader-facts">{facts.map(([key, value]) => <div key={key}><dt>{key === "timeout" ? t("chatActivityDetailView.timeLimitSeconds") : t("chatActivityDetailView.folder")}</dt><dd>{readerValue(value)}</dd></div>)}</dl></Disclosure> : null}
    </>}
  </section>;
}
