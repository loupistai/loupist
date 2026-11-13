import { useState } from "react";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ConfigScope, LoupistSummary, ModelCatalog } from "../../types";
import { AgentSettings } from "./AgentSettings";

async function setup() {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const current = await bridge.configRead({ scope: { kind: "global" } });
  await bridge.configUpdate({ scope: current.scope, expectedRevision: current.revision, set: { pi: { retry: { max_retries: 5 } } }, unset: [] });
  const loupists = await bridge.loupistList();
  const catalog = await bridge.modelCatalogGet();
  return { bridge, loupists, catalog };
}

function Settings({ bridge, loupists, catalog }: { bridge: MockLoupistBridge; loupists: LoupistSummary[]; catalog: ModelCatalog }) {
  const [scope, setScope] = useState<ConfigScope>({ kind: "global" });
  return <AgentSettings bridge={bridge} loupists={loupists} catalog={catalog} scope={scope} onScopeChange={setScope} />;
}

describe("Agent configuration", () => {
  it("uses scoped Memory defaults for new Chats and preserves existing Chat choices", async () => {
    const user = userEvent.setup();
    const fixture = await setup();
    const existing = (await fixture.bridge.loupistSelect("loupist-quality")).threads[0];
    render(<Settings {...fixture} />);
    const privacy = await screen.findByRole("switch", { name: "Remember new conversations" });
    expect(privacy).toBeChecked();
    await user.click(privacy);
    await waitFor(async () => expect((await fixture.bridge.configRead({ scope: { kind: "global" } })).effective.chat.update_memory).toBe(false));
    const thread = await fixture.bridge.chatCreate({ loupist_id: "loupist-quality" });
    expect(thread).toMatchObject({ use_memory: false, update_memory: false });
    expect((await fixture.bridge.loupistSelect("loupist-quality")).threads.find((item) => item.id === existing.id)).toMatchObject({ use_memory: existing.use_memory, update_memory: existing.update_memory });
    await user.selectOptions(screen.getByLabelText("Settings apply to"), "loupist-quality");
    await waitFor(() => expect(screen.getByRole("switch", { name: "Remember new conversations" })).toBeEnabled());
    expect(screen.getByRole("switch", { name: "Remember new conversations" })).not.toBeChecked();
    await user.click(screen.getByRole("switch", { name: "Remember new conversations" }));
    await waitFor(async () => expect((await fixture.bridge.configRead({ scope: { kind: "project", loupistId: "loupist-quality" } })).effective.chat.update_memory).toBe(true));
    expect(await fixture.bridge.chatCreate({ loupist_id: "loupist-quality" })).toMatchObject({ use_memory: true, update_memory: true });
    await user.click(screen.getByRole("button", { name: "Reset Remember new conversations" }));
    await waitFor(() => expect(screen.getByRole("switch", { name: "Remember new conversations" })).not.toBeChecked());
    const reset = await fixture.bridge.configRead({ scope: { kind: "project", loupistId: "loupist-quality" } });
    expect(reset.explicit?.chat?.use_memory).toBeUndefined();
    expect(reset.explicit?.chat?.update_memory).toBeUndefined();
    expect(reset.effective.chat).toMatchObject({ use_memory: false, update_memory: false });
  });

  it("shows the chosen conversation context preset and saves the other one", async () => {
    const user = userEvent.setup();
    const fixture = await setup();
    const current = await fixture.bridge.configRead({ scope: { kind: "global" } });
    await fixture.bridge.configUpdate({ scope: current.scope, expectedRevision: current.revision, set: { pi: { compaction: { reserve_tokens: 16384, keep_recent_tokens: 20000 } } }, unset: [] });
    render(<Settings {...fixture} />);
    await user.click(await screen.findByText("Advanced"));
    const presets = screen.getByRole("radiogroup", { name: "Conversation context" });
    expect(within(presets).getByRole("radio", { name: "Standard" })).toBeChecked();
    expect(within(presets).getByRole("radio", { name: "Keep more" })).not.toBeChecked();
    await user.click(within(presets).getByRole("radio", { name: "Keep more" }));
    await waitFor(async () => expect((await fixture.bridge.configRead({ scope: { kind: "global" } })).effective.pi.compaction.keep_recent_tokens).toBe(40000));
    await waitFor(() => expect(within(presets).getByRole("radio", { name: "Keep more" })).toBeChecked());
  });

  it("starts with Chat defaults and confirms a saved selection while advanced controls stay collapsed", async () => {
    const user = userEvent.setup();
    const fixture = await setup();
    render(<Settings {...fixture} />);
    const model = await screen.findByLabelText("Default model");
    await user.selectOptions(model, "model-test");
    expect(await screen.findByText("Saved · Applies to new responses")).toBeInTheDocument();
    expect((await fixture.bridge.configRead({ scope: { kind: "global" } })).explicit?.chat?.model_profile_id).toBe("model-test");
    await user.selectOptions(screen.getByLabelText("Settings apply to"), "loupist-quality");
    await waitFor(() => expect(screen.getByLabelText("Default model")).toHaveValue("model-test"));
    expect(screen.queryByRole("button", { name: "Reset Default model" })).not.toBeInTheDocument();
    expect(screen.queryByText("Saved · Applies to new responses")).not.toBeInTheDocument();
  });

  it("inherits global defaults, saves a project override and removes it with Use inherited", async () => {
    const user = userEvent.setup();
    const fixture = await setup();
    render(<Settings {...fixture} />);
    await user.click(await screen.findByText("Advanced"));
    await screen.findByDisplayValue("5");
    await user.selectOptions(screen.getByLabelText("Settings apply to"), "loupist-quality");
    const field = screen.getByLabelText("Maximum retries");
    expect(field).toHaveValue(5);
    expect(screen.queryByRole("button", { name: "Reset Maximum retries" })).not.toBeInTheDocument();
    await user.clear(field);
    await user.type(field, "2");
    await user.click(screen.getByRole("button", { name: "Save Maximum retries" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Reset Maximum retries" })).toBeEnabled());
    expect((await fixture.bridge.configRead({ scope: { kind: "project", loupistId: "loupist-quality" } })).explicit?.pi?.retry?.max_retries).toBe(2);
    await user.selectOptions(screen.getByLabelText("Settings apply to"), "loupist-cycle");
    expect(screen.getByLabelText("Maximum retries")).toHaveValue(5);
    await user.selectOptions(screen.getByLabelText("Settings apply to"), "loupist-quality");
    await screen.findByDisplayValue("2");
    await user.click(screen.getByRole("button", { name: "Reset Maximum retries" }));
    await waitFor(() => expect(screen.getByLabelText("Maximum retries")).toHaveValue(5));
    expect((await fixture.bridge.configRead({ scope: { kind: "project", loupistId: "loupist-quality" } })).explicit?.pi?.retry?.max_retries).toBeUndefined();
    expect((await fixture.bridge.configRead({ scope: { kind: "global" } })).explicit?.pi?.retry?.max_retries).toBe(5);
  });

  it("keeps an external edit on conflict and recovers through Reload", async () => {
    const user = userEvent.setup();
    const fixture = await setup();
    render(<Settings {...fixture} />);
    await user.click(await screen.findByText("Advanced"));
    const field = await screen.findByLabelText("Maximum retries");
    await user.clear(field);
    await user.type(field, "2");
    const current = await fixture.bridge.configRead({ scope: { kind: "global" } });
    await fixture.bridge.configUpdate({ scope: current.scope, expectedRevision: current.revision, set: { pi: { retry: { max_retries: 8 } } }, unset: [] });
    await user.click(screen.getByRole("button", { name: "Save Maximum retries" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("changed on disk");
    expect((await fixture.bridge.configRead({ scope: current.scope })).effective.pi.retry.max_retries).toBe(8);
    await user.click(screen.getByRole("button", { name: "Reload" }));
    await waitFor(() => expect(screen.getByLabelText("Maximum retries")).toHaveValue(8));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("shows file diagnostics and disables edits without overwriting invalid configuration", async () => {
    const user = userEvent.setup();
    const fixture = await setup();
    const snapshot = await fixture.bridge.configRead({ scope: { kind: "global" } });
    vi.spyOn(fixture.bridge, "configRead").mockResolvedValue({ ...snapshot, explicit: null, diagnostics: [{ path: snapshot.path, line: 3, column: 9, message: "Expected an integer" }] });
    const update = vi.spyOn(fixture.bridge, "configUpdate");
    const reveal = vi.spyOn(fixture.bridge, "configReveal");
    render(<Settings {...fixture} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Problem in your preferences file (line 3) — Expected an integer");
    expect(screen.getByLabelText("Maximum retries")).not.toBeVisible();
    await user.click(screen.getByText("Advanced"));
    expect(screen.getByLabelText("Maximum retries")).toBeDisabled();
    await user.click(screen.getByText("Preferences file"));
    await user.click(screen.getByRole("button", { name: "Open preferences file" }));
    expect(reveal).toHaveBeenCalledWith({ scope: { kind: "global" } });
    expect(update).not.toHaveBeenCalled();
  });
});
