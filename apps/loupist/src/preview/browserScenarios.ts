/**
 * In-app browser states. A plain click on an answer link opens the panel
 * through the app. The article is fictional; blocked and failure events use
 * the same typed host results the native browser sends.
 */
import { act } from "react";
import type { BrowserTabEvent } from "../contracts/browser";
import type { ChatMessage } from "../types";
import { prepareFixture, previewKo, type PreviewBridge } from "./fixtures";
import type { PreviewScenario } from "./scenarios";

/** Public article used by every browser preview. */
export const BROWSER_ARTICLE_URL = "https://issuer.example/hanul/q3";
/** Rust's internal command text. The panel must show a localized reason instead. */
export const BROWSER_RAW_ENGINE_MESSAGE = "only http and https links can be opened";

type Helpers = {
  base: (id: string, title: string, landmark: PreviewScenario["landmark"], enter?: PreviewScenario["enter"]) => PreviewScenario;
  select: (selector: string, root?: ParentNode) => Element | null;
  until: <T>(read: () => T | null | false | undefined, reason: string, timeout?: number) => Promise<T>;
  click: (selector: string, root?: ParentNode) => Promise<void>;
};

type BrowserHost = {
  blockLoadedArticle: () => void;
};

const ARTICLE_EVIDENCE_ID = `ev_${"c".repeat(32)}`;

const articleMessages = (): ChatMessage[] => {
  const ko = previewKo();
  const title = ko ? "한울정밀 실적 기사" : "Hanul Precision article";
  return [
    {
      id: "preview-browser-question",
      role: "user",
      status: "complete",
      created_at: "2026-10-28T00:00:00Z",
      content: ko ? "한울정밀 실적 기사를 열어 줘." : "Open the Hanul Precision results article.",
    },
    {
      id: "preview-browser-answer",
      role: "assistant",
      status: "complete",
      created_at: "2026-10-28T00:00:20Z",
      content: ko
        ? `실적 발표일은 [${title}](#evidence-${ARTICLE_EVIDENCE_ID})에 있어요.`
        : `The results date is in the [${title}](#evidence-${ARTICLE_EVIDENCE_ID}).`,
      evidence_refs: [{
        id: ARTICLE_EVIDENCE_ID,
        title,
        kind: "original",
        url: BROWSER_ARTICLE_URL,
        provider: "Hanul Precision IR",
        tool_name: "web_fetch",
        retrieved_at: "2026-10-28T00:00:00Z",
        published_at: "2026-10-27",
        excerpt: ko ? "실적 발표일은 11월 12일입니다." : "Results are released on 12 November.",
        excerpt_truncated: false,
        content_sha256: "c".repeat(64),
        warnings: [],
      }],
    },
  ];
};

function installArticle(bridge: PreviewBridge) {
  const selectLoupist = bridge.loupistSelect.bind(bridge);
  bridge.loupistSelect = async (loupistId) => {
    const workspace = await selectLoupist(loupistId);
    const home = workspace.threads.find((thread) => thread.home) ?? workspace.threads[0];
    if (home) {
      home.title = previewKo() ? "한울정밀 실적 기사" : "Hanul Precision results article";
      home.messages = articleMessages();
    }
    return workspace;
  };
}

function addressField() {
  return document.querySelector<HTMLInputElement>(".workspace-panel .browser-toolbar input");
}

function historyBack() {
  return document.querySelector<HTMLButtonElement>(".workspace-panel .browser-history-actions button");
}

function articleReady(select: Helpers["select"]) {
  const viewport = select(".workspace-panel .browser-viewport");
  const address = addressField();
  const back = historyBack();
  if (!viewport || select(".browser-error") || address?.value !== BROWSER_ARTICLE_URL || !back || back.disabled) return null;
  return viewport;
}

function articleKept(select: Helpers["select"], kind: "blocked" | "failure") {
  const alert = select(".workspace-panel .browser-error");
  const address = addressField();
  const back = historyBack();
  const retry = document.querySelector<HTMLButtonElement>(".workspace-panel .browser-recovery-actions button");
  const raw = alert?.textContent?.includes(BROWSER_RAW_ENGINE_MESSAGE) || address?.value.includes("about:blank");
  if (!alert || raw || address?.value !== BROWSER_ARTICLE_URL || !back || !retry || retry.disabled) return null;
  if (kind === "blocked") {
    return !back.disabled && select(".workspace-panel .browser-viewport") ? alert : null;
  }
  return back.disabled ? alert : null;
}

/** The latest open is the child the panel kept. A discarded open must not emit into a closed tab. */
function hostBrowser(bridge: PreviewBridge, mode: "ready" | "blocked" | "failure"): BrowserHost {
  const open = bridge.browserTabOpen.bind(bridge);
  let emit: ((event: BrowserTabEvent) => void) | undefined;
  let tabId = "";
  bridge.browserTabOpen = async (request, observer) => {
    if (mode === "failure") {
      throw Object.assign(new Error(BROWSER_RAW_ENGINE_MESSAGE), { code: "webkit_internal" });
    }
    const native = await open(request, observer);
    emit = observer;
    tabId = native.tab_id;
    return native;
  };
  return {
    blockLoadedArticle() {
      if (!emit || !tabId) throw new Error("Browser tab did not open");
      emit({ type: "navigation_blocked", tab_id: tabId, reason: "unsupported_protocol" });
      emit({ type: "load_started", tab_id: tabId, url: "about:blank" });
      emit({ type: "load_finished", tab_id: tabId, url: "about:blank" });
      emit({ type: "load_finished", tab_id: tabId, url: BROWSER_ARTICLE_URL });
    },
  };
}

async function openArticle(root: HTMLElement, until: Helpers["until"], click: Helpers["click"]) {
  await until(() => root.querySelector("article.message.assistant.complete"), "Answer did not load");
  await click("article.message.assistant.complete button.loupist-citation", root);
}

export function browserScenarios({ base, select, until, click }: Helpers): PreviewScenario[] {
  const prepare = (mode: "ready" | "blocked" | "failure") => {
    let host: BrowserHost;
    return {
      setup: async (bridge: PreviewBridge) => {
        await prepareFixture(bridge, "answer");
        installArticle(bridge);
        host = hostBrowser(bridge, mode);
      },
      enter: async (root: HTMLElement) => {
        await openArticle(root, until, click);
        if (mode === "failure") {
          await until(() => articleKept(select, "failure"), "Browser failure did not stay on the article");
          return;
        }
        await until(() => articleReady(select), "Browser did not finish opening the article");
        if (mode === "blocked") {
          // Flush the host event in tests. Waiting on the poll inside `act` never reaches its timer.
          if (import.meta.env.MODE === "test") await act(async () => { host.blockLoadedArticle(); });
          else host.blockLoadedArticle();
          await until(() => articleKept(select, "blocked"), "Blocked navigation did not stay on the article");
        }
      },
    };
  };
  const ready = prepare("ready");
  const blocked = prepare("blocked");
  const failure = prepare("failure");
  return [
    {
      ...base("browser-ready", "An answer link opens its article in the in-app browser", () => articleReady(select), ready.enter),
      setup: ready.setup,
    },
    {
      ...base("browser-blocked", "A blocked navigation keeps the article and explains the typed reason", () => articleKept(select, "blocked"), blocked.enter),
      setup: blocked.setup,
    },
    {
      ...base("browser-failure", "A browser failure stays on the requested article with a localized explanation", () => articleKept(select, "failure"), failure.enter),
      setup: failure.setup,
    },
  ];
}
