import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { hanulFixture, previewLineAlert } from "../../preview/assets";
import type { LoupistSummary } from "../../types";
import { ThreadAlert } from "./ThreadAlert";

async function alertFor(bridge: MockLoupistBridge) {
  const [loupist] = await bridge.loupistList() as LoupistSummary[];
  const fixture = hanulFixture("crossed");
  bridge.assets.install(loupist!.id, fixture.page, fixture.delta);
  render(<ThreadAlert message={previewLineAlert()} bridge={bridge} loupistId={loupist!.id} loupistName={loupist!.name} />);
  return { loupist: loupist!, user: userEvent.setup() };
}

it("undoes the crossing by its move and says so", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const { loupist, user } = await alertFor(bridge);
  const undo = vi.spyOn(bridge, "assetVerdictUndo");
  await user.click(screen.getByRole("button", { name: "Undo" }));
  expect(undo).toHaveBeenCalledWith({ loupistId: loupist.id, assetId: previewLineAlert().alert!.asset_id, moveId: "preview-move-crossed" });
  expect(await screen.findByText(/Back to Weakening · you undid the .* change/)).toBeVisible();
  expect(screen.queryByRole("alert")).toBeNull();
});

it("says inline when the Undo fails or a newer change came first", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const { user } = await alertFor(bridge);
  bridge.assets.undoFails = true;
  await user.click(screen.getByRole("button", { name: "Undo" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't undo this change. Try again.");
  bridge.assets.undoFails = false;
  vi.spyOn(bridge, "assetVerdictUndo").mockRejectedValueOnce(Object.assign(new Error("newer"), { code: "conflict" }));
  await user.click(screen.getByRole("button", { name: "Undo" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("A newer change was made, so this one can't be undone.");
  expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
});
