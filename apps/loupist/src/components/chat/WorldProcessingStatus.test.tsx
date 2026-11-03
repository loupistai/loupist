import { act, fireEvent, render, screen } from "@testing-library/react";
import { WorldProcessingStatus } from "./WorldProcessingStatus";
import type { GraphJobView } from "../../types";

const job = (status: string): GraphJobView => ({ jobId: "job-1", loupistId: "loupist-a", worldRecordId: "world-a", worldRevisionId: "revision-a", status, payloadDigest: "digest", createdAtMs: 1, hostMayRedrive: false });

describe("committed World status in research updates", () => {
  afterEach(() => vi.useRealTimers());
  it("polls the committed job and refreshes readers exactly when it finishes", async () => {
    vi.useFakeTimers();
    const read = vi.fn().mockResolvedValueOnce(job("running")).mockResolvedValueOnce(job("completed"));
    const changed = vi.fn();
    render(<WorldProcessingStatus jobId="job-1" initialStatus="queued" readJob={read} onChanged={changed} />);
    await act(async () => {});
    expect(screen.getByRole("status")).toHaveTextContent("processing relationships");
    await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });
    expect(screen.getByRole("status")).toHaveTextContent("relationships updated");
    expect(changed).toHaveBeenCalledOnce();
    await act(async () => { await vi.advanceTimersByTimeAsync(6_000); });
    expect(read).toHaveBeenCalledTimes(2);
    expect(read).toHaveBeenNthCalledWith(1, "job-1");
  });

  it("leaves saved sources usable without a helper and only reads an uncertain extraction's status", async () => {
    const read = vi.fn().mockResolvedValue(job("reconcile_required"));
    const view = render(<WorldProcessingStatus initialStatus="not_configured" readJob={read} />);
    expect(screen.getByRole("status")).toHaveTextContent("Source saved");
    expect(read).not.toHaveBeenCalled();
    view.rerender(<WorldProcessingStatus jobId="job-1" initialStatus="reconcile_required" readJob={read} />);
    await act(async () => {});
    expect(screen.getByRole("status")).toHaveTextContent("outcome uncertain");
    expect(read).toHaveBeenCalledOnce();
    read.mockResolvedValue(job("reconciled_without_retry"));
    fireEvent.click(screen.getByRole("button", { name: "Refresh status" }));
    await act(async () => {});
    expect(screen.getByRole("status")).toHaveTextContent("Source retained · Interrupted World processing was not retried");
    expect(read).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("button", { name: "Refresh status" })).toBeNull();
  });

  it("ignores an old job response after navigating away", async () => {
    let resolve!: (value: GraphJobView) => void;
    const changed = vi.fn();
    const read = vi.fn(() => new Promise<GraphJobView>((done) => { resolve = done; }));
    const view = render(<WorldProcessingStatus jobId="job-1" initialStatus="queued" readJob={read} onChanged={changed} />);
    view.unmount();
    await act(async () => resolve(job("completed")));
    expect(changed).not.toHaveBeenCalled();
  });
});
