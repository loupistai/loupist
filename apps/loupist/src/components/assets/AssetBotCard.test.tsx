import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { AssetBotCard } from "./AssetBotCard";

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
