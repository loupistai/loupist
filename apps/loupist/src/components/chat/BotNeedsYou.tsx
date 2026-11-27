import { useCurrentVisit } from "../../app/loupistVisit";
import { useState } from "react";
import { useTranslation } from "@/i18n";
import type { AttentionItem } from "../../contracts/attention";
import type { LoupistBridge, ScheduledTaskListItem } from "../../types";
import { useBotScheduleTasks } from "./BotScheduleTasks";
import { cadenceLabel, scheduledTaskTitle } from "../briefings/scheduledPresentation";
import { attentionItemKey, attentionWhat, settleAttention, useSettledAttention } from "../app/NeedsAttention";
import { Button } from "../ui/button";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";

export function BotNeedsYou({ items, bridge, onOpen, onAsk, loupistId }: {
  items: readonly AttentionItem[];
  loupistId: string;
  bridge: LoupistBridge;
  onOpen: (item: AttentionItem) => void;
  onAsk: (text: string) => void;
}) {
  const { t } = useTranslation("briefings");
  const visit = useCurrentVisit(loupistId);
  const [expanded, setExpanded] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const [deleting, setDeleting] = useState<AttentionItem | null>(null);
  const settled = useSettledAttention();
  const waiting = items.filter((item) => !settled.has(attentionItemKey(item)));
  const { tasks } = useBotScheduleTasks(bridge, loupistId, items.some(item => item.kind === "proposal") ? "proposals" : "");
  const collapsed = waiting.length > 3 && !expanded;
  if (waiting.length === 0) return null;
  const act = async (item: AttentionItem, action: "discard" | "resume" | "delete" | "confirm", proposal?: ScheduledTaskListItem) => {
    const identity = proposal
      ? { loupist_id: proposal.loupist_id, task_id: proposal.id }
      : item.recordId ? { loupist_id: item.loupistId, task_id: item.recordId } : undefined;
    if (!identity || busy) return;
    setBusy(identity.task_id);
    setFailed(false);
    try {
      if (action === "confirm") {
        if (!proposal?.awaiting_confirmation) return;
        await bridge.scheduledTaskConfirm({ ...identity, expected_revision: proposal.revision });
      }
      else if (action === "discard") {
        if (visit) await bridge.scheduledProposalSetDeclined({ ...identity, visit: visit.openedAtMs, declined: true });
        else await bridge.scheduledTaskDiscard(identity);
      }
      else {
        const task = await bridge.scheduledTaskGet(identity);
        if (!task) return;
        const revision = { ...identity, expected_revision: task.revision };
        if (action === "resume") await bridge.scheduledTaskSetStatus({ ...revision, status: "active" });
        else {
          await bridge.scheduledTaskDelete(revision);
          setDeleting(null);
        }
      }
      settleAttention(item);
    } catch {
      setFailed(true);
    } finally {
      setBusy(null);
    }
  };
  const heading = t("standing.needsYou", { count: waiting.length });
  return <section className="bot-needs-you" aria-label={heading}>
    {waiting.length > 3 ? <button type="button" className="morning-link" aria-expanded={!collapsed} onClick={() => setExpanded(!expanded)}>{heading}</button> : <strong>{heading}</strong>}
    {!collapsed && <ul>{waiting.map(item => {
      const proposal = item.kind === "proposal" && item.recordId ? tasks?.get(item.recordId) : undefined;
      return <li key={`${item.kind}:${item.recordId ?? item.chatId ?? item.title}`}>
      <span>{attentionWhat(proposal ? { ...item, title: scheduledTaskTitle(proposal.kind, proposal.title) } : item)}</span>
      {item.kind === "triggerFailing" ? <div className="bot-needs-you-actions">
        <Button size="sm" onClick={() => onAsk(t("schedule.check.stopped", { title: item.title, error: item.title }))}>{t("schedule.check.askFix")}</Button>
        <Button size="sm" variant="ghost" disabled={busy !== null} onClick={() => {
          if (!item.recordId) return;
          setBusy(item.recordId);
          void bridge.scheduledTaskGet({ loupist_id: item.loupistId, task_id: item.recordId }).then(async task => {
            const prior = task?.trigger?.approved_versions.at(-2);
            if (task && prior) await bridge.scheduledTriggerRevert({ loupistId: item.loupistId, taskId: item.recordId!, toVersion: prior.version });
          }).finally(() => setBusy(null));
        }}>{t("schedule.check.revert")}</Button>
        <Button size="sm" variant="ghost" disabled={busy !== null || !item.recordId} onClick={() => {
          if (!item.recordId) return;
          setBusy(item.recordId);
          void bridge.scheduledTaskGet({ loupist_id: item.loupistId, task_id: item.recordId }).then(async task => {
            if (!task) return;
            await bridge.scheduledTaskSetStatus({ loupist_id: item.loupistId, task_id: item.recordId!, expected_revision: task.revision, status: "paused" });
          }).finally(() => setBusy(null));
        }}>{t("schedule.pause")}</Button>
      </div> : item.kind === "triggerUpdate" ? <div className="bot-needs-you-actions">
        <Button size="sm" variant="ghost" onClick={() => onOpen(item)}>{t("schedule.turnOn")}</Button>
      </div> : item.kind === "offer" && item.recordId ? <div className="bot-needs-you-actions">
        {item.dueDate
          ? <Button size="sm" variant="ghost" onClick={() => onOpen(item)}>{t("schedule.open")}</Button>
          : <Button size="sm" disabled={busy !== null} onClick={() => void bridge.botOfferAccept({ loupistId: item.loupistId, offerId: item.recordId! }).catch(() => setFailed(true))}>{t("schedule.turnOn")}</Button>}
        <Button size="sm" variant="ghost" disabled={busy !== null} onClick={() => void bridge.botOfferDismiss({ loupistId: item.loupistId, offerId: item.recordId! }).catch(() => setFailed(true))}>{t("standing.notNow")}</Button>
      </div> : item.kind === "watchQuiet" && item.recordId ? <div className="bot-needs-you-actions">
        <Button size="sm" disabled={busy !== null} onClick={() => void (async () => {
          const task = await bridge.scheduledTaskGet({ loupist_id: item.loupistId, task_id: item.recordId! });
          if (!task) return;
          await bridge.scheduledTaskSetStatus({ loupist_id: item.loupistId, task_id: item.recordId!, expected_revision: task.revision, status: "paused" });
        })().catch(() => setFailed(true))}>{t("schedule.pause")}</Button>
      </div> : item.kind === "watchLoud" && item.recordId ? <div className="bot-needs-you-actions">
        <Button size="sm" disabled={busy !== null} onClick={() => void (async () => {
          const task = await bridge.scheduledTaskGet({ loupist_id: item.loupistId, task_id: item.recordId! });
          if (!task) return;
          await bridge.scheduledTaskSetDelivery({ loupistId: item.loupistId, taskId: item.recordId!, expectedRevision: task.revision, notify: "digest" });
        })().catch(() => setFailed(true))}>{t("standing.notifyQuietly")}</Button>
      </div> : item.kind === "proposal" && proposal && (proposal.trigger || proposal.preset === "price-move") && !proposal.replaces
        ? <Button size="sm" variant="ghost" onClick={() => onOpen(item)}>{t("schedule.open")}</Button>
        : item.kind === "proposal" ? <div className="bot-needs-you-actions">
        {proposal && <span>{cadenceLabel(proposal.schedule)}</span>}
        <Button size="sm" disabled={busy !== null || !proposal?.awaiting_confirmation} onClick={() => { if (proposal) void act(item, "confirm", proposal); }}>{t("schedule.turnOn")}</Button>
        <Button size="sm" variant="ghost" disabled={busy !== null} onClick={() => void act(item, "discard")}>{t("standing.notNow")}</Button>
      </div> : item.kind === "taskPaused" ? <div className="bot-needs-you-actions">
        <Button size="sm" disabled={busy !== null} onClick={() => void act(item, "resume")}>{t("schedule.resume")}</Button>
        <Button size="sm" variant="ghost" onClick={() => onAsk(t("schedule.changePrompt", { title: item.title }))}>{t("schedule.askBot")}</Button>
        {item.recordId === `briefing-${item.loupistId}`
          ? <Button size="sm" variant="ghost" onClick={() => onOpen(item)}>{t("schedule.open")}</Button>
          : <Button size="sm" variant="ghost" disabled={busy !== null} onClick={() => { setFailed(false); setDeleting(item); }}>{t("schedule.deleteEllipsis")}</Button>}
      </div> : <Button size="sm" variant="ghost" onClick={() => onOpen(item)}>{t("schedule.open")}</Button>}
    </li>;
    })}</ul>}
    {failed && !deleting && <p className="inline-error" role="alert">{t("schedule.statusFailed")}</p>}
    <Dialog open={deleting !== null} onOpenChange={open => { if (!open && busy === null) setDeleting(null); }}>
      {deleting && <DialogContent size="sm">
        <DialogHeader><DialogTitle>{t("schedule.deleteTitle", { title: deleting.title })}</DialogTitle>
          <DialogDescription>{t("schedule.deleteBody")}</DialogDescription></DialogHeader>
        {failed && <DialogBody><p className="inline-error" role="alert">{t("schedule.deleteFailed")}</p></DialogBody>}
        <DialogFooter>
          <Button variant="outline" autoFocus disabled={busy !== null} onClick={() => setDeleting(null)}>{t("schedule.cancel")}</Button>
          <Button variant="destructive" disabled={busy !== null} onClick={() => void act(deleting, "delete")}>{t("schedule.delete")}</Button>
        </DialogFooter>
      </DialogContent>}
    </Dialog>
  </section>;
}
