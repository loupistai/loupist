import type { JournalListRequest, JournalCreateRequest, JournalUpdateRequest, JournalArchiveRequest, JournalInput } from "../../contracts/knowledge";
import type { Principle, PrincipleListRequest, PrincipleCreateRequest, PrincipleUpdateRequest, PrincipleArchiveRequest } from "../../contracts/knowledge";
import type {
  GrowthEvent,
  GrowthEventsQuery,
  GrowthEventsPage,
  BindingRole,
  ClaimStatus,
  JournalRecord,
  JournalSourceRead,
  GraphClaim,
  GraphEdge,
  GraphJobView,
  GraphNode,
  GraphProcessingSettings,
  GraphProcessingView,
  KeepAwakeStatus,
  KnowledgeCatalog,
  KnowledgeCatalogRequest,
  KnowledgeJournalReadRequest,
  KnowledgeJournalSourceReadRequest,
  KnowledgeLensReadRequest,
  KnowledgeResearchSearchRequest,
  KnowledgeSourceReadRequest,
  KnowledgeSourceRetainRequest,
  ResearchRecord,
  RetainedSource,
  SourceDetail,
  TimePoint,
  WorldGraphQuery,
  WorldGraphView,
} from "../../contracts/knowledge";
import type { ModelCatalog } from "../../contracts/model";
import type { LearnedDigest, LensKind, LensLogEntryView, LensSearchHit, PageSummary, PageView, SectionView } from "../../contracts/lens";
import { clone, makeId, shortPause, type MockBridgeState } from "./state";

const WORLD_GRAPH_BYTE_LIMIT = 262_144;
const WORLD_GRAPH_SERVER_CAP = 100;
const sorted = (values: readonly string[] | undefined) =>
  [...(values ?? [])].sort((left, right) =>
    left < right ? -1 : left > right ? 1 : 0,
  );

/** Cursor is excluded so a later page of the same filters stays comparable. */
const worldQueryKey = (request: WorldGraphQuery) =>
  JSON.stringify(
    {
      loupistId: request.loupistId,
      recordId: request.recordId,
      revisionId: request.revisionId,
      search: request.search.trim(),
      centerNodeId: request.centerNodeId ?? null,
      centerClaimId: request.centerClaimId ?? null,
      kinds: sorted(request.kinds),
      relations: sorted(request.relations),
      bindingRoles: sorted(request.bindingRoles),
      statuses: sorted(request.statuses),
      publicationTimes: sorted(request.publicationTimes),
    },
  );


const SEEDED = new Set(["loupist-quality", "loupist-value", "loupist-cycle"]);
const WORLD = "world-protocols";
const LENS = "lens-compare-accounts";
const SOURCE = "source-pack";
const OWNED = "rev-owned-1";
const SOURCE_REVISION = "rev-source-1";
const LENS_REVISION = "rev-lens-1";
const ORDER = [
  "node-tcp",
  "node-http",
  "node-tls",
  "node-udp",
  "node-quic",
  "node-raft",
  "node-paxos",
  "node-clock",
  "node-byzantine",
  "node-treaty",
];

type GraphBody = {
  nodes: GraphNode[];
  edges: GraphEdge[];
  indexingState: WorldGraphView["indexingState"];
  originLabel?: string;
  revisionDigest: string;
};

type LogRow = { recordId: string; entry: LensLogEntryView; before?: SectionView; after?: SectionView };

type Store = {
  memoryControls?: import("../../contracts/knowledge").MemoryControls;
  learnMethods?: boolean;
  seeded: boolean;
  principles: Principle[];
  owned: GraphBody;
  sources: SourceDetail[];
  acquisitions: Record<string, RetainedSource>;
  lenses: PageView[];
  log: LogRow[];
  settings: GraphProcessingSettings;
  jobs: GraphJobView[];
  growth: GrowthEvent[];
  research: ResearchRecord[];
  journals: JournalRecord[];
};

const node = (
  id: string,
  name: string,
  description: string,
  kind: string,
): GraphNode => ({
  id,
  name,
  aliases: id === "node-byzantine" ? ["BFT"] : [],
  description,
  kind,
  sourceRole: id === "node-treaty" ? "entity" : "concept",
  properties: {},
});

const claim = (
  id: string,
  proposition: string,
  relation: string,
  endpointIds: string[],
  extras: {
    attribution?: string;
    qualifiers?: GraphClaim["qualifiers"];
    spanId?: string;
    role?: BindingRole;
    status?: ClaimStatus;
    publicationTime?: TimePoint;
    effectiveTime?: TimePoint;
  } = {},
): GraphClaim => {
  const endpoints = endpointIds.map((nodeId, index) => ({
    nodeId,
    role:
      endpointIds.length === 1 ? "subject" : index === 0 ? "source" : "target",
  }));
  const evidence =
    extras.spanId || extras.role
      ? [
          {
            spanId: extras.spanId ?? "",
            role: extras.role ?? "supports",
            attribution: extras.attribution ?? "Retained account",
          },
        ]
      : [];
  return {
    id,
    proposition,
    attribution: extras.attribution ?? "Retained account",
    relation,
    endpoints,
    qualifiers: extras.qualifiers ?? [],
    evidence,
    status: extras.status ?? "active",
    publicationTime: extras.publicationTime,
    effectiveTime: extras.effectiveTime,
  };
};

const edge = (id: string, body: GraphClaim): GraphEdge => ({
  id,
  relation: body.relation,
  endpoints: body.endpoints,
  status: body.status,
  claim: body,
});

const ownedGraph = (): GraphBody => {
  const nodes = [
    node("node-tcp", "TCP", "Orders bytes on a connection.", "technology:Protocol"),
    node("node-http", "HTTP", "Application protocol for requests.", "technology:Protocol"),
    node("node-tls", "TLS", "Protects a byte stream in transit.", "technology:Protocol"),
    node("node-udp", "UDP", "Exposes datagrams without retransmission.", "technology:Protocol"),
    node("node-quic", "QUIC", "Carries streams over datagrams.", "technology:Protocol"),
    node("node-raft", "Raft", "Elects a leader to replicate a log.", "technology:Algorithm"),
    node("node-paxos", "Paxos", "Agrees on a single value.", "technology:Algorithm"),
    node("node-clock", "Clock synchronization", "Offsets were discussed without a publication date.", "technology:Problem"),
    node("node-byzantine", "Byzantine agreement", "Agreement among members that may fail.", "technology:Problem"),
    node("node-treaty", "Antarctic mineral treaty", "A geopolitical instrument, not a market ticker.", "geopolitics:Treaty"),
  ];
  const claims = [
    claim("claim-http-tcp", "HTTP depends on TCP for ordered delivery.", "depends_on", ["node-http", "node-tcp"], {
      attribution: "RFC 9110 excerpt",
      qualifiers: [{ key: "condition", value: "reliable byte stream" }],
      spanId: "span-status-codes",
      publicationTime: { precision: "date", value: "2022-06-01" },
    }),
    claim("claim-tls-http", "TLS restricts how HTTP bytes are exposed.", "restricts", ["node-tls", "node-http"], {
      status: "disputed",
      publicationTime: { precision: "date", value: "2018-08-01" },
    }),
    claim("claim-quic-udp", "QUIC depends on UDP.", "depends_on", ["node-quic", "node-udp"], {
      publicationTime: { precision: "date", value: "2021-05-01" },
    }),
    claim("claim-raft-paxos", "Raft was influenced by Paxos.", "influenced_by", ["node-raft", "node-paxos"], {
      role: "analysis",
      publicationTime: { precision: "unknown" },
    }),
    claim("claim-byzantine-raft", "Byzantine agreement restricts what Raft assumes.", "restricts", ["node-byzantine", "node-raft"], {
      qualifiers: [{ key: "fault", value: "arbitrary member behavior" }],
      publicationTime: { precision: "date", value: "1999-01-01" },
    }),
    claim("claim-clock", "Clock offsets were not dated.", "described_by", ["node-clock"], {
      publicationTime: { precision: "unknown" },
    }),
    claim("claim-treaty", "The treaty text is the retained account.", "occurs_in", ["node-treaty"], {
      qualifiers: [{ key: "geography", value: "Antarctica" }],
      publicationTime: { precision: "date", value: "1991-10-04" },
    }),
  ];
  return {
    nodes,
    edges: claims.map((item) => edge(`edge-${item.id}`, item)),
    indexingState: "ready",
    originLabel: "Working copy",
    revisionDigest: "digest-owned",
  };
};


const sources = (): SourceDetail[] => [
  {
    sourceId: "source-rfc-9110",
    recordId: SOURCE,
    revisionId: SOURCE_REVISION,
    title: "RFC 9110 HTTP semantics",
    sourceClass: "publisher_document",
    originLabel: "IETF RFC 9110",
    coverage: "included_excerpt",
    material: "extracted_text",
    verification: "verified_bytes",
    acquiredAt: { precision: "instant", value: "2026-10-15T12:00:00.000Z" },
    relatedClaims: [],
    spans: [
      {
        spanId: "span-status-codes",
        selector: "utf8_line_range:1-4",
        contextNote: "Excerpt of section 15, not the full RFC",
        mediaType: "text/plain",
        text: "The status-code element is a three-digit integer code.",
      },
    ],
  },
  {
    sourceId: "source-tool-note",
    recordId: "source-tool-note",
    revisionId: SOURCE_REVISION,
    title: "Tool excerpt of a status line",
    sourceClass: "tool_result",
    originLabel: "Normalized tool text",
    coverage: "upstream_truncated",
    material: "derived",
    verification: "declared_digest",
    acquiredAt: { precision: "instant", value: "2026-10-12T12:00:00.000Z" },
    derivation: "Derived from a tool response, not a publisher original.",
    relatedClaims: [],
    spans: [
      {
        spanId: "span-tool-line",
        selector: "utf8_line_range:1-1",
        contextNote: "Single truncated line",
        mediaType: "text/plain",
        text: "HTTP/1.1 200",
      },
    ],
  },
];

const AT = Date.parse("2026-10-17T15:00:00.000Z");
const MOCK_FILED_MESSAGE = "mock-filed-answer";

const section = (key: string, heading: string, markdown: string, extra: Partial<SectionView> = {}): SectionView => ({
  key, heading, markdown, digest: `digest-${key}`, basis: { refs: [] }, chip: "none", origin: "loupist",
  contested: false, contestedClaimIds: [], updatedAtMs: AT, ...extra,
});

const pageOf = (recordId: string, kind: LensKind, title: string, summary: string, sections: SectionView[], extra: Partial<PageView> = {}): PageView => ({
  recordId, revisionId: `rev-${recordId}-1`, revisionDigest: `digest-${recordId}-1`, kind, title,
  archived: false, paused: false, learned: false, createdAtMs: AT, updatedAtMs: AT,
  body: kind === "idea"
    ? { kind, summary, sections, idea: { status: "open", registeredAtMs: AT, statusAtMs: AT } }
    : { kind: kind as "perspective" | "method" | "subject", summary, sections },
  suggestedSections: [], anchors: [], links: [], backlinks: [], history: [], ...extra,
});

const lensView = (): PageView => pageOf(LENS, "method", "Compare independent accounts", "Read several accounts before preferring one explanation.", [
  section("use-when", "When to use it", "Questions with more than one published account."),
  section("steps", "Steps", "List agreements, disagreements, and missing dates."),
  section("checks", "Checks", "Prefer the account that states its scope."),
  section("limits", "Limits", "This method does not decide which account is true."),
], { revisionId: LENS_REVISION, revisionDigest: "digest-lens-1", suggestedSections: ["use-when", "steps", "checks", "limits"] });

const perspectiveView = (): PageView => pageOf("lens-perspective", "perspective", "Evidence-led perspective", "Treat explanations as claims to examine, with clear sources and conditions.", [
  section("beliefs", "Beliefs", "Separate observation, inference, and uncertainty."),
  section("priorities", "Priorities", "Update the view when stronger counterevidence appears."),
], { suggestedSections: ["beliefs", "priorities", "avoid"] });

const subjectView = (): PageView => pageOf("lens-subject-tcp", "subject", "TCP", "What the Loupist knows about TCP: where it sits in the web stack and where it is being replaced.", [
  section("overview", "Overview", "TCP carries most web traffic. HTTP/1.1 and HTTP/2 depend on it; HTTP/3 does not.", {
    chip: "source", basis: { refs: [{ kind: "claim", claimId: "claim-http-tcp" }, { kind: "span", recordId: SOURCE, revisionDigest: "digest-source-rfc-9110", spanId: "span-status-codes" }] },
  }),
  section("facts", "Facts you told me", "You run TCP-heavy services and care about head-of-line blocking.", {
    chip: "said", origin: "user", basis: { refs: [{ kind: "said", messageId: "mock-user-message" }] },
  }),
  section("views", "Where it is heading", "UDP-based transports will likely take over latency-sensitive traffic first.", {
    chip: "view", basis: { refs: [{ kind: "page", recordId: "lens-idea-quic" }], uncertainty: "Based on one analyst report and adoption trends; no measurement of your own traffic." },
  }),
  section("risks", "Risks", "Middleboxes still drop unfamiliar transports, so migration is uneven.", {
    chip: "source", contested: true, contestedClaimIds: ["claim-quic-blocked"], basis: { refs: [{ kind: "claim", claimId: "claim-quic-blocked" }] },
  }),
], { anchors: [{ role: "subject", nodeId: "node-tcp", name: "TCP" }, { role: "mention", nodeId: "node-quic", name: "QUIC" }], suggestedSections: ["overview", "facts", "value-chain", "drivers", "risks", "views", "open-questions"], learned: true });

const ideaView = (): PageView => pageOf("lens-idea-quic", "idea", "QUIC could displace TCP for mobile traffic", "A connection the Loupist has not verified: handshake savings matter most on lossy mobile links.", [
  section("claim", "Claim", "QUIC saves a round trip, so mobile pages load faster where loss is high.", {
    chip: "source", basis: { refs: [{ kind: "claim", claimId: "claim-http-tcp" }] },
  }),
], { anchors: [{ role: "subject", nodeId: "node-quic", name: "QUIC" }], suggestedSections: ["claim", "mechanism", "evidence-for", "evidence-against", "would-break", "checks"], learned: true });

const seedLog = (): LogRow[] => [
  { recordId: "lens-subject-tcp", entry: { opId: "op-seed-1", op: "put", outcome: "added", key: "overview", origin: "turn", chatId: "mock-chat-1", messageId: MOCK_FILED_MESSAGE, createdAtMs: AT } },
  { recordId: "lens-subject-tcp", entry: { opId: "op-seed-2", op: "put", outcome: "added", key: "risks", origin: "turn", chatId: "mock-chat-1", messageId: MOCK_FILED_MESSAGE, createdAtMs: AT + 1 } },
  { recordId: "lens-idea-quic", entry: { opId: "op-seed-3", op: "put", outcome: "added", key: "claim", origin: "turn", chatId: "mock-chat-1", messageId: MOCK_FILED_MESSAGE, createdAtMs: AT + 2 } },
];

const buildStore = (loupistId: string): Store => ({
  seeded: true,
  principles: [],
  owned: ownedGraph(),
  sources: sources(),
  acquisitions: {},
  growth: [],
  lenses: [lensView(), perspectiveView(), subjectView(), ideaView()],
  log: seedLog(),
  settings: defaultSettings(),
  jobs: [],
  journals: [seedJournal(loupistId)],
  research: [
    {
      id: "research-http",
      title: "HTTP depends on TCP",
      recordedAt: "2026-09-02T00:00:00Z",
      journal: {
        sectionId: "journal-http",
        conclusion: "Treat the dependency as a reported protocol fact, not a valuation.",
        uncertainty: "The excerpt does not cover every extension.",
      },
      inputs: [
        {
          kind: "world_claim",
          loupistId,
          recordId: WORLD,
          revisionId: OWNED,
          objectId: "claim-http-tcp",
          label: "HTTP depends on TCP",
        },
        {
          kind: "source",
          loupistId,
          recordId: SOURCE,
          revisionId: SOURCE_REVISION,
          objectId: "source-rfc-9110",
          spanId: "span-status-codes",
          label: "RFC 9110 HTTP semantics",
        },
      ],
    },
  ],
});

const emptyStore = (): Store => ({
  seeded: false,
  principles: [],
  owned: { nodes: [], edges: [], indexingState: "unavailable", revisionDigest: "none" },
  sources: [],
  acquisitions: {},
  growth: [],
  lenses: [perspectiveView()],
  log: [],
  settings: defaultSettings(),
  jobs: [],
  research: [],
  journals: [],
});

const ensure = (state: MockBridgeState, loupistId: string) => {
  const existing = state.knowledgeStores[loupistId] as Store | undefined;
  if (existing) return existing;
  const created = SEEDED.has(loupistId) ? buildStore(loupistId) : emptyStore();
  if (created.seeded) {
    const atMs = Date.parse("2026-10-17T15:00:00.000Z");
    for (const node of created.owned.nodes) created.growth.push(growthEvent(loupistId, WORLD, "entity_added", node.name, { atMs, subjectId: node.id, actor: "processing" }));
    for (const edge of created.owned.edges) created.growth.push(growthEvent(loupistId, WORLD, "claim_added", edge.claim.proposition, { atMs, subjectId: edge.claim.id, actor: "processing" }));
    for (const lens of created.lenses) created.growth.push(growthEvent(loupistId, lens.recordId, "lens_created", lens.title, { atMs, afterSummary: growthLine(lens.body.kind === "principle" ? lens.body.statement : lens.body.summary || lens.title) }));
    for (const source of created.sources) created.growth.push(growthEvent(loupistId, source.recordId, "source_retained", source.title, { atMs }));
  }
  if (loupistId === "loupist-quality") {
    for (const statement of ["Check operating cash flow before trusting reported profit.", "Treat one customer above 40% of revenue as a concentration risk to investigate."]) {
      const principle: Principle = { recordId: makeId("rec"), revisionId: makeId("rev"), loupistId, statement, origin: "settings", createdAtMs: Date.parse("2026-09-30T00:00:00Z"), updatedAtMs: Date.parse("2026-09-30T00:00:00Z"), archived: false };
      created.principles.push(principle);
      created.lenses.push(principleLens(principle));
      created.growth.push({...growthEvent(loupistId,principle.recordId,"lens_created",statement,{atMs:principle.createdAtMs,actor:"user"}),origin:"settings"});
    }
  }
  state.knowledgeStores[loupistId] = created;
  return created;
};

const encodeCursor = (body: {
  loupistId: string;
  recordId: string;
  revisionId: string;
  queryKey: string;
  offset: number;
}) => `kg1.${btoa(JSON.stringify(body))}`;

const decodeCursor = (cursor: string) => {
  if (!cursor.startsWith("kg1.")) return null;
  try {
    const parsed = JSON.parse(atob(cursor.slice(4))) as {
      loupistId?: string;
      recordId?: string;
      revisionId?: string;
      queryKey?: string;
      offset?: number;
    };
    if (
      !parsed.loupistId ||
      !parsed.recordId ||
      !parsed.revisionId ||
      !parsed.queryKey ||
      typeof parsed.offset !== "number"
    ) {
      return null;
    }
    return parsed as Required<typeof parsed>;
  } catch {
    return null;
  }
};

const publicationValue = (edge: GraphEdge) => edge.claim.publicationTime?.value ?? "undated";

const filterGraph = (graph: GraphBody, request: WorldGraphQuery) => {
  const search = (request.search ?? "").trim().toLowerCase();
  const kinds = request.kinds ?? [];
  const relations = request.relations ?? [];
  const roles = request.bindingRoles ?? [];
  const statuses = request.statuses ?? [];
  const publicationTimes = request.publicationTimes ?? [];
  const eligibleNodes = graph.nodes.filter((node) => !kinds.length || kinds.includes(node.kind));
  const nodesById = new Map(eligibleNodes.map((node) => [node.id, node]));
  const searchNodes = new Set(eligibleNodes.filter((node) => !search || [node.name, node.kind, node.description, ...node.aliases]
    .some((value) => value.toLowerCase().includes(search))).map((node) => node.id));
  const edges = graph.edges.filter((edge) => (!relations.length || relations.includes(edge.relation))
    && (!roles.length || edge.claim.evidence.some((binding) => roles.includes(binding.role)))
    && (!statuses.length || statuses.includes(edge.status))
    && (!publicationTimes.length || publicationTimes.includes(publicationValue(edge)))
    && edge.endpoints.every((endpoint) => nodesById.has(endpoint.nodeId))
    && (!search || edge.endpoints.some((endpoint) => searchNodes.has(endpoint.nodeId))
      || [edge.claim.proposition, edge.claim.attribution, edge.relation, ...edge.claim.qualifiers.map((qualifier) => qualifier.value)]
        .some((value) => value.toLowerCase().includes(search))));
  const visible = new Set(relations.length || roles.length || statuses.length || publicationTimes.length ? [] : searchNodes);
  for (const edge of edges) for (const endpoint of edge.endpoints) visible.add(endpoint.nodeId);
  return { nodes: eligibleNodes.filter((node) => visible.has(node.id)), edges };
};

const project = (graph: GraphBody, request: WorldGraphQuery) => {
  if (request.centerClaimId) {
    if (request.centerNodeId) throw new Error("Choose one World focus target.");
    const target = graph.edges.find((edge) => edge.claim.id === request.centerClaimId);
    if (!target) throw new Error("World claim not found.");
    const ids = new Set(target.endpoints.map((endpoint) => endpoint.nodeId));
    return { nodes: graph.nodes.filter((node) => ids.has(node.id)), edges: [target] };
  }
  let { nodes, edges } = filterGraph(graph, request);
  const anchors = new Set(request.centerNodeId ? [request.centerNodeId] : []);
  if (anchors.size) {
    const center = graph.nodes.find((node) => node.id === request.centerNodeId);
    if (!center) throw new Error("World entity not found.");
    if (!nodes.some((node) => node.id === center.id)) nodes.push(center);
    const keep = new Set(anchors);
    edges = edges.filter((edge) => edge.endpoints.some((endpoint) => anchors.has(endpoint.nodeId)));
    for (const item of edges) item.endpoints.forEach((endpoint) => keep.add(endpoint.nodeId));
    nodes = nodes.filter((item) => keep.has(item.id));
  }
  const visible = new Set(nodes.map((item) => item.id));
  edges = edges.filter((item) =>
    item.endpoints.every((endpoint) => visible.has(endpoint.nodeId)),
  );
  nodes.sort((left, right) => Number(anchors.has(right.id)) - Number(anchors.has(left.id)) || ORDER.indexOf(left.id) - ORDER.indexOf(right.id));
  return { nodes, edges };
};

const blank = (
  request: WorldGraphQuery,
  graph: GraphBody,
): WorldGraphView => ({
  recordId: request.recordId,
  revisionId: request.revisionId,
  revisionDigest: graph.revisionDigest,
  nodes: [],
  edges: [],
  coverage: "sealed_snapshot",
  indexingState: graph.indexingState,
  truncated: false,
  originLabel: graph.originLabel,
  appliedCursor: null,
});

export const knowledgeCatalog = async (
  state: MockBridgeState,
  request: KnowledgeCatalogRequest,
): Promise<KnowledgeCatalog> => {
  await shortPause(state);
  const store = ensure(state, request.loupistId);
  return {
    loupistId: request.loupistId,
    worlds: [{ recordId: WORLD, kind: "world", title: "World", revisionId: OWNED, revisionDigest: store.owned.revisionDigest,
      readOnly: false, indexingState: store.owned.indexingState, objects: [] }],
    lenses: store.lenses.map((lens) => ({ recordId: lens.recordId, kind: "lens" as const, title: lens.title, lensKind: lens.kind, archived: lens.archived,
      updatedAtMs: lens.updatedAtMs, summary: pageSummary(lens), learned: lens.learned || store.log.some((row) => row.recordId === lens.recordId),
      revisionId: lens.revisionId, revisionDigest: lens.revisionDigest, readOnly: false, indexingState: "ready",
      objects: [{ id: lens.recordId, title: lens.title }] })),
    sources: store.sources.map((source) => ({ recordId: source.recordId, kind: "source", title: source.title,
      revisionId: source.revisionId, revisionDigest: `digest-${source.sourceId}`,
      readOnly: false, indexingState: "ready", objects: [{ id: source.sourceId, title: source.title, summary: source.spans[0]?.text?.slice(0, 180), originLabel: source.originLabel, origin: source.origin, acquiredAt: source.acquiredAt, publicationTime: source.publicationTime, processing: source.processing }] })),
  };
};

export const knowledgeWorldQuery = async (
  state: MockBridgeState,
  request: WorldGraphQuery,
): Promise<WorldGraphView> => {
  await shortPause(state);
  const store = ensure(state, request.loupistId);
  if (request.recordId !== WORLD) throw new Error("World not found.");
  const graph =
    request.revisionId === OWNED ? store.owned : null;
  if (!graph) throw new Error("World revision not found.");
  const queryKey = worldQueryKey(request);
  let offset = 0;
  if (request.cursor) {
    const decoded = decodeCursor(request.cursor);
    const matches =
      decoded &&
      decoded.loupistId === request.loupistId &&
      decoded.recordId === request.recordId &&
      decoded.revisionId === request.revisionId &&
      decoded.queryKey === queryKey;
    if (!matches) return blank(request, graph);
    offset = decoded.offset;
  }
  const projected = project(graph, request);
  const requiredEndpoints = request.centerClaimId ? projected.edges.find((edge) => edge.claim.id === request.centerClaimId)?.endpoints.length ?? 0 : 0;
  const limit = Math.min(WORLD_GRAPH_SERVER_CAP, Math.max(1, requiredEndpoints, request.nodeLimit));
  const pageNodes = projected.nodes.slice(offset, offset + limit);
  const ids = new Set(pageNodes.map((item) => item.id));
  const pageEdges = projected.edges.filter((item) =>
    item.endpoints.every((endpoint) => ids.has(endpoint.nodeId)),
  );
  const nextOffset = offset + pageNodes.length;
  return clone({
    recordId: request.recordId,
    revisionId: request.revisionId,
    revisionDigest: graph.revisionDigest,
    nodes: pageNodes,
    edges: pageEdges,
    sourceTargets: Object.fromEntries(pageEdges.flatMap((edge) => edge.claim.evidence.flatMap((binding) => {
      const source = store.sources.find((item) => item.spans.some((span) => span.spanId === binding.spanId));
      return source ? [[binding.spanId, { recordId: source.recordId, revisionId: source.revisionId, sourceId: source.sourceId, spanId: binding.spanId, worldRecordId: request.recordId, worldRevisionId: request.revisionId }]] : [];
    }))),
    nextCursor:
      nextOffset < projected.nodes.length
        ? encodeCursor({
            loupistId: request.loupistId,
            recordId: request.recordId,
            revisionId: request.revisionId,
            queryKey,
            offset: nextOffset,
          })
        : undefined,
    coverage: "sealed_snapshot",
    indexingState: graph.indexingState,
    truncated: nextOffset < projected.nodes.length || request.byteLimit < WORLD_GRAPH_BYTE_LIMIT,
    originLabel: graph.originLabel,
    appliedCursor: request.cursor ?? null,
  });
};

export const knowledgeSourceRead = async (
  state: MockBridgeState,
  request: KnowledgeSourceReadRequest,
): Promise<SourceDetail> => {
  await shortPause(state);
  const store = ensure(state, request.loupistId);
  const found = store.sources.find((item) => {
    if (item.recordId !== request.recordId || item.revisionId !== request.revisionId) return false;
    if (request.sourceId && item.sourceId !== request.sourceId) return false;
    if (request.spanId) return item.spans.some((span) => span.spanId === request.spanId);
    return !request.sourceId || item.sourceId === request.sourceId;
  });
  if (!found) throw new Error("Source revision not found.");
  return clone({
    ...found,
    origin: found.origin,
    relatedClaims: store.owned.edges.filter((edge) => edge.claim.evidence.some((evidence) => found.spans.some((span) => span.spanId === evidence.spanId))).map((edge) => ({ worldRecordId: WORLD, worldRevisionId: OWNED, claimId: edge.claim.id, proposition: edge.claim.proposition, relation: edge.relation })),
    selectedSpanId:
      request.spanId && found.spans.some((span) => span.spanId === request.spanId)
        ? request.spanId
        : found.spans[0]?.spanId,
  });
};

const pageSummary = (page: PageView) => page.body.kind === "principle" ? page.body.statement : page.body.summary;
const pageSections = (page: PageView): SectionView[] => page.body.kind === "principle" ? [] : page.body.sections;
const pageText = (page: PageView) => `${page.title} ${pageSummary(page)} ${pageSections(page).map((item) => `${item.heading} ${item.markdown}`).join(" ")}`;

function lensLinkIds(page: PageView): Set<string> {
  const ids = new Set<string>();
  for (const item of pageSections(page)) for (const match of item.markdown.matchAll(/\]\(lens:([^)\s]+)\)/gu)) ids.add(match[1]!);
  for (const item of pageSections(page)) for (const ref of item.basis.refs) if (ref.kind === "page") ids.add(ref.recordId);
  return ids;
}

const linkView = (page: PageView) => ({ recordId: page.recordId, title: page.title, kind: page.kind });

function readPage(store: Store, page: PageView): PageView {
  const outgoing = lensLinkIds(page);
  const current = [...store.lenses].filter((lens) => !lens.archived).sort((a, b) => a.title.localeCompare(b.title));
  const history = store.log.filter((row) => row.recordId === page.recordId).map((row) => row.entry).sort((a, b) => b.createdAtMs - a.createdAtMs).slice(0, 20);
  return clone({
    ...page,
    links: current.filter((lens) => outgoing.has(lens.recordId)).map(linkView),
    backlinks: current.filter((lens) => lensLinkIds(lens).has(page.recordId)).map(linkView),
    history,
  });
}

export const knowledgeLensRead = async (state: MockBridgeState, request: KnowledgeLensReadRequest): Promise<PageView> => {
  await shortPause(state);
  const store = ensure(state, request.loupistId);
  const lens = store.lenses.find((item) => item.recordId === request.recordId);
  if (!lens) throw new Error("Lens page not found.");
  return readPage(store, lens);
};

export const knowledgeLensPagesForNode = async (state: MockBridgeState, request: { loupistId: string; nodeId: string }): Promise<PageSummary[]> => {
  await shortPause(state);
  return ensure(state, request.loupistId).lenses
    .filter((lens) => !lens.archived && lens.anchors.some((anchor) => anchor.nodeId === request.nodeId))
    .map((lens) => ({ recordId: lens.recordId, title: lens.title, kind: lens.kind, summary: pageSummary(lens) }));
};

export const knowledgeLensSearch = async (state: MockBridgeState, request: { loupistId: string; query: string; kinds?: LensKind[] }): Promise<LensSearchHit[]> => {
  await shortPause(state);
  const terms = request.query.normalize("NFKC").toLocaleLowerCase("en-US").match(/[\p{L}\p{N}]+/gu) ?? [];
  return ensure(state, request.loupistId).lenses
    .filter((lens) => !lens.archived && (!request.kinds?.length || request.kinds.includes(lens.kind)))
    .filter((lens) => {
      const text = pageText(lens).normalize("NFKC").toLocaleLowerCase("en-US");
      return terms.every((term) => text.includes(term));
    })
    .map((lens) => ({ recordId: lens.recordId, revisionId: lens.revisionId, kind: lens.kind, title: lens.title, summary: pageSummary(lens), snippet: pageSummary(lens).slice(0, 180) }));
};

export function mockFileSection(state: MockBridgeState, loupistId: string, recordId: string, next: { key: string; heading: string; markdown: string; chatId?: string; messageId?: string }): PageView {
  const store = ensure(state, loupistId);
  const page = store.lenses.find((item) => item.recordId === recordId);
  if (!page || page.body.kind === "principle") throw new Error("Lens page not found.");
  const now = Date.now();
  const before = page.body.sections.find((item) => item.key === next.key);
  const filed: SectionView = section(next.key, next.heading, next.markdown, { digest: makeId("digest"), updatedAtMs: now, basis: { refs: [] } });
  page.body.sections = before ? page.body.sections.map((item) => item.key === next.key ? filed : item) : [...page.body.sections, filed];
  page.updatedAtMs = now;
  page.revisionId = makeId("rev");
  store.log.push({ recordId, before, after: filed, entry: { opId: makeId("op"), op: "put", outcome: before ? "replaced" : "added", key: next.key, origin: "turn", chatId: next.chatId, messageId: next.messageId, createdAtMs: now } });
  return readPage(store, page);
}

export const memorySectionForget = async (state: MockBridgeState, request: { loupistId: string; recordId: string; key: string }): Promise<PageView> => {
  await shortPause(state);
  const store = ensure(state, request.loupistId);
  const page = store.lenses.find((item) => item.recordId === request.recordId);
  if (!page || page.body.kind === "principle") throw new Error("Lens page not found.");
  const before = page.body.sections.find((item) => item.key === request.key);
  if (!before) throw new Error("Section not found.");
  const now = Date.now();
  page.body.sections = page.body.sections.filter((item) => item.key !== request.key);
  page.updatedAtMs = now;
  store.log.push({ recordId: request.recordId, before, entry: { opId: makeId("op"), op: "retire", outcome: "retired", key: request.key, origin: "user", createdAtMs: now } });
  return readPage(store, page);
};

export const memoryOpUndo = async (state: MockBridgeState, request: { loupistId: string; opId: string }): Promise<PageView> => {
  await shortPause(state);
  const store = ensure(state, request.loupistId);
  const row = store.log.find((item) => item.entry.opId === request.opId);
  const page = row && store.lenses.find((item) => item.recordId === row.recordId);
  if (!row || !page || page.body.kind === "principle") throw new Error("Nothing to undo.");
  const key = row.entry.key!;
  const later = store.log.some((item) => item.recordId === row.recordId && item.entry.key === key && item.entry.createdAtMs > row.entry.createdAtMs);
  if (later || store.log.some((item) => item.entry.undoesOpId === request.opId)) throw new Error("A later change touched this section; undo it first.");
  const now = Date.now();
  const restored = row.entry.outcome === "added" ? undefined : row.before;
  page.body.sections = restored
    ? page.body.sections.some((item) => item.key === key) ? page.body.sections.map((item) => item.key === key ? restored : item) : [...page.body.sections, restored]
    : page.body.sections.filter((item) => item.key !== key);
  page.updatedAtMs = now;
  store.log.push({ recordId: row.recordId, entry: { opId: makeId("op"), op: row.entry.outcome === "retired" ? "put" : "retire", outcome: row.entry.outcome === "retired" ? "added" : "retired", key, origin: "user", createdAtMs: now, undoesOpId: request.opId } });
  return readPage(store, page);
};

export const memoryLearned = async (state: MockBridgeState, request: { loupistId: string; messageIds: string[] }): Promise<LearnedDigest[]> => {
  await shortPause(state);
  const store = ensure(state, request.loupistId);
  return request.messageIds.map((messageId) => {
    const rows = store.log.filter((row) => row.entry.messageId === messageId);
    const pages = [...new Set(rows.map((row) => row.recordId))].flatMap((recordId) => {
      const page = store.lenses.find((item) => item.recordId === recordId);
      if (!page) return [];
      const own = rows.filter((row) => row.recordId === recordId);
      return [{ recordId, title: page.title, kind: page.kind, created: own.some((row) => row.entry.outcome === "added") && !store.log.some((row) => row.recordId === recordId && row.entry.createdAtMs < own[0]!.entry.createdAtMs),
        outcomes: own.map((row) => ({ opId: row.entry.opId, outcome: row.entry.outcome, key: row.entry.key })) }];
    });
    return { messageId, pages };
  });
};

export const knowledgeResearchSearch = async (
  state: MockBridgeState,
  request: KnowledgeResearchSearchRequest,
) => {
  await shortPause(state);
  const store = ensure(state, request.loupistId);
  const needle = request.query?.trim().toLocaleLowerCase("en-US") ?? "";
  const records = store.research.filter((record) => {
    if (request.claimId && !record.inputs.some((input) => input.objectId === request.claimId)) {
      return false;
    }
    if (!needle) return true;
    return `${record.title} ${record.journal?.conclusion ?? ""}`
      .toLocaleLowerCase("en-US")
      .includes(needle);
  });
  return { loupistId: request.loupistId, records: clone(records) };
};

const defaultSettings = (): GraphProcessingSettings => ({
  enrichmentEnabled: false,
  paused: false,
  helperModelProfileId: null,
  thinkingLevel: "off",
  runOptions: {},
  maxLlmCalls: 16,
  timeoutMs: 240_000,
  researchFocus: "",
  relationHints: [],
  anchors: [],
  fastModel: null,
});

const textMedia = (mediaType: string) =>
  mediaType.startsWith("text/") || mediaType === "application/json";

export const knowledgeSourceRetain = async (
  state: MockBridgeState,
  request: KnowledgeSourceRetainRequest,
): Promise<RetainedSource> => {
  await shortPause(state);
  const store = ensure(state, request.loupistId);
  if (request.file && request.text.trim()) {
    throw new Error("send pasted text or one original file, not both");
  }
  if (request.file && request.coverage === "metadata_only") {
    throw new Error("an original file needs included coverage");
  }
  const acquisition = JSON.stringify([
    request.originLocator, request.coverage,
    request.publicationTime?.precision ?? null,
    request.publicationTime?.value ?? null,
    request.publicationTime?.approximate ?? false,
    request.file?.fileName ?? "imported-text",
    request.file?.mediaType ?? "text/plain",
    request.coverage === "metadata_only" ? null : request.file?.bytesBase64 ?? request.text,
  ]);
  const previous = store.acquisitions[acquisition];
  if (previous) return clone(previous);
  const sourceId = makeId("source");
  const recordId = sourceId;
  const revisionId = makeId("source-revision");
  const spanId = request.coverage === "metadata_only" ? undefined : makeId("span");
  const retainedFile = request.file;
  const mediaType = retainedFile?.mediaType ?? "text/plain";
  let fileText: string | undefined;
  if (retainedFile && textMedia(mediaType)) {
    try {
      fileText = atob(retainedFile.bytesBase64);
    } catch {
      fileText = undefined;
    }
  }
  store.sources.push({
    sourceId,
    recordId,
    revisionId,
    title: request.title,
    sourceClass: request.file ? "user_file" : "user_statement",
    originLabel: request.originLocator,
    coverage: request.coverage,
    material: request.coverage === "included_excerpt" ? "excerpt" : "original",
    verification: request.coverage === "metadata_only" ? "declared_digest" : "verified_bytes",
    publicationTime: request.publicationTime,
    relatedClaims: [],
    spans: spanId
      ? [
          {
            spanId,
            selector: request.file ? "byte_range:0" : "utf8_line_range:1-1",
            contextNote: request.file ? "Original file bytes" : "Exact original text",
            mediaType,
            text: request.file ? fileText : request.text,
          },
        ]
      : [],
  });
  const retained = store.sources.at(-1)!;
  const readable = retained.spans.some((span) => Boolean(span.text));
  if (readable && store.settings.enrichmentEnabled) {
    const job: GraphJobView = { jobId: makeId("job"), loupistId: request.loupistId, worldRecordId: WORLD, worldRevisionId: OWNED, status: store.settings.paused ? "paused_budget" : "queued", payloadDigest: spanId!, createdAtMs: Date.now(), hostMayRedrive: false };
    store.jobs.push(job);
    retained.processing = { jobId: job.jobId, status: job.status };
  } else retained.processing = { status: readable ? "disabled" : "no_text" };
  const result: RetainedSource = {
    recordId,
    sourceId,
    processing: retained.processing,
    captureId: makeId("capture"),
    spanId,
    revisionId,
    revisionDigest: "digest-retained",
  };
  store.growth.push(growthEvent(request.loupistId, recordId, "source_retained", request.title));
  store.acquisitions[acquisition] = result;
  return clone(result);
};

export const knowledgeGraphJobs = async (
  state: MockBridgeState,
  request: { loupistId: string },
) => {
  await shortPause(state);
  return clone(ensure(state, request.loupistId).jobs);
};

export const knowledgeGraphJob = async (
  state: MockBridgeState,
  request: { loupistId: string; jobId: string },
) => {
  await shortPause(state);
  const job = ensure(state, request.loupistId).jobs.find((item) => item.jobId === request.jobId);
  if (!job) throw new Error("Job not found.");
  return clone(job);
};

export const graphProcessingView = (settings: GraphProcessingSettings, catalog: ModelCatalog, jobs: GraphJobView[] = []): GraphProcessingView => {
  const model = catalog.models.find((item) => item.id === settings.helperModelProfileId);
  const provider = catalog.providers.find((item) => item.id === model?.provider);
  const status = jobs.some((job) => job.status === "reconcile_required") ? "reconciliation_required" : !settings.enrichmentEnabled ? "disabled" : settings.paused ? "paused" : !settings.helperModelProfileId ? "model_required" : !model ? "model_unavailable" : model.credential_source === "missing" || provider?.credential_source === "missing" ? "provider_disconnected" : "ready";
  return { settings, readiness: { status, providerName: provider?.label ?? null, modelName: model?.name ?? null, authKind: provider?.api_key === false && provider?.oauth ? "oauth" : model ? "api_key" : null, note: null } };
};

export const knowledgeGraphRecover = async (state: MockBridgeState, request: { loupistId: string }) => {
  await shortPause(state);
  const store = ensure(state, request.loupistId);
  const interrupted = new Set(store.jobs.filter((job) => job.status === "reconcile_required").map((job) => job.jobId));
  store.jobs = store.jobs.map((job) => interrupted.has(job.jobId) ? { ...job, status: "reconciled_without_retry", hostMayRedrive: false } : job);
  store.sources = store.sources.map((source) => source.processing?.jobId && interrupted.has(source.processing.jobId) ? { ...source, processing: { ...source.processing, status: "reconciled_without_retry" } } : source);
  return clone(store.settings);
};

export const knowledgeGraphSettingsGet = async (
  state: MockBridgeState,
  request: { loupistId: string },
) => {
  await shortPause(state);
  ensure(state, request.loupistId);
  return clone(ensure(state, request.loupistId).settings);
};

export const knowledgeGraphSettingsSet = async (
  state: MockBridgeState,
  request: { loupistId: string; settings: GraphProcessingSettings },
) => {
  await shortPause(state);
  ensure(state, request.loupistId).settings = clone(request.settings);
  return clone(request.settings);
};

export const knowledgeGraphPause = async (
  state: MockBridgeState,
  request: { loupistId: string; paused: boolean },
) => {
  const current = await knowledgeGraphSettingsGet(state, request);
  current.paused = request.paused;
  return knowledgeGraphSettingsSet(state, { loupistId: request.loupistId, settings: current });
};

/** One choice for every Loupist, like the host's. The preview has no World work left to wait for. */
export const worldKeepAwakeGet = async (state: MockBridgeState): Promise<KeepAwakeStatus> => {
  await shortPause(state);
  return { supported: true, enabled: state.keepAwake ?? false, active: false, onBattery: false, remaining: 0 };
};

export const worldKeepAwakeSet = async (state: MockBridgeState, request: { enabled: boolean }) => {
  await shortPause(state);
  state.keepAwake = request.enabled;
  return worldKeepAwakeGet(state);
};

export const knowledgeJournalRead = async (state: MockBridgeState, request: KnowledgeJournalReadRequest): Promise<JournalRecord> => {
  await shortPause(state);
  const record = ensure(state, request.loupistId).journals.find(j => j.journalId === request.journalId);
  if (!record) throw new Error("Journal entry not found.");
  return clone(record);
};
function seedJournal(loupistId: string): JournalRecord {
  return {
    journalId: "journal-http",
    check: { claim: "Hanul’s inventory days fall below 50", metric: "Inventory days", direction: "below", threshold: 50, byDate: "2026-12-31", confidence: 70 },
    outcome: null,
    judgment: "Treat the dependency as a reported protocol fact, not a valuation.",
    reasoning: "The retained excerpt states the dependency.",
    loupistId, researchId: "research-http", entities: ["node-http"], originChatId: "chat-quality-1", createdAtMs: Date.parse("2026-09-02T00:00:00Z"), updatedAtMs: Date.parse("2026-09-02T00:00:00Z"), archived: false, inputDigest: "mock-input",
    inputs: {
      sources: [
        {
          receiptId: "receipt-http",
          recordId: SOURCE,
          revisionId: SOURCE_REVISION,
          spanId: "span-status-codes",
          coverage: "included_excerpt",
          attribution: "IETF RFC 9110",
          publicationTime: { precision: "date", value: "2022-06-01" },
        },
      ],
      worlds: [
        {
          recordId: WORLD,
          revisionId: OWNED,
          nodes: [node("node-http", "HTTP", "Copied node", "technology:Protocol")],
          edges: [],
        },
      ],
      lenses: [
        {
          recordId: LENS,
          revisionId: LENS_REVISION,
          markdown: "# Compare independent accounts\n\n## use-when\nQuestions with more than one published account.",
          status: "considered",
        },
      ],
    },
  };
};

export const knowledgeJournalSourceRead = async (
  state: MockBridgeState,
  request: KnowledgeJournalSourceReadRequest,
): Promise<JournalSourceRead> => {
  await shortPause(state);
  const journal = await knowledgeJournalRead(state, request);
  const pinned = journal.inputs.sources.find(
    (source) => source.receiptId === request.receiptId,
  );
  if (!pinned) throw new Error("Receipt not found.");
  return {
    mediaType: "text/plain",
    byteLength: 52,
    text: "The status-code element is a three-digit integer code.",
    pinned,
  };
};

const growthLine = (text: string) => [...text.trim().replace(/\s+/gu, " ")].slice(0, 240).join("");

const growthEvent = (loupistId: string, recordId: string, kind: GrowthEvent["kind"], summary: string, extra: Partial<GrowthEvent> = {}): GrowthEvent => ({
  eventId: makeId("growth"), loupistId, recordId, atMs: Date.now(), kind, summary: growthLine(summary),
  subjectId: null, originChatId: null, originMessageId: null, actor: "user", beforeSummary: null, afterSummary: null,
  ...extra,
});

export const knowledgeGrowthEvents = async (state: MockBridgeState, request: GrowthEventsQuery): Promise<GrowthEventsPage> => {
  await shortPause(state);
  const limit = request.limit ?? 500;
  if (!Number.isInteger(limit) || limit < 1 || limit > 5000) throw new Error("growth event limit must be between 1 and 5000");
  for (const time of [request.sinceMs, request.untilMs]) if (time != null && (!Number.isSafeInteger(time) || time < 0)) throw new Error("growth event time is invalid");
  if (request.sinceMs != null && request.untilMs != null && request.sinceMs > request.untilMs) throw new Error("growth event time range is invalid");
  type Cursor = { loupistId: string; sinceMs: number | null; untilMs: number | null; atMs: number; eventId: string };
  let cursor: Cursor | undefined;
  if (request.cursor != null) {
    try {
      if (request.cursor.length > 4096) throw new Error();
      cursor = JSON.parse(request.cursor) as Cursor;
      if (!cursor || cursor.loupistId !== request.loupistId || cursor.sinceMs !== (request.sinceMs ?? null) || cursor.untilMs !== (request.untilMs ?? null) || !Number.isSafeInteger(cursor.atMs) || typeof cursor.eventId !== "string") throw new Error();
    } catch { throw new Error("growth event cursor is invalid"); }
  }
  const candidates = ensure(state, request.loupistId).growth.filter((event) =>
    (request.sinceMs == null || event.atMs >= request.sinceMs) && (request.untilMs == null || event.atMs <= request.untilMs)
    && (!cursor || event.atMs > cursor.atMs || (event.atMs === cursor.atMs && event.eventId > cursor.eventId))
  ).sort((a, b) => a.atMs - b.atMs || (a.eventId < b.eventId ? -1 : a.eventId > b.eventId ? 1 : 0));
  const events = candidates.slice(0, limit);
  const last = events.at(-1);
  return clone({ events, nextCursor: candidates.length > limit && last ? JSON.stringify({ loupistId: request.loupistId, sinceMs: request.sinceMs ?? null, untilMs: request.untilMs ?? null, atMs: last.atMs, eventId: last.eventId }) : null });
};

/** Fixed 30-day financial research fixture for the Map design preview. */
export async function knowledgeMap(state: MockBridgeState, request: import("../../contracts/knowledge").KnowledgeMapQuery): Promise<import("../../contracts/knowledge").KnowledgeMap> {
  const limit = request.maxNodes ?? 1000;
  if (!Number.isInteger(limit) || limit < 1 || limit > 1500) throw new Error("map maxNodes must be between 1 and 1500");
  ensure(state, request.loupistId);
  const start = Date.UTC(2026, 8, 1);
  const day = 86_400_000;
  const labels: Array<[string, string, string[]?]> = [
    ["Apple", "Organization"], ["Microsoft", "Organization"], ["TSMC", "Organization"],
    ["NVIDIA", "Organization"], ["ASML", "Organization"], ["Samsung Electronics", "Organization", ["삼성전자"]],
    ["Amazon", "Organization"], ["Alphabet", "Organization"], ["Broadcom", "Organization"],
    ["SK hynix", "Organization", ["SK하이닉스"]], ["United States", "Place"], ["South Korea", "Place", ["대한민국"]],
    ["Taiwan", "Place"], ["Netherlands", "Place"], ["Japan", "Place"],
    ["Semiconductors", "Entity"], ["Cloud computing", "Entity"], ["Consumer electronics", "Entity"],
    ["Artificial intelligence", "Product"], ["Advanced packaging", "Product"], ["EUV lithography", "Product"],
    ["High bandwidth memory", "Product"], ["Data centers", "Product"], ["Electric grid", "Product"],
    ["Federal Reserve", "Organization"], ["Bank of Korea", "Organization", ["한국은행"]], ["US dollar", "Asset"],
    ["Korean won", "Asset", ["원화"]], ["Capital expenditure", "Metric"], ["Operating margin", "Metric"],
    ["Free cash flow", "Metric"], ["Inventory cycle", "Entity"], ["Export controls", "Policy"],
    ["Supply concentration", "Entity"], ["Energy demand", "Entity"], ["Pricing power", "Entity"], ["Earnings season", "Event"],
  ];
  const nodes: import("../../contracts/knowledge").KnowledgeMapNode[] = SEEDED.has(request.loupistId) ? labels.map(([label, entityKind, localNames], i) => ({
    origin: undefined,
    id: `map-entity-${String(i).padStart(2, "0")}`, kind: "entity", label: label!, entityKind, localNames,
    degree: 0, firstSeenAtMs: start + Math.floor(i * 29 / 39) * day, watched: i < 2, held: i === 2,
  })) : [];
  if (nodes.length) for (const [i, label] of ["Evidence before conviction", "Compare competitive advantages", "Trace the capital cycle"].entries()) {
    nodes.push({ id: `map-lens-${i}`, recordId: `map-lens-${i}`, kind: i === 0 ? "lens_perspective" : "lens_method", label, degree: 0, archived: false, watched: false, held: false, firstSeenAtMs: start + (i === 0 ? 0 : 27 + i) * day });
  }
  if (nodes.length) nodes.push({ id: "map-lens-3", recordId: "lens-subject-tcp", kind: "lens_subject", label: "TCP", degree: 0, archived: false, watched: false, held: false, firstSeenAtMs: start + 28 * day }, { id: "map-lens-4", recordId: "lens-idea-quic", kind: "lens_idea", label: "QUIC could displace TCP for mobile traffic", degree: 0, archived: false, watched: false, held: false, firstSeenAtMs: start + 29 * day });
  for (const p of ensure(state,request.loupistId).principles) nodes.push({id:p.recordId,recordId:p.recordId,kind:"lens_principle",label:p.statement,degree:0,archived:p.archived,watched:false,held:false,firstSeenAtMs:p.createdAtMs});
  const edges: import("../../contracts/knowledge").KnowledgeMapEdge[] = SEEDED.has(request.loupistId) ? labels.slice(1).map((_, i) => ({
    id: `map-claim-${String(i).padStart(2, "0")}`, kind: "claim", source: nodes[Math.floor(i / 4)]!.id, target: nodes[i + 1]!.id,
    relation: ["supplies", "depends_on", "exposed_to", "associated_with"][i % 4], status: "active", firstSeenAtMs: nodes[i + 1]!.firstSeenAtMs,
  })) : [];
  if (nodes.length) {
    edges.push({ id: "map-lens-link-1", kind: "lens_link", source: "map-lens-0", target: "map-lens-1" }, { id: "map-lens-link-2", kind: "lens_link", source: "map-lens-1", target: "map-lens-2" });
    edges.push({ id: "map-application-subject", kind: "applies_to", source: "map-lens-3", target: "map-entity-00", weight: 10, firstSeenAtMs: start + 28 * day }, { id: "map-application-idea", kind: "applies_to", source: "map-lens-4", target: "map-entity-01", weight: 8, firstSeenAtMs: start + 29 * day });
    for (let i = 0; i < 10; i++) edges.push({ id: `map-application-${i}`, kind: "applies_to", source: `map-lens-${i % 2 + 1}`, target: nodes[i]!.id, weight: 12 - i, firstSeenAtMs: start + (28 + i % 2) * day });
  }
  for (const edge of edges) for (const id of new Set([edge.source, edge.target])) { const node = nodes.find((n) => n.id === id); if (node) node.degree++; }
  const ranked = [...nodes].sort((a, b) => Number(b.watched || b.held) - Number(a.watched || a.held) || Number(b.kind !== "entity") - Number(a.kind !== "entity") || b.degree - a.degree || a.id.localeCompare(b.id));
  const chosen = new Set(ranked.slice(0, limit).map((node) => node.id));
  const shownNodes = nodes.filter((node) => chosen.has(node.id)).sort((a, b) => a.id.localeCompare(b.id));
  const shownEdges = edges.filter((edge) => chosen.has(edge.source) && chosen.has(edge.target)).sort((a, b) => a.kind.localeCompare(b.kind) || a.id.localeCompare(b.id));
  const counts = {} as import("../../contracts/knowledge").KnowledgeMap["counts"];
  for (const kind of ["entity", "lens_perspective", "lens_method", "lens_principle", "lens_subject", "lens_idea", "claim", "lens_link", "applies_to"] as const) {
    const total = nodes.filter((node) => node.kind === kind).length + edges.filter((edge) => edge.kind === kind).length;
    const shown = shownNodes.filter((node) => node.kind === kind).length + shownEdges.filter((edge) => edge.kind === kind).length;
    counts[kind] = { total, shown, truncated: shown < total };
  }
  return { nodes: shownNodes, edges: shownEdges, counts, unmatchedHoldings: [], unmatchedWatchlist: [], worldRevisionId: nodes.length ? "map-world-revision-1" : null, generatedAtMs: Math.max(nodes.length ? start + 29 * day : 0, ...ensure(state,request.loupistId).principles.map(p => p.updatedAtMs)) };
}

function principleLens(p: Principle): PageView {
  return pageOf(p.recordId, "principle", [...p.statement.replace(/\s+/g, " ")].slice(0, 80).join(""), p.statement, [], {
    revisionId: p.revisionId, revisionDigest: p.revisionId, archived: p.archived, createdAtMs: p.createdAtMs, updatedAtMs: p.updatedAtMs,
    body: { kind: "principle", statement: p.statement, why: p.why ?? "", example: p.example ?? "", origin: p.origin },
  });
}
function checkPrinciple(p: Pick<Principle, "statement" | "why" | "example">) {
  p.statement = p.statement.trim();
  if (!p.statement || [...p.statement].length > 280 || /[\x00]/u.test(p.statement)) throw Object.assign(new Error("statement must contain 1–280 characters"), { code: "invalid" });
  for (const field of ["why", "example"] as const) if (p[field] !== undefined) {
    p[field] = p[field]!.trim();
    if ([...p[field]!].length > 500) throw Object.assign(new Error(`${field} must contain at most 500 characters`), { code: "invalid" });
  }
}
function uniquePrinciple(store: Store, statement: string, except?: string) {
  const fold = (value: string) => value.trim().replace(/\s+/gu, " ").toLowerCase();
  if (store.principles.some(p => !p.archived && p.recordId !== except && fold(p.statement) === fold(statement))) {
    throw Object.assign(new Error("You already have this principle."), {code:"duplicate_principle"});
  }
}
export async function principleList(state: MockBridgeState, request: PrincipleListRequest): Promise<Principle[]> {
  return clone(ensure(state, request.loupistId).principles.filter(p => request.includeArchived || !p.archived).sort((a,b) => b.updatedAtMs-a.updatedAtMs || a.recordId.localeCompare(b.recordId)));
}
export async function principleCreate(state: MockBridgeState, request: PrincipleCreateRequest): Promise<Principle> {
  const store = ensure(state, request.loupistId);
  const p: Principle = { ...request, recordId: makeId("rec"), revisionId: makeId("rev"), createdAtMs: Date.now(), updatedAtMs: Date.now(), archived: false };
  checkPrinciple(p);
  uniquePrinciple(store,p.statement);
  if (p.origin === "feedback" && (!p.originChatId || !p.originMessageId)) throw Object.assign(new Error("feedback requires origin Chat and message"), {code:"invalid"});
  store.principles.push(p); store.lenses.push(principleLens(p));
  store.growth.push({ ...growthEvent(p.loupistId,p.recordId,"lens_created",p.statement,{actor:"user"}),origin:p.origin,originChatId:p.originChatId ?? null,originMessageId:p.originMessageId ?? null });
  return clone(p);
}
export async function principleUpdate(state: MockBridgeState, request: PrincipleUpdateRequest): Promise<Principle> {
  const store=ensure(state,request.loupistId); const p=store.principles.find(p=>p.recordId===request.recordId);
  if (!p) throw Object.assign(new Error("Principle not found"),{code:"not_found"});
  if (p.revisionId!==request.expectedRevisionId) throw Object.assign(new Error("revision conflict"),{code:"conflict"});
  const next={...p,statement:request.statement??p.statement,why:request.why??p.why,example:request.example??p.example,origin:"settings" as const,originChatId:undefined,originMessageId:undefined,revisionId:makeId("rev"),updatedAtMs:Date.now()};
  checkPrinciple(next); uniquePrinciple(store,next.statement,p.recordId); Object.assign(p,next); store.lenses[store.lenses.findIndex(l=>l.recordId===p.recordId)]=principleLens(p);
  store.growth.push({...growthEvent(p.loupistId,p.recordId,"lens_revised",p.statement,{actor:"user"}),origin:"settings"}); return clone(p);
}
export async function principleArchive(state: MockBridgeState, request: PrincipleArchiveRequest): Promise<Principle> {
  const store=ensure(state,request.loupistId); const p=store.principles.find(p=>p.recordId===request.recordId);
  if (!p) throw Object.assign(new Error("Principle not found"),{code:"not_found"});
  if (!request.archived) uniquePrinciple(store,p.statement,p.recordId);
  p.archived=request.archived; p.updatedAtMs=Date.now(); store.lenses[store.lenses.findIndex(l=>l.recordId===p.recordId)]=principleLens(p);
  store.growth.push({...growthEvent(p.loupistId,p.recordId,p.archived?"lens_archived":"lens_restored",p.statement,{actor:"user"}),origin:"settings"}); return clone(p);
}

export async function learningReviewList(state: MockBridgeState, request: import("../../contracts/knowledge").LearningReviewQuery): Promise<import("../../contracts/knowledge").LearningReview[]> {
  ensure(state, request.loupistId);
  const limit = request.limit ?? 50;
  if (!Number.isInteger(limit) || limit < 1 || limit > 200) throw new Error("learning review limit must be 1–200");
  return [];
}
export async function memoryLearningSettingsGet(state: MockBridgeState, request: { loupistId: string }) {
  return { learnMethods: ensure(state, request.loupistId).learnMethods ?? true };
}
export async function memoryLearningSettingsSet(state: MockBridgeState, request: { loupistId: string; learnMethods: boolean }) {
  ensure(state, request.loupistId).learnMethods = request.learnMethods;
  return { learnMethods: request.learnMethods };
}

export async function knowledgeMemoryUsage(state: MockBridgeState, request: { loupistId: string }): Promise<import("../../contracts/knowledge").MemoryUsage[]> {
  return structuredClone(state.memoryUsage?.[request.loupistId] ?? []);
}

export async function knowledgeLensArchive(state: MockBridgeState, request: { loupistId: string; recordId: string; archived: boolean }): Promise<void> {
  const store = ensure(state, request.loupistId);
  const lens = store.lenses.find(item => item.recordId === request.recordId);
  if (!lens) throw new Error("Lens not found");
  if (request.archived && lens.kind === "perspective") throw new Error("This Lens cannot be archived");
  if (Boolean(lens.archived) === request.archived) return;
  lens.archived = request.archived;
  store.growth.push(growthEvent(request.loupistId, lens.recordId, request.archived ? "lens_archived" : "lens_restored", lens.title, { actor: "user", beforeSummary: request.archived ? "active" : "archived", afterSummary: request.archived ? "archived" : "active" }));
}

function validateJournal(input: JournalInput) {
  if (!input.judgment.trim() || [...input.judgment].length > 1000 || [...(input.reasoning ?? "")].length > 2000) throw new Error("Journal text is invalid.");
  if (input.check) {
    const c = input.check;
    if (!c.claim.trim() || !c.metric.trim() || !Number.isFinite(c.threshold) || (c.byDate === undefined ? input.thesis?.role !== "line" : !/^\d{4}-\d{2}-\d{2}$/u.test(c.byDate) || Number.isNaN(Date.parse(c.byDate))) || !["above", "below"].includes(c.direction)) throw new Error("Check is invalid.");
    if (c.confidence !== undefined && (!Number.isInteger(c.confidence) || c.confidence < 10 || c.confidence > 90 || c.confidence % 10)) throw new Error("Confidence must be 10–90 in steps of 10.");
  }
}
export async function journalList(state: MockBridgeState, request: JournalListRequest): Promise<JournalRecord[]> {
  return clone(ensure(state, request.loupistId).journals.filter(j => request.includeArchived || !j.archived).sort((a, b) => b.createdAtMs - a.createdAtMs || a.journalId.localeCompare(b.journalId)));
}
export async function journalCreate(state: MockBridgeState, request: JournalCreateRequest): Promise<JournalRecord> {
  validateJournal(request);
  const record: JournalRecord = { ...request, judgment: request.judgment, reasoning: request.reasoning ?? "", entities: request.entities ?? [], journalId: makeId("jnl"), researchId: "", inputDigest: "mock-input", createdAtMs: Date.now(), updatedAtMs: Date.now(), archived: false, inputs: { sources: [], worlds: [], lenses: [], earlier: [] } };
  ensure(state, request.loupistId).journals.push(record); return clone(record);
}
export async function journalUpdate(state: MockBridgeState, request: JournalUpdateRequest): Promise<JournalRecord> {
  validateJournal(request);
  const record = ensure(state, request.loupistId).journals.find(j => j.journalId === request.journalId);
  if (!record) throw new Error("Journal entry not found.");
  if (record.updatedAtMs !== request.expectedUpdatedAtMs) throw Object.assign(new Error("The entry changed; reopen it before editing."), { code: "conflict" });
  if (record.outcome && JSON.stringify(record.check ?? null) !== JSON.stringify(request.check ?? null)) throw new Error("A reviewed check cannot change.");
  Object.assign(record, { judgment: request.judgment, reasoning: request.reasoning ?? "", entities: request.entities ?? [], check: request.check, thesis: request.thesis ?? record.thesis, updatedAtMs: Math.max(Date.now(), record.updatedAtMs + 1) }); return clone(record);
}
export async function journalArchive(state: MockBridgeState, request: JournalArchiveRequest): Promise<JournalRecord> {
  const record = ensure(state, request.loupistId).journals.find(j => j.journalId === request.journalId);
  if (!record) throw new Error("Journal entry not found.");
  record.archived = request.archived; record.updatedAtMs = Math.max(Date.now(), record.updatedAtMs + 1); return clone(record);
}

export async function knowledgeMemoryControls(state: MockBridgeState, request: { loupistId: string }) {
  const store = ensure(state, request.loupistId);
  return clone(store.memoryControls ?? { paused: [], removed: [] });
}
export async function knowledgeLensPause(state: MockBridgeState, request: { loupistId: string; recordId: string; paused: boolean }) {
  const store = ensure(state, request.loupistId);
  const lens = store.lenses.find(item => item.recordId === request.recordId);
  if (!lens || lens.kind === "perspective") throw new Error("Only a principle or page can be paused");
  const controls = await knowledgeMemoryControls(state, request);
  controls.paused = [...new Set(controls.paused.filter(id => id !== request.recordId).concat(request.paused ? [request.recordId] : []))];
  store.memoryControls = controls;
  return clone(controls);
}
export async function knowledgeMemoryResetPreview(state: MockBridgeState, request: { loupistId: string }): Promise<import("../../contracts/knowledge").MemoryResetCounts> {
  const store = ensure(state, request.loupistId);
  const owned = store.growth.filter(event => event.kind === "lens_created" && event.actor === "loupist").map(event => event.recordId);
  return { principles: 0, pages: store.lenses.filter(lens => lens.kind !== "perspective" && owned.includes(lens.recordId)).length, sections: 0, names: 0, relationships: 0, sources: 0 };
}
export async function knowledgeMemoryReset(state: MockBridgeState, request: { loupistId: string; confirm: boolean; expected: import("../../contracts/knowledge").MemoryResetCounts }) {
  const counts = await knowledgeMemoryResetPreview(state, request);
  if (!request.confirm || JSON.stringify(counts) !== JSON.stringify(request.expected)) throw new Error("Review the reset counts again");
  const store = ensure(state, request.loupistId);
  const owned = store.growth.filter(event => event.kind === "lens_created" && event.actor === "loupist").map(event => event.recordId);
  const controls = await knowledgeMemoryControls(state, request);
  store.lenses = store.lenses.filter(lens => { if (lens.kind === "perspective" || !owned.includes(lens.recordId)) return true; controls.removed.push(lens.recordId); return false; });
  controls.paused = controls.paused.filter(id => !controls.removed.includes(id));
  store.memoryControls = controls;
  return counts;
}
