import { claimIdForEdge } from "../map/MapList";
import { entityLabel } from "../map/labels";
import { relationName } from "../map/relations";
import i18n, { useTranslation } from "@/i18n";
import { useEffect, useId, useState } from "react";
import { libraryState } from "../map/libraryState";
import type { LoupistBridge, ChatEvidenceRef, ChatMessage, GraphJobView, KnowledgeLocation, MemoryUpdateChange } from "../../types";
import { memoryChangeVerb } from "../../chat/memoryActivity";
import { knowledgeLocationFromMemoryChange } from "../../knowledge/location";
import { chatReceipt, savesMemory, showsReceipt, thesisMovesFor, type SavedReceiptGroup } from "../../chat/receipt";
import { useAssetPageOpener } from "../assets/assetOpener";
import { useAssetNames } from "../assets/assetName";
import { declinedSensitiveMemory, useChatSetAside } from "../../chat/chatSetAside";
import { ChatReceipt } from "./ChatReceipt";
import { revealFirstNumberToCheck } from "./NumberChip";
import { errorMessage } from "../../lib/errors";
import { proposalMatchError } from "../../chat/dismissLensProposal";
import { useChatMap } from "../workspace/ChatMapContext";
import { addedMapItems } from "../workspace/chatMapData";
import { SavedPrinciple } from "./SavedPrinciple";
import { WorldProcessingStatus } from "./WorldProcessingStatus";

const t = i18n.getFixedT(null, "chat");

type Props = {
  bridge?: LoupistBridge;
  active?: boolean;
  loupistId: string;
  chatId?: string;
  message: ChatMessage;
  onOpenMemory: (location: KnowledgeLocation, title: string) => void;
  onOpenMemoryPage: (location: KnowledgeLocation) => void;
  /** Opens a source the answer read; omitted where no reader exists. */
  onOpenEvidence?: (evidence: ChatEvidenceRef, anchor?: HTMLElement) => void;
  onDismissLensProposal?: (target: { recordId: string; messageId: string }) => Promise<void>;
  onReadWorldJob?: (jobId: string) => Promise<GraphJobView>;
  onWorldChanged?: () => void;
};

export function MessageMemoryRow({
  bridge,
  active = true,
  loupistId,
  chatId,
  message,
  onOpenMemory,
  onOpenMemoryPage,
  onOpenEvidence,
  onDismissLensProposal,
  onReadWorldJob,
  onWorldChanged,
}: Props) {
  useTranslation("chat");
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
  const [expanded, setExpanded] = useState(() => changes.some(change => change.kind === "principle" || change.kind === "Journal"));
  const principleIds = changes.filter(change => change.kind === "principle").map(change => change.recordId).join("\n");
  useEffect(() => { if (principleIds || changes.some(change => change.kind === "Journal")) setExpanded(true); }, [principleIds]);
  const chipsId = useId();
  const usedId = useId();
  const [usedExpanded, setUsedExpanded] = useState(false);
  const memoryContext = useChatSetAside();
  const saveOff = memoryContext?.saveOff ?? false;
  const openAsset = useAssetPageOpener();
  const movedReads = chatMap && chatMap.loupistId === loupistId && chatMap.chatId === chatId
    ? thesisMovesFor(chatMap.events.filter(event => event.originChatId === chatId), message.id) : [];
  // Names read only when this answer moved a read.
  const assetName = useAssetNames(bridge, loupistId, movedReads.length > 0);
  const thesisMoves = movedReads.map(move => ({ ...move, name: assetName({ name: move.name, assetId: move.assetId }) }));
  const receipt = chatReceipt(message, additions, { saveOff, thesisMoves });
  const declined = declinedSensitiveMemory(message) && memoryContext?.firstDeclinedMessageId === message.id
    ? <p className="memory-declined" role="note">{t("saveProof.declined")}</p> : null;
  if (!showsReceipt(message) || receipt.empty && !savesMemory(message, saveOff)) return declined;

  // Principles and methods open on "What {Loupist} knows".
  const viewPrinciple = (recordId: string) => onOpenMemoryPage({ loupistId, surface: "lens", recordId });
  const openSaved = (change: MemoryUpdateChange) => {
    if (change.kind === "principle" || change.kind === "Lens") {
      viewPrinciple(change.recordId);
      return;
    }
    if ((change.kind === "Journal" || change.kind === "Research") && bridge) Object.assign(libraryState(bridge, loupistId), { open: true, tab: "note" as const });
    const location = knowledgeLocationFromMemoryChange(loupistId, change);
    if (location) onOpenMemory(location, change.title);
    else if (change.kind === "Research") onOpenMemoryPage({ loupistId, surface: "note" });
  };
  /** One saved item opens directly; the list stays open below for its actions. */
  const openSavedGroup = (group: SavedReceiptGroup) => {
    if (group.items.length === 1) { setExpanded(true); openSaved(group.items[0]); return; }
    setExpanded((current) => !current);
  };
  const review = (change: MemoryUpdateChange) => {
    if (!chatId || !message.id) {
      setDismissError({ recordId: change.recordId, message: proposalMatchError() });
      return;
    }
    const location = knowledgeLocationFromMemoryChange(loupistId, change);
    if (location) onOpenMemory({ ...location, revisionId: undefined, view: { ...location.view, reviewProposal: { chatId, messageId: message.id } } }, change.title);
  };
  const dismiss = (recordId: string) => {
    if (!onDismissLensProposal || pendingId) return;
    setPendingId(recordId);
    setDismissError(null);
    void onDismissLensProposal({ recordId, messageId: message.id }).then(() => {
      setDismissed((current) => new Set(current).add(recordId));
    }).catch((cause: unknown) => {
      setDismissError({ recordId, message: errorMessage(cause, t("messageMemoryRow.couldNotDismissThisProposal")) });
    }).finally(() => setPendingId(null));
  };

  const undoPrinciple = async (recordId: string) => {
    if (!bridge || pendingId) return;
    setPendingId(recordId); setDismissError(null);
    try { await bridge.principleArchive({ loupistId, recordId, archived: true }); setDismissed(current => new Set(current).add(recordId)); onWorldChanged?.(); }
    catch { setDismissError({ recordId, message: t("principles.error") }); }
    finally { setPendingId(null); }
  };
  return (
    <div className="memory-row" data-expanded={expanded}>
      {edge && source && target && <button className="answer-map-preview" onClick={() => onOpenMemoryPage({
        loupistId, surface: "world", view: { followCurrent: true }, selection: { kind: "claim", id: claimIdForEdge(edge) },
      })}>
        <span>{entityLabel(source, i18n.language)}</span><small>{edge.relation ? relationName(edge.relation, i18n.getFixedT(null, "knowledge")) : "→"}</small><span>{entityLabel(target, i18n.language)}</span>
      </button>}
      <ChatReceipt
        receipt={receipt}
        showNothingNew={savesMemory(message, saveOff)}
        listId={chipsId}
        listExpanded={expanded}
        onOpenSource={onOpenEvidence}
        onShowOnMap={chatMap ? (ids) => chatMap.select(ids) : undefined}
        usedListId={usedId}
        usedExpanded={usedExpanded}
        onToggleUsed={() => setUsedExpanded(current => !current)}
        onSaved={openSavedGroup}
        onToggleList={() => setExpanded((current) => !current)}
        onOpenThesis={openAsset ? move => openAsset({ loupistId, assetId: move.assetId, name: move.name, section: "history" }) : undefined}
        onCheckNumbers={() => revealFirstNumberToCheck(message.id)}
      />
      {usedExpanded ? <UsedMemory id={usedId} bridge={bridge} loupistId={loupistId} refreshKey={chatMap?.map} principles={receipt.usedPrinciples}
        methods={receipt.usedMethods}
        onEdit={viewPrinciple} onChanged={onWorldChanged} /> : null}
      {expanded ? <ul className="memory-update-changes" id={chipsId} aria-label={t("messageMemoryRow.savedInThisAnswer")}>
        {changes.map((change) => {
          if (change.kind === "principle") return <li className="memory-update-change memory-principle" key={change.recordId} data-memory-change="create:principle">
            <SavedPrinciple showLabel={false} statement={change.statement} undone={dismissed.has(change.recordId)} pending={pendingId !== null} onUndo={!dismissed.has(change.recordId) && bridge ? () => void undoPrinciple(change.recordId) : undefined} onView={() => viewPrinciple(change.recordId)} />
            {dismissError?.recordId === change.recordId ? <span role="alert">{dismissError.message}</span> : null}
          </li>;
          if (change.kind === "Journal") return <li className="memory-update-change memory-journal" key={change.recordId} data-memory-change={`${change.operation}:journal`}><span>{t("journal.saved", { judgment: change.title })}</span><button type="button" onClick={() => openSaved(change)}>{t("journal.view")}</button></li>;
          const location = change.kind === "Research" ? null : knowledgeLocationFromMemoryChange(loupistId, change);
          const proposal = change.kind === "Lens" && change.operation === "proposed";
          const learned = change.kind === "World" || change.kind === "Lens";
          return <li className="memory-update-change" key={change.recordId} data-memory-change={`${change.operation}:${change.kind}`}>
            <div className="memory-change-title">
              <span className="memory-chip-verb">{memoryChangeVerb(change)}</span>
              {location ? <button type="button" onClick={() => openSaved(change)}>{change.title}</button> : <span>{change.title}</span>}
            </div>
            {learned ? <>
              {change.lesson ? <span>{change.lesson}</span> : null}
              {change.basis ? <small>{t("messageMemoryRow.basis", { value: change.basis })}</small> : null}
              {change.futureUse ? <small>{t("messageMemoryRow.nextUse", { value: change.futureUse })}</small> : null}
            </> : null}
            <WorldProcessingStatus active={active && expanded} jobId={change.worldJobId} initialStatus={change.worldStatus} readJob={onReadWorldJob} onChanged={onWorldChanged} />
            {proposal ? <div className="memory-proposal" role="group" aria-label={t("messageMemoryRow.proposalLabel", { title: change.title })}>
              {dismissed.has(change.recordId) ? <span role="status">{t("messageMemoryRow.dismissed")}</span> : <>
                <button type="button" aria-label={t("messageMemoryRow.reviewProposal", { title: change.title })} disabled={pendingId !== null} onClick={() => review(change)}>{t("messageMemoryRow.review")}</button>
                {onDismissLensProposal ? <button type="button" aria-label={t("messageMemoryRow.dismissProposal", { title: change.title })} disabled={pendingId !== null} onClick={() => dismiss(change.recordId)}>{t("messageMemoryRow.dismiss")}</button> : null}
              </>}
              {dismissError?.recordId === change.recordId ? <span role="alert">{dismissError.message}</span> : null}
            </div> : learned && location ? <button type="button" onClick={() => onOpenMemoryPage(location)}>{t("messageMemoryRow.openIn", { value: change.kind })}</button> : null}
          </li>;
        })}
      </ul> : null}
      {/* After the receipt: it names the one thing that was not kept, under what was. */}
      {declined}
    </div>
  );
}

type UsedItem = { recordId: string; title: string };

/**
 * The principles and methods supplied to this answer, quoted, with "Not this
 * time" (Rust leaves the record out of this chat until Undo) and Edit (opens
 * it on "What {Loupist} knows"). Methods can also be archived here.
 */
function UsedMemory({ id, bridge, loupistId, refreshKey, principles, methods, onEdit, onChanged }: {
  id: string; bridge?: LoupistBridge; loupistId: string; refreshKey?: unknown; principles: readonly UsedItem[]; methods: readonly UsedItem[];
  onEdit: (recordId: string) => void; onChanged?: () => void;
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
  const row = (item: UsedItem, kind: "principle" | "method") => {
    const aside = setAside?.ids.has(item.recordId) ?? false;
    return <li key={`${kind}:${item.recordId}`} className="used-memory-item" data-set-aside={aside || undefined}>
      <span className="used-memory-label">{t(kind === "principle" ? "usedMemory.principleLabel" : "usedMemory.methodLabel")}</span>
      {available && !available.has(item.recordId) ? <p className="used-memory-aside"><s>{item.title}</s> · {t("usedMemory.removed")}</p> : aside ? <p className="used-memory-aside" role="status">{t("usedMemory.setAside")} · <button type="button" disabled={pending !== null} onClick={() => void run(item.recordId, () => setAside!.toggle(item.recordId, false))}>{t("usedMemory.undo")}</button></p>
        : archived.has(item.recordId) ? <p className="used-memory-aside" role="status">{t("messageMemoryRow.dismissed")}</p>
          : <>
            <blockquote>{item.title}</blockquote>
            <div className="used-memory-actions">
              {setAside ? <button type="button" disabled={pending !== null} onClick={() => void run(item.recordId, () => setAside.toggle(item.recordId, true))}>{t("usedMemory.notThisTime")}</button> : null}
              <button type="button" onClick={() => onEdit(item.recordId)}>{t("usedMemory.edit")}</button>
              {kind === "method" && bridge ? <button type="button" disabled={pending !== null} onClick={() => void run(item.recordId, async () => { await bridge.knowledgeLensArchive({ loupistId, recordId: item.recordId, archived: true }); setArchived(current => new Set(current).add(item.recordId)); onChanged?.(); })}>{t("usedMemory.archive")}</button> : null}
            </div>
          </>}
      {error === item.recordId ? <span role="alert">{t("usedMemory.error")}</span> : null}
    </li>;
  };
  return <>
    <p className="used-memory-label">{t("usedMemory.contextNotice")}</p>
    <ul className="used-memory" id={id}>
      {principles.map(item => row(item, "principle"))}
      {methods.map(item => row(item, "method"))}
    </ul>
  </>;
}
