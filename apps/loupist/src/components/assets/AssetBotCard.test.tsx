import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { PresetCommand } from "../../contracts/generated";
import { OpenProvider } from "../navigation/Open";
import { AssetBotCard } from "./AssetBotCard";
import type { ScheduledTaskListItem } from "@/types";

it("says a trigger preset runs no model until the check is met, and a model-run preset keeps its charge line", () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  render(<AssetBotCard bridge={bridge} loupistId="loupist-quality" assetId="asset-test" hasLines />);
  expect(screen.getByText("Checks may incur AI charges.")).toBeVisible();
  expect(screen.getAllByText("No model runs until the check is met.")).toHaveLength(2);
  expect(screen.getByRole("switch", { name: "Price moves" })).toBeVisible();
  expect(screen.getByRole("switch", { name: "Fact changes" })).toBeVisible();
  const fact = screen.getByRole("switch", { name: "Fact changes" }).closest(".asset-bot-watch")!;
  expect(fact).toHaveTextContent("No model runs until the check is met.");
});

it("starts the filings preset for this asset and reflects the host's saved status", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const create = vi.spyOn(bridge, "scheduledTaskFromPreset");
  const status = vi.spyOn(bridge, "scheduledTaskSetStatus");
  render(<AssetBotCard bridge={bridge} loupistId="loupist-quality" assetId="asset-test" hasLines />);
  const filings = screen.getByRole("switch", { name: "Watch for filings and results" });
  await waitFor(() => expect(filings).toBeEnabled());
  fireEvent.click(filings);
  await waitFor(() => expect(filings).toHaveAttribute("data-state", "checked"));
  expect(create).toHaveBeenCalledWith({
    loupistId: "loupist-quality", preset: "filings-and-results", subjects: [{ asset: "asset-test" }],
  });
  const [task] = (await bridge.scheduledTasksList({ loupist_id: "loupist-quality" })).filter(task => task.preset === "filings-and-results");
  expect(task?.latest_run).not.toBeNull();
  fireEvent.click(filings);
  await waitFor(() => expect(filings).toHaveAttribute("data-state", "unchecked"));
  expect(status).toHaveBeenCalledWith(expect.objectContaining({ task_id: task?.id, status: "paused" }));
});

it.each([
  { state: "off", task: null, line: "Learns about Northwind each weekday before your briefing, on your AI account." },
  { state: "on", task: { status: "active", latest_run: { status: "succeeded", summary: "Filing landed." } }, line: "Filing landed." },
  { state: "running", task: { status: "active", latest_run: { status: "running", summary: "" } }, line: "Running…" },
  { state: "waiting", task: { status: "active", hold: { reason: "allowance" }, latest_run: { status: "succeeded", summary: "Earlier" } }, line: "Waiting for tomorrow's allowance" },
] as const)("Track daily is $state", async ({ state, task, line }) => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "assetsList").mockResolvedValue([{ assetId: "asset-test", name: "Northwind", reference: null, followedAtMs: 1, createdAtMs: 0 }] as never);
  vi.spyOn(bridge, "scheduledTasksList").mockResolvedValue(task ? [{
    id: "daily", kind: "learning", preset: "asset-daily", title: "Northwind each weekday", revision: 2,
    subjects: [{ asset: "asset-test" }], ...task,
  } as unknown as ScheduledTaskListItem] : []);
  render(<AssetBotCard bridge={bridge} loupistId="loupist-quality" assetId="asset-test" hasLines={false} />);
  const row = (await screen.findByText("Track daily")).closest(".asset-track-daily")!;
  expect(row).toHaveAttribute("data-track-daily", state);
  await waitFor(() => expect(row).toHaveTextContent(line));
  const daily = withinRow(row);
  expect(daily).toHaveAttribute("data-state", state === "off" ? "unchecked" : "checked");
});

it("turns Track daily on from the preset and pauses it, and does not offer it for an unfollowed asset", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const open = vi.fn(async () => undefined);
  vi.spyOn(bridge, "assetsList").mockResolvedValue([{ assetId: "asset-test", name: "Northwind", reference: null, followedAtMs: 1, createdAtMs: 0 }] as never);
  vi.spyOn(bridge, "scheduledTasksList").mockResolvedValue([{
    id: "other", kind: "learning", title: "Semiconductors", revision: 1, status: "active",
    subjects: [{ asset: "asset-test" }, { asset: "other-asset" }], latest_run: { status: "succeeded", summary: "New filing." },
  } as unknown as ScheduledTaskListItem]);
  const create = vi.spyOn(bridge, "scheduledTaskFromPreset").mockResolvedValue({ id: "daily" } as never);
  render(<OpenProvider open={open}><AssetBotCard bridge={bridge} loupistId="loupist-quality" assetId="asset-test" hasLines={false} /></OpenProvider>);
  const daily = await screen.findByRole("switch", { name: "Track daily" });
  await waitFor(() => expect(daily).toBeEnabled());
  fireEvent.click(daily);
  const preset: PresetCommand = { loupistId: "loupist-quality", preset: "asset-daily", subjects: [{ asset: "asset-test" }] };
  await waitFor(() => expect(create).toHaveBeenCalledWith(preset));
  expect(await screen.findByText(/Also learned in: Semiconductors/)).toBeVisible();
  expect(screen.getByText(/New filing/)).toBeVisible();
  await userClick(screen.getByRole("button", { name: "Open" }));
  expect(open).toHaveBeenCalledWith({ kind: "schedule", taskId: "other" });

  vi.spyOn(bridge, "assetsList").mockResolvedValue([{ assetId: "asset-test", name: "Northwind", reference: null, followedAtMs: null, createdAtMs: 0 }] as never);
  render(<AssetBotCard bridge={bridge} loupistId="loupist-quality" assetId="asset-test" hasLines={false} />);
  expect(await screen.findByText("Follow Northwind to track it daily.")).toBeVisible();
  const blocked = screen.getAllByRole("switch", { name: "Track daily" }).at(-1)!;
  expect(blocked).toBeDisabled();
});

it("says Track daily is waiting when the app-wide learning slot is taken", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "assetsList").mockResolvedValue([{ assetId: "asset-test", name: "Northwind", reference: null, followedAtMs: 1, createdAtMs: 0 }] as never);
  vi.spyOn(bridge, "scheduledTasksList").mockResolvedValue([{
    id: "daily", kind: "learning", preset: "asset-daily", title: "Northwind each weekday", revision: 2, status: "active",
    subjects: [{ asset: "asset-test" }], latest_run: { status: "succeeded", summary: "Filing landed." },
  } as unknown as ScheduledTaskListItem]);
  vi.spyOn(bridge, "usageSummary").mockResolvedValue({ allowance: { learningRunning: true, background: { limit: 400, used: 0, reserved: 0, state: "ok" }, worldChat: { limit: 300, used: 0, reserved: 0, state: "ok" }, worldShare: { used: 0, limit: 200 }, providerHolds: {} } } as never);
  render(<AssetBotCard bridge={bridge} loupistId="loupist-quality" assetId="asset-test" hasLines={false} />);
  const row = (await screen.findByText("Track daily")).closest(".asset-track-daily")!;
  await waitFor(() => expect(row).toHaveTextContent("Waiting for another learning run"));
  expect(row).not.toHaveTextContent("Filing landed.");
});

function withinRow(row: Element) {
  return row.querySelector("[role='switch']")!;
}

function userClick(node: HTMLElement) {
  fireEvent.click(node);
}
