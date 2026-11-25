import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { mockUsageSummary } from "../../bridge/mock/usage";
import type { LoupistBridge } from "../../types";
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
  expect(within(accounts).getByText(/ChatGPT does not tell apps/)).toBeVisible();
  expect(within(accounts).getByText(/SuperGrok does not tell apps/)).toBeVisible();
  expect(within(accounts).getByText(/set a weekly limit/)).toBeVisible();
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
