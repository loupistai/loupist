import { chooseLoupist } from "../renderApp";
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { mainTab, openApp } from "../renderApp";

const openWorldProcessing = async (user: ReturnType<typeof userEvent.setup>) => {
  await user.click(await mainTab(/Loupist settings/));
  await user.click(screen.getByRole("button", { name: "Map building" }));
};

it("saves the World model per Loupist without touching global configuration", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const save = vi.spyOn(bridge, "knowledgeGraphSettingsSet");
  const config = vi.spyOn(bridge, "configUpdate");
  await openApp(bridge);
  await openWorldProcessing(user);
  expect(await screen.findByRole("textbox", { name: "Name" })).toHaveValue("Quality Compounder");
  await user.click(await screen.findByRole("button", { name: /Map building model/ }));
  await user.type(screen.getByRole("searchbox", { name: "Search models" }), "ChatGPT");
  await user.click(screen.getByRole("menuitemradio", { name: "GPT-5.6 Luna" }));
  await user.click(screen.getByRole("menuitem", { name: "Advanced" }));
  await user.click(screen.getByRole("menuitemradio", { name: "High" }));
  await user.keyboard("{Escape}");
  await user.click(screen.getByRole("button", { name: "Save processing settings" }));
  await screen.findByText("Map building settings saved for this Loupist.");
  expect(save).toHaveBeenCalledExactlyOnceWith({ loupistId: "loupist-quality", settings: expect.objectContaining({ helperModelProfileId: "model-test", thinkingLevel: "high" }) });
  expect(config).not.toHaveBeenCalled();
  await chooseLoupist(user, "Contrarian Value");
  await openWorldProcessing(user);
  expect(await screen.findByRole("button", { name: /Map building model/ })).toHaveTextContent("Choose model");
  await chooseLoupist(user, "Quality Compounder");
  await openWorldProcessing(user);
  expect(await screen.findByRole("button", { name: /Map building model/ })).toHaveTextContent("GPT-5.6 Luna · High");
});

it("recovers Map building and refreshes an uncertain Chat update without retrying extraction", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const workspace = await bridge.loupistSelect("loupist-quality");
  workspace.threads[0].messages[0].memory_update = {
    status: "applied", commitId: "saved-source",
    changes: [{ recordId: "source-rfc-9110", kind: "Source", operation: "create", title: "Saved protocol source", knowledge: { kind: "source", recordId: "source-rfc-9110", revisionId: "source-revision", objectId: "source-rfc-9110" }, lesson: "", basis: "", futureUse: "", worldJobId: "interrupted-world", worldStatus: "reconcile_required" }],
  };
  vi.spyOn(bridge, "loupistSelect").mockResolvedValue(workspace);
  let recovered = false;
  vi.spyOn(bridge, "knowledgeGraphJob").mockImplementation(async ({ loupistId, jobId }) => ({ jobId, loupistId, worldRecordId: "world-protocols", worldRevisionId: "rev-owned-1", payloadDigest: "digest", createdAtMs: 0, hostMayRedrive: false, status: recovered ? "reconciled_without_retry" : "reconcile_required" }));
  const settings = await bridge.knowledgeGraphSettingsGet({ loupistId: "loupist-quality" });
  vi.spyOn(bridge, "knowledgeGraphSettingsGet").mockImplementation(async () => recovered ? settings : { ...settings, readiness: { status: "reconciliation_required" } });
  const recover = vi.spyOn(bridge, "knowledgeGraphRecover").mockImplementation(async () => { recovered = true; return settings; });
  const chatSend = vi.spyOn(bridge, "chatSend");
  await openApp(bridge);
  await user.click(await screen.findByRole("button", { name: "Saved 1 source" }));
  expect(await screen.findByText(/Map: outcome uncertain/)).toBeVisible();
  await openWorldProcessing(user);
  await user.click(screen.getByRole("button", { name: "Recover map building" }));
  await screen.findByText(/Map building recovered/);
  await user.click(await mainTab(/Chat/));
  await user.click(await screen.findByRole("button", { name: "Saved 1 source" }));
  expect(await screen.findByText("Source retained · Interrupted Map building was not retried")).toBeVisible();
  expect(recover).toHaveBeenCalledExactlyOnceWith({ loupistId: "loupist-quality" });
  expect(chatSend).not.toHaveBeenCalled();
});

it("opens a source's processing settings and returns to the same filtered reader", async () => {
  const user = userEvent.setup();
  await openApp();
  await user.click(await mainTab(/Map/));
  await user.click(await screen.findByRole("button", { name: "Library" }));
  await user.click(await screen.findByRole("tab", { name: "Sources" }));
  await user.type(screen.getByRole("textbox", { name: "Search sources" }), "RFC 9110");
  await user.click(await screen.findByRole("button", { name: /RFC 9110 HTTP semantics/ }));
  const sourceId = (await screen.findByRole("article", { name: "Source reader" })).getAttribute("data-source-id");
  await user.click(screen.getByRole("button", { name: "Map building settings" }));
  expect(await screen.findByRole("textbox", { name: "Name" })).toHaveValue("Quality Compounder");
  await user.click(within(screen.getByRole("region", { name: "Loupist settings" })).getByRole("button", { name: "Back" }));
  expect(await screen.findByRole("article", { name: "Source reader" })).toHaveAttribute("data-source-id", sourceId);
  expect(screen.getByRole("textbox", { name: "Search sources" })).toHaveValue("RFC 9110");
});
