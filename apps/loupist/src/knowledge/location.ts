import type { KnowledgeReference, MemoryRef, MemoryUpdateChange } from "../contracts/memory";
import { sourceTitle } from "../components/knowledge/hostSource";
import type {
  GraphCamera,
  GraphClaim,
  GraphNode,
  KnowledgeLocation,
  KnowledgePin,
  KnowledgeSelection,
  SourceDetail,
} from "../contracts/knowledge";

export type KnowledgeNavigation = {
  current: KnowledgeLocation;
  views: Partial<Record<"world" | "source" | "lens" | "note", KnowledgeLocation>>;
  worldSurface: "world" | "source" | "note";
  past: KnowledgeLocation[];
  generation: number;
};

const selectionKey = (selection: KnowledgeSelection | undefined) => {
  if (!selection) return "";
  switch (selection.kind) {
    case "node":
    case "claim":
      return JSON.stringify([selection.kind, selection.id]);
    case "source":
      return JSON.stringify([
        "source", selection.sourceId, selection.spanId,
        selection.worldRecordId, selection.worldRevisionId,
      ]);
    case "lens":
      return JSON.stringify(["lens", selection.recordId]);
    case "research":
      return JSON.stringify(["research", selection.recordId, selection.sectionId]);
    case "journal":
      return JSON.stringify(["journal", selection.journalId]);
    case "snapshot":
      return knowledgePinKey(selection.pin);
  }
};

const objectIdFor = (selection: KnowledgeSelection | undefined) => {
  if (!selection) return undefined;
  switch (selection.kind) {
    case "node":
    case "claim":
      return selection.id;
    case "source":
      return selection.sourceId ?? selection.spanId;
    case "lens":
      return selection.recordId;
    case "research":
      return selection.recordId;
    case "journal":
      return selection.journalId;
    case "snapshot":
      return selection.pin.objectId;
  }
};

export const sameKnowledgeTarget = (
  left: KnowledgeLocation,
  right: KnowledgeLocation,
) =>
  left.loupistId === right.loupistId &&
  left.surface === right.surface &&
  left.recordId === right.recordId &&
  left.revisionId === right.revisionId &&
  selectionKey(left.selection) === selectionKey(right.selection);

const withCamera = (
  location: KnowledgeLocation,
  camera: GraphCamera,
): KnowledgeLocation => ({
  ...location,
  view: { ...location.view, camera },
});

const withSelection = (
  location: KnowledgeLocation,
  selection: KnowledgeSelection | undefined,
): KnowledgeLocation => ({
  ...location,
  selection,
  objectId: objectIdFor(selection),
  view: selectionKey(location.selection) !== selectionKey(selection) && (selection?.kind === "node" || selection?.kind === "claim") ? { ...location.view, inspectorScrollTop: 0, inspectorCollapsed: false } : location.view,
});

export const initialKnowledgeNavigation = (
  loupistId: string,
): KnowledgeNavigation => ({
  current: { loupistId, surface: "home" },
  views: {},
  worldSurface: "world",
  past: [],
  generation: 0,
});

const rememberView = (nav: KnowledgeNavigation, location: KnowledgeLocation): KnowledgeNavigation => {
  const surface = location.surface;
  if (surface !== "world" && surface !== "source" && surface !== "lens" && surface !== "note") return nav;
  return { ...nav, views: { ...nav.views, [surface]: location }, worldSurface: surface === "lens" ? nav.worldSurface : surface };
};

/** Surface selection replaces the active view. Reader navigation alone creates a back trail. */
export const patchKnowledgeLocation = (
  nav: KnowledgeNavigation,
  next: KnowledgeLocation,
  camera?: GraphCamera,
): KnowledgeNavigation => {
  if (nav.current.loupistId !== next.loupistId) nav = initialKnowledgeNavigation(next.loupistId);
  const present = nav.current.surface === "world" && camera ? withCamera(nav.current, camera) : nav.current;
  const changedSurface = present.surface !== next.surface;
  return rememberView({ ...rememberView(nav, present), current: next,
    past: changedSurface ? [] : nav.past, generation: nav.generation + (changedSurface ? 1 : 0) }, next);
};

export const openKnowledgeLocation = (
  nav: KnowledgeNavigation,
  next: KnowledgeLocation,
  camera?: GraphCamera,
): KnowledgeNavigation => {
  if (nav.current.loupistId !== next.loupistId) nav = initialKnowledgeNavigation(next.loupistId);
  const present = nav.current.surface === "world" && camera ? withCamera(nav.current, camera) : nav.current;
  if (sameKnowledgeTarget(present, next)) return nav;
  return rememberView({ ...rememberView(nav, present), current: next,
    past: [...nav.past, present].slice(-40), generation: nav.generation + 1 }, next);
};

export const backKnowledgeLocation = (nav: KnowledgeNavigation): KnowledgeNavigation => {
  const previous = nav.past.at(-1);
  if (!previous) return nav;
  return rememberView({ ...rememberView(nav, nav.current), current: previous,
    past: nav.past.slice(0, -1), generation: nav.generation + 1 }, previous);
};

export const knowledgeLocationFromReference = (
  loupistId: string,
  ref: KnowledgeReference,
): KnowledgeLocation => {
  switch (ref.kind) {
    case "source":
      return withSelection(
        {
          loupistId,
          surface: "source",
          recordId: ref.recordId,
          revisionId: ref.revisionId,
          objectId: ref.objectId,
        },
        { kind: "source", sourceId: ref.objectId === ref.spanId ? undefined : ref.objectId, spanId: ref.spanId },
      );
    case "world":
      return { loupistId, surface: "world", recordId: ref.recordId, revisionId: ref.revisionId };
    case "world_node":
      return withSelection(
        {
          loupistId,
          surface: "world",
          recordId: ref.recordId,
          revisionId: ref.revisionId,
          objectId: ref.objectId,
        },
        { kind: "node", id: ref.objectId },
      );
    case "world_claim":
      return withSelection(
        {
          loupistId,
          surface: "world",
          recordId: ref.recordId,
          revisionId: ref.revisionId,
          objectId: ref.objectId,
        },
        { kind: "claim", id: ref.objectId },
      );
    case "lens":
      return withSelection(
        {
          loupistId,
          surface: "lens",
          recordId: ref.recordId,
          revisionId: ref.revisionId,
          objectId: ref.objectId,
        },
        { kind: "lens", recordId: ref.recordId },
      );
    case "journal":
      return withSelection({ loupistId, surface: "note", objectId: ref.objectId }, { kind: "journal", journalId: ref.objectId });
    case "research":
      return withSelection(
        { loupistId, surface: "note", objectId: ref.objectId },
        {
          kind: "research",
          recordId: ref.objectId,
          sectionId: ref.spanId,
        },
      );
  }
};

export const knowledgeLocationFromMemoryRef = (
  loupistId: string,
  ref: Pick<MemoryRef, "knowledge">,
): KnowledgeLocation | null => {
  if (!ref.knowledge) return null;
  return knowledgeLocationFromReference(loupistId, ref.knowledge);
};

export const knowledgeLocationFromMemoryChange = (
  loupistId: string,
  change: MemoryUpdateChange,
) =>
  knowledgeLocationFromMemoryRef(loupistId, change);

export const knowledgePinKey = (pin: KnowledgePin) =>
  JSON.stringify([
    pin.kind,
    pin.loupistId,
    pin.recordId ?? "",
    pin.revisionId ?? "",
    pin.objectId,
    pin.spanId ?? "",
    pin.selection ? JSON.stringify(pin.selection) : "",
  ]);

export const sameKnowledgePin = (left: KnowledgePin, right: KnowledgePin) =>
  knowledgePinKey(left) === knowledgePinKey(right);

export const pinForNode = (
  loupistId: string,
  recordId: string,
  revisionId: string,
  node: GraphNode,
): KnowledgePin => ({
  kind: "world_node",
  loupistId,
  recordId,
  revisionId,
  objectId: node.id,
  label: node.name,
});

export const pinForClaim = (
  loupistId: string,
  recordId: string,
  revisionId: string,
  claim: GraphClaim,
): KnowledgePin => ({
  kind: "world_claim",
  loupistId,
  recordId,
  revisionId,
  objectId: claim.id,
  label: claim.proposition,
});

export const pinForSource = (detail: SourceDetail, loupistId: string): KnowledgePin => ({
  kind: "source",
  loupistId,
  recordId: detail.recordId,
  revisionId: detail.revisionId,
  objectId: detail.sourceId,
  spanId: detail.selectedSpanId ?? detail.spans[0]?.spanId,
  label: sourceTitle(detail),
});

export const pinForLens = (loupistId: string, view: { recordId: string; revisionId: string; title: string }): KnowledgePin => ({
  kind: "lens",
  loupistId,
  recordId: view.recordId,
  revisionId: view.revisionId,
  objectId: view.recordId,
  label: view.title,
});
