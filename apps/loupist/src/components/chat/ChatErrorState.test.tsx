import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ChatErrorState } from "./ChatErrorState";
import { CitationChip } from "./CitationChip";
import type { ChatEvidenceRef } from "../../types";

const base = { provider: "OpenAI", fallback: "The response could not be completed.", attempt: 1 };

describe("error shapes", () => {
  it("counts a busy account down and retries once on its own, or right away", async () => {
    vi.useFakeTimers();
    const onRetry = vi.fn();
    const { unmount } = render(<ChatErrorState {...base} kind="rate_limited" onRetry={onRetry} />);
    expect(screen.getByText("Your AI account is busy. Retrying in 20 s…")).toBeInTheDocument();
    await act(async () => { vi.advanceTimersByTime(5_000); });
    expect(screen.getByText("Your AI account is busy. Retrying in 15 s…")).toBeInTheDocument();
    await act(async () => { vi.advanceTimersByTime(15_000); });
    expect(onRetry).toHaveBeenCalledTimes(1);
    await act(async () => { vi.advanceTimersByTime(5_000); });
    expect(onRetry).toHaveBeenCalledTimes(1);
    unmount();

    const now = vi.fn();
    render(<ChatErrorState {...base} attempt={2} kind="rate_limited" onRetry={now} />);
    expect(screen.getByText("Your AI account is busy. Retrying in 40 s…")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry now" }));
    expect(now).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });

  it("counts down from the provider's own retry hint when Rust sent one", async () => {
    vi.useFakeTimers();
    const onRetry = vi.fn();
    const { unmount } = render(<ChatErrorState {...base} kind="rate_limited" retryAfterSeconds={7} onRetry={onRetry} />);
    expect(screen.getByText("Your AI account is busy. Retrying in 7 s…")).toBeInTheDocument();
    await act(async () => { vi.advanceTimersByTime(7_000); });
    expect(onRetry).toHaveBeenCalledTimes(1);
    unmount();
    // The second busy answer uses its own hint too; the third still asks to try later.
    render(<ChatErrorState {...base} attempt={2} kind="rate_limited" retryAfterSeconds={90} onRetry={vi.fn()} />);
    expect(screen.getByText("Your AI account is busy. Retrying in 90 s…")).toBeInTheDocument();
    vi.useRealTimers();
  });

  it("stops retrying after the third busy answer and names the provider", () => {
    render(<ChatErrorState {...base} attempt={3} kind="rate_limited" onRetry={vi.fn()} />);
    expect(screen.getByText("Try again later (OpenAI)")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Retry now" })).not.toBeInTheDocument();
  });

  it("offers Reconnect for a sign-in that stopped working and plain retries for an unreachable or failing provider", async () => {
    const user = userEvent.setup();
    const onProviderAction = vi.fn();
    const { rerender } = render(<ChatErrorState {...base} kind="needs_reconnect" onRetry={vi.fn()} onProviderAction={onProviderAction} />);
    expect(screen.getByText("Sign in to OpenAI again to continue")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Reconnect" }));
    expect(onProviderAction).toHaveBeenCalledWith("reconnect");
    expect(screen.queryByRole("button", { name: "Try again" })).not.toBeInTheDocument();
    rerender(<ChatErrorState {...base} kind="offline" onRetry={vi.fn()} />);
    expect(screen.getByText("Couldn't reach OpenAI. Check your connection, then try again.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Try again" })).toBeInTheDocument();
    rerender(<ChatErrorState {...base} kind="provider_unavailable" onRetry={vi.fn()} />);
    expect(screen.getByText("OpenAI is having trouble right now. Try again in a moment.")).toBeInTheDocument();
    rerender(<ChatErrorState {...base} kind="context_too_long" onRetry={vi.fn()} />);
    expect(screen.getByText(/This chat is too long for the model/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Try again" })).not.toBeInTheDocument();
    rerender(<ChatErrorState {...base} kind="provider_error" onRetry={vi.fn()} />);
    expect(screen.getByText(base.fallback)).toBeInTheDocument();
  });

  it("never counts a plan limit down: it names the shared limit and offers usage, a retry and another account", async () => {
    vi.useFakeTimers();
    const onRetry = vi.fn();
    const onProviderAction = vi.fn();
    render(<ChatErrorState {...base} provider="ChatGPT" kind="plan_limit" onRetry={onRetry} onProviderAction={onProviderAction} />);
    expect(screen.getByText("Your ChatGPT plan is at its limit. Loupist won't retry on its own.")).toBeInTheDocument();
    expect(screen.getByText(/shared by every app you connect/)).toBeInTheDocument();
    const buttons = screen.getAllByRole("button").map((button) => button.textContent);
    expect(buttons).toEqual(["Manage usage", "Try again", "Use another account"]);
    expect(screen.getByRole("button", { name: "Manage usage" })).toHaveAttribute("data-primary");
    await act(async () => { vi.advanceTimersByTime(120_000); });
    expect(onRetry).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Manage usage" }));
    fireEvent.click(screen.getByRole("button", { name: "Use another account" }));
    expect(onProviderAction.mock.calls).toEqual([["manage_usage"], ["use_another"]]);
    vi.useRealTimers();
  });

  it("gives each account problem one next step", () => {
    const onProviderAction = vi.fn();
    const { rerender } = render(<ChatErrorState {...base} kind="plan_not_eligible" onRetry={vi.fn()} onProviderAction={onProviderAction} />);
    expect(screen.getByText(/can't be used in other apps/)).toBeInTheDocument();
    expect(screen.getAllByRole("button").map((button) => button.textContent)).toEqual(["Use an API key"]);
    fireEvent.click(screen.getByRole("button", { name: "Use an API key" }));
    expect(onProviderAction).toHaveBeenLastCalledWith("use_api_key");

    rerender(<ChatErrorState {...base} provider="Claude" kind="extra_usage_off" onRetry={vi.fn()} onProviderAction={onProviderAction} />);
    expect(screen.getByText(/Claude needs extra usage turned on/)).toBeInTheDocument();
    expect(screen.getAllByRole("button").map((button) => button.textContent)).toEqual(["Open Claude settings", "Use an API key", "Try again"]);
    fireEvent.click(screen.getByRole("button", { name: "Open Claude settings" }));
    expect(onProviderAction).toHaveBeenLastCalledWith("open_claude");

    rerender(<ChatErrorState {...base} provider="Anthropic" kind="out_of_credit" onRetry={vi.fn()} onProviderAction={onProviderAction} />);
    expect(screen.getByText("Your Anthropic account is out of credit. Add credit with Anthropic, then try again.")).toBeInTheDocument();
    expect(screen.getAllByRole("button").map((button) => button.textContent)).toEqual(["Try again"]);
  });
});

describe("citation card", () => {
  const evidence = {
    id: "e1", title: "Hanul Precision Q3 operating review", kind: "original", url: "https://hanul.example/q3", provider: "Hanul IR",
    tool_name: "web_fetch", retrieved_at: "2026-10-28T00:00:00Z", published_at: "2026-10-27",
    excerpt: "Revenue ₩184.2bn; operating margin 14.8%.", excerpt_truncated: false, content_sha256: "a".repeat(64), warnings: [],
  } as ChatEvidenceRef;

  it("shows title, domain, date and passage on hover or keyboard focus, and opens Evidence on click", async () => {
    const user = userEvent.setup();
    const onOpen = vi.fn();
    render(<CitationChip evidence={evidence} label="Source 1" title={evidence.title} source="hanul.example" onOpen={onOpen}>1</CitationChip>);
    await user.tab();
    const card = await screen.findByRole("dialog", { name: evidence.title });
    expect(card).toHaveTextContent("hanul.example · Oct 27, 2026");
    expect(card).toHaveTextContent("operating margin 14.8%");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Source 1" }));
    expect(onOpen).toHaveBeenCalled();
  });
});
