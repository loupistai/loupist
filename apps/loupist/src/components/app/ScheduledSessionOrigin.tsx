import { useEffect, useState } from "react";
import { useTranslation } from "@/i18n";
import { formatDate } from "@/i18n/format";
import type { scheduledSessionOrigin } from "@/app/chatTitle";
import type { LoupistBridge } from "@/types";
import { RunRecordRemoved, type ChatDestination } from "../../chat/navigation";
import { scheduledTaskTitle } from "../briefings/scheduledPresentation";
import { useOpen } from "../navigation/Open";
import { ContextTitle, titlePlaceholder } from "../ui/context-title";

export function ScheduledSessionOrigin({ bridge, loupistId, origin: { turn, date, shared } }: {
  bridge: LoupistBridge;
  loupistId: string;
  origin: NonNullable<ReturnType<typeof scheduledSessionOrigin>>;
}) {
  const { t } = useTranslation("app");
  const open = useOpen();
  const [record, setRecord] = useState<{ title: string; fresh: boolean; present: boolean; hasRun: boolean } | null>(null);
  useEffect(() => {
    let generation = 0;
    const refresh = async () => {
      const current = ++generation;
      const request = { loupist_id: loupistId, task_id: turn.task_id };
      try {
        const task = await bridge.scheduledTaskGet(request);
        const runs = task?.target.kind === "new_chat" ? await bridge.scheduledTaskRuns(request) : [];
        if (current === generation) {
          setRecord(previous => task ? {
            title: task.title, fresh: task.target.kind === "new_chat", present: true,
            hasRun: runs.some(run => run.run_id === turn.run_id),
          } : previous && { ...previous, present: false, hasRun: false });
        }
      } catch {
        if (current === generation) setRecord(previous => previous && { ...previous, present: false, hasRun: false });
      }
    };
    void refresh();
    const off = bridge.onInboxChanged(event => { if (event.loupistId === loupistId) void refresh(); });
    return () => { generation++; off(); };
  }, [bridge, loupistId, turn.task_id, turn.run_id]);
  const title = scheduledTaskTitle(turn.task_kind, record?.title ?? turn.task_title);
  const fresh = record?.fresh === true && !shared;
  const template = fresh ? t("loupistChatItem.fromRun", { title: titlePlaceholder, date: formatDate(Date.parse(date)) })
    : t("loupistChatItem.fromTask", { title: titlePlaceholder });
  const label = <ContextTitle template={template} title={title} />;
  const destination: ChatDestination | null = !record?.present ? null : !fresh ? { kind: "schedule", taskId: turn.task_id }
    : record.hasRun ? { kind: "run", taskId: turn.task_id, runId: turn.run_id } : null;
  return <small className="scheduled-session-origin">{destination && open ? <button type="button" className="morning-link"
    onClick={() => void open(destination).catch(error => { if (error instanceof RunRecordRemoved) setRecord(previous => previous && { ...previous, hasRun: false }); })}>{label}</button> : label}</small>;
}
