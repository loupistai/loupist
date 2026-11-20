/**
 * An answer's grid link (finance-korea-retail-disagreement.md §8). Fictional names; the
 * scenario reaches its screen through the app's own controls.
 */
import type { ChatMessage } from "../types";
import { prepareFixture, previewKo, type PreviewBridge } from "./fixtures";
import { previewGridLinkAnswer, previewGridLinkQuestion, SENSITIVITY_EVIDENCE } from "./korea";
import type { PreviewScenario } from "./scenarios";

type Helpers = {
  base: (id: string, title: string, landmark: PreviewScenario["landmark"], enter?: PreviewScenario["enter"]) => PreviewScenario;
  select: (selector: string, root?: ParentNode) => Element | null;
  until: <T>(read: () => T | null | false | undefined, reason: string, timeout?: number) => Promise<T>;
  hasLayout: () => boolean;
};

/** The home Thread holds one question and its answer. */
function installThread(bridge: PreviewBridge, messages: () => ChatMessage[]) {
  const select = bridge.loupistSelect.bind(bridge);
  bridge.loupistSelect = async loupistId => {
    const workspace = await select(loupistId);
    const home = workspace.threads.find(thread => thread.home) ?? workspace.threads[0];
    if (home) {
      home.messages = messages();
      home.title = previewKo() ? "솔숲식품 주가에 담긴 기대" : "What Solsup’s price implies";
    }
    return workspace;
  };
}

export function koreaScenarios({ base, select, until, hasLayout }: Helpers): PreviewScenario[] {
  /** The grid link's paragraph: a citation of its own, with the checked number after it. */
  const gridLink = `.message-content p .loupist-citation[data-evidence-id="${SENSITIVITY_EVIDENCE.grid.id}"]`;
  return [
    // An answer's link to its grid result stays the paragraph it was written as; the grid is behind PRICE IMPLIED.
    { ...base("chat-grid-link", "An answer links its reverse-DCF result in a paragraph of its own, between two paragraphs of prose", () => select(gridLink), async root => {
      await until(() => root.querySelector("article.message.assistant.complete"), "Answer did not load");
      await until(() => select(gridLink), "The grid link did not appear in the answer");
      if (hasLayout()) select(gridLink)?.scrollIntoView({ block: "center" });
    }), setup: async bridge => {
      await prepareFixture(bridge, "answer");
      installThread(bridge, () => [previewGridLinkQuestion(), previewGridLinkAnswer()]);
    } },
  ];
}
