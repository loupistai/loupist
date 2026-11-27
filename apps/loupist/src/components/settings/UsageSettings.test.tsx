import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { mockUsageSummary } from "../../bridge/mock/usage";
import type { LoupistBridge } from "../../types";
import { formatTime } from "../../i18n/format";
import { UsageSettings } from "./UsageSettings";

type Request = Parameters<LoupistBridge["usageSummary"]>[0];

it("shows plan context, key estimates, billed costs and program links", async () => {
  const catalog = await new MockLoupistBridge().modelCatalogGet();
  const providerOpenUsage = vi.fn(async () => {});
  const onOpenProgram = vi.fn();
  const usageSummary = vi.fn(async (request: Request) => {
    const summary = mockUsageSummary(request);
    summary.totals.unknownRuns = 2;
    const key = summary.accounts[1].totals;
    summary.accounts.push(
      { provider: "anthropic", credentialKind: "api_key", totals: { ...key, costEstimate: { microusd: 3_100_000, fromProvider: false } } },
      { provider: "xai", credentialKind: "plan", totals: { ...key, costEstimate: null } },
    );
    summary.programs.push({ loupistId: "other", taskId: "task", title: "Markets report", runs: 1, totals: key });
    return summary;
  });
  render(<UsageSettings bridge={{ providerOpenUsage, usageSummary }} catalog={catalog} onOpenProgram={onOpenProgram} />);
  const user = userEvent.setup();
  const accounts = await screen.findByRole("region", { name: "AI accounts" });
  expect(within(accounts).getByText(/Remaining ChatGPT quota unavailable/)).toBeVisible();
  expect(within(accounts).getByText(/Remaining SuperGrok quota unavailable/)).toBeVisible();
  expect(within(accounts).getByText(/Set weekly limits in ChatGPT/)).toBeVisible();
  expect(within(accounts).getByText("8,000 tokens (60% cached) · 700 output")).toBeVisible();
  expect(within(accounts).getByText("about $3.10 (estimate)")).toBeVisible();
  expect(within(accounts).getByText("billed $0.02")).toBeVisible();
  expect(screen.getByText("Usage totals are partial. 2 runs are missing figures.")).toBeVisible();
  expect(within(screen.getByRole("table")).getByRole("cell", { name: "5" })).toBeVisible();
  await user.click(within(accounts).getByRole("button", { name: "Manage usage" }));
  expect(providerOpenUsage).toHaveBeenCalledExactlyOnceWith({ provider: "openai" });
  await user.click(screen.getByRole("button", { name: "Markets report" }));
  expect(onOpenProgram).toHaveBeenCalledExactlyOnceWith("other", "task");
});

it("shows an empty period and reads Today from local midnight", async () => {
  const usageSummary = vi.fn(async (request: Request) => ({
    ...mockUsageSummary(request),
    totals: { runs: 0, unknownRuns: 0, requests: 0, inputTokens: 0, cachedInputTokens: 0, outputTokens: 0, costEstimate: null },
  }));
  render(<UsageSettings bridge={{ usageSummary, providerOpenUsage: vi.fn() }} catalog={null} />);
  expect(await screen.findByText("No model usage in this period.")).toBeVisible();
  await userEvent.click(screen.getByRole("button", { name: "Today" }));
  expect(await screen.findByText("No model usage in this period.")).toBeVisible();
  const { fromMs, toMs, timeZone } = usageSummary.mock.calls.at(-1)![0];
  const midnight = new Date(toMs - 1);
  midnight.setHours(0, 0, 0, 0);
  expect(fromMs).toBe(midnight.getTime());
  expect(timeZone).toBe(Intl.DateTimeFormat().resolvedOptions().timeZone);
});


it("shows check activity for a program with no model runs", async () => {
  const at = new Date(2026, 10, 25, 14, 30).getTime();
  const usageSummary = vi.fn(async (request: Request) => {
    const summary = mockUsageSummary(request);
    const totals = { runs: 0, unknownRuns: 0, requests: 0, inputTokens: 0, cachedInputTokens: 0, outputTokens: 0, costEstimate: null };
    return { ...summary, totals, programs: [{
      taskId: "check", loupistId: "loupist", title: "Price band", runs: 0, totals,
      checks: { evaluations: 40, reads: 80, met: 0, lastCheckAtMs: at },
    }] };
  });
  render(<UsageSettings bridge={{ usageSummary, providerOpenUsage: vi.fn() }} catalog={null} />);
  await userEvent.click(screen.getByRole("button", { name: "Today" }));
  const programs = await screen.findByRole("region", { name: "Programs" });
  expect(within(programs).getByText("Price band")).toBeVisible();
  expect(programs).toHaveTextContent(`Last check ${formatTime(at)} · 40 checks today · 80 data reads · 0 met · Model requests 0`);
});

it("shows the allowance, holds that do not promise a reset, and the OpenRouter balance", async () => {
  const usageSummary = vi.fn(async (request: Request) => {
    const summary = mockUsageSummary(request);
    summary.allowance = {
      background: { limit: 400, used: 320, reserved: 0, state: "warning" },
      worldChat: { limit: 0, used: 0, reserved: 0, state: "off" },
      worldShare: { limit: 200, used: 10 },
      learningRunning: false,
      providerHolds: {
        openai: { reason: "plan_limit", local_day: "2026-11-25", provider: "openai" },
        openrouter: { reason: "out_of_credit", local_day: "2026-11-25", provider: "openrouter" },
      },
    };
    return summary;
  });
  const providerQuotaRead = vi.fn(async () => ({ limitUsd: 50, remainingUsd: 12.5, usageDailyUsd: 1.25, usageMonthlyUsd: 37.5, reset: "monthly" as const }));
  render(<UsageSettings bridge={{ usageSummary, providerOpenUsage: vi.fn(), providerQuotaRead }} catalog={null} />);
  const allowance = await screen.findByRole("region", { name: "Allowance" });
  expect(allowance).toHaveTextContent("Background work has used 320 of 400 requests today.");
  expect(within(allowance).getByText("Off")).toBeVisible();
  expect(allowance.querySelector("[data-allowance='sources']")).toHaveTextContent("Sources from Chat");
  expect(allowance).toHaveTextContent("World uses 10 of 200 of the background allowance.");
  expect(allowance).toHaveTextContent("ChatGPT reached its plan limit.");
  expect(allowance).toHaveTextContent("OpenRouter is out of credit.");
  expect(allowance).not.toHaveTextContent(/resumes|try again tomorrow/i);
  expect(await within(allowance).findByText("$12.50 of $50.00 left · resets monthly")).toBeVisible();
  expect(providerQuotaRead).toHaveBeenCalledWith("openrouter");
});
