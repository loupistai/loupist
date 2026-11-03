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

const t = i18n.getFixedT(null, "chat");

export type QueuedChatMessage = ChatSubmission;

type Props = {
  queued: QueuedChatMessage[];
  models: ModelCatalog["models"];
  holdReason?: string | null;
  onRemove: (id: string) => void;
  onEdit: (id: string, text: string) => void;
  onMove: (id: string, offset: -1 | 1) => void;
  onSendNow: (id: string) => void;
  canSendNow?: boolean;
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
  offline = false,
}: Props) {
  useTranslation("chat");
  const modality = useInputModality();
  const listId = useId();
  const [expanded, setExpanded] = useState(true);
  if (queued.length === 0 && !holdReason) return null;

  return (
    <div className="chat-pending-queue" aria-label={t("chatPendingQueue.pendingChatInstructions")}>
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
      {holdReason && (
        <p className="chat-pending-hold" role="status">
          {holdReason}
        </p>
      )}
      {queued.length > 0 ? (
        <ul id={listId} className="chat-pending-list" hidden={!expanded}>
          {queued.map((item, index) => (
            <li key={item.id}>
              <QueuedCard
                item={item}
                modelName={item.modelProfileId ? models.find((model) => model.id === item.modelProfileId)?.name ?? null : ""}
                canMoveUp={index > 0}
                canMoveDown={index < queued.length - 1}
                onRemove={() => onRemove(item.id)}
                onEdit={(text) => onEdit(item.id, text)}
                onMoveUp={() => onMove(item.id, -1)}
                onMoveDown={() => onMove(item.id, 1)}
                onSendNow={() => onSendNow(item.id)}
                canSendNow={canSendNow && !offline}
              />
            </li>
          ))}
        </ul>
      ) : null}
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
}) {
  useTranslation("chat");
  const modality = useInputModality();
  const [draft, setDraft] = useState(item.text);
  const [editing, setEditing] = useState(false);
  const editorRef = useRef<HTMLTextAreaElement>(null);
  const actionsRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (editing) {
      editorRef.current?.focus();
      editorRef.current?.setSelectionRange(draft.length, draft.length);
    }
  }, [editing]);

  const finishEdit = () => {
    setEditing(false);
    requestAnimationFrame(() => actionsRef.current?.focus());
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
              <DropdownMenuItem
                disabled={!canSendNow}
                title={canSendNow ? t("chatPendingQueue.sendNow") : t("chatPendingQueue.waitsUntilTheCurrentResponseFinishes")}
                onSelect={onSendNow}
              >
                {t("chatPendingQueue.sendQueuedMessageNow")}
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
