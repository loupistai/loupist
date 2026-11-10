import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { ChatMessage, LoupistBridge } from "../../types";
import type { GrowthEvent, KnowledgeMap } from "../../contracts/knowledge";
import type { WorldReadinessStatus } from "../../knowledge/worldReadiness";
import { addedMapItems, chatMapData, chatSourceWork, stepEntityIds } from "../../chat/chatMapData";

type ChatMapState = {
  loupistId: string; chatId: string; map?: KnowledgeMap; events: GrowthEvent[];
  loading: boolean; error: boolean; retry: () => void;
  learningPending: boolean;
  learningWaiting?: WorldReadinessStatus | "unknown";
  active: boolean; select: (ids?: ReadonlySet<string>) => void; deselect: () => void;
  highlightIds: ReadonlySet<string>; glowNodeIds: ReadonlySet<string>; glowEdgeIds: ReadonlySet<string>;
  /** Names the running reply's steps read that are on this chat's map (chat-run.md A4). */
  liveNodeIds: ReadonlySet<string>;
  /** The chat view reports its running reply's step names; the thread list receives a turn only when it ends. */
  reportLive: (loupistId: string, chatId: string, ids: readonly string[]) => void;
  /** New names arrived while the user read another panel tab: the Map tab carries a dot. */
  badged: boolean;
};
/** What the panel did with this chat's first map growth (see `side-panel.md` §3.1, §3.3). */
export type MapRevealResult = "opened" | "badged" | "none";
const ChatMapContext = createContext<ChatMapState | null>(null);
export const useChatMap = () => useContext(ChatMapContext);
const emptyIds = new Set<string>();
const POLL_INTERVAL_MS = 5000;
// A legacy receipt or unavailable job can only be checked for a bounded time.
const MAX_UNCONFIRMED_POLLS = 36;

export function ChatMapProvider(props: { bridge: LoupistBridge; loupistId: string; chatId: string; messages: readonly ChatMessage[]; onOpen: () => void; onFirstNames?: () => MapRevealResult; refreshKey?: unknown; children: ReactNode }) {
  const { bridge, loupistId, chatId, messages, onOpen, onFirstNames, refreshKey, children } = props;
  const scopeKey = JSON.stringify([loupistId, chatId]);
  const [result, setResult] = useState<{ scopeKey: string; map: KnowledgeMap; events: GrowthEvent[] }>();
  const [loading, setLoading] = useState(true);
  const [learning, setLearning] = useState<{ scopeKey: string; waiting?: WorldReadinessStatus | "unknown" }>();
  const [error, setError] = useState(false);
  const [revision, setRevision] = useState(0);
  const [activeScope, setActiveScope] = useState<string>();
  const [highlightIds, setHighlightIds] = useState<ReadonlySet<string>>(emptyIds);
  const [glow, setGlow] = useState<{ scopeKey?: string; nodeIds: ReadonlySet<string>; edgeIds: ReadonlySet<string> }>({ nodeIds: emptyIds, edgeIds: emptyIds });
  const observed = useRef<{ scopeKey: string; ids: Set<string>; hadNames: boolean } | null>(null);
  const [badge, setBadge] = useState<string>();
  const firstNames = useRef(onFirstNames);
  firstNames.current = onFirstNames;
  const glowTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  // A saved receipt keeps its original status even after its job completes.
  // eslint-disable-next-line react-hooks/exhaustive-deps -- a new completion cache per bridge and chat scope; they are its identity, not values it reads.
  const completedJobs = useMemo(() => new Set<string>(), [bridge, scopeKey]);
  // Ignore streaming text updates; reload when a turn or its Memory delivery changes.
  const messageKey = JSON.stringify(messages.map(message => [message.id, message.status, message.memory_update, message.memory_refs, stepEntityIds(message)]));
  const completionKey = JSON.stringify(messages.filter(message => message.role === "assistant" && message.status !== "streaming").map(message => [message.id, message.memory_update?.commitId]));
  useEffect(() => {
    if (!loupistId || !chatId) { setLoading(false); return; }
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const { jobIds, legacyMessages } = chatSourceWork(messages);
    for (const id of completedJobs) jobIds.delete(id);
    let polls = 0;
    setLoading(true); setError(false);
    const readMap = async () => {
      const mapPromise = bridge.knowledgeMap({ loupistId, maxNodes: 1000 });
      const eventsPromise = (async () => {
        const events: GrowthEvent[] = [];
        let cursor: string | undefined;
        do {
          const page = await bridge.knowledgeGrowthEvents({ loupistId, limit: 5000, cursor });
          if (cancelled) return events;
          events.push(...page.events.filter(event => event.originChatId === chatId));
          cursor = page.nextCursor ?? undefined;
        } while (cursor);
        return events;
      })();
      const [map, events] = await Promise.all([mapPromise, eventsPromise]);
      if (cancelled) return;
      const fresh = events.filter(event => observed.current?.scopeKey === scopeKey ? !observed.current.ids.has(event.eventId) : Date.now() - event.atMs >= 0 && Date.now() - event.atMs < 4000);
      const additions = addedMapItems(map, fresh);
      // The first names this chat adds open the Map beside it (A5); a chat seen with none before qualifies.
      const hadNames = events.some(event => event.kind === "entity_added" && map.nodes.some(node => node.kind === "entity" && node.id === event.subjectId));
      const firstGrowth = observed.current?.scopeKey === scopeKey && !observed.current.hadNames && additions.nodeIds.size > 0;
      observed.current = { scopeKey, ids: new Set(events.map(event => event.eventId)), hadNames };
      setResult({ scopeKey, map, events });
      for (const event of events) {
        if (event.originMessageId && (event.kind === "entity_added" || event.kind === "claim_added")) legacyMessages.delete(event.originMessageId);
      }
      if (firstGrowth) {
        const reveal = firstNames.current?.() ?? "none";
        if (reveal === "opened") setActiveScope(scopeKey);
        else if (reveal === "badged") setBadge(scopeKey);
      }
      if (additions.count) {
        clearTimeout(glowTimer.current);
        setGlow({ scopeKey, ...additions });
        glowTimer.current = setTimeout(() => setGlow({ nodeIds: emptyIds, edgeIds: emptyIds }), 4000);
      }
    };
    const refresh = async () => {
      const jobs = await Promise.all([...jobIds].map(async jobId => {
        const job = await bridge.knowledgeGraphJob({ loupistId, jobId }).catch(() => null);
        return { jobId, status: job?.status };
      }));
      if (cancelled) return;
      let pending = false;
      let settled = false;
      for (const { jobId, status } of jobs) {
        if (status === "queued" || status === "running") pending = true;
        else if (status || polls >= MAX_UNCONFIRMED_POLLS) {
          if (status === "completed") completedJobs.add(jobId);
          jobIds.delete(jobId);
          settled = true;
        }
      }
      if (pending) {
        const running = jobs.some(job => job.status === "running");
        const readiness = running ? "ready" : (await bridge.knowledgeGraphSettingsGet({ loupistId }).catch(() => null))?.readiness.status ?? "unknown";
        if (cancelled) return;
        setLearning({ scopeKey, waiting: readiness === "ready" ? undefined : readiness });
      }
      // Read after job status so the final snapshot includes its committed growth.
      if (polls === 0 || settled || (legacyMessages.size && polls <= MAX_UNCONFIRMED_POLLS)) {
        try {
          await readMap();
          if (!cancelled) setError(false);
        } catch {
          if (!cancelled) setError(true);
        }
      }
      if (cancelled) return;
      if (!pending) setLearning(undefined);
      setLoading(false);
      if (jobIds.size || (legacyMessages.size && polls < MAX_UNCONFIRMED_POLLS)) {
        polls += 1;
        timer = setTimeout(() => void refresh(), POLL_INTERVAL_MS);
      }
    };
    void refresh();
    return () => { cancelled = true; clearTimeout(timer); };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `completionKey` names the messages' jobs, and the scope follows `loupistId` and `chatId`.
  }, [bridge, loupistId, chatId, completedJobs, completionKey, revision, refreshKey]);
  useEffect(() => () => clearTimeout(glowTimer.current), []);
  const [live, setLive] = useState<{ scopeKey: string; key: string }>();
  const reportLive = useCallback((liveLoupistId: string, liveChatId: string, ids: readonly string[]) => {
    const next = ids.length ? { scopeKey: JSON.stringify([liveLoupistId, liveChatId]), key: ids.join("\n") } : undefined;
    setLive(current => current?.scopeKey === next?.scopeKey && current?.key === next?.key ? current : next);
  }, []);
  const liveKey = live?.scopeKey === scopeKey ? live.key : "";
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `messageKey` names the messages the map reads.
  const scoped = useMemo(() => result?.scopeKey === scopeKey ? { ...result.map, ...chatMapData(result.map, messages, result.events, chatId, liveKey ? liveKey.split("\n") : []) } : undefined, [result, messageKey, chatId, scopeKey, liveKey]);
  const select = useCallback((ids: ReadonlySet<string> = emptyIds) => { setHighlightIds(ids); setActiveScope(scopeKey); setBadge(undefined); onOpen(); }, [onOpen, scopeKey]);
  // Names the running reply's steps read glow on the map while it runs; the canvas fades them after.
  const liveNodeIds = useMemo(() => {
    const ids = liveKey ? liveKey.split("\n").filter(id => scoped?.nodes.some(node => node.id === id)) : [];
    return ids.length ? new Set(ids) : emptyIds;
  }, [liveKey, scoped]);
  return <ChatMapContext.Provider value={{ loupistId, chatId, map: scoped, events: result?.scopeKey === scopeKey ? result.events : [], learningPending: learning?.scopeKey === scopeKey && !learning.waiting, learningWaiting: learning?.scopeKey === scopeKey ? learning.waiting : undefined, loading: loading || !scoped && !error, error, retry: () => setRevision(value => value + 1), active: activeScope === scopeKey, select, deselect: () => setActiveScope(undefined), highlightIds: activeScope === scopeKey ? highlightIds : emptyIds, glowNodeIds: glow.scopeKey === scopeKey ? glow.nodeIds : emptyIds, glowEdgeIds: glow.scopeKey === scopeKey ? glow.edgeIds : emptyIds, liveNodeIds, reportLive, badged: badge === scopeKey && activeScope !== scopeKey }}>{children}</ChatMapContext.Provider>;
}
