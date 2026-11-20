import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { PreviewBridge } from "../../preview/fixtures";
import { prepareConflictsFixture } from "../../preview/fixtures/conflicts";
import { hanulFixture } from "../../preview/assets";
import { AssetPageOpenerProvider } from "../assets/assetOpener";
import { MapInspector } from "./MapInspector";
it("shows both attributed conflicting reports and opens each exact source", async () => {
  const bridge = new PreviewBridge({delay_ms:0}); await prepareConflictsFixture(bridge);
  const catalog = await bridge.knowledgeCatalog({loupistId:"loupist-quality"});
  const map = await bridge.knowledgeMap({loupistId:"loupist-quality"});
  const location = {loupistId:"loupist-quality",surface:"world" as const,recordId:"preview-world",revisionId:"preview-revision"};
  const onOpen = vi.fn();
  render(<MapInspector node={map.nodes[0]} map={map} catalog={catalog} location={location} bridge={bridge} onClose={vi.fn()} onSelect={vi.fn()} onOpen={onOpen} onAsk={vi.fn()} />);
  const conflicts = await screen.findByRole("region",{name:"Sources disagree"});
  expect(within(conflicts).getByText(/expanded supplier capacity by 18%/)).toBeVisible();
  expect(within(conflicts).getByText(/reports unchanged capacity/)).toBeVisible();
  expect(within(conflicts).getByText(/Oct 27, 2026/)).toBeVisible();
  expect(within(conflicts).getByText(/Oct 26, 2026/)).toBeVisible();
  const user = userEvent.setup(); const links = within(conflicts).getAllByRole("button",{name:"Open source"});
  expect(links).toHaveLength(2);
  await user.click(links[1]);
  expect(onOpen).toHaveBeenCalledWith(expect.objectContaining({surface:"source",objectId:"preview-source-1",selection:expect.objectContaining({spanId:"conflict-span-1"})}));
});

it("names the entity before its kind, so the sheet's first line and a screen reader start with the name", async () => {
  const bridge = new PreviewBridge({ delay_ms: 0 }); await prepareConflictsFixture(bridge);
  const map = await bridge.knowledgeMap({ loupistId: "loupist-quality" });
  const node = map.nodes[0]!;
  render(<MapInspector node={node} map={map} location={{ loupistId: "loupist-quality", surface: "world" }} bridge={bridge} onClose={vi.fn()} onSelect={vi.fn()} onOpen={vi.fn()} onAsk={vi.fn()} />);
  const inspector = screen.getByRole("complementary", { name: node.label });
  const heading = within(inspector).getByRole("heading", { level: 2, name: node.label });
  expect(heading.parentElement?.firstElementChild).toBe(heading);
  expect(heading.nextElementSibling).toHaveTextContent("Company");
});

it("reads an entity's thesis in the inspector and opens its asset page on request", async () => {
  const bridge = new PreviewBridge({ delay_ms: 0 });
  await prepareConflictsFixture(bridge);
  const loupistId = "loupist-quality";
  const map = await bridge.knowledgeMap({ loupistId });
  const node = map.nodes[0]!;
  const fixture = hanulFixture();
  const assetId = fixture.asset.assetId;
  bridge.assets.installAsset(fixture.asset);
  bridge.assets.install(loupistId, assetId, { ...fixture.page, entityId: node.id });
  const get = vi.spyOn(bridge, "assetPageGet");
  const check = vi.spyOn(bridge, "assetPriceCheck");
  const series = vi.spyOn(bridge, "assetPriceSeries");
  const open = vi.fn();
  render(<AssetPageOpenerProvider value={open}><MapInspector bridge={bridge} map={map} node={node}
    location={{ loupistId, surface: "world" }} onClose={vi.fn()} onSelect={vi.fn()} onOpen={vi.fn()} onAsk={vi.fn()} /></AssetPageOpenerProvider>);
  const thesis = await screen.findByRole("region", { name: "Your thesis" });
  expect(thesis).toHaveTextContent("Weakening");
  expect(get).toHaveBeenCalledWith({ loupistId, entityId: node.id });
  expect(check).not.toHaveBeenCalled();
  expect(series).not.toHaveBeenCalled();
  await userEvent.setup().click(within(thesis).getByRole("button", { name: "Open asset page" }));
  expect(open).toHaveBeenCalledWith({ loupistId, assetId, name: fixture.asset.name });
});
