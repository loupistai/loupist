import i18n from "../i18n";
import { setPreviewLanguage } from "../i18n/useLanguage";
import { act, cleanup, render, waitFor } from "@testing-library/react";
import { App } from "../App";
import { BROWSER_ARTICLE_URL, BROWSER_RAW_ENGINE_MESSAGE } from "./browserScenarios";
import { configureTheme, PreviewBridge } from "./fixtures";
import { scenarios } from "./scenarios";

// The offline scenario shadows `navigator.onLine`; drop it so later scenarios run online.
afterEach(() => { cleanup(); localStorage.clear(); setPreviewLanguage("en"); Reflect.deleteProperty(window.navigator, "onLine"); });

describe("design preview scenarios", () => {
  for (const scenario of scenarios) {
    it(`${scenario.id} reaches its visible landmark`, async () => {
      const bridge = new PreviewBridge({ delay_ms: 0 });
      await scenario.setup(bridge);
      await configureTheme(bridge, "light");
      const view = render(<App bridge={bridge} />);
      // Let React flush between DOM actions while polling asynchronously.
      const entering = scenario.enter(view.container);
      await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
      await entering;
      await waitFor(() => expect(scenario.landmark()).toBeVisible(), { timeout: 8000 });
      if (scenario.id === "shell-sidebar-many") {
        const profiles = await bridge.loupistList();
        expect(profiles).toHaveLength(6);
        for (const profile of profiles) expect((await bridge.loupistSelect(profile.id)).loupist).toMatchObject({ id: profile.id, name: profile.name });
      }
      if (scenario.id === "chat-empty") expect(scenario.landmark()!.querySelectorAll("button").length).toBeGreaterThanOrEqual(3);
      if (scenario.id === "thread-morning") {
        expect(scenario.landmark()!.querySelectorAll(".morning-question")).toHaveLength(3);
        expect(document.querySelectorAll(".morning-bullets > li")).toHaveLength(4);
      }
      // The strip's check is not repeated as a bullet: the overdue check and the later one remain.
      if (scenario.id === "thread-upcoming") {
        expect(document.querySelectorAll(".morning-upcoming li")).toHaveLength(5);
        expect(document.querySelectorAll(".morning-bullets > li")).toHaveLength(4);
      }
      if (scenario.id === "chat-streaming") await act(() => bridge.chatAbort("preview"));
      if (scenario.id === "browser-ready" || scenario.id === "browser-blocked" || scenario.id === "browser-failure") {
        expect(document.querySelector<HTMLInputElement>(".browser-toolbar input")).toHaveValue(BROWSER_ARTICLE_URL);
        expect(document.body).not.toHaveTextContent(BROWSER_RAW_ENGINE_MESSAGE);
      }
      if (scenario.id === "browser-ready") {
        expect(document.querySelector(".browser-error")).toBeNull();
        expect(document.querySelector(".browser-history-actions button")).toBeEnabled();
      }
      if (scenario.id === "browser-blocked") {
        expect(scenario.landmark()).toHaveTextContent(i18n.t("chat:useNativeBrowserTabs.onlyWebAddresses"));
        expect(document.querySelector(".browser-viewport")).toBeVisible();
        expect(document.querySelector(".browser-history-actions button")).toBeEnabled();
      }
      if (scenario.id === "browser-failure") {
        expect(scenario.landmark()).toHaveTextContent(i18n.t("chat:useNativeBrowserTabs.couldNotOpenThisWebPage"));
        expect(document.querySelector(".browser-history-actions button")).toBeDisabled();
        expect(document.querySelector(".browser-recovery-actions button")).toBeEnabled();
      }
    });
  }
  it("resolves the dark theme through app configuration", async () => {
    const theme = "dark";
    const bridge = new PreviewBridge({ delay_ms: 0 });
    await scenarios[0].setup(bridge);
    await configureTheme(bridge, theme);
    render(<App bridge={bridge} />);
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe(theme));
  });
});
