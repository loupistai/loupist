import i18n from "../i18n";
import { setPreviewLanguage } from "../i18n/useLanguage";
import { act, cleanup, render, waitFor } from "@testing-library/react";
import { App } from "../App";
import { enableSharingPreviewHarness } from "../sharing/releaseVisibility";

enableSharingPreviewHarness();
import { BROWSER_ARTICLE_URL, BROWSER_RAW_ENGINE_MESSAGE } from "./browserScenarios";
import { configureTheme, PreviewBridge } from "./fixtures";
import { scenarios } from "./scenarios";
import { selectPanel } from "./scenarios/kit";

// The offline scenario shadows `navigator.onLine`; drop it so later scenarios run online.
afterEach(() => { cleanup(); localStorage.clear(); setPreviewLanguage("en"); Reflect.deleteProperty(window.navigator, "onLine"); });

describe("design preview scenarios", () => {
  for (const scenario of scenarios) {
    it(`${scenario.id} reaches its visible landmark`, async () => {
      const bridge = new PreviewBridge({ delay_ms: 0 });
      await scenario.setup(bridge);
      await configureTheme(bridge, "light");
      const view = render(scenario.render ? scenario.render() : <App bridge={bridge} />);
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
      if (scenario.id === "answer-reading-column") {
        const answer = document.querySelector('article.message.assistant[data-message-id="preview-answer"]');
        for (const selector of ["h2", ".message-content p", ".message-content ul", ".chat-artifact-block", ".message-sources", ".memory-row", ".answer-feedback", ".message-action-bar > .message-actions"]) {
          expect(answer?.querySelector(selector)?.closest("article.message")).toBe(answer);
        }
        expect(answer?.querySelectorAll("table")).toHaveLength(2);
        expect(answer?.querySelector(".citation-keep")).toHaveTextContent("direction. 13");
      }
      if (scenario.id === "map-from-chat") {
        expect(document.querySelector("[data-map-around]")).toHaveAttribute("title", "Around TSMC");
        expect(document.querySelector('[data-scope="1"]')).toHaveAttribute("aria-pressed", "true");
        await selectPanel("lens");
        await selectPanel("world");
        await waitFor(() => expect(document.querySelector("[data-map-around]")).toHaveAttribute("title", "Whole World"));
        expect(document.querySelector('[data-scope="all"]')).toHaveAttribute("aria-pressed", "true");
        expect(document.querySelector("#map-inspector-title")).toHaveTextContent("TSMC");
      }
      if (scenario.id === "settings-application") {
        expect(document.querySelector("#settings-section-skills")).toBeNull();
      }
      if (scenario.id === "connections") {
        expect(document.querySelector("[data-custom-connections]")).toBeNull();
      }
      if (scenario.id === "composer-slash-retail-skills") {
        expect(document.querySelector('.composer-assist [role="option"][aria-label="Personal fund comparison"]')).toBeNull();
      }
      if (scenario.id === "chat-empty") expect(scenario.landmark()!.querySelectorAll("button").length).toBeGreaterThanOrEqual(3);
      // Six values one Source reported about NVIDIA are one row, and no row relates a name to itself.
      if (scenario.id === "map-whats-new-reported") {
        const labels = [...document.querySelectorAll(".map-whats-new-label")].map((label) => label.textContent);
        expect(labels.filter((label) => label?.includes("NVIDIA"))).toEqual(["6 new facts about NVIDIA", "NVIDIA"]);
      }
      // The greeting asks about the followed asset by name and says nothing it cannot back.
      if (scenario.id === "thread-morning") {
        expect(document.querySelector('.bot-greeting [data-ask="status"]')).toHaveTextContent("Hanul Precision");
        expect(document.querySelector(".morning-message")).toBeNull();
      }
      // A tile only for an asset with a stored price; the greeting's bubble is its prompt, never a price move.
      if (scenario.id === "bot-greeting") {
        expect(document.querySelectorAll(".bot-greeting .bot-ask")).toHaveLength(4);
        expect(document.querySelectorAll(".bot-greeting .bot-tile")).toHaveLength(3);
        expect(document.querySelectorAll(".bot-greeting .bot-bubble")).toHaveLength(1);
        expect(document.querySelector(".bot-greeting .bot-tiles")).not.toHaveTextContent(/close|%/iu);
      }
      // The unread briefing and the two checks.
      if (scenario.id === "thread-news") expect(document.querySelectorAll(".morning-bullets > li")).toHaveLength(3);
      if (scenario.id === "chat-streaming") await act(() => bridge.chatAbort("preview"));
      if (scenario.id === "browser-ready" || scenario.id === "browser-blocked" || scenario.id === "browser-failure") {
        expect(document.querySelector<HTMLInputElement>(".browser-toolbar input")).toHaveValue(BROWSER_ARTICLE_URL);
        expect(document.body).not.toHaveTextContent(BROWSER_RAW_ENGINE_MESSAGE);
      }
      // Three presses start one session; the opened link offers nothing more to press.
      if (scenario.id === "bot-link-opened") {
        expect(document.querySelector(".thread-link .inline-error")).toBeNull();
        expect(document.querySelectorAll(".thread-link .bot-quiet-action")).toHaveLength(1);
      }
      // The note stays closed and is never a second bubble; a session from before notes has none.
      if (scenario.id === "session-handoff-note") {
        expect(document.querySelectorAll(".message.user")).toHaveLength(0);
        expect(document.querySelector(".handoff-opening")).toBeVisible();
        expect(document.querySelector(".message-handoff-note")).not.toHaveAttribute("open");
      }
      if (scenario.id === "session-handoff-legacy") expect(document.querySelector(".message-handoff-note")).toBeNull();
      if (scenario.id === "browser-ready") {
        expect(document.querySelector(".browser-error")).toBeNull();
        expect(document.querySelector(".browser-history-actions button")).toBeDisabled();
        expect(document.querySelector(".browser-history-actions button:last-child")).toBeEnabled();
      }
      if (scenario.id === "browser-blocked") {
        expect(scenario.landmark()).toHaveTextContent(i18n.t("chat:useNativeBrowserTabs.onlyWebAddresses"));
        expect(document.querySelector(".browser-viewport")).toBeVisible();
        expect(document.querySelector(".browser-history-actions button")).toBeDisabled();
        expect(document.querySelector(".browser-history-actions button:last-child")).toBeEnabled();
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
