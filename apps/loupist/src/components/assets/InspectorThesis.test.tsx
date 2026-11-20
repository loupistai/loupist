import { render, screen } from "@testing-library/react";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { LoupistSummary } from "../../types";
import { HANUL, hanulFixture } from "../../preview/assets";
import { InspectorThesis } from "./InspectorThesis";

it("words an undone move in the Map inspector the way the asset page does", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList() as LoupistSummary[];
  const asset = hanulFixture();
  bridge.assets.installAsset(asset.asset);
  bridge.assets.install(loupist!.id, asset.asset.assetId, asset.page!);
  await bridge.assetVerdictUndo({ loupistId: loupist!.id, assetId: asset.asset.assetId, moveId: "preview-move-1" });
  render(<InspectorThesis bridge={bridge} loupistId={loupist!.id} loupistName={loupist!.name} entityId={HANUL.entityId} />);
  expect(await screen.findByText(/^Back to Holding up · you undid the .+ change$/u)).toBeVisible();
  // The undo move's own reason is the host's English note, never the sentence.
  expect(screen.queryByText(/Undone by you/u)).toBeNull();
});
