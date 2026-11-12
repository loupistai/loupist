import { claimIdForEdge, entityLabel } from "../../knowledge/mapItems";
import { relationName } from "../map/relations";
import i18n, { useTranslation } from "@/i18n";
import { useEffect, useId, useState } from "react";
import { libraryState } from "../map/libraryState";
import type { LoupistBridge, ChatMessage, GraphJobView, LensKind, MemoryUpdateChange } from "../../types";
import { memoryChangeVerb } from "../../chat/memoryActivity";
import { knowledgeLocationFromMemoryChange } from "../../knowledge/location";
import { chatReceipt, savesMemory, showsReceipt, thesisMovesFor, type SavedReceiptGroup } from "../../chat/receipt";
import { useGo } from "../navigation/Open";
import { useAssetNames } from "../assets/assetName";
import { declinedSensitiveMemory, useChatSetAside } from "../../chat/chatSetAside";
import { WatchAllOffer } from "./WatchAllOffer";
import { ChatReceipt } from "./ChatReceipt";
import { revealFirstNumberToCheck } from "./NumberChip";
import { useChatMap } from "../workspace/ChatMapContext";
import { addedMapItems } from "../../chat/chatMapData";
import { SavedPrinciple } from "./SavedPrinciple";
import { useLearnedDigest } from "../../chat/useLearnedDigest";
import { LearnedCard } from "./LearnedCard";
import { pageTitle } from "../memory/PageReader";
import { WorldProcessingStatus } from "./WorldProcessingStatus";

const t = i18n.getFixedT(null, "chat");

type Props = {
  bridge?: LoupistBridge;
  active?: boolean;
  loupistId: string;
  chatId?: string;
  message: ChatMessage;
  onReadWorldJob?: (jobId: string) => Promise<GraphJobView>;
  onWorldChanged?: () => void;
};

export function MessageMemoryRow({
  bridge,
  active = true,
  loupistId,
  chatId,
  message,
  onReadWorldJob,
  onWorldChanged,
}: Props) {
  useTranslation("chat");
  const go = useGo();
  const chatMap = useChatMap();
  const additions = chatMap?.map && chatMap.loupistId === loupistId && chatMap.chatId === chatId
    ? addedMapItems(chatMap.map, chatMap.events.filter(event => event.originChatId === chatId && event.originMessageId === message.id)) : undefined;
  const edge = chatMap?.map?.edges.find(item => additions?.edgeIds.has(item.id));
  const source = chatMap?.map?.nodes.find(node => node.id === edge?.source);
  const target = chatMap?.map?.nodes.find(node => node.id === edge?.target);
  const update = message.memory_update;
  const changes = update?.status === "applied" ? update.changes : [];
  const [pendingId, setPendingId] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState<ReadonlySet<string>>(() => new Set());
  const [dismissError, setDismissError] = useState<{ recordId: string; message: string } | null>(null);
  const [learnedExpanded, setLearnedExpanded] = useState(false);
  const learnedId = useId();
  const [expanded, setExpanded] = useState(() => changes.some(change => change.kind === "principle" || change.kind === "Journal"));
  const principleIds = changes.filter(change => change.kind === "principle").map(change => change.recordId).join("\n");
  // eslint-disable-next-line react-hooks/exhaustive-deps -- expands once when principles arrive; later changes must not reopen a row the user closed.
  useEffect(() => { if (principleIds || changes.some(change => change.kind === "Journal")) setExpanded(true); }, [principleIds]);
  const chipsId = useId();
  const usedId = useId();
  const [usedExpanded, setUsedExpanded] = useState(false);
  const memoryContext = useChatSetAside();
  const saveOff = memoryContext?.saveOff ?? false;
  const movedReads = chatMap && chatMap.loupistId === loupistId && chatMap.chatId === chatId
    ? thesisMovesFor(chatMap.events.filter(event => event.originChatId === chatId), message.id) : [];
  // Names read only when this answer moved a read.
  const assetName = useAssetNames(bridge, loupistId, movedReads.length > 0);
  const thesisMoves = movedReads.map(move => ({ ...move, name: assetName({ name: move.name, assetId: move.assetId }) }));
  const digest = useLearnedDigest(bridge, loupistId, message, savesMemory(message, saveOff));
  const receipt = chatReceipt(message, additions, { saveOff, thesisMoves, learned: digest });
  const declined = declinedSensitiveMemory(message) && memoryContext?.firstDeclinedMessageId === message.id
    ? <p className="memory-declined" role="note">{t("saveProof.declined")}</p> : null;
  if (!showsReceipt(message) || receipt.empty && !savesMemory(message, saveOff)) return declined;

  // A principle has no page for the Evidence reader to show; it opens on "What {Loupist} knows".
  const viewPrinciple = (recordId: string) => go?.({ kind: "page", location: { loupistId, surface: "lens", recordId } });
  const openSaved = (change: MemoryUpdateChange) => {
    if (change.kind === "principle") { viewPrinciple(change.recordId); return; }
    if (change.kind === "Lens") { go?.({ kind: "lens", recordId: change.recordId }); return; }
    if ((change.kind === "Journal" || change.kind === "Research") && bridge) Object.assign(libraryState(bridge, loupistId), { open: true, tab: "note" as const });
    const location = knowledgeLocationFromMemoryChange(loupistId, change);
    if (location) go?.({ kind: "memory", location, title: change.title });
    else if (change.kind === "Research") go?.({ kind: "page", location: { loupistId, surface: "note" } });
  };
  /** One saved item opens directly; the list stays open below for its actions. */
  const openSavedGroup = (group: SavedReceiptGroup) => {
    if (group.items.length === 1) { setExpanded(true); openSaved(group.items[0]); return; }
    setExpanded((current) => !current);
  };
  const undoPrinciple = async (recordId: string) => {
    if (!bridge || pendingId) return;
    setPendingId(recordId); setDismissError(null);
    try { await bridge.principleArchive({ loupistId, recordId, archived: true }); setDismissed(current => new Set(current).add(recordId)); onWorldChanged?.(); }
    catch { setDismissError({ recordId, message: t("principles.error") }); }
    finally { setPendingId(null); }
  };
  return (
    <div className="memory-row" data-testid="memory-row" data-expanded={expanded}>
      {go && edge && source && target && <button className="answer-map-preview" onClick={() => go({ kind: "page", location: {
        loupistId, surface: "world", view: { followCurrent: true }, selection: { kind: "claim", id: claimIdForEdge(edge) },
      } })}>
        <span>{entityLabel(source, i18n.language)}</span><small>{edge.relation ? relationName(edge.relation, i18n.getFixedT(null, "knowledge")) : "→"}</small><span>{entityLabel(target, i18n.language)}</span>
      </button>}
      <ChatReceipt
        receipt={receipt}
        showNothingNew={savesMemory(message, saveOff)}
        listId={chipsId}
        listExpanded={expanded}
        onShowOnMap={chatMap ? (ids) => chatMap.select(ids) : undefined}
        usedListId={usedId}
        usedExpanded={usedExpanded}
        onToggleUsed={() => setUsedExpanded(current => !current)}
        learnedListId={learnedId}
        learnedExpanded={learnedExpanded}
        onToggleLearned={() => setLearnedExpanded(current => !current)}
        onSaved={openSavedGroup}
        onOpenThesis={go ? move => go({ kind: "asset", target: { loupistId, assetId: move.assetId, name: move.name, section: "history" } }) : undefined}
        onCheckNumbers={() => revealFirstNumberToCheck(message.id)}
      />
      {receipt.setup.some(change => change.change === "followed") ? <WatchAllOffer bridge={bridge} loupistId={loupistId} refreshKey={message.id} /> : null}
      {learnedExpanded && digest ? <LearnedCard id={learnedId} bridge={bridge} loupistId={loupistId} digest={digest}
        onChanged={onWorldChanged} /> : null}
      {usedExpanded ? <UsedMemory id={usedId} bridge={bridge} loupistId={loupistId} refreshKey={chatMap?.map} pages={receipt.usedPages}
        onEdit={go ? viewPrinciple : undefined} onChanged={onWorldChanged} /> : null}
      {expanded ? <ul className="memory-update-changes" id={chipsId} aria-label={t("messageMemoryRow.savedInThisAnswer")}>
        {changes.filter((change) => change.kind !== "Lens").map((change) => {
          if (change.kind === "principle") return <li className="memory-update-change memory-principle" key={change.recordId} data-memory-change="create:principle">
            <SavedPrinciple showLabel={false} statement={change.statement} undone={dismissed.has(change.recordId)} pending={pendingId !== null} onUndo={!dismissed.has(change.recordId) && bridge ? () => void undoPrinciple(change.recordId) : undefined} onView={go ? () => viewPrinciple(change.recordId) : undefined} />
            {dismissError?.recordId === change.recordId ? <span role="alert">{dismissError.message}</span> : null}
          </li>;
          if (change.kind === "Journal") return <li className="memory-update-change memory-journal" key={change.recordId} data-memory-change={`${change.operation}:journal`}><span>{t("journal.saved", { judgment: change.title })}</span>{go ? <button type="button" onClick={() => openSaved(change)}>{t("journal.view")}</button> : null}</li>;
          const location = change.kind === "Research" ? null : knowledgeLocationFromMemoryChange(loupistId, change);
          const learned = change.kind === "World";
          return <li className="memory-update-change" key={change.recordId} data-memory-change={`${change.operation}:${change.kind}`}>
            <div className="memory-change-title">
              <span className="memory-chip-verb">{memoryChangeVerb(change)}</span>
              {location && go ? <button type="button" onClick={() => openSaved(change)}>{change.title}</button> : <span>{change.title}</span>}
            </div>
            {learned ? <>
              {change.lesson ? <span>{change.lesson}</span> : null}
              {change.basis ? <small>{t("messageMemoryRow.basis", { value: change.basis })}</small> : null}
              {change.futureUse ? <small>{t("messageMemoryRow.nextUse", { value: change.futureUse })}</small> : null}
            </> : null}
            <WorldProcessingStatus active={active && expanded} jobId={change.worldJobId} initialStatus={change.worldStatus} readJob={onReadWorldJob} onChanged={onWorldChanged} />
            {learned && location && go ? <button type="button" onClick={() => go({ kind: "page", location })}>{t("messageMemoryRow.openIn", { value: t("workspaceEvidence.kind.world") })}</button> : null}
          </li>;
        })}
      </ul> : null}
      {/* After the receipt: it names the one thing that was not kept, under what was. */}
      {declined}
    </div>
  );
}

type UsedItem = { recordId: string; title: string; kind: LensKind };

/**
 * The pages that shaped this answer, quoted, with "Not this time" (Rust leaves
 * the page out of this chat until Undo) and Edit (opens it on "What {Loupist}
 * knows"). Ways of working can also be archived here.
 */
function UsedMemory({ id, bridge, loupistId, refreshKey, pages, onEdit, onChanged }: {
  id: string; bridge?: LoupistBridge; loupistId: string; refreshKey?: unknown; pages: readonly UsedItem[];
  onEdit?: (recordId: string) => void; onChanged?: () => void;
}) {
  const setAside = useChatSetAside();
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [archived, setArchived] = useState<ReadonlySet<string>>(() => new Set());
  const [available, setAvailable] = useState<ReadonlySet<string> | null>(null);
  useEffect(() => {
    if (!bridge) return;
    let live = true;
    void bridge.knowledgeCatalog({ loupistId }).then(catalog => { if (live) setAvailable(new Set(catalog.lenses.map(item => item.recordId))); }).catch(() => { /* A failed read does not prove deletion. */ });
    return () => { live = false; };
  }, [bridge, loupistId, refreshKey]);
  const run = async (recordId: string, action: () => Promise<void>) => {
    if (pending) return;
    setPending(recordId); setError(null);
    try { await action(); }
    catch { setError(recordId); }
    finally { setPending(null); }
  };
  const row = (item: UsedItem) => {
    const aside = setAside?.ids.has(item.recordId) ?? false;
    return <li key={item.recordId} className="used-memory-item" data-set-aside={aside || undefined}>
      <span className="used-memory-label">{t("usedMemory.pageLabel", { kind: i18n.t(`knowledge:pageReader.kind.${item.kind}`) })}</span>
      {available && !available.has(item.recordId) ? <p className="used-memory-aside"><s>{pageTitle(item)}</s> · {t("usedMemory.removed")}</p> : aside ? <p className="used-memory-aside" role="status">{t("usedMemory.setAside")} · <button type="button" disabled={pending !== null} onClick={() => void run(item.recordId, () => setAside!.toggle(item.recordId, false))}>{t("usedMemory.undo")}</button></p>
        : archived.has(item.recordId) ? <p className="used-memory-aside" role="status">{t("messageMemoryRow.dismissed")}</p>
          : <>
            <blockquote>{pageTitle(item)}</blockquote>
            <div className="used-memory-actions">
              {setAside ? <button type="button" disabled={pending !== null} onClick={() => void run(item.recordId, () => setAside.toggle(item.recordId, true))}>{t("usedMemory.notThisTime")}</button> : null}
              {onEdit ? <button type="button" onClick={() => onEdit(item.recordId)}>{t("usedMemory.edit")}</button> : null}
              {item.kind !== "principle" && item.kind !== "perspective" && bridge ? <button type="button" disabled={pending !== null} onClick={() => void run(item.recordId, async () => { await bridge.knowledgeLensArchive({ loupistId, recordId: item.recordId, archived: true }); setArchived(current => new Set(current).add(item.recordId)); onChanged?.(); })}>{t("usedMemory.archive")}</button> : null}
            </div>
          </>}
      {error === item.recordId ? <span role="alert">{t("usedMemory.error")}</span> : null}
    </li>;
  };
  return <ul className="used-memory" id={id}>
    {pages.map(row)}
  </ul>;
}
