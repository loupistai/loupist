/** Preview scenarios, in the capture order `src/preview/scenarios.ts` assembles. */
import { prepareFixture } from "../fixtures";
import i18n from "../../i18n";
import type { ProviderConnectFailure } from "../../contracts/model";
import { signInTiming } from "../../components/settings/signIn";
import { base, click, hasLayout, openNavigation, select, settings, until, type PreviewScenario } from "./kit";

/** First launch, sign-in and the new Loupist dialogs. */
export const onboardingScenarios: PreviewScenario[] = [
  ...["welcome", "notice"].map((state): PreviewScenario => ({
    ...base(`onboarding-${state}`, `Onboarding ${state}`, () => select('[data-onboarding="1"] h1'), async () => {
      if (state === "notice" && hasLayout()) select("[data-onboarding-notice]")?.scrollIntoView({ block: "center" });
    }),
    setup: bridge => prepareFixture(bridge, "provider"),
  })),
  // Step 3 with an account already connected: templates, Name and Create, then the spending line.
  { ...base("onboarding-create", "Onboarding: create the first Loupist", () => select('[data-onboarding="3"] [data-onboarding-create]') && select('[data-onboarding="3"] [data-background-reading]'), async root => {
    await click("[data-notice-acknowledgment]", root);
    await click("[data-onboarding-start]", root);
  }), setup: bridge => prepareFixture(bridge, "loupist") },
  { ...base("onboarding-bot", "First launch opens Home after the notice", () => document.getElementById("current-location")?.textContent === i18n.t("app:thread.title") ? select(".chat-layout") : null, async root => {
    await click("[data-notice-acknowledgment]", root);
    await click("[data-onboarding-start]", root);
    await click("[data-onboarding-create]", root);
    await until(() => select("textarea[data-chat-composer]"), "Bot did not initialize");
  }), setup: async bridge => {
    await prepareFixture(bridge, "loupist");
    bridge.scheduleTasks = "none";
    bridge.inboxList = async () => ({ items: [], nextCursor: null });
    bridge.inboxUnreadCounts = async () => [];
  } },
  // W17.1/W17.4: onboarding step 2. ChatGPT sign-in and its waiting, paste and device cards, error rows, and the connect sheet.
  ...(() => {
    const states = ["connect", "connect-waiting", "connect-paste", "connect-device", "connect-declined", "connect-not-eligible", "connect-port-busy", "connect-key", "connect-openai-replaces", "connect-local", "connect-local-none", "connect-custom-test", "connect-custom-error"] as const;
    type ConnectState = (typeof states)[number];
    const failures: Partial<Record<ConnectState, ProviderConnectFailure>> = { "connect-declined": "plan_permission_declined", "connect-not-eligible": "plan_not_eligible", "connect-port-busy": "port_in_use" };
    const sheet = (kind: string, selector: string) => () => select(`[data-connect-sheet="${kind}"] ${selector}`);
    const landmarks: Record<ConnectState, () => Element | null> = {
      connect: () => select('[data-onboarding="2"] .onboarding-other[open] [data-connect-row="custom"]'),
      "connect-waiting": () => select('[data-onboarding="2"] [data-signin="waiting"] [data-paste-toggle]'),
      "connect-paste": () => select('[data-onboarding="2"] [data-signin="waiting"] .signin-paste input'),
      "connect-device": () => select('[data-onboarding="2"] [data-signin="device"] .signin-device-code'),
      "connect-declined": () => select('[data-onboarding="2"] .signin-error[data-reason="plan_permission_declined"]'),
      "connect-not-eligible": () => select('[data-onboarding="2"] .signin-error[data-reason="plan_not_eligible"]'),
      "connect-port-busy": () => select('[data-onboarding="2"] .signin-error[data-reason="port_in_use"]'),
      "connect-key": sheet("api_key", '[data-key-sheet="google"] input'),
      "connect-openai-replaces": sheet("api_key", "[data-replaces-chatgpt]"),
      "connect-local": () => select('[data-connect-sheet="local"] [data-engine="ollama"] input[type="radio"]:checked') && select('[data-connect-sheet="local"] [data-engine="ollama"] input[type="radio"]:disabled'),
      "connect-local-none": sheet("local", "[data-local-none]"),
      "connect-custom-test": () => select('[data-connect-sheet="custom"] .connect-test-result[data-outcome="ok"]') && select('[data-connect-sheet="custom"] .connect-test-models input:checked'),
      "connect-custom-error": sheet("custom", '.connect-test-result[data-outcome="not_found"]'),
    };
    const typeInto = (input: HTMLInputElement, value: string) => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    };
    const openOtherWays = async () => {
      const disclosure = await until(() => select('[data-onboarding="2"] details.onboarding-other'), "Other ways did not appear") as HTMLDetailsElement;
      if (!disclosure.open) (disclosure.querySelector("summary") as HTMLElement).click();
      await until(() => select('[data-onboarding="2"] [data-connect-row="custom"]'), "Other ways did not open");
    };
    return states.map((state): PreviewScenario => ({
      ...base(`onboarding-${state}`, `Onboarding ${state}`, landmarks[state], async root => {
        await click("[data-notice-acknowledgment]", root);
        await click("[data-onboarding-start]", root);
        if (state === "connect-waiting" || state === "connect-paste" || failures[state]) {
          await click("[data-onboarding-connect]", root);
          if (state === "connect-paste") await click("[data-paste-toggle]");
          return;
        }
        if (state === "connect-device") { await click('[data-onboarding-connect="xai"]', root); return; }
        await openOtherWays();
        if (state === "connect") return;
        const row = state === "connect-key" ? "key-google" : state === "connect-openai-replaces" ? "key-openai" : state.startsWith("connect-local") ? "local" : "custom";
        await click(`[data-connect-row="${row}"]`);
        if (state === "connect-custom-test" || state === "connect-custom-error") {
          const form = await until(() => select("[data-custom-sheet]"), "Custom endpoint sheet did not open");
          typeInto(form.querySelector<HTMLInputElement>('input:not([type="radio"]):not([type="url"])')!, "Office gateway");
          typeInto(form.querySelector<HTMLInputElement>('input[type="url"]')!, "https://gateway.example/v1");
          await click("[data-custom-test]");
        }
      }),
      setup: async bridge => {
        await prepareFixture(bridge, "provider");
        // The paste route normally appears 20 s after the browser opens.
        signInTiming.pasteRevealMs = state === "connect-waiting" || state === "connect-paste" ? 0 : 20_000;
        const failure = failures[state];
        if (state === "connect-waiting" || state === "connect-paste") {
          bridge.connectScript = async (_provider, observer) => {
            observer({ type: "opening_browser", message: "", opened: true, url: null });
            observer({ type: "manual_entry" });
            await new Promise(() => {});
          };
        } else if (state === "connect-device") {
          bridge.connectScript = async (_provider, observer) => {
            observer({ type: "device_code", user_code: "WDJB-MJHT", verification_uri: "https://accounts.x.ai/oauth2/device", expires_in_s: 900 });
            await new Promise(() => {});
          };
        } else if (failure) {
          bridge.connectScript = async (_provider, observer) => {
            observer({ type: "failed", reason: failure });
            throw { code: "provider_connect_failed", message: failure };
          };
        }
        // A ChatGPT plan stays signed in on this Mac while an OpenAI key is being added.
        if (state === "connect-openai-replaces") bridge.setProviderStatus("openai", { credential_kind: "plan", state: "connected" });
        if (state === "connect-local-none") bridge.localDiscovery = { engines: bridge.localDiscovery.engines.map(engine => ({ ...engine, running: false, models: [] })) };
        if (state === "connect-custom-error") bridge.customTestResult = { outcome: "not_found", models: [] };
      },
    }));
  })(),
  base("new-loupist-dialog", "New Loupist templates", () => select(".loupist-template-fields"), async () => {
    await until(() => select('textarea[data-chat-composer]'), "Chat did not initialize");
    await openNavigation(".loupist-header-new");
    await click(".loupist-header-new");
  }),
  { ...base("loupist-import-dialog", "Open a Loupist folder", () => {
    const dialog = select("[data-loupist-import-dialog]");
    return dialog?.querySelector(".workspace-path") && dialog.querySelector('input[type="checkbox"]:not(:checked)') ? dialog : null;
  }, async root => {
    await settings(root, "loupists");
    await click("[data-loupist-import]", root);
    await click("[data-loupist-import-dialog] .loupist-folder-heading button");
  }), setup: async bridge => {
    await prepareFixture(bridge);
    bridge.loupistChooseFolder = async () => "/Users/research/Backups/Quality Compounder";
  } },
];
