import type { LegacyHoldingsStatus } from "../../contracts/legacyHoldings";
import { commandError } from "./commandError";

/** Host export lifecycle controls for notice previews and tests. */
export class MockLegacyHoldings {
  private status: LegacyHoldingsStatus = { state: "none", dismissed: false };
  private readonly handlers = new Set<(status: LegacyHoldingsStatus) => void>();
  /** Configure the next retry result, including counts and time on success. */
  retryResult: LegacyHoldingsStatus = {
    state: "exported", dismissed: false, exportedAtMs: 1_794_916_800_000,
    counts: { wallets: 2, snapshots: 3, positions: 14 },
  };
  /** False models cancellation of the native save panel. */
  saveCopyResult = true;

  setStatus(status: LegacyHoldingsStatus) {
    const next = structuredClone(status);
    if (JSON.stringify(next) === JSON.stringify(this.status)) return;
    this.status = next;
    for (const handler of this.handlers) handler(structuredClone(next));
  }
  legacyHoldingsStatus(): Promise<LegacyHoldingsStatus> { return Promise.resolve(structuredClone(this.status)); }
  async legacyHoldingsRetry(): Promise<LegacyHoldingsStatus> {
    if (this.status.state === "pending" || this.status.state === "failed") this.setStatus(this.retryResult);
    return this.legacyHoldingsStatus();
  }
  legacyHoldingsReveal(): Promise<void> { return this.requireExported(); }
  async legacyHoldingsSaveCopy(): Promise<boolean> { await this.requireExported(); return this.saveCopyResult; }
  async legacyHoldingsDismiss(): Promise<LegacyHoldingsStatus> {
    if (this.status.state === "exported" || this.status.state === "failed") this.setStatus({ ...this.status, dismissed: true });
    return this.legacyHoldingsStatus();
  }
  onLegacyHoldingsChanged(handler: (status: LegacyHoldingsStatus) => void, onListening?: () => void): () => void {
    this.handlers.add(handler);
    onListening?.();
    return () => { this.handlers.delete(handler); };
  }
  private requireExported(): Promise<void> {
    return this.status.state === "exported" ? Promise.resolve() : Promise.reject(commandError("invalid_request", "Legacy holdings have not been exported"));
  }
}
