import { prepareFixture } from "./fixtures";
import type { PreviewScenario } from "./scenarios";

type Helpers = {
  base: (id: string, title: string, landmark: PreviewScenario["landmark"], enter?: PreviewScenario["enter"]) => PreviewScenario;
  select: (selector: string, root?: ParentNode) => Element | null;
  click: (selector: string, root?: ParentNode) => Promise<void>;
  settings: (root: HTMLElement, section: string) => Promise<void>;
};

export function dataScenarios({ base, select, click, settings }: Helpers): PreviewScenario[] {
  return [
    {
      ...base("connection-krx-coverage", "KRX key approved for KOSPI, with KOSDAQ approval still needed", () => select(".marketplace-service-coverage"), async root => {
        await settings(root, "connections");
        await click('[aria-labelledby="connection-krx.market-data"] button');
      }),
      setup: async bridge => {
        await prepareFixture(bridge);
        const snapshot = bridge.marketplaceSnapshot.bind(bridge);
        bridge.marketplaceSnapshot = async () => {
          const result = await snapshot();
          const connector = result.connectors.find(item => item.entry_id === "krx.market-data")!;
          connector.readiness = "ready";
          connector.credentials = connector.credentials.map(status => ({
            ...status, stored: true, active: true, verification: "verified", verified_at: Date.now(),
            krx_coverage: { kospi: "verified", kosdaq: "needs_approval" },
          }));
          return result;
        };
      },
    },
    {
      ...base("chat-krx-range", "Daily prices for a KRX date range", () => select(".chat-progress-items"), async () => {
        await click(".chat-progress-toggle");
      }),
      setup: async bridge => {
        await prepareFixture(bridge, "answer");
        const selectLoupist = bridge.loupistSelect.bind(bridge);
        bridge.loupistSelect = async id => {
          const workspace = await selectLoupist(id);
          const answer = workspace.threads[0].messages.find(message => message.id === "preview-answer");
          if (answer) answer.progress = { startedAtMs: Date.now() - 4000, items: [{
            id: "krx-range", kind: "tool", category: "finance", operation: "read",
            action: "Read daily prices", running: "Reading daily prices", status: "succeeded",
            target: "990221 · 2026-03-03 → 2026-03-31", source: { kind: "finance", name: "KRX" },
          }] };
          return workspace;
        };
      },
    },
  ];
}
