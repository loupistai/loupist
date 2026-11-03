import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "@/App";
import { MockLoupistBridge } from "@/bridge/mockBridge";

async function fresh(connected = true) {
  const bridge = new MockLoupistBridge({ delay_ms: 0, notice_acknowledged_version: 0 });
  for (const loupist of await bridge.loupistList()) await bridge.loupistDelete({ loupist_id: loupist.id });
  if (!connected) for (const provider of (await bridge.modelCatalogGet()).providers) await bridge.providerDisconnect(provider.id);
  return bridge;
}
async function start(bridge: MockLoupistBridge) {
  const user = userEvent.setup(); render(<App bridge={bridge} />);
  await user.click(await screen.findByRole("checkbox", { name: "I understand" }));
  await user.click(await screen.findByRole("button", { name: "Get started" })); return user;
}

it("creates the default Bot and lets the user ask immediately", async () => {
  const bridge = await fresh();
  const create = vi.spyOn(bridge, "loupistCreate");
  const schedule = vi.spyOn(bridge, "briefingSettingsSet");
  await start(bridge);
  expect(await screen.findByRole("textbox", { name: "Message Loupist" })).toBeEnabled();
  expect(create).toHaveBeenCalledTimes(1);
  expect(schedule).not.toHaveBeenCalled();
});

it("allows retry after creating the default Bot fails", async () => {
  const bridge = await fresh();
  vi.spyOn(bridge, "loupistCreate").mockRejectedValueOnce(new Error("Storage unavailable"));
  const user = await start(bridge);
  expect(await screen.findByRole("alert")).toHaveTextContent("Storage unavailable");
  await user.click(screen.getByRole("button", { name: "Get started" }));
  expect(await screen.findByRole("textbox", { name: "Message Loupist" })).toBeEnabled();
});

it("cancels OAuth, ignores its stale completion, and retries provider errors", async () => {
  const bridge = await fresh(false); const catalog = await new MockLoupistBridge({ delay_ms: 0 }).modelCatalogGet();
  let resolve!: (value: typeof catalog) => void;
  const pending = new Promise<typeof catalog>(done => { resolve = done; });
  const connect = vi.spyOn(bridge, "providerConnect").mockReturnValueOnce(pending).mockRejectedValueOnce(new Error("Sign-in expired"));
  const cancel = vi.spyOn(bridge, "providerConnectCancel").mockResolvedValue();
  const user = await start(bridge);
  expect(screen.queryByRole("button", { name: "New Loupist" })).toBeNull();
  await user.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
  expect(connect).toHaveBeenLastCalledWith("openai", expect.any(Function));
  expect(screen.getByRole("status")).toHaveTextContent("Finish signing in to ChatGPT in your browser");
  await user.click(screen.getByRole("button", { name: "Cancel" }));
  expect(cancel).toHaveBeenCalledTimes(1);
  await act(async () => { resolve(catalog); await pending; });
  expect(screen.queryByText(/Connected to/)).toBeNull();
  await user.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Sign-in expired");
  await user.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByRole("textbox", { name: "Message Loupist" })).toBeVisible();
  expect(connect).toHaveBeenCalledTimes(3);
});

it("imports an existing folder from the full-window wizard and opens its Chat", async () => {
  const bridge = await fresh();
  vi.spyOn(bridge, "loupistChooseFolder").mockResolvedValue("/Users/research/Imported Loupist");
  const imported = vi.spyOn(bridge, "loupistImport");
  const user = userEvent.setup(); render(<App bridge={bridge} />);
  await user.click(await screen.findByRole("checkbox", { name:"I understand" }));
  await user.click(screen.getByRole("button", { name: "Already have a Loupist folder? Import it" }));
  const dialog = await screen.findByRole("dialog", { name: "Open a Loupist folder" });
  await user.click(within(dialog).getByRole("button", { name: "Choose folder" }));
  await user.click(await within(dialog).findByRole("checkbox"));
  await user.click(within(dialog).getByRole("button", { name: "Import" }));
  await waitFor(() => expect(imported).toHaveBeenCalledWith({ workspace_path: "/Users/research/Imported Loupist" }));
  expect(await screen.findByRole("textbox", { name: "Message Loupist" })).toBeVisible();
  expect(await screen.findByRole("article", { name: /^Message from / })).toBeVisible();
});

it("gates welcome on acknowledgment and persists the version when onboarding completes", async () => {
  const bridge = await fresh();
  const acknowledge = vi.spyOn(bridge, "noticeAcknowledge");
  const user = userEvent.setup(); render(<App bridge={bridge} />);
  const startButton = await screen.findByRole("button", { name: "Get started" });
  expect(startButton).toBeDisabled();
  expect(screen.getByRole("checkbox", { name: "I understand" })).not.toBeChecked();
  await user.click(screen.getByRole("checkbox", { name: "I understand" }));
  await user.click(startButton);
  await waitFor(() => expect(acknowledge).toHaveBeenCalledTimes(1));
});

it("keeps acknowledgment required if saving at onboarding completion fails", async () => {
  const bridge = await fresh();
  const create = vi.spyOn(bridge, "loupistCreate");
  vi.spyOn(bridge, "noticeAcknowledge").mockRejectedValueOnce(new Error("Storage unavailable"));
  const user = await start(bridge);
  const dialog = await screen.findByRole("dialog", { name: "Before you start" });
  await waitFor(() => expect(within(dialog).getByRole("checkbox")).toBeEnabled());
  expect(within(dialog).getByRole("button", { name: "Continue" })).toBeDisabled();
  await user.click(within(dialog).getByRole("checkbox"));
  await user.click(within(dialog).getByRole("button", { name: "Continue" }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Before you start" })).toBeNull());
  expect(await bridge.noticeSettingsGet()).toEqual({ notice_acknowledged_version: 1 });
  expect(create).toHaveBeenCalledTimes(1);
});
