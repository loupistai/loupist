import { chooseSelectOption } from "@/test/selectOption";
import { useState } from "react";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import i18n from "@/i18n";
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
    await chooseSelectOption(user, screen.getByLabelText("Settings apply to"), "Quality Compounder");
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

  it("keeps showing a stored default that cannot run and says Chat uses the recommended model, in both languages", async () => {
    const fixture = await setup();
    await fixture.bridge.providerConfigure({ provider: "anthropic", api_key: "test-key" });
    const global = await fixture.bridge.configRead({ scope: { kind: "global" } });
    await fixture.bridge.configUpdate({ scope: global.scope, expectedRevision: global.revision, set: { chat: { model_profile_id: "model-test", thinking_level: "max" } }, unset: [] });
    await fixture.bridge.modelVisibilityUpdate({ model_profile_id: "model-test", visible_in_chat: false });
    const snapshot = await fixture.bridge.configRead({ scope: { kind: "global" } });
    // Chat runs Auto meanwhile: the selection is sendable and names another model.
    expect(snapshot.modelSelection).toMatchObject({ is_default: true, unavailable_reason: null, stored_unavailable: "model_hidden" });
    expect(snapshot.modelSelection?.model?.provider).toBe("anthropic");
    const caption = "Not available now. Chat uses the recommended model until it is.";
    const { unmount } = render(<Settings {...fixture} catalog={await fixture.bridge.modelCatalogGet()} />);
    const control = await screen.findByRole("button", { name: / · Default model$/ });
    expect(control).toHaveTextContent("GPT-5.6 Luna");
    expect(control).not.toHaveTextContent("Claude");
    expect(screen.getByRole("note")).toHaveTextContent(caption);
    await act(async () => { await i18n.changeLanguage("ko"); });
    try {
      expect(screen.getByRole("note")).toHaveTextContent("지금은 사용할 수 없습니다. 사용할 수 있을 때까지 채팅은 권장 모델을 씁니다.");
    } finally {
      await act(async () => { await i18n.changeLanguage("en"); });
    }
    unmount();
    // Shown again, the stored model applies and the caption goes.
    await fixture.bridge.modelVisibilityUpdate({ model_profile_id: "model-test", visible_in_chat: true });
    expect((await fixture.bridge.configRead({ scope: { kind: "global" } })).modelSelection).toMatchObject({ is_default: false, stored_unavailable: null, model: { id: "model-test" } });
    render(<Settings {...fixture} catalog={await fixture.bridge.modelCatalogGet()} />);
    expect(await screen.findByRole("button", { name: / · Default model$/ })).toHaveTextContent("GPT-5.6 Luna");
    expect(screen.queryByText(caption)).toBeNull();
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
    const catalog = await fixture.bridge.providerConfigure({ provider: "google", api_key: "fixture" });
    render(<Settings {...fixture} catalog={catalog} />);
    const model = await screen.findByRole("button", { name: /GPT-5.6 Luna.*Default model/ });
    await user.click(model);
    expect(screen.getByRole("menuitemradio", { name: "GPT-5.6 Luna Default" })).toBeChecked();
    await user.click(screen.getByRole("menuitemradio", { name: "google default" }));
    await user.keyboard("{Escape}");
    expect(await screen.findByText("Saved · Applies to new responses")).toBeInTheDocument();
    expect((await fixture.bridge.configRead({ scope: { kind: "global" } })).explicit?.chat?.model_profile_id).toBe("google/google-default");
    await chooseSelectOption(user, screen.getByLabelText("Settings apply to"), "Quality Compounder");
    await user.click(await screen.findByRole("button", { name: /google default.*Default model/ }));
    expect(screen.getByRole("menuitemradio", { name: "google default" })).toBeChecked();
    expect(screen.queryByText("Default")).toBeNull();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("button", { name: "Reset Default model" })).not.toBeInTheDocument();
    expect(screen.queryByText("Saved · Applies to new responses")).not.toBeInTheDocument();
  });

  it.each<ConfigScope>([{ kind: "global" }, { kind: "project", loupistId: "loupist-quality" }])(
    "saves a selected default model and its controls together in the $kind scope",
    async (scope) => {
      const user = userEvent.setup();
      const fixture = await setup();
      const catalog = await fixture.bridge.providerConfigure({ provider: "google", api_key: "fixture" });
      const current = await fixture.bridge.configRead({ scope });
      await fixture.bridge.configUpdate({
        scope, expectedRevision: current.revision,
        set: { chat: { model_profile_id: "google/google-default", run_options: { obsolete: "previous" } } },
        unset: [],
      });
      const update = vi.spyOn(fixture.bridge, "configUpdate");
      render(<AgentSettings {...fixture} catalog={catalog} scope={scope} />);
      await user.click(await screen.findByRole("button", { name: /google default.*Default model/ }));
      await user.click(screen.getByRole("menuitemradio", { name: "GPT-5.6 Luna" }));
      await user.keyboard("{Escape}");
      expect(await screen.findByText("Saved · Applies to new responses")).toBeInTheDocument();
      const chat = { model_profile_id: "model-test", run_options: { performance: "standard" } };
      expect(update).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
        scope, set: { chat }, unset: [["chat", "run_options", "obsolete"]],
      }));
      expect((await fixture.bridge.configRead({ scope })).explicit?.chat).toEqual(chat);
    },
  );

  it.each<ConfigScope>([{ kind: "global" }, { kind: "project", loupistId: "loupist-quality" }])(
    "reports a saved Chat model, reasoning level or run option in the $kind scope, and no other setting",
    async (scope) => {
      const user = userEvent.setup();
      const fixture = await setup();
      const saved = vi.fn();
      render(<AgentSettings {...fixture} scope={scope} onChatDefaultSaved={saved} />);
      await user.click(await screen.findByRole("switch", { name: "Remember new conversations" }));
      await waitFor(async () => expect((await fixture.bridge.configRead({ scope })).effective.chat.update_memory).toBe(false));
      expect(saved).not.toHaveBeenCalled();
      await waitFor(() => expect(screen.getByLabelText("Reasoning level")).toBeEnabled());
      await chooseSelectOption(user, screen.getByLabelText("Reasoning level"), "High");
      await waitFor(() => expect(saved).toHaveBeenCalledExactlyOnceWith(scope));
      await user.click(screen.getByText("Advanced"));
      await chooseSelectOption(user, screen.getByRole("combobox", { name: "Performance" }), "Fast");
      await waitFor(() => expect(saved).toHaveBeenCalledTimes(2));
      expect((await fixture.bridge.configRead({ scope })).explicit?.chat).toMatchObject({ thinking_level: "high", run_options: { performance: "fast" } });
    },
  );

  it("turns retrying failed requests off and on with a switch, like the other on/off setting", async () => {
    const user = userEvent.setup();
    const fixture = await setup();
    render(<Settings {...fixture} />);
    await user.click(await screen.findByText("Advanced"));
    const retry = await screen.findByRole("switch", { name: "Retry failed requests" });
    expect(retry).toBeChecked();
    await user.click(retry);
    await waitFor(async () => expect((await fixture.bridge.configRead({ scope: { kind: "global" } })).explicit?.pi?.retry?.enabled).toBe(false));
    await waitFor(() => expect(screen.getByRole("switch", { name: "Retry failed requests" })).not.toBeChecked());
    await user.click(screen.getByRole("switch", { name: "Retry failed requests" }));
    await waitFor(async () => expect((await fixture.bridge.configRead({ scope: { kind: "global" } })).effective.pi.retry.enabled).toBe(true));
  });

  it("inherits global defaults, saves a project override and removes it with Use inherited", async () => {
    const user = userEvent.setup();
    const fixture = await setup();
    render(<Settings {...fixture} />);
    await user.click(await screen.findByText("Advanced"));
    await screen.findByDisplayValue("5");
    await chooseSelectOption(user, screen.getByLabelText("Settings apply to"), "Quality Compounder");
    const field = screen.getByLabelText("Maximum retries");
    expect(field).toHaveValue(5);
    expect(screen.queryByRole("button", { name: "Reset Maximum retries" })).not.toBeInTheDocument();
    await user.clear(field);
    await user.type(field, "2");
    await user.click(screen.getByRole("button", { name: "Save Maximum retries" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Reset Maximum retries" })).toBeEnabled());
    expect((await fixture.bridge.configRead({ scope: { kind: "project", loupistId: "loupist-quality" } })).explicit?.pi?.retry?.max_retries).toBe(2);
    await chooseSelectOption(user, screen.getByLabelText("Settings apply to"), "Cycle Reader");
    expect(screen.getByLabelText("Maximum retries")).toHaveValue(5);
    await chooseSelectOption(user, screen.getByLabelText("Settings apply to"), "Quality Compounder");
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
