import { formatNumber } from "@/i18n/format";
import { AsOfTag } from "./ComposerAsOf";
import i18n, { useTranslation, Trans } from "@/i18n";
import type { ModelCatalog } from "../../types";
import type { ChatSubmission } from "../../chat/submission";
import { Textarea } from "../ui/textarea";
import { useInputModality } from "@/lib/inputModality";
import { useEffect, useId, useRef, useState } from "react";
import {
  CheckIcon,
  ChevronDownIcon,
  EllipsisIcon,
  XIcon,
} from "lucide-react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "../ui/dropdown-menu";

import { MEMORY_BUSY, REPLY_FAILED, REPLY_STOPPED, type QueueHold } from "../../chat/draftSession";

const t = i18n.getFixedT(null, "chat");

export type QueuedChatMessage = ChatSubmission;

type Props = {
  queued: QueuedChatMessage[];
  models: ModelCatalog["models"];
  holdReason?: QueueHold;
  onRemove: (id: string) => void;
  onEdit: (id: string, text: string) => void;
  onMove: (id: string, offset: -1 | 1) => void;
  onSendNow: (id: string) => void;
  canSendNow?: boolean;
  /** Why Send now is unavailable when it is not the running reply (a folder or an account to reconnect). */
  blockedReason?: string;
  /** While offline nothing can be guided or sent now; queued messages wait for the connection. */
  offline?: boolean;
};

export function ChatPendingQueue({
  queued,
  models,
  holdReason,
  onRemove,
  onEdit,
  onMove,
  onSendNow,
  canSendNow = true,
  blockedReason,
  offline = false,
}: Props) {
  useTranslation("chat");
  const modality = useInputModality();
  const listId = useId();
  const rootRef = useRef<HTMLDivElement>(null);
  const [expanded, setExpanded] = useState(true);
  // A row whose menu removed or sent it takes its menu button with it: focus moves to the row now in
  // its place, or to the message when none is left, never to the page.
  const leaving = useRef<{ id: string; index: number } | null>(null);
  useEffect(() => {
    const row = leaving.current;
    if (!row || queued.some((item) => item.id === row.id)) return undefined;
    leaving.current = null;
    // After the menu's own focus return, which aims at the button that left.
    const timer = window.setTimeout(() => {
      const focused = document.activeElement;
      if (focused && focused !== document.body && focused.isConnected) return;
      const menus = rootRef.current?.querySelectorAll<HTMLElement>(".chat-pending-more") ?? [];
      const next = menus[Math.min(row.index, menus.length - 1)] ?? document.querySelector<HTMLElement>("textarea[data-chat-composer]");
      next?.focus();
    }, 0);
    return () => window.clearTimeout(timer);
  }, [queued]);
  if (queued.length === 0 && !holdReason) return null;
  const sendable = canSendNow && !offline;
  const leave = (id: string, index: number) => { leaving.current = { id, index }; };
  const unavailable = offline ? t("chatPendingQueue.waitsUntilOnline") : blockedReason ?? t("chatPendingQueue.waitsUntilTheCurrentResponseFinishes");

  const notice = holdReason && (
    <p className="chat-pending-hold" role="status">
      <span>{holdReason === MEMORY_BUSY ? t("chatView.waitingForMemory")
        : holdReason === REPLY_STOPPED ? t(queued.length ? "chatView.responseStoppedQueuedMessagesWereKept" : "useLoupistChat.responseStopped")
        : holdReason === REPLY_FAILED ? t(queued.length ? "chatView.responseFailedQueuedMessagesWereKept" : "useLoupistChat.responseFailed")
        : holdReason}</span>
      {queued[0] && sendable ? <button type="button" className="chat-pending-resume" onClick={() => { leave(queued[0]!.id, 0); onSendNow(queued[0]!.id); }}>
        {t("chatPendingQueue.sendNextNow")}
      </button> : null}
    </p>
  );

  return (
    <div ref={rootRef} className="chat-pending-queue" role="group" aria-label={t("chatPendingQueue.pendingChatInstructions")}>
      {queued.length > 0 ? (
        <button
          type="button"
          className="chat-pending-heading"
          aria-expanded={expanded}
          aria-controls={listId}
          data-input={modality}
          onClick={() => setExpanded((open) => !open)}
        >
          <strong><Trans ns="chat" i18nKey="chatPendingQueue.queueHeading" values={{ formattedCount: formatNumber(queued.length) }} components={[<span />]} /></strong>
          <ChevronDownIcon aria-hidden="true" />
        </button>
      ) : null}
      {holdReason !== MEMORY_BUSY && notice}
      {queued.length > 0 ? (
        <ul id={listId} className="chat-pending-list" hidden={!expanded}>
          {queued.map((item, index) => (
            <li key={item.id}>
              <QueuedCard
                item={item}
                modelName={item.modelProfileId ? models.find((model) => model.id === item.modelProfileId)?.name ?? null : ""}
                canMoveUp={index > 0}
                canMoveDown={index < queued.length - 1}
                onRemove={() => { leave(item.id, index); onRemove(item.id); }}
                onEdit={(text) => onEdit(item.id, text)}
                onMoveUp={() => onMove(item.id, -1)}
                onMoveDown={() => onMove(item.id, 1)}
                onSendNow={() => { leave(item.id, index); onSendNow(item.id); }}
                canSendNow={sendable}
                unavailable={unavailable}
              />
            </li>
          ))}
        </ul>
      ) : null}
      {holdReason === MEMORY_BUSY && notice}
    </div>
  );
}

function QueuedCard({
  item,
  modelName,
  canMoveUp,
  canMoveDown,
  onRemove,
  onEdit,
  onMoveUp,
  onMoveDown,
  onSendNow,
  canSendNow,
  unavailable,
}: {
  item: QueuedChatMessage;
  /** Null when the queued turn's model is no longer available. */
  modelName: string | null;
  canMoveUp: boolean;
  canMoveDown: boolean;
  onRemove: () => void;
  onEdit: (text: string) => void;
  onMoveUp: () => void;
  onMoveDown: () => void;
  onSendNow: () => void;
  canSendNow: boolean;
  /** Why Send now is unavailable, said under it. */
  unavailable: string;
}) {
  useTranslation("chat");
  const modality = useInputModality();
  const [draft, setDraft] = useState(item.text);
  const [editing, setEditing] = useState(false);
  const editorRef = useRef<HTMLTextAreaElement>(null);
  const actionsRef = useRef<HTMLButtonElement>(null);
  const returnFocus = useRef(false);
  useEffect(() => {
    if (editing) {
      editorRef.current?.focus();
      editorRef.current?.setSelectionRange(draft.length, draft.length);
    } else if (returnFocus.current) {
      returnFocus.current = false;
      actionsRef.current?.focus();
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps -- places the caret when editing starts, not while typing.
  }, [editing]);

  const finishEdit = () => {
    returnFocus.current = true;
    setEditing(false);
  };

  const commit = () => {
    const next = draft.trim();
    if (!next) return;
    onEdit(next);
    finishEdit();
  };

  const beginEdit = () => {
    setDraft(item.text);
    setEditing(true);
  };

  return (
    <article className="chat-pending-card is-queued">
      {item.asOf ? <div className="message-as-of"><AsOfTag asOf={item.asOf} kind="request" /></div> : null}
      <div className="chat-pending-row">
        {editing ? (
          <Textarea
            ref={editorRef}
            className="chat-pending-editor"
            aria-label={t("chatPendingQueue.queuedMessageText")}
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.nativeEvent.isComposing || event.keyCode === 229) return;
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                commit();
              }
              if (event.key === "Escape") {
                event.preventDefault();
                event.stopPropagation();
                setDraft(item.text);
                finishEdit();
              }
            }}
          />
        ) : (
          <p className="chat-pending-text" title={item.text}>{item.text}</p>
        )}
        {editing ? (
          <div className="chat-pending-actions">
            <button
              type="button"
              aria-label={t("chatPendingQueue.saveQueuedMessage")}
              onClick={commit}
              disabled={!draft.trim()}
            >
              <CheckIcon aria-hidden="true" />
            </button>
            <button
              type="button"
              aria-label={t("chatPendingQueue.cancelEdit")}
              onClick={() => {
                setDraft(item.text);
                finishEdit();
              }}
            >
              <XIcon aria-hidden="true" />
            </button>
          </div>
        ) : (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button
                ref={actionsRef}
                type="button"
                className="chat-pending-more"
                aria-label={t("chatPendingQueue.messageActions")}
                data-input={modality}
              >
                <EllipsisIcon aria-hidden="true" />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="chat-pending-menu" data-input={modality}>
              <DropdownMenuItem onSelect={beginEdit}>{t("chatPendingQueue.editQueuedMessage")}</DropdownMenuItem>
              <DropdownMenuItem disabled={!canMoveUp} onSelect={onMoveUp}>{t("chatPendingQueue.moveQueuedMessageUp")}</DropdownMenuItem>
              <DropdownMenuItem disabled={!canMoveDown} onSelect={onMoveDown}>{t("chatPendingQueue.moveQueuedMessageDown")}</DropdownMenuItem>
              {/* A disabled item takes no pointer, so its reason is a line of its own rather than a tooltip. */}
              <DropdownMenuItem disabled={!canSendNow} className="chat-pending-send-now" onSelect={onSendNow}>
                <span>{t("chatPendingQueue.sendQueuedMessageNow")}</span>
                {canSendNow ? null : <small>{unavailable}</small>}
              </DropdownMenuItem>
              <DropdownMenuItem onSelect={onRemove}>{t("chatPendingQueue.removeQueuedMessage")}</DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </div>
      {(() => {
        // Only what differs from an ordinary turn: the composer already shows the run profile (no model name here).
        const parts = [
          modelName === null ? t("chatPendingQueue.unavailableModel") : "",
          item.privateChat ? t("chatPendingQueue.memoryOff") : "",
          item.files.length > 0 ? t("chatPendingQueue.files", { count: item.files.length, formattedCount: formatNumber(item.files.length) }) : "",
          item.pins.length > 0 ? t("chatPendingQueue.pins", { count: item.pins.length, formattedCount: formatNumber(item.pins.length) }) : "",
          item.selectedArtifact ? t("chatPendingQueue.selectedWork") : "",
        ].filter(Boolean);
        return parts.length ? <small className="chat-pending-context">{parts.join(" · ")}</small> : null;
      })()}
    </article>
  );
}
