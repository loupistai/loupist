import { act, fireEvent, render, screen } from "@testing-library/react";
import { WorldProcessingStatus } from "./WorldProcessingStatus";
import type { GraphJobView } from "../../types";

const job = (status: string): GraphJobView => ({ jobId: "job-1", loupistId: "loupist-a", worldRecordId: "world-a", worldRevisionId: "revision-a", status, payloadDigest: "digest", createdAtMs: 1, hostMayRedrive: false });

describe("committed World status in research updates", () => {
  afterEach(() => vi.useRealTimers());
  it("keeps polling across callback changes and does not restart a completed job", async () => {
    vi.useFakeTimers();
    const read = vi.fn().mockResolvedValueOnce(job("running")).mockResolvedValue(job("completed"));
    const previousChanged = vi.fn();
    const changed = vi.fn();
    const view = render(<WorldProcessingStatus jobId="job-1" initialStatus="queued" readJob={(id) => read(id)} onChanged={previousChanged} />);
    await act(async () => {});
    for (let tick = 0; tick < 4; tick++) {
      await act(async () => { await vi.advanceTimersByTimeAsync(32); });
      view.rerender(<WorldProcessingStatus jobId="job-1" initialStatus="queued" readJob={(id) => read(id)} onChanged={() => changed()} />);
    }
    expect(read).toHaveBeenCalledOnce();
    await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });
    expect(changed).toHaveBeenCalledOnce();
    expect(previousChanged).not.toHaveBeenCalled();
    view.rerender(<WorldProcessingStatus jobId="job-1" initialStatus="queued" readJob={(id) => read(id)} onChanged={() => changed()} />);
    await act(async () => { await vi.advanceTimersByTimeAsync(6_000); });
    expect(read).toHaveBeenCalledTimes(2);
    expect(changed).toHaveBeenCalledOnce();
    expect(screen.getByRole("status")).toHaveTextContent("relationships updated");
  });

  it("polls the committed job and refreshes readers exactly when it finishes", async () => {
    vi.useFakeTimers();
    const read = vi.fn().mockResolvedValueOnce(job("running")).mockResolvedValueOnce(job("completed"));
    const changed = vi.fn();
    render(<WorldProcessingStatus jobId="job-1" initialStatus="queued" readJob={read} onChanged={changed} retrySource={vi.fn()} />);
    await act(async () => {});
    expect(screen.getByRole("status")).toHaveTextContent("connecting relationships");
    await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });
    expect(screen.getByRole("status")).toHaveTextContent(/^World: relationships updated$/);
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
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
    read.mockResolvedValue(job("usage_uncertain"));
    fireEvent.click(screen.getByRole("button", { name: "Refresh status" }));
    await act(async () => {});
    expect(screen.getByRole("status")).toHaveTextContent("Source saved · World building stopped because model usage could not be confirmed");
    expect(read).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("button", { name: "Refresh status" })).toBeNull();
  });

  it.each([
    ["interrupted", "World building was interrupted"],
    ["timed_out", "World building timed out"],
    ["usage_uncertain", "World building stopped because model usage could not be confirmed"],
    ["reconciled_without_retry", "World building did not finish"],
  ])("shows the reason for a %s attempt without offering to replay it", async (status, label) => {
    const read = vi.fn();
    render(<WorldProcessingStatus jobId="job-1" initialStatus={status} readJob={read} retrySource={vi.fn()} />);
    await act(async () => {});
    expect(screen.getByRole("status")).toHaveTextContent(`Source saved · ${label}`);
    expect(screen.queryByRole("button")).toBeNull();
    expect(read).not.toHaveBeenCalled();
  });

  it("says a saved price table is not added to World and never polls its final row", async () => {
    const read = vi.fn();
    render(<WorldProcessingStatus jobId="job-1" initialStatus="price_data" readJob={read} retrySource={vi.fn()} />);
    expect(screen.getByRole("status")).toHaveTextContent(/^Source saved · Price data, not added to World$/);
    await act(async () => {});
    expect(read).not.toHaveBeenCalled();
    expect(screen.queryByRole("button")).toBeNull();
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

it("reads parked jobs once and distinguishes a Source stop from the user's pause", async () => {
  const read = vi.fn().mockResolvedValue(job("paused_budget"));
  render(<WorldProcessingStatus jobId="job-1" initialStatus="paused_budget" readJob={read} />);
  await act(async () => {});
  expect(screen.getByRole("status")).toHaveTextContent("World: building paused for this source (request allowance reached)");
  expect(read).toHaveBeenCalledOnce();
  read.mockResolvedValue(job("paused"));
  fireEvent.click(screen.getByRole("button", { name: "Refresh status" }));
  await act(async () => {});
  expect(screen.getByRole("status")).toHaveTextContent("World: building paused");
  read.mockResolvedValue(job("paused_budget"));
  fireEvent.click(screen.getByRole("button", { name: "Refresh status" }));
  await act(async () => {});
  expect(screen.getByRole("status")).toHaveTextContent("World: building paused for this source (request allowance reached)");
});

describe("Try again on a Source that stopped", () => {
  it("is offered only for a Source stop when the host can retry it", async () => {
    const read = vi.fn().mockResolvedValue(job("paused"));
    const retry = vi.fn();
    const view = render(<WorldProcessingStatus jobId="job-1" initialStatus="paused" readJob={read} retrySource={retry} />);
    await act(async () => {});
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
    read.mockResolvedValue({ ...job("paused_budget"), retryable: true });
    view.rerender(<WorldProcessingStatus jobId="job-1" initialStatus="paused_budget" readJob={read} />);
    await act(async () => {});
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
    view.rerender(<WorldProcessingStatus jobId="job-1" initialStatus="paused_budget" readJob={read} retrySource={retry} />);
    await act(async () => {});
    expect(screen.getByRole("button", { name: "Try again" })).toBeEnabled();
    expect(screen.getByRole("status")).toHaveTextContent("Uses one more allowance of model requests per source, as set in World building.");
  });

  it("disables while pending and shows the host's refusal inline", async () => {
    const read = vi.fn().mockResolvedValue({ ...job("paused_budget"), retryable: true });
    let refuse!: (cause: Error) => void;
    const retry = vi.fn(() => new Promise<GraphJobView>((_, reject) => { refuse = reject; }));
    render(<WorldProcessingStatus jobId="job-1" initialStatus="paused_budget" readJob={read} retrySource={retry} />);
    await act(async () => {});
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(screen.getByRole("button", { name: "Trying again…" })).toBeDisabled();
    await act(async () => refuse(new Error("Source processing is active or uncertain")));
    expect(screen.getByRole("alert")).toHaveTextContent("Source processing is active or uncertain");
    expect(screen.getByRole("button", { name: "Try again" })).toBeEnabled();
  });

  it("follows the returned job after the host accepts", async () => {
    vi.useFakeTimers();
    try {
      const read = vi.fn().mockResolvedValueOnce({ ...job("paused_budget"), retryable: true }).mockResolvedValueOnce({ ...job("running"), jobId: "job-2" }).mockResolvedValue({ ...job("completed"), jobId: "job-2" });
      const changed = vi.fn();
      render(<WorldProcessingStatus jobId="job-1" initialStatus="paused_budget" readJob={read} onChanged={changed} retrySource={() => Promise.resolve({ ...job("queued"), jobId: "job-2" })} />);
      await act(async () => {});
      await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Try again" })); });
      expect(changed).toHaveBeenCalledOnce();
      expect(read).toHaveBeenLastCalledWith("job-2");
      expect(screen.getByRole("status")).toHaveTextContent("World: connecting relationships");
      expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
      await act(async () => vi.advanceTimersByTimeAsync(2_000));
      expect(screen.getByRole("status")).toHaveTextContent("World: relationships updated");
      expect(changed).toHaveBeenCalledTimes(2);
    } finally { vi.useRealTimers(); }
  });

  it("says World is full with no action, and a model stop only when it can run again", async () => {
    const read = vi.fn().mockResolvedValue({ ...job("paused_budget"), failure: { kind: "world_full" as const, atMs: 1 }, retryable: true });
    const view = render(<WorldProcessingStatus jobId="job-1" initialStatus="paused_budget" readJob={read} retrySource={vi.fn()} />);
    await act(async () => {});
    expect(screen.getByRole("status")).toHaveTextContent("World is full. This Source is kept but not read into World.");
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
    read.mockResolvedValue({ ...job("failed_extraction"), failure: { kind: "invalid_json" as const, atMs: 1 }, retryable: true });
    view.rerender(<WorldProcessingStatus jobId="job-1" initialStatus="failed_extraction" readJob={read} retrySource={vi.fn()} />);
    await act(async () => {});
    expect(screen.getByRole("status")).toHaveTextContent("The model's reply could not be used.");
    expect(screen.getByRole("button", { name: "Try again" })).toBeEnabled();
    expect(screen.queryByText(/one more allowance/)).toBeNull();
  });

  it("offers Read now when a Source is waiting on the allowance or a plan limit", async () => {
    const retry = vi.fn().mockResolvedValue(job("queued"));
    const allowance = render(<WorldProcessingStatus jobId="job-1" initialStatus="daily_allowance" retrySource={retry} />);
    expect(screen.getByRole("status")).toHaveTextContent("Waiting for tomorrow's allowance.");
    expect(screen.getByRole("button", { name: "Read now" })).toBeEnabled();
    expect(screen.queryByText(/one more allowance/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Read now" }));
    await act(async () => {});
    expect(retry).toHaveBeenCalledOnce();
    allowance.unmount();
    const read = vi.fn().mockResolvedValue({ ...job("paused"), hold: { cause: "plan_limit" } });
    render(<WorldProcessingStatus jobId="job-1" initialStatus="paused" readJob={read} retrySource={vi.fn()} />);
    await act(async () => {});
    expect(screen.getByRole("status")).toHaveTextContent("Your AI plan's limit was reached. Reading resumes tomorrow.");
    expect(screen.getByRole("button", { name: "Read now" })).toBeVisible();
  });
});
