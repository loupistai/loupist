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

const createLoupist = async (user: ReturnType<typeof userEvent.setup>) => user.click(await screen.findByRole("button", { name: "Create Loupist" }));

it("offers the presets after connecting and creates the Loupist from the chosen one", async () => {
  const bridge = await fresh();
  const create = vi.spyOn(bridge, "loupistCreate");
  const user = await start(bridge);
  expect(screen.getByRole("button", { name: /My portfolio/ })).toHaveAttribute("aria-pressed", "true");
  await user.click(screen.getByRole("button", { name: /Semiconductors/ }));
  expect(screen.getByRole("textbox", { name: "Name" })).toHaveValue("Chip Loupist");
  expect(create).not.toHaveBeenCalled();
  await createLoupist(user);
  expect(await screen.findByRole("textbox", { name: "Message Loupist" })).toBeEnabled();
  expect(create).toHaveBeenCalledTimes(1);
  expect(create).toHaveBeenCalledWith(expect.objectContaining({ template: "semiconductors", name: "Chip Loupist" }));
});

it("creates the default Bot and lets the user ask immediately", async () => {
  const bridge = await fresh();
  const create = vi.spyOn(bridge, "loupistCreate");
  const schedule = vi.spyOn(bridge, "briefingSettingsSet");
  await createLoupist(await start(bridge));
  expect(await screen.findByRole("textbox", { name: "Message Loupist" })).toBeEnabled();
  expect(create).toHaveBeenCalledTimes(1);
  expect(schedule).not.toHaveBeenCalled();
});

it("allows retry after creating the default Bot fails", async () => {
  const bridge = await fresh();
  vi.spyOn(bridge, "loupistCreate").mockRejectedValueOnce(new Error("Storage unavailable"));
  const user = await start(bridge);
  await createLoupist(user);
  expect(await screen.findByRole("alert")).toHaveTextContent("Storage unavailable");
  await createLoupist(user);
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
  await createLoupist(user);
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
  await createLoupist(user);
  await waitFor(() => expect(acknowledge).toHaveBeenCalledTimes(1));
});

it("keeps acknowledgment required if saving at onboarding completion fails", async () => {
  const bridge = await fresh();
  const create = vi.spyOn(bridge, "loupistCreate");
  vi.spyOn(bridge, "noticeAcknowledge").mockRejectedValueOnce(new Error("Storage unavailable"));
  const user = await start(bridge);
  await createLoupist(user);
  const dialog = await screen.findByRole("dialog", { name: "Before you start" });
  await waitFor(() => expect(within(dialog).getByRole("checkbox")).toBeEnabled());
  expect(within(dialog).getByRole("button", { name: "Continue" })).toBeDisabled();
  await user.click(within(dialog).getByRole("checkbox"));
  await user.click(within(dialog).getByRole("button", { name: "Continue" }));
  await waitFor(() => expect(screen.queryByRole("dialog", { name: "Before you start" })).toBeNull());
  expect(await bridge.noticeSettingsGet()).toEqual({ notice_acknowledged_version: 1 });
  expect(create).toHaveBeenCalledTimes(1);
});

it("lets a user without an AI account look around first, and goes back between steps", async () => {
  const bridge = await fresh(false);
  const acknowledge = vi.spyOn(bridge, "noticeAcknowledge");
  const user = await start(bridge);
  expect(screen.getByRole("heading", { name: "Connect your AI account" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Back" }));
  expect(screen.getByRole("checkbox", { name: "I understand" })).toBeChecked();
  await user.click(screen.getByRole("button", { name: "Get started" }));
  await user.click(screen.getByRole("button", { name: "Look around first" }));
  expect(screen.getByRole("heading", { name: "Create your first Loupist" })).toBeVisible();
  expect(screen.getByText(/reads it in the background to build its Map/)).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Back" }));
  expect(screen.getByRole("heading", { name: "Connect your AI account" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Look around first" }));
  await createLoupist(user);
  expect((await screen.findAllByRole("button", { name: "Connect an AI account" }))[0]).toBeVisible();
  await waitFor(() => expect(acknowledge).toHaveBeenCalledTimes(1));
});

it("follows a template's starter from the first Loupist step", async () => {
  const bridge = await fresh();
  const follow = vi.spyOn(bridge, "watchlistAdd");
  const user = await start(bridge);
  await user.click(screen.getByRole("button", { name: /^Crypto/ }));
  await user.click(screen.getByRole("button", { name: "Follow Bitcoin" }));
  await waitFor(() => expect(follow).toHaveBeenCalledWith({ symbol: "BTC", market: "CRYPTO", displayName: "Bitcoin" }));
  expect(screen.getByRole("button", { name: "Follow Bitcoin" })).toHaveAttribute("aria-pressed", "true");
});
