import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { LoupistSettings } from "./LoupistSettings";

it("offers global Retry when a focus reload fails after a failed capability toggle", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const agents = await bridge.loupistList();
  const catalog = vi.spyOn(bridge, "agentSkillCatalog");
  vi.spyOn(bridge, "loupistCapabilityDisable").mockRejectedValueOnce(new Error("Toggle failed."));
  render(<LoupistSettings catalog={null} onChanged={vi.fn()} onBack={vi.fn()} onManageProviders={vi.fn()} bridge={bridge} selectedAgent={agents[0]} mutationBusy={false} mutationError={null} onOpenMarketplace={vi.fn()} onDelete={vi.fn()} onAgentUpdated={vi.fn()} />);
  const user = userEvent.setup();
  await user.click(document.querySelector('[data-loupist-section="capabilities"] > summary')!);
  await user.click(await screen.findByRole("switch", { name: "World Bank Indicators: enabled" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Toggle failed.");
  catalog.mockRejectedValueOnce(new Error("Configuration reload failed."));
  await act(async () => { window.dispatchEvent(new Event("focus")); });
  await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Configuration reload failed."));
  await user.click(within(screen.getByRole("alert")).getByRole("button", { name: "Retry" }));
  await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
});

it("preserves capability search, filter and confirmation through configuration and focus reloads", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const agents = await bridge.loupistList();
  const catalog = vi.spyOn(bridge, "agentSkillCatalog");
  const props = { catalog: null, onChanged: vi.fn(), onBack: vi.fn(), onManageProviders: vi.fn(), bridge, selectedAgent: agents[0], mutationBusy: false, mutationError: null, onOpenMarketplace: vi.fn(), onDelete: vi.fn(), onAgentUpdated: vi.fn() };
  const view = render(<LoupistSettings {...props} />);
  const user = userEvent.setup();
  await user.click(document.querySelector('[data-loupist-section="capabilities"] > summary')!);
  const research = await screen.findByRole("switch", { name: "Research: enabled" });
  const search = screen.getByRole("searchbox", { name: "Search skills and connections" });
  await user.type(search, "Research");
  await user.click(research);
  await screen.findByRole("switch", { name: "Research: disabled" });
  const confirmation = await screen.findByText(`Research disabled for ${agents[0].name}.`);
  expect(search).toHaveValue("Research");
  await user.click(screen.getByRole("radio", { name: "Enabled" }));
  const calls = catalog.mock.calls.length;
  await act(async () => { window.dispatchEvent(new Event("focus")); });
  await waitFor(() => expect(catalog).toHaveBeenCalledTimes(calls + 1));
  expect(search).toHaveValue("Research");
  expect(screen.getByRole("radio", { name: "Enabled" })).toBeChecked();
  expect(confirmation).toBeVisible();
  view.rerender(<LoupistSettings {...props} selectedAgent={agents[1]} />);
  await waitFor(() => expect(search).toHaveValue(""));
  expect(screen.getByRole("radio", { name: "All" })).toBeChecked();
  expect(screen.queryByText(`Research disabled for ${agents[0].name}.`)).toBeNull();
});

it("searches the selected Loupist’s capabilities and reveals that Loupist’s own workspace", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const agents = await bridge.loupistList();
  const selectedAgent = agents[0];
  const reveal = vi.spyOn(bridge, "workspaceReveal").mockResolvedValue(undefined);
  vi.spyOn(bridge, "agentSkillCatalog").mockResolvedValue([
    { id: "filing", name: "Filing comparison", description: "Compare financial statements", enabled: true, fixed: false },
    { id: "valuation", name: "Valuation", description: "Model cash flows", enabled: false, fixed: false },
  ]);
  render(<LoupistSettings catalog={null} onChanged={vi.fn()} onBack={vi.fn()} onManageProviders={vi.fn()} bridge={bridge} selectedAgent={selectedAgent} mutationBusy={false} mutationError={null} onOpenMarketplace={vi.fn()} onDelete={vi.fn()} onAgentUpdated={vi.fn()} />);
  const user = userEvent.setup();
  await user.click(document.querySelector('[data-loupist-section="capabilities"] > summary')!);
  expect(await screen.findByRole("switch", { name: "Filing comparison: enabled" })).toBeChecked();
  await user.type(screen.getByRole("searchbox", { name: "Search skills and connections" }), "cash flows");
  expect(screen.queryByRole("switch", { name: "Filing comparison: enabled" })).toBeNull();
  expect(screen.getByRole("switch", { name: "Valuation: disabled" })).not.toBeChecked();
  await user.click(document.querySelector('[data-loupist-section="workspace"] > summary')!);
  await user.click(screen.getByRole("button", { name: "Show in file manager" }));
  await waitFor(() => expect(reveal).toHaveBeenCalledExactlyOnceWith({ loupist_id: selectedAgent.id }));
  expect(screen.getByLabelText("Workspace folder")).toHaveTextContent(selectedAgent.workspace_path);
});

it("lists Skill Creator last under Advanced, as Settings › Skills does", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const agents = await bridge.loupistList();
  render(<LoupistSettings catalog={null} onChanged={vi.fn()} onBack={vi.fn()} onManageProviders={vi.fn()} bridge={bridge} selectedAgent={agents[0]} mutationBusy={false} mutationError={null} onOpenMarketplace={vi.fn()} onDelete={vi.fn()} onAgentUpdated={vi.fn()} />);
  const user = userEvent.setup();
  await user.click(document.querySelector('[data-loupist-section="capabilities"] > summary')!);
  const creator = await screen.findByRole("switch", { name: /^Skill Creator:/ });
  const list = creator.closest(".agent-capability-list")!;
  const switches = within(list as HTMLElement).getAllByRole("switch");
  expect(switches.at(-1)).toBe(creator);
  const advanced = within(list as HTMLElement).getByRole("heading", { level: 3, name: "Advanced" });
  expect(advanced.compareDocumentPosition(creator) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(advanced.compareDocumentPosition(switches[0]) & Node.DOCUMENT_POSITION_PRECEDING).toBeTruthy();
});

it("filters capabilities by availability and opens the selected connection’s details", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const agents = await bridge.loupistList();
  const openConnection = vi.fn();
  render(<LoupistSettings catalog={null} onChanged={vi.fn()} onBack={vi.fn()} onManageProviders={vi.fn()} bridge={bridge} selectedAgent={agents[0]} mutationBusy={false} mutationError={null} onOpenMarketplace={openConnection} onDelete={vi.fn()} onAgentUpdated={vi.fn()} />);
  const user = userEvent.setup();
  await user.click(document.querySelector('[data-loupist-section="capabilities"] > summary')!);
  await screen.findByRole("switch", { name: "Research: enabled" });
  await user.click(screen.getByRole("radio", { name: "Needs setup" }));
  expect(screen.queryByRole("switch", { name: "Research: enabled" })).toBeNull();
  expect(screen.queryByRole("switch", { name: "World Bank Indicators: enabled" })).toBeNull();
  expect(screen.getByRole("switch", { name: "Korea Investment Open Trading API: disabled" })).toBeDisabled();
  await user.click(screen.getByRole("button", { name: "Connection settings for Korea Investment Open Trading API" }));
  expect(openConnection).toHaveBeenCalledWith("koreainvestment.market-data");
  await user.click(screen.getByRole("radio", { name: "Enabled" }));
  expect(screen.getByRole("switch", { name: "Research: enabled" })).toBeChecked();
});

it("shows the unavailable workspace and blocks mutations until it is reconnected", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const agents = await bridge.loupistList();
  const selectedAgent = { ...agents[0], availability: { status: "workspace_unavailable" as const } };
  const reconnect = vi.fn();
  const skillCatalog = vi.spyOn(bridge, "agentSkillCatalog");
  render(<LoupistSettings catalog={null} onChanged={vi.fn()} onBack={vi.fn()} onManageProviders={vi.fn()} bridge={bridge} selectedAgent={selectedAgent} mutationBusy={false} mutationError={null} onReconnect={reconnect} onOpenMarketplace={vi.fn()} onDelete={vi.fn()} onAgentUpdated={vi.fn()} />);
  await userEvent.click(document.querySelector('[data-loupist-section="workspace"] > summary')!);
  expect(screen.getByLabelText("Workspace folder")).toHaveTextContent(selectedAgent.workspace_path);
  expect(screen.queryByRole("button", { name: "Rename" })).toBeNull();
  expect(screen.getByRole("button", { name: "Remove Loupist" })).toBeDisabled();
  expect(skillCatalog).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole("button", { name: "Reconnect folder" }));
  expect(reconnect).toHaveBeenCalledOnce();
});

it("keeps a failed tool toggle unchanged and lets the user retry", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const agents = await bridge.loupistList();
  const disable = vi.spyOn(bridge, "loupistCapabilityDisable").mockRejectedValueOnce(new Error("Workspace is temporarily busy."));
  render(<LoupistSettings catalog={null} onChanged={vi.fn()} onBack={vi.fn()} onManageProviders={vi.fn()} bridge={bridge} selectedAgent={agents[0]} mutationBusy={false} mutationError={null} onOpenMarketplace={vi.fn()} onDelete={vi.fn()} onAgentUpdated={vi.fn()} />);
  const user = userEvent.setup();
  await user.click(document.querySelector('[data-loupist-section="capabilities"] > summary')!);
  await user.click(await screen.findByRole("switch", { name: "World Bank Indicators: enabled" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Workspace is temporarily busy.");
  expect(screen.getByRole("switch", { name: "World Bank Indicators: enabled" })).toBeChecked();
  await user.click(screen.getByRole("switch", { name: "World Bank Indicators: enabled" }));
  expect(await screen.findByRole("switch", { name: "World Bank Indicators: disabled" })).not.toBeChecked();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(disable).toHaveBeenCalledTimes(2);
  expect(disable).toHaveBeenLastCalledWith({ loupist_id: agents[0].id, entry_id: "world-bank.indicators" });
});

it("saves a profile, publishes its new identity, and discards later edits", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList();
  const update = vi.spyOn(bridge, "loupistProfileUpdate");
  const onUpdated = vi.fn();
  render(<LoupistSettings catalog={null} onChanged={vi.fn()} onBack={vi.fn()} onManageProviders={vi.fn()} bridge={bridge} selectedAgent={loupist} mutationBusy={false} mutationError={null} onOpenMarketplace={vi.fn()} onDelete={vi.fn()} onAgentUpdated={onUpdated} />);
  const user = userEvent.setup();
  const save = screen.getByRole("button", { name: "Save" });
  expect(save).toBeDisabled();
  const avatars = screen.getByRole("group", { name: "Avatar" });
  expect(within(avatars).getAllByRole("button")).toHaveLength(10);
  expect(within(avatars).getAllByRole("button", { pressed: true })).toHaveLength(1);
  await user.clear(screen.getByLabelText("Name", { exact: true }));
  await user.type(screen.getByLabelText("Name", { exact: true }), "  My research  ");
  await user.clear(screen.getByLabelText("Focus", { exact: true }));
  await user.type(screen.getByLabelText("Focus", { exact: true }), "Energy transitions");
  await user.click(screen.getByRole("button", { name: "Hourglass" }));
  await user.click(save);
  await waitFor(() => expect(onUpdated).toHaveBeenCalledWith(expect.objectContaining({ name: "My research", philosophy: "Energy transitions", avatar: "loupist-08" })));
  expect(update).toHaveBeenCalledOnce();
  expect(await screen.findByText("Saved", { exact: true })).toBeVisible();
  expect(save).toBeDisabled();
  expect((await bridge.loupistList()).find(item => item.id === loupist.id)?.name).toBe("My research");
  await waitFor(() => expect(screen.queryByText("Saved", { exact: true })).toBeNull(), { timeout: 3000 });
  await user.type(screen.getByLabelText("Name", { exact: true }), " draft");
  await user.click(screen.getByRole("button", { name: "Discard" }));
  expect(screen.getByLabelText("Name", { exact: true })).toHaveValue("My research");
  expect(save).toBeDisabled();
});

it("retains a failed profile draft and retries it", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList();
  const update = vi.spyOn(bridge, "loupistProfileUpdate").mockRejectedValueOnce(new Error("Workspace is busy."));
  render(<LoupistSettings catalog={null} onChanged={vi.fn()} onBack={vi.fn()} onManageProviders={vi.fn()} bridge={bridge} selectedAgent={loupist} mutationBusy={false} mutationError={null} onOpenMarketplace={vi.fn()} onDelete={vi.fn()} onAgentUpdated={vi.fn()} />);
  const user = userEvent.setup();
  await user.type(screen.getByLabelText("Name", { exact: true }), " edited");
  await user.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Workspace is busy.");
  expect(screen.getByLabelText("Name", { exact: true })).toHaveValue(`${loupist.name} edited`);
  await user.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByText("Saved", { exact: true })).toBeVisible();
  expect(update).toHaveBeenCalledTimes(2);
  expect(screen.queryByRole("alert")).toBeNull();
});
