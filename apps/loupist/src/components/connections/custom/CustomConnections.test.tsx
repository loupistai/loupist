import { render, screen, within, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { MockLoupistBridge } from "../../../bridge/mockBridge";
import { AddConnectionDialog } from "./AddConnectionDialog";
import { ConnectionDetail } from "./ConnectionDetail";
import { ToolReview } from "./ToolReview";
import { CustomConnections } from "./CustomConnections";

function setup() { return new MockLoupistBridge({ delay_ms: 0, customConnectionsAvailable: true }); }
async function connection(bridge: MockLoupistBridge) {
  const pending = await bridge.customConnectionAddUrl({ url: "https://research.example/mcp", name: "Research" });
  return bridge.customConnectionConfirm({ connection_id: pending.id });
}
it("offers an explicit saved-key check for a blocked custom connection", async () => {
  const bridge = setup();
  const saved = await connection(bridge);
  const confirm = vi.spyOn(bridge, "customConnectionConfirm").mockResolvedValue(saved);
  const onChanged = vi.fn();
  render(<ConnectionDetail bridge={bridge} connection={{ ...saved, last_error: "credential_access_unavailable" }} loupistId="loupist-one" onChanged={onChanged} onClose={() => {}} onRemoved={() => {}} />);
  expect(screen.getByText("macOS has not allowed Loupist to use this saved key yet. Choose Always Allow when macOS asks, then check again.")).toBeVisible();
  expect(confirm).not.toHaveBeenCalled();
  await userEvent.setup().click(screen.getByRole("button", { name: "Check saved key" }));
  expect(confirm).toHaveBeenCalledExactlyOnceWith({ connection_id: saved.id });
  expect(onChanged).toHaveBeenCalledWith(saved);
});
it("connects then requires a choice for every eligible tool before Done", async () => {
  const bridge = setup();
  const confirmed = await connection(bridge);
  confirmed.tools.push({ ...confirmed.tools[0], name: "read_filings", definition_hash: "c".repeat(64) });
  const confirm = vi.spyOn(bridge, "customConnectionConfirm").mockResolvedValue(confirmed);
  vi.spyOn(bridge, "customToolApprove").mockImplementation(async request => {
    confirmed.tools = confirmed.tools.map(tool => tool.name === request.tool_name ? { ...tool, approval: request.choice } : tool);
    return structuredClone(confirmed);
  });
  const onClose = vi.fn();
  render(<AddConnectionDialog bridge={bridge} loupistId="loupist-one" onClose={onClose} onChanged={() => {}} />);
  const user = userEvent.setup();
  await user.type(screen.getByRole("textbox", { name: "Server URL" }), "https://research.example/mcp");
  await user.click(screen.getByRole("button", { name: "Connect" }));
  expect(await screen.findByRole("button", { name: "Done" })).toBeDisabled();
  // Why Done waits is said beside Done, not below the tool list.
  expect(screen.getByRole("button", { name: "Done" }).parentElement).toHaveTextContent("Choose for each tool");
  expect(screen.getByText("Read prices")).toBeVisible();
  expect(screen.getByText("read_prices")).toBeVisible();
  await user.click(within(screen.getByRole("radiogroup", { name: "Approval for Read prices" })).getByRole("radio", { name: "Allow" }));
  expect(screen.getByRole("button", { name: "Done" })).toBeDisabled();
  await user.click(within(screen.getByRole("radiogroup", { name: "Approval for Read filings" })).getByRole("radio", { name: "Don’t allow" }));
  expect(screen.getByRole("button", { name: "Done" })).toBeEnabled();
  expect(screen.queryByRole("radiogroup", { name: "Approval for Place order" })).toBeNull();
  expect(screen.queryByText(/Choose for each tool/)).toBeNull();
  const enable = vi.spyOn(bridge, "customConnectionSetEnabled");
  await user.click(screen.getByRole("button", { name: "Done" }));
  expect(onClose).toHaveBeenCalledOnce();
  expect(confirm).toHaveBeenCalledWith(expect.objectContaining({ loupist_id: "loupist-one" }));
  expect(enable).not.toHaveBeenCalled();
});
it("enables the current Loupist at confirm, then Allow sticks before Done closes", async () => {
  const bridge = setup();
  const enable = vi.spyOn(bridge, "customConnectionSetEnabled");
  const onClose = vi.fn();
  render(<AddConnectionDialog bridge={bridge} loupistId="loupist-one" onClose={onClose} onChanged={() => {}} />);
  const user = userEvent.setup();
  await user.type(screen.getByRole("textbox", { name: "Server URL" }), "https://research.example/mcp");
  await user.click(screen.getByRole("button", { name: "Connect" }));
  const allow = await screen.findByRole("radio", { name: "Allow" });
  const listed = await bridge.customConnectionList({ loupist_id: "loupist-one" });
  expect(listed.some(record => record.enabled)).toBe(true);
  await user.click(allow);
  expect(screen.getByRole("radio", { name: "Allow" })).toHaveAttribute("aria-checked", "true");
  expect(screen.getByRole("button", { name: "Done" })).toBeEnabled();
  await user.click(screen.getByRole("button", { name: "Done" }));
  expect(onClose).toHaveBeenCalledOnce();
  expect(enable).not.toHaveBeenCalled();
});
it("stages JSON entries without confirming until Add and shows only secret names", async () => {
  const bridge = setup(); const confirm = vi.spyOn(bridge, "customConnectionConfirm");
  render(<AddConnectionDialog bridge={bridge} loupistId="loupist-one" onClose={() => {}} onChanged={() => {}} />);
  const user = userEvent.setup(); await user.click(screen.getByRole("radio", { name: "Paste JSON" }));
  const json = JSON.stringify({ mcpServers: { local: { command: "node", args: ["/research/server.mjs", "two words"], env: { RESEARCH_TOKEN: "fixture-secret" } }, remote: { url: "https://research.example", headers: { Authorization: "fixture-header" } } } });
  await user.click(screen.getByRole("textbox", { name: "Paste JSON" })); await user.paste(json);
  await user.click(screen.getByRole("button", { name: "Review" }));
  expect(await screen.findByText("local")).toBeVisible(); expect(confirm).not.toHaveBeenCalled();
  expect(screen.getByText("node /research/server.mjs 'two words'")).toBeVisible();
  // The reviewer sees where an online entry sends data before adding it.
  expect(screen.getByText(/^https:\/\/research\.example\/?$/)).toBeVisible();
  expect(document.body.textContent).not.toContain("fixture-secret"); expect(document.body.textContent).not.toContain("fixture-header");
  expect(screen.queryByRole("textbox", { name: "Paste JSON" })).toBeNull();
  await user.click(screen.getAllByRole("button", { name: "Add" })[0]);
  expect(confirm).toHaveBeenCalledOnce();
});
it("warns before adding a pasted local command but not an online server", async () => {
  const bridge = setup();
  render(<AddConnectionDialog bridge={bridge} loupistId="loupist-one" onClose={() => {}} onChanged={() => {}} />);
  const user = userEvent.setup();
  await user.click(screen.getByRole("radio", { name: "Paste JSON" }));
  const json = JSON.stringify({ mcpServers: { local: { command: "npx", args: ["@example/research-archive@1.2.3"] }, remote: { url: "https://research.example" } } });
  await user.click(screen.getByRole("textbox", { name: "Paste JSON" }));
  await user.paste(json);
  await user.click(screen.getByRole("button", { name: "Review" }));
  const local = (await screen.findByText("local")).closest("li");
  const remote = screen.getByText("remote").closest("li");
  expect(local).not.toBeNull();
  expect(remote).not.toBeNull();
  expect(within(local!).getByText("npx @example/research-archive@1.2.3")).toBeVisible();
  const warning = within(local!).getByText(/This runs a program on this Mac with your user permissions/);
  expect(warning.nextElementSibling).toHaveAccessibleName("Add");
  expect(within(remote!).queryByText(/This runs a program on this Mac/)).toBeNull();
});
it("masks command environment values and clears them after staging", async () => {
  const bridge = setup(); const confirm = vi.spyOn(bridge, "customConnectionConfirm");
  render(<AddConnectionDialog bridge={bridge} loupistId="loupist-one" onClose={() => {}} onChanged={() => {}} />);
  const user = userEvent.setup(); await user.click(screen.getByText("Advanced")); await user.click(screen.getByRole("button", { name: "Local command" }));
  await user.type(screen.getByRole("textbox", { name: "Command" }), "node");
  await user.type(screen.getByRole("textbox", { name: "Arguments (one per line)" }), "/research/server.mjs");
  await user.click(screen.getByRole("button", { name: "Add variable" }));
  await user.type(screen.getByLabelText("Variable 1 name"), "TOKEN");
  const secret = screen.getByLabelText("Variable 1 value"); expect(secret).toHaveAttribute("type", "password");
  await user.type(secret, "fixture-secret"); expect(document.body.textContent).not.toContain("fixture-secret");
  await user.click(screen.getByRole("button", { name: "Review" }));
  expect(await screen.findByText("node /research/server.mjs")).toBeVisible(); expect(screen.queryByLabelText("Variable 1 value")).toBeNull(); expect(confirm).not.toHaveBeenCalled();
});
it("resets the displayed choice when the host returns a changed definition", async () => {
  const bridge = setup(); const record = await connection(bridge); record.tools[0].approval = "always";
  const props = { bridge, loupistId: "loupist-one", onChanged: vi.fn(), onError: vi.fn() };
  const view = render(<ToolReview {...props} connection={record} />);
  expect(screen.getByRole("radio", { name: "Allow" })).toHaveAttribute("aria-checked", "true");
  view.rerender(<ToolReview {...props} connection={{ ...record, tools: record.tools.map(tool => ({ ...tool, definition_hash: "new-hash", definition_changed: true, approval: null })) }} />);
  expect(screen.getByRole("radio", { name: "Allow" })).toHaveAttribute("aria-checked", "false");
  expect(screen.getByText("Changed since you allowed it. Review again.")).toBeVisible();
});
it("requires confirmation before removing and writes per-Loupist enablement", async () => {
  const bridge = setup(); const record = await connection(bridge); const remove = vi.spyOn(bridge, "customConnectionRemove"); const enable = vi.spyOn(bridge, "customConnectionSetEnabled");
  function Detail() { const [current, setCurrent] = useState(record); return <ConnectionDetail bridge={bridge} connection={current} loupistId="loupist-one" onChanged={setCurrent} onRemoved={() => {}} onClose={() => {}} />; }
  render(<Detail />); const user = userEvent.setup();
  expect(screen.getByRole("switch", { name: "Save results to Memory" })).not.toBeChecked();
  const memory = vi.spyOn(bridge, "customConnectionSetMemory");
  await user.click(screen.getByRole("switch", { name: "Save results to Memory" }));
  await waitFor(() => expect(memory).toHaveBeenCalledWith({ connection_id: record.id, dont_save_to_memory: false }));
  await user.click(screen.getByRole("switch", { name: "Enable for this Loupist" }));
  await waitFor(() => expect(enable).toHaveBeenCalledWith({ connection_id: record.id, loupist_id: "loupist-one", enabled: true }));
  await user.click(screen.getByRole("button", { name: "Remove connection" })); expect(remove).not.toHaveBeenCalled();
  const dialog = screen.getByRole("dialog", { name: "Remove Research?" });
  await user.click(within(dialog).getByRole("button", { name: "Remove connection" }));
  expect(remove).toHaveBeenCalledWith({ connection_id: record.id });
});
it("moves focus to Add a connection after a connection is removed", async () => {
  const bridge = setup();
  await connection(bridge);
  const loupists = await bridge.loupistList();
  render(<CustomConnections bridge={bridge} loupists={loupists} initialLoupistId={loupists[0]!.id} />);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: /^Research/u }));
  await user.click(screen.getByRole("button", { name: "Remove connection" }));
  await user.click(within(screen.getByRole("dialog", { name: "Remove Research?" })).getByRole("button", { name: "Remove connection" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "Add a connection" })).toHaveFocus());
  expect(screen.queryByRole("button", { name: /^Research/u })).toBeNull();
});

it("cancels browser sign-in and keeps Done disabled while authorization is pending", async () => {
  const bridge = setup(); const record = await connection(bridge);
  record.pending = true; record.tools = []; record.auth_state = "sign_in_needed";
  vi.spyOn(bridge, "customConnectionConfirm").mockResolvedValue(record);
  let complete: (value: typeof record) => void = () => {};
  let pendingSignIn = false;
  const signIn = vi.spyOn(bridge, "customConnectionSignIn").mockImplementation(() => {
    if (pendingSignIn) return Promise.reject(new Error("Sign-in is already in progress"));
    pendingSignIn = true;
    return new Promise(resolve => { complete = (value) => { pendingSignIn = false; resolve(value); }; });
  });
  const cancel = vi.spyOn(bridge, "customConnectionSignInCancel").mockImplementation(async () => { complete(record); });
  render(<AddConnectionDialog bridge={bridge} loupistId="loupist-one" onClose={() => {}} onChanged={() => {}} />);
  const user = userEvent.setup(); await user.type(screen.getByRole("textbox", { name: "Server URL" }), "https://research.example/mcp");
  await user.click(screen.getByRole("button", { name: "Connect" }));
  expect(await screen.findByText("Finish signing in to Research in your browser…")).toBeVisible(); expect(signIn).toHaveBeenCalledWith({ connection_id: record.id });
  await user.click(screen.getByRole("button", { name: "Open the sign-in page again" }));
  await waitFor(() => expect(signIn).toHaveBeenCalledTimes(2));
  expect(cancel).toHaveBeenCalledTimes(1);
  expect(pendingSignIn).toBe(true);
  expect(screen.getByText("Finish signing in to Research in your browser…")).toBeVisible();
  expect(screen.getByRole("button", { name: "Done" })).toBeDisabled();
  await user.click(screen.getByRole("button", { name: "Cancel" }));
  expect(cancel).toHaveBeenCalledWith({ connection_id: record.id });
});
it("confirms for the opening Loupist once browser sign-in finishes", async () => {
  const bridge = setup(); const record = await connection(bridge);
  const needsSignIn = { ...record, pending: true, tools: [], auth_state: "sign_in_needed" as const };
  const signedIn = { ...record, pending: false, auth_state: "signed_in" as const };
  const confirm = vi.spyOn(bridge, "customConnectionConfirm").mockResolvedValueOnce(needsSignIn).mockResolvedValueOnce(signedIn);
  vi.spyOn(bridge, "customConnectionSignIn").mockResolvedValue(signedIn);
  render(<AddConnectionDialog bridge={bridge} loupistId="loupist-one" onClose={() => {}} onChanged={() => {}} />);
  const user = userEvent.setup(); await user.type(screen.getByRole("textbox", { name: "Server URL" }), "https://research.example/mcp");
  await user.click(screen.getByRole("button", { name: "Connect" }));
  await waitFor(() => expect(confirm).toHaveBeenCalledTimes(2));
  expect(confirm).toHaveBeenLastCalledWith({ connection_id: record.id, loupist_id: "loupist-one" });
});
it("shows approvals only for the selected Loupist", async () => {
  const bridge = setup();
  const first = (await bridge.loupistList())[0];
  const second = { ...first, id: "loupist-other", name: "Another Loupist" };
  const record = await connection(bridge);
  await bridge.customToolApprove({ connection_id: record.id, loupist_id: first.id, tool_name: record.tools[0].name, definition_hash: record.tools[0].definition_hash, choice: "always" });
  render(<CustomConnections bridge={bridge} loupists={[first, second]} initialLoupistId={first.id} />);
  expect(await screen.findByText("Online server · 1 tool allowed")).toBeVisible();
  await userEvent.setup().selectOptions(screen.getByRole("combobox", { name: "Loupist" }), second.id);
  expect(await screen.findByText("Online server · 0 tools allowed")).toBeVisible();
  expect(screen.queryByText("Online server · 1 tool allowed")).toBeNull();
});
it("offers Show more only for a tool description the clamp cuts short", async () => {
  const bridge = setup();
  const record = await connection(bridge);
  record.tools[1] = { ...record.tools[1], name: "read_filings", server_description: "Read public company filings. ".repeat(20), eligibility: "read_only" };
  // jsdom has no layout: a clamped paragraph with a long text reports more content than its three-line box.
  const scroll = vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockImplementation(function (this: HTMLElement) { return this.classList.contains("custom-description-clamp") && (this.textContent ?? "").length > 100 ? 200 : 60; });
  const client = vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(60);
  try {
    render(<ToolReview bridge={bridge} connection={record} loupistId="loupist-one" onChanged={() => {}} onError={() => {}} />);
    const short = screen.getByText("Read market prices.").closest("li")!;
    const long = screen.getByText(/Read public company filings/).closest("li")!;
    expect(within(short).queryByRole("button", { name: "Show more" })).toBeNull();
    await userEvent.setup().click(await within(long).findByRole("button", { name: "Show more" }));
    expect(within(long).getByRole("button", { name: "Show less" })).toHaveAttribute("aria-expanded", "true");
  } finally {
    scroll.mockRestore(); client.mockRestore();
  }
});

it("locates malformed JSON under the field and submits only after syntax is corrected", async () => {
  const bridge = setup();
  const add = vi.spyOn(bridge, "customConnectionAddJson").mockRejectedValue({ message: "No server entries were found." });
  render(<AddConnectionDialog bridge={bridge} loupistId="loupist-one" onClose={() => {}} onChanged={() => {}} />);
  const user = userEvent.setup();
  await user.click(screen.getByRole("radio", { name: "Paste JSON" }));
  const input = screen.getByRole("textbox", { name: "Paste JSON" });
  const submit = screen.getByRole("button", { name: "Review" });
  for (const [text, column] of [["{", 2], ["{ broken", 3]] as const) {
    await user.clear(input);
    await user.click(input);
    await user.paste(text);
    expect(input).toHaveValue(text);
    expect(input).toHaveAccessibleDescription(new RegExp(`Line 1, column ${column}`));
    expect(submit).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
  }
  expect(add).not.toHaveBeenCalled();
  await user.clear(input);
  await user.paste("{}");
  expect(input).not.toHaveAttribute("aria-invalid", "true");
  await user.click(submit);
  expect(await screen.findByRole("alert")).toHaveTextContent("No server entries were found.");
  expect(add).toHaveBeenCalledExactlyOnceWith({ json: "{}" });
});
