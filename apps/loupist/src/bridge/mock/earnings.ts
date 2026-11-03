/**
 * Mock results snapshots for the browser preview and renderer tests. Rust owns
 * events, captures and every difference; this keeps installed snapshots and
 * mirrors Prepare now turning an upcoming event into captured expectations.
 */
import type {
  EarningsBridge, EarningsEventSummary, EarningsEventsListRequest, EarningsPrepareRequest,
  EarningsSnapshot, EarningsSnapshotGetRequest,
} from "../../contracts/earnings";

const clone = <T>(value: T): T => structuredClone(value);
const key = (loupistId: string, assetId: string) => `${loupistId}:${assetId}`;

export class MockEarnings implements EarningsBridge {
  /** loupist:asset → eventId → snapshot. */
  private readonly snapshots = new Map<string, Map<string, EarningsSnapshot>>();
  /** loupist:asset → the event the asset page shows now. */
  private readonly current = new Map<string, string>();
  /** Preview: Prepare now resolves only when this is false. */
  prepareHold = false;
  prepareFails = false;
  /** Every Prepare now request, for tests. */
  readonly prepareCalls: EarningsPrepareRequest[] = [];
  /** What a capture adds; by default the snapshot only becomes "before captured". */
  prepared?: (snapshot: EarningsSnapshot) => EarningsSnapshot;

  /** Installs one event's snapshot; `current` makes it the card the page shows now. */
  install(loupistId: string, snapshot: EarningsSnapshot, options: { current?: boolean } = {}) {
    const assetKey = key(loupistId, snapshot.assetId);
    const events = this.snapshots.get(assetKey) ?? new Map<string, EarningsSnapshot>();
    events.set(snapshot.eventId, clone(snapshot));
    this.snapshots.set(assetKey, events);
    if (options.current ?? true) this.current.set(assetKey, snapshot.eventId);
  }

  async earningsSnapshotGet(request: EarningsSnapshotGetRequest): Promise<EarningsSnapshot | null> {
    const assetKey = key(request.loupistId, request.assetId);
    const eventId = request.eventId ?? this.current.get(assetKey);
    const snapshot = eventId ? this.snapshots.get(assetKey)?.get(eventId) : undefined;
    return snapshot ? clone(snapshot) : null;
  }

  async earningsEventsList(request: EarningsEventsListRequest): Promise<EarningsEventSummary[]> {
    return [...(this.snapshots.get(key(request.loupistId, request.assetId))?.values() ?? [])]
      .sort((a, b) => b.reportDate.localeCompare(a.reportDate))
      .map(({ eventId, period, reportDate, dateSource, status }) => ({ eventId, period, reportDate, dateSource, status }));
  }

  async earningsPrepare(request: EarningsPrepareRequest): Promise<EarningsSnapshot> {
    this.prepareCalls.push(clone(request));
    const snapshot = this.snapshots.get(key(request.loupistId, request.assetId))?.get(request.eventId);
    if (!snapshot) throw new Error("Results event not found.");
    while (this.prepareHold) await new Promise(resolve => setTimeout(resolve, 50));
    if (this.prepareFails) {
      snapshot.failed = { side: snapshot.status === "upcoming" || snapshot.status === "before_captured" ? "before" : "after", atMs: Date.now() };
      throw new Error("Couldn't capture expectations now.");
    }
    const captured: EarningsSnapshot = this.prepared ? this.prepared(clone(snapshot)) : {
      ...snapshot,
      status: snapshot.status === "upcoming" ? "before_captured" : snapshot.status,
      before: snapshot.before ?? { capturedAtMs: Date.now() },
      captureAtMs: undefined,
    };
    captured.preparing = false;
    captured.failed = undefined;
    this.snapshots.get(key(request.loupistId, request.assetId))!.set(request.eventId, clone(captured));
    return clone(captured);
  }
}
