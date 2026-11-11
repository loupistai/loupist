/** Preview scenarios, in the capture order `src/preview/scenarios.ts` assembles. */
import { prepareFixture, previewKo } from "../fixtures";
import { base, centerOn, click, openLoupistSettings, prepareAttention, select, settings, until, type PreviewScenario } from "./kit";

/** Models and providers, the notice, the command palette and Settings pages. */
export const settingsPageScenarios: PreviewScenario[] = [
  // W17.3 (providers §2.3): one card per account; ChatGPT plan line, key accounts.
  ...([
    ["settings-models", "Models & providers", '[data-provider-card="anthropic"]'],
    ["settings-models-plan-limit", "A ChatGPT plan at its limit", '[data-provider-card="openai"] [data-notice="plan_limit"]'],
    ["settings-models-needs-reconnect", "A sign-in that needs reconnecting", '[data-provider-card="openai"][data-state="needs_reconnect"]'],
    ["settings-models-disconnected", "A disconnected account schedules still use", '[data-provider-card="xai"][data-state="disconnected"]'],
    ["settings-models-expanded", "A card's models with In menu switches", '[data-provider-card="openai"] .provider-card-models .provider-card-switch'],
  ] as const).map(([id, title, landmark]): PreviewScenario => ({
    ...base(id, title, () => select(landmark), async (root) => {
      await settings(root, "model");
      if (id === "settings-models-expanded") await click('[data-provider-card="openai"] [aria-controls="provider-models-openai"]');
      if (id === "settings-models-disconnected") await centerOn(landmark)();
    }),
    setup: async (bridge) => {
      await prepareFixture(bridge);
      await bridge.providerConfigure({ provider: "anthropic", api_key: "preview-key" });
      await bridge.providerConfigure({ provider: "google", api_key: "preview-key" });
      const hour = 60 * 60 * 1000;
      if (id === "settings-models-plan-limit") bridge.setProviderStatus("openai", { notice: { kind: "plan_limit", at_ms: Date.now() - hour }, last_limit_at_ms: Date.now() - hour });
      if (id === "settings-models-needs-reconnect") bridge.setProviderStatus("openai", { state: "needs_reconnect" });
      if (id === "settings-models-disconnected") bridge.setProviderStatus("xai", { state: "disconnected", credential_kind: "none", connected_at_ms: null, used_by: { schedules: 2, chat_default: false, world_models: 0 } });
    },
  })),
  { ...base("settings-models-custom", "Custom model provider", () => select('[data-provider-card^="custom-"]'), async (root) => { await settings(root, "model"); }), setup: async bridge => { await prepareFixture(bridge); await bridge.providerCustomSave({ name: "Office Ollama", base_url: "http://localhost:11434/v1", api: "openai-completions", model_ids: ["qwen2.5:7b", "llama3.1:8b"] }); } },
  base("custom-provider-form", "Add a custom provider", () => select("[data-custom-sheet]"), async (root) => {
    await settings(root, "model");
    await click("[data-provider-connect]");
    await click('[data-connect-row="custom"]');
    const form = await until(() => select("[data-custom-sheet]"), "Custom provider sheet did not open");
    const fields = [...form.querySelectorAll<HTMLInputElement>("input:not([type=password]):not([type=radio])")];
    for (const [field, value] of fields.map((field, index) => [field, ["Office Ollama", "http://localhost:11434/v1"][index]] as const)) {
      if (value === undefined) continue;
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(field, value);
      field.dispatchEvent(new Event("input", { bubbles: true }));
    }
  }),
  { ...base("existing-user-notice", "Existing user notice", () => select("[data-notice-dialog]")), setup: async bridge => { await prepareFixture(bridge); bridge.noticeSettingsGet = async () => ({ notice_acknowledged_version: 0 }); } },
  base("settings-application", "Application notice", () => select("[data-notice-settings]"), root => settings(root, "agent")),
  // W15.37: the ⌘K command palette and Settings › Keyboard.
  { ...base("palette-open", "The command palette searches what needs attention, chats, names and actions", () => select('.command-palette-list [data-palette-row^="attention:"]') && select(".command-palette-list"), async (root) => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true, ctrlKey: !navigator.userAgent.includes("Mac"), bubbles: true }));
    const input = await until(() => select(".command-palette-input") as HTMLInputElement | null, "Palette did not open");
    // One letter reaches every group in the fixture: needs attention, chats, Map names, principles and actions.
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "a");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  }), setup: async (bridge) => { await prepareFixture(bridge, "answer"); prepareAttention(bridge); } },
  // B82: the palette searches every Loupist; each group lists the active Loupist's rows first, each captioned.
  { ...base("palette-loupists", "The command palette finds names and principles in every Loupist", () => select('.command-palette-list [data-palette-row^="principle:"]') && select('.command-palette-list [role="group"] [data-palette-row^="map:"]') && select(".command-palette-list"), async (root) => {
    await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true, ctrlKey: !navigator.userAgent.includes("Mac"), bubbles: true }));
    const input = await until(() => select(".command-palette-input") as HTMLInputElement | null, "Palette did not open");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, previewKo() ? "삼성" : "samsung");
    input.dispatchEvent(new Event("input", { bubbles: true }));
    // Both Loupists' Maps answer before the capture.
    await until(() => document.querySelectorAll('.command-palette-list [data-palette-row^="map:"]').length >= 2, "The other Loupist's names did not arrive");
  }), setup: async (bridge) => {
    await prepareFixture(bridge, "answer");
    const ko = previewKo();
    const cycle = await bridge.loupistCreate({ name: ko ? "사이클 리더" : "Cycle Reader", template: "semiconductors" });
    await bridge.principleCreate({ loupistId: cycle.id, statement: ko ? "삼성전자의 메모리 사이클은 마진보다 먼저 돈다" : "Samsung's memory cycle turns before its margins do", origin: "settings" });
    // A new Loupist's mock Map is empty; it shares the seeded names so both Maps answer.
    const read = bridge.knowledgeMap.bind(bridge);
    bridge.knowledgeMap = async (request) => read(request.loupistId === cycle.id ? { ...request, loupistId: "loupist-quality" } : request);
  } },
  base("settings-keyboard", "Keyboard shortcuts", () => select(".keyboard-table"), (root) => settings(root, "keyboard")),
  base("settings-world", "World processing", () => select(".world-settings-card"), async () => { await openLoupistSettings(); await click('[data-loupist-section="world"] > summary'); select(".world-settings-card")?.scrollIntoView({ block: "start" }); }),
];
