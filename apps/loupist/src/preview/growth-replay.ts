import type { GrowthEvent, KnowledgeMap, LearningReview, Principle } from "../contracts/knowledge";
import type { InboxItem } from "../contracts/scheduled";
import type { KnowledgeCatalog, LensChangeRecord, LensView } from "../types";
import { prepareFixture, type PreviewBridge } from "./fixtures";
import recorded from "./growth-replay.json";

export const growthReplayPending = "fixturePending" in recorded && recorded.fixturePending === true;

type RecordedGrowth = {
  knowledgeMap: KnowledgeMap;
  growth_events: GrowthEvent[];
  learning_reviews: LearningReview[];
  principles: Principle[];
  methods: { lens: LensView; changes: LensChangeRecord[] }[];
  inbox: InboxItem[];
};

/** Only recorded fictional state enters this scenario; production bridge is untouched. */
export async function prepareGrowthReplay(bridge: PreviewBridge) {
  await prepareFixture(bridge);
  await bridge.loupistProfileUpdate({ loupistId: "loupist-quality", name: "Semiconductors", focus: "Fictional capacity ramps, margins and cash conversion" });
  const data = structuredClone(recorded) as unknown as RecordedGrowth;
  const copy = <T,>(value: T) => structuredClone(value);
  bridge.knowledgeMap = async () => copy(data.knowledgeMap);
  bridge.knowledgeGrowthEvents = async ({ sinceMs, untilMs, limit }) => ({ events: copy(data.growth_events.filter(event => (sinceMs === undefined || event.atMs >= sinceMs) && (untilMs === undefined || event.atMs <= untilMs)).slice(0, limit ?? 200)), nextCursor: null });
  bridge.learningReviewList = async ({ limit }) => copy(data.learning_reviews.slice(0, limit ?? 50));
  bridge.principleList = async ({ includeArchived }) => copy(data.principles.filter(principle => includeArchived || !principle.archived).map(principle => ({ ...principle, loupistId: "loupist-quality" })));
  bridge.inboxList = async ({ limit }) => ({ items: copy(data.inbox.slice(0, limit)), nextCursor: null });
  bridge.inboxUnreadCounts = async () => [{ loupistId: "loupist-quality", count: data.inbox.filter(item => item.readAtMs === null).length }];
  bridge.journalList = async () => [];
  bridge.watchlistList = async () => [];
  const lenses: KnowledgeCatalog["lenses"] = data.methods.map(({ lens }) => ({ recordId: lens.recordId, kind: "lens", title: lens.title, revisionId: lens.revisionId, revisionDigest: lens.revisionDigest, readOnly: false, indexingState: "snapshot_only", objects: [], lensKind: lens.lensKind, applicability: lens.document.applicability }));
  for (const principle of data.principles) lenses.push({ recordId: principle.recordId, kind: "lens", title: principle.statement, revisionId: principle.revisionId, revisionDigest: "", readOnly: false, indexingState: "snapshot_only", objects: [], lensKind: "principle" });
  bridge.knowledgeCatalog = async ({ loupistId }) => ({ loupistId, sources: [], worlds: [], lenses });
  bridge.knowledgeLensRead = async ({ recordId }) => {
    const method = data.methods.find(item => item.lens.recordId === recordId);
    if (!method) throw new Error("This growth fixture contains only the recorded method documents");
    return copy(method.lens);
  };
  bridge.knowledgeLensChanges = async (request?: import("../types").KnowledgeLensChangesRequest) => copy(data.methods.find(item => item.lens.recordId === request?.recordId)?.changes ?? []);
  // This scenario demonstrates the Map snapshot and replay, not synthetic Source readers.
  bridge.knowledgeWorldQuery = async () => { throw new Error("Full World readers are outside the growth replay snapshot"); };
}
