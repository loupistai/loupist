import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatActivityDetail, LoupistWorkspace } from "../../types";
import { openApp, openChat } from "../renderApp";

const commandProgress = { startedAtMs: 10, finishedAtMs: 20, items: [
  { id: "command", kind: "tool" as const, toolName: "bash", category: "compute" as const, operation: "execute" as const, action: "Ran a command", status: "succeeded" as const, detailId: "durable-command", startedAtMs: 10, finishedAtMs: 20 },
] };
const commandDetail = (output: string, file_path?: string): ChatActivityDetail => ({ id: "durable-command", tool_name: "bash", status: "succeeded", input: "python analysis.py", output, file_path, started_at_ms: 10, finished_at_ms: 20, truncated: false });
const unavailable = (bridge: MockLoupistBridge, edit: (workspace: LoupistWorkspace) => void = () => {}) => {
  const select = bridge.loupistSelect.bind(bridge);
  return vi.spyOn(bridge, "loupistSelect").mockImplementationOnce(async (id) => {
    const workspace = await select(id);
    workspace.loupist.availability = { status: "workspace_unavailable" };
    edit(workspace);
    return workspace;
  });
};
const reconnectFolder = async (user: ReturnType<typeof userEvent.setup>) => {
  await user.click(screen.getByRole("button", { name: "Reconnect folder" }));
  const dialog = screen.getByRole("dialog", { name: "Reconnect workspace folder" });
  await user.click(within(dialog).getByRole("button", { name: "Choose folder" }));
  await user.click(within(dialog).getByRole("checkbox", { name: /I trust this folder/ }));
  await user.click(within(dialog).getByRole("button", { name: "Reconnect folder" }));
};

it.each(["empty", "cached without credentials"])("keeps saved research readable with %s models and no connected account", async (modelState) => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const catalog = await bridge.modelCatalogGet();
  vi.spyOn(bridge, "modelCatalogGet").mockResolvedValue({ hidden_model_profile_ids: [], providers: [],
    models: modelState === "empty" ? [] : catalog.models.map((model) => ({ ...model, credential_source: "missing" })) });
  render(<App bridge={bridge} />);
  await openChat();
  expect(await screen.findByText(/Want to determine whether this quarter's margin decline is temporary/)).toBeVisible();
  expect(screen.queryByRole("button", { name: "Get started" })).toBeNull();
  expect(screen.getByRole("button", { name: "Connect an AI account" })).toBeVisible();
});

it("keeps first-run onboarding when the default Loupist has no account yet", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0, notice_acknowledged_version: 0 });
  for (const provider of (await bridge.modelCatalogGet()).providers) await bridge.providerDisconnect(provider.id);
  render(<App bridge={bridge} />);
  expect(await screen.findByRole("button", { name: "Get started" })).toBeVisible();
  expect(screen.queryByRole("textbox", { name: "Message Loupist" })).toBeNull();
});

it("inspects a saved command inline and sends a file reference only after Ask and Send", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const select = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async (id) => {
    const workspace = await select(id);
    const message = workspace.threads[0]!.messages.at(-1)!;
    message.content += "\n\n[Analysis script](analysis.py)";
    message.progress = commandProgress;
    return workspace;
  });
  const activity = vi.spyOn(bridge, "chatActivityRead").mockResolvedValue(commandDetail("Margin 12.4%", "analysis.py"));
  vi.spyOn(bridge, "workspaceRead").mockResolvedValue({ path: "analysis.py", content: "margin = profit / revenue", size_bytes: 25, media_type: "text/plain", content_sha256: "file-revision", binary: false, truncated: false });
  const chatSend = vi.spyOn(bridge, "chatSend");
  await openApp(bridge);
  await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "Compare margins.");
  expect(activity).not.toHaveBeenCalled();
  await user.click(within(screen.getByLabelText("Work progress")).getByRole("button"));
  await user.click(screen.getByRole("button", { name: "Inspect Ran a command" }));
  expect(await within(screen.getByLabelText("Work progress")).findByText("Margin 12.4%")).toBeVisible();
  expect(activity).toHaveBeenCalledWith({ loupist_id: "loupist-quality", thread_id: "thread-margin", detail_id: "durable-command" });
  await user.click(screen.getByRole("link", { name: "Analysis script" }));
  const panel = await screen.findByLabelText("Chat workspace panel");
  await within(panel).findByText("margin = profit / revenue");
  await user.click(within(panel).getByRole("button", { name: "Ask about this" }));
  expect(screen.getByRole("textbox", { name: "Message Loupist" })).toHaveValue("Compare margins. @analysis.py ");
  expect(chatSend).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Send" }));
  await waitFor(() => expect(chatSend).toHaveBeenCalledTimes(1));
  expect(chatSend.mock.calls[0][0].prompt).toContain("analysis.py");
  expect(chatSend.mock.calls[0][0].prompt).not.toContain("profit / revenue");
});

it("keeps a disconnected Loupist readable and reconnects it with the draft intact", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList();
  vi.spyOn(bridge, "loupistList").mockResolvedValueOnce([{ ...loupist!, availability: { status: "workspace_unavailable" } }]);
  unavailable(bridge, (workspace) => { workspace.threads[0]!.messages.at(-1)!.progress = commandProgress; });
  vi.spyOn(bridge, "chatActivityRead").mockResolvedValue(commandDetail("Retained command result"));
  vi.spyOn(bridge, "loupistChooseFolder").mockResolvedValue("/Users/research/moved");
  const reconnect = vi.spyOn(bridge, "loupistReconnect");
  const chatSend = vi.spyOn(bridge, "chatSend");
  const create = vi.spyOn(bridge, "loupistCreate");
  render(<App bridge={bridge} />);
  await screen.findByRole("status", { name: "Workspace folder unavailable" });
  await openChat();
  const composer = screen.getByRole("textbox", { name: "Message Loupist" });
  await user.type(composer, "Retained draft");
  expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  await user.keyboard("{Enter}");
  expect(chatSend).not.toHaveBeenCalled();
  await user.click(within(screen.getByLabelText("Work progress")).getByRole("button"));
  await user.click(screen.getByRole("button", { name: "Inspect Ran a command" }));
  expect(await within(screen.getByLabelText("Work progress")).findByText("Retained command result")).toBeVisible();
  await reconnectFolder(user);
  await waitFor(() => expect(reconnect).toHaveBeenCalledWith({ loupist_id: loupist!.id, workspace_path: "/Users/research/moved" }));
  await waitFor(() => expect(screen.queryByRole("status", { name: "Workspace folder unavailable" })).toBeNull());
  expect(screen.getByRole("textbox", { name: "Message Loupist" })).toHaveValue("Retained draft");
  expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
  expect(create).not.toHaveBeenCalled();
});

it("refuses a Loupist an earlier version made and offers only removing it", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList();
  const earlier = { ...loupist!, availability: { status: "earlier_version" as const } };
  vi.spyOn(bridge, "loupistList").mockResolvedValue([earlier]);
  vi.spyOn(bridge, "loupistSelect").mockResolvedValue({ loupist: earlier, threads: [] });
  render(<App bridge={bridge} />);
  const refusal = await screen.findByRole("alert", { name: "Made by an earlier version" });
  expect(within(refusal).getByText("This Loupist was made by an earlier version and can't be opened. Create a new Loupist.")).toBeVisible();
  expect(screen.queryByRole("textbox", { name: "Message Loupist" })).toBeNull();
  await user.click(within(refusal).getByRole("button", { name: "Remove Loupist" }));
  expect(await screen.findByRole("dialog", { name: "Remove Loupist?" })).toBeVisible();
});

it("replaces a longer cached conversation with the restored folder snapshot", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList();
  vi.spyOn(bridge, "loupistList").mockResolvedValueOnce([{ ...loupist!, availability: { status: "workspace_unavailable" } }]);
  let restored = false;
  const select = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async (id) => {
    const result = await select(id);
    return {
      ...result,
      loupist: restored ? result.loupist : { ...result.loupist, availability: { status: "workspace_unavailable" as const } },
      threads: result.threads.map((thread, index) => index ? thread : {
        ...thread,
        messages: restored ? thread.messages.slice(0, 2) : [...thread.messages, { ...thread.messages.at(-1)!, id: "reply-after-copy", content: "Reply after folder copy." }],
      }),
    };
  });
  const reconnect = bridge.loupistReconnect.bind(bridge);
  vi.spyOn(bridge, "loupistReconnect").mockImplementation(async (request) => { const result = await reconnect(request); restored = true; return result; });
  vi.spyOn(bridge, "loupistChooseFolder").mockResolvedValue("/Users/research/copied-loupist");
  render(<App bridge={bridge} />);
  await openChat();
  await screen.findByText("Reply after folder copy.");
  await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "Unsent draft");
  await reconnectFolder(user);
  await waitFor(() => expect(screen.queryByText("Reply after folder copy.")).toBeNull());
  expect(screen.getByRole("textbox", { name: "Message Loupist" })).toHaveValue("Unsent draft");
});
