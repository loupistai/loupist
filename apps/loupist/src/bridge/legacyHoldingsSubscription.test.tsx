import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { act, render, screen } from "@testing-library/react";
import { createLoupistBridge } from "./tauriBridge";
import { LegacyHoldingsNotice } from "../components/app/LegacyHoldingsNotice";
import type { LegacyHoldingsStatus } from "../contracts/legacyHoldings";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), Channel: class {} }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const EXPORTED = "Holdings are no longer part of Loupist. What was stored has been saved to a file on this computer, and the assets are still in Following.";
const FAILED = "Holdings are no longer part of Loupist. The stored data has not been deleted, but the copy could not be saved yet.";
const pending: LegacyHoldingsStatus = { state: "pending", dismissed: false };
const exported: LegacyHoldingsStatus = { state: "exported", dismissed: false, exportedAtMs: 1_794_916_800_000 };
const failed: LegacyHoldingsStatus = { state: "failed", dismissed: false };

type Listener = (message: { event: string; id: number; payload: LegacyHoldingsStatus }) => void;
const flush = async () => { for (let index = 0; index < 5; index += 1) await Promise.resolve(); };
/** Native `listen()` resolves after the call returns; hold it until the test attaches it. */
const lateListen = () => {
  let attach!: () => void, fail!: (error: Error) => void, deliver: Listener | undefined;
  vi.mocked(listen).mockImplementationOnce((_event, handler) => {
    deliver = handler as unknown as Listener;
    return new Promise((resolve, reject) => { attach = () => resolve(() => undefined); fail = reject; });
  });
  return { attach: () => attach(), fail: (error: Error) => fail(error), emit: (payload: LegacyHoldingsStatus) => deliver?.({ event: "legacy-holdings-changed", id: 1, payload }) };
};
const hostStatus = (read: () => LegacyHoldingsStatus | Promise<LegacyHoldingsStatus>) => {
  vi.mocked(invoke).mockImplementation(async command => command === "legacy_holdings_status" ? read() : undefined);
};
const mount = () => render(<LegacyHoldingsNotice bridge={createLoupistBridge()} hidden={false} onGone={() => undefined} />);
const line = () => screen.queryByRole("status", { name: "Holdings notice" });

beforeEach(() => { vi.mocked(invoke).mockReset(); vi.mocked(listen).mockReset(); });

describe("the holdings status on the native bridge", () => {
  it("does not miss an export that ends before the change listener is attached", async () => {
    const native = lateListen();
    let host = pending;
    hostStatus(() => host);
    mount();
    await act(flush);
    expect(line()).toBeNull();
    // The export ends while `listen()` is still pending: its event reaches no handler.
    host = exported;
    await act(async () => { native.attach(); await flush(); });
    expect(line()).toHaveTextContent(EXPORTED);
  });

  it("lets an event heard after the listener is attached beat the read", async () => {
    const native = lateListen();
    let answer!: (status: LegacyHoldingsStatus) => void;
    hostStatus(() => new Promise(resolve => { answer = resolve; }));
    mount();
    await act(async () => { native.attach(); await flush(); });
    expect(invoke).toHaveBeenCalledWith("legacy_holdings_status", undefined);
    act(() => native.emit(exported));
    await act(async () => { answer(pending); await flush(); });
    expect(line()).toHaveTextContent(EXPORTED);
  });

  it("falls back to the read when the listener cannot be attached", async () => {
    const native = lateListen();
    hostStatus(() => failed);
    mount();
    await act(async () => { native.fail(new Error("no event bus")); await flush(); });
    expect(line()).toHaveTextContent(FAILED);
  });
});
