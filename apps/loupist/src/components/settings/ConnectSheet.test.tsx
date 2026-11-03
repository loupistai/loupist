import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "@/bridge/mockBridge";
import type { ModelCatalog } from "@/types";
import { ConnectSheet, type ConnectSheetTarget } from "./ConnectSheet";

async function open(target: ConnectSheetTarget, prepare?: (bridge: MockLoupistBridge) => Promise<void> | void) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await prepare?.(bridge);
  const catalog = await bridge.modelCatalogGet();
  const onConnected = vi.fn();
  const onClose = vi.fn();
  const onCatalogChange = vi.fn<(catalog: ModelCatalog) => void>();
  const user = userEvent.setup();
  render(<ConnectSheet target={target} catalog={catalog} bridge={bridge} onCatalogChange={onCatalogChange} onConnected={onConnected} onClose={onClose} />);
  const sheet = await screen.findByRole("dialog");
  return { bridge, user, sheet, onConnected, onClose, onCatalogChange };
}

it("warns that an OpenAI key replaces the ChatGPT sign-in and saves the key once", async () => {
  const { bridge, user, sheet, onConnected, onClose } = await open({ kind: "provider", provider: "openai", method: "api_key" });
  const configure = vi.spyOn(bridge, "providerConfigure");
  const consolePage = vi.spyOn(bridge, "providerOpenConsole");
  expect(within(sheet).getByText("This replaces your ChatGPT sign-in on this Mac. You can switch back any time.")).toBeVisible();
  expect(within(sheet).getByText("Your key stays on this device. Requests go straight to OpenAI.")).toBeVisible();
  await user.click(within(sheet).getByRole("button", { name: "Get a key from OpenAI" }));
  expect(consolePage).toHaveBeenCalledWith({ provider: "openai" });
  const field = within(sheet).getByLabelText("API key");
  expect(field).toHaveAttribute("type", "password");
  await user.click(within(sheet).getByRole("button", { name: "Show" }));
  expect(field).toHaveAttribute("type", "text");
  expect(within(sheet).getByRole("button", { name: "Connect" })).toBeDisabled();
  await user.type(field, "sk-fixture");
  await user.click(within(sheet).getByRole("button", { name: "Connect" }));
  await waitFor(() => expect(onConnected).toHaveBeenCalledWith("openai", "OpenAI"));
  expect(configure).toHaveBeenCalledExactlyOnceWith({ provider: "openai", api_key: "sk-fixture" });
  expect(onClose).toHaveBeenCalled();
});

it("does not show the replacement note for other key providers", async () => {
  const { sheet } = await open({ kind: "provider", provider: "google", method: "api_key" });
  expect(within(sheet).getByRole("heading", { name: "Google Gemini" })).toBeVisible();
  expect(within(sheet).queryByText(/replaces your ChatGPT sign-in/)).toBeNull();
});

it("explains a refused Upstage key", async () => {
  const { bridge, user, sheet, onConnected } = await open({ kind: "upstage" });
  vi.spyOn(bridge, "providerPresetSave").mockRejectedValue({ code: "key_rejected", message: "rejected" });
  await user.type(within(sheet).getByLabelText("API key"), "up-fixture");
  await user.click(within(sheet).getByRole("button", { name: "Connect" }));
  expect(await within(sheet).findByRole("alert")).toHaveTextContent("Upstage didn't accept this key. Check it and try again.");
  expect(onConnected).not.toHaveBeenCalled();
});

it("lists local models by engine, refuses too-short contexts and connects the chosen model", async () => {
  const { bridge, user, sheet, onConnected } = await open({ kind: "local" });
  const save = vi.spyOn(bridge, "providerCustomSave");
  const ollama = await within(sheet).findByRole("group", { name: "Ollama · 2 models" });
  const usable = within(ollama).getByRole("radio", { name: /qwen3:32b/ });
  const short = within(ollama).getByRole("radio", { name: /llama3\.2:3b/ });
  expect(usable).toBeChecked();
  expect(short).toBeDisabled();
  expect(within(ollama).getByText("Too short for research")).toBeVisible();
  expect(within(ollama).getByText("40,960 context")).toBeVisible();
  expect(within(sheet).getByText("LM Studio isn't running")).toBeVisible();
  expect(within(sheet).getByText(/Research needs a model that can read at least 20,000 tokens/)).toBeVisible();
  await user.click(within(sheet).getByRole("button", { name: "Connect Ollama" }));
  await waitFor(() => expect(onConnected).toHaveBeenCalledWith(expect.stringMatching(/^custom-/), "Ollama"));
  expect(save).toHaveBeenCalledWith({ name: "Ollama", base_url: "http://localhost:11434/v1", api: "openai-completions", model_ids: ["qwen3:32b"] });
});

it("says when no model app is running and looks again", async () => {
  const { bridge, user, sheet } = await open({ kind: "local" }, bridge => {
    bridge.localDiscovery = { engines: bridge.localDiscovery.engines.map(engine => ({ ...engine, running: false, models: [] })) };
  });
  expect(await within(sheet).findByText("No model app is running. Open Ollama, LM Studio or llama.cpp, then look again.")).toBeVisible();
  expect(within(sheet).queryByRole("button", { name: /^Connect / })).toBeNull();
  const discover = vi.spyOn(bridge, "providerLocalDiscover");
  await user.click(within(sheet).getByRole("button", { name: "Look again" }));
  expect(discover).toHaveBeenCalledTimes(1);
});

it("adds a custom endpoint only after a successful Test or typed model IDs", async () => {
  const { bridge, user, sheet, onConnected } = await open({ kind: "custom" });
  const test = vi.spyOn(bridge, "providerCustomTest");
  const save = vi.spyOn(bridge, "providerCustomSave");
  const add = within(sheet).getByRole("button", { name: "Add" });
  await user.type(within(sheet).getByLabelText("Name"), "Office gateway");
  await user.type(within(sheet).getByLabelText("Server address"), "https://gateway.example/v1");
  await user.click(within(sheet).getByRole("radio", { name: "OpenAI Responses" }));
  expect(add).toBeDisabled();

  bridge.customTestResult = { outcome: "not_found", models: [] };
  await user.click(within(sheet).getByRole("button", { name: "Test" }));
  expect(await within(sheet).findByText("API path not found. Check the address and format.")).toBeVisible();
  expect(add).toBeDisabled();

  bridge.customTestResult = { outcome: "ok", models: ["alpha", "beta"] };
  await user.click(within(sheet).getByRole("button", { name: "Test" }));
  expect(await within(sheet).findByText("Connected. Found 2 models.")).toBeVisible();
  expect(test).toHaveBeenLastCalledWith({ base_url: "https://gateway.example/v1", api: "openai-responses", api_key: undefined });
  expect(within(sheet).getByRole("checkbox", { name: "alpha" })).toBeChecked();
  await user.click(within(sheet).getByRole("checkbox", { name: "beta" }));
  expect(add).toBeEnabled();
  // Changing the address needs a new Test.
  await user.type(within(sheet).getByLabelText("Server address"), "/");
  expect(add).toBeDisabled();
  await user.click(within(sheet).getByRole("button", { name: "Test" }));
  await within(sheet).findByText("Connected. Found 2 models.");
  await user.click(within(sheet).getByRole("checkbox", { name: "beta" }));
  await user.click(add);
  await waitFor(() => expect(onConnected).toHaveBeenCalledWith("custom-office-gateway", "Office gateway"));
  expect(save).toHaveBeenCalledWith({ provider: undefined, name: "Office gateway", base_url: "https://gateway.example/v1/", api: "openai-responses", api_key: undefined, model_ids: ["alpha"] });
});

it("suggests /v1 only when the custom endpoint has no path", async () => {
  const { bridge, user, sheet } = await open({ kind: "custom" });
  bridge.customTestResult = { outcome: "not_found", models: [] };
  await user.type(within(sheet).getByLabelText("Server address"), "https://gateway.example");
  await user.click(within(sheet).getByRole("button", { name: "Test" }));
  expect(await within(sheet).findByText("The server answered, but not at this path. Try adding /v1.")).toBeVisible();
});

it("says a new address drops the saved key before an edit is saved", async () => {
  const edit = { id: "custom-office", name: "Office", base_url: "https://gateway.example/v1", api: "openai-completions" as const, model_ids: ["alpha"], credential_source: "saved" as const };
  const { user, sheet } = await open({ kind: "custom", edit });
  const address = within(sheet).getByLabelText("Server address");
  const key = within(sheet).getByLabelText("API key (optional)");
  expect(key).toHaveAccessibleDescription("Leave blank to keep the saved key.");
  await user.clear(address);
  await user.type(address, "https://other.example/v1");
  expect(key).toHaveAccessibleDescription("A new address removes the saved key. Enter it again if this server needs one.");
  // The same address, written with a trailing slash, keeps the key.
  await user.clear(address);
  await user.type(address, "https://gateway.example/v1/");
  expect(key).toHaveAccessibleDescription("Leave blank to keep the saved key.");
});

it("accepts typed model IDs without a Test", async () => {
  const { user, sheet } = await open({ kind: "custom" });
  await user.type(within(sheet).getByLabelText("Name"), "Lab");
  await user.type(within(sheet).getByLabelText("Server address"), "http://localhost:8000/v1");
  await user.click(within(sheet).getByRole("button", { name: "Enter model IDs instead" }));
  expect(within(sheet).getByRole("button", { name: "Add" })).toBeDisabled();
  await user.type(within(sheet).getByLabelText("Model IDs"), "qwen2.5:7b");
  expect(within(sheet).getByRole("button", { name: "Add" })).toBeEnabled();
});

it("groups the picker like onboarding and keeps the rest of Pi's providers collapsed", async () => {
  const { user, sheet } = await open({ kind: "picker" });
  expect(within(sheet).getByRole("heading", { name: "Add a provider" })).toBeVisible();
  const chatgpt = within(sheet).getByRole("button", { name: /Continue with ChatGPT/ });
  // ChatGPT is signed in, so its row shows its state instead of "Recommended".
  expect(chatgpt).toHaveTextContent("Connected");
  expect(chatgpt).not.toHaveTextContent("Recommended");
  const more = within(sheet).getByText("More providers (2)").closest("details")!;
  expect(more).not.toHaveAttribute("open");
  await user.click(within(sheet).getByRole("button", { name: "Google Gemini" }));
  expect(within(sheet).getByRole("heading", { name: "Google Gemini" })).toBeVisible();
  await user.click(within(sheet).getByRole("button", { name: "All ways" }));
  expect(within(sheet).getByRole("heading", { name: "Add a provider" })).toBeVisible();
});

it("opens Claude's billing note before its sign-in starts", async () => {
  const { bridge, user, sheet } = await open({ kind: "picker" });
  const connect = vi.spyOn(bridge, "providerConnect").mockImplementation(() => new Promise(() => {}));
  await user.click(within(sheet).getByRole("button", { name: /Sign in with Claude/ }));
  expect(within(sheet).getByText(/Claude sign-in draws on Claude extra usage/)).toBeVisible();
  expect(connect).not.toHaveBeenCalled();
  await user.click(within(sheet).getByRole("button", { name: "Continue" }));
  expect(connect).toHaveBeenCalledWith("anthropic", expect.any(Function));
  expect(await within(sheet).findByRole("status")).toHaveTextContent("Finish signing in to Claude in your browser");
});
