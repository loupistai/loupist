/** Preview scenarios, in the capture order `src/preview/scenarios.ts` assembles. */
import { prepareConnectionSetupFixture, prepareCustomConnectionFixture } from "../fixtures/connections";
import { prepareFixture, type PreviewBridge } from "../fixtures";
import { commandError } from "../../bridge/mock/commandError";
import { base, click, hasLayout, select, settings, until, type PreviewScenario } from "./kit";

type SetupDialog = { id: string; title: string; entry: string; key?: string; landmark: string; prepare?: (bridge: PreviewBridge) => void | Promise<void> };
const SETUP_DIALOGS: SetupDialog[] = [
  { id: "connection-setup-verifying", title: "OpenDART: a key typed and saved, verification under way", entry: "opendart.disclosures", key: "preview-opendart-key",
    landmark: ".marketplace-setup-progress", prepare: bridge => { bridge.marketplaceCredentialVerify = () => new Promise(() => {}); } },
  { id: "connection-setup-verified", title: "OpenDART: the key verified, ready and on for every Loupist", entry: "opendart.disclosures", key: "preview-opendart-key",
    landmark: '.marketplace-setup-message[data-tone="success"]' },
  { id: "connection-setup-rejected", title: "KRX: the key rejected, with KRX's per-market approval named", entry: "krx.market-data", key: "preview-krx-key",
    landmark: '.marketplace-setup-message[data-tone="error"]', prepare: bridge => { bridge.marketplaceCredentialVerify = () => Promise.reject(commandError("credential_rejected", "The provider rejected these credentials.")); } },
  { id: "connection-setup-unchecked", title: "OpenDART: the key saved but not checked, and Verify again", entry: "opendart.disclosures", key: "preview-opendart-key",
    landmark: '.marketplace-setup-message[data-tone="error"]', prepare: bridge => { bridge.marketplaceCredentialVerify = () => Promise.reject(commandError("provider_unavailable", "The provider could not verify these credentials right now.")); } },
  { id: "connection-setup-openfigi", title: "Asset lookup: no key needed, and off for this Loupist with Turn on", entry: "openfigi.lookup",
    landmark: ".marketplace-setup-here", prepare: async bridge => { await bridge.loupistCapabilityDisable({ loupist_id: "loupist-quality", entry_id: "openfigi.lookup" }); } },
];

/** Custom connections and a connection set up from Chat. */
export const connectionScenarios: PreviewScenario[] = [
  ...["connections-custom-empty", "connections-custom-list", "connection-add-url-signin", "connection-add-json-review", "connection-review-tools", "connection-detail", "chat-needs-approval"].map((id): PreviewScenario => ({
    id, title: id, setup: bridge => prepareCustomConnectionFixture(bridge, id),
    landmark: () => select(id === "chat-needs-approval" ? ".approval-cards" : id === "connection-detail" ? ".custom-detail" : id === "connection-add-url-signin" ? "[data-custom-signing-in]" : id === "connection-add-json-review" || id === "connection-review-tools" ? ".custom-add .custom-tools" : id === "connections-custom-empty" ? "[data-custom-connections] > summary" : "[data-custom-connections][open] .custom-rows"),
    enter: async root => {
      if (id === "chat-needs-approval") {
        await until(() => select(".approval-cards"), "Approval activity did not load");
        if (hasLayout()) {
          await until(() => root.querySelector("button.loupist-citation"), "Answer rendering did not finish");
          const activity = select(".approval-cards");
          const viewport = activity?.closest<HTMLElement>(".message-scroll");
          if (activity && viewport) {
            viewport.scrollTop += activity.getBoundingClientRect().top - viewport.getBoundingClientRect().top;
            viewport.dispatchEvent(new Event("scroll", { bubbles: true }));
            for (let parent = viewport.parentElement; parent; parent = parent.parentElement) parent.scrollTop = 0;
            window.scrollTo(0, 0);
          }
        }
        return;
      }
      await settings(root, "connections");
      // Your own connections sit in a closed advanced disclosure until one exists.
      const custom = await until(() => select("[data-custom-connections]") as HTMLDetailsElement | null, "Custom connections did not load");
      if (id !== "connections-custom-empty" && !custom.open) custom.querySelector("summary")?.click();
      if (id === "connection-detail") { await click('[data-custom-connection="preview-custom-changed"]'); return; }
      if (!id.startsWith("connection-add") && id !== "connection-review-tools") return;
      await click("[data-custom-add]");
      const fill = async (selector: string, value: string) => {
        const input = await until(() => select(selector), "Custom connection field did not load") as HTMLInputElement | HTMLTextAreaElement;
        Object.getOwnPropertyDescriptor(input.tagName === "TEXTAREA" ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype, "value")!.set!.call(input, value);
        input.dispatchEvent(new Event("input", { bubbles: true }));
      };
      if (id === "connection-add-json-review") {
        await click('.custom-add [role="radio"][data-state="unchecked"]');
        await fill(".custom-add textarea", JSON.stringify({ mcpServers: { investor: { url: "https://research.example/mcp" }, archive: { command: "npx", args: ["@example/research-archive@1.2.3", "--read-only"] } } }));
      } else await fill('.custom-add input[type="url"]', "https://research.example/mcp");
      await click('.custom-add [data-slot="dialog-footer"] button');
    },
  })),

  ...(["needs_setup", "ready"] as const).map((state): PreviewScenario => ({
    ...base(`chat-connection-setup-${state.replace("_", "-")}`, "A connection set up from Chat", () => select(`.connection-setup-card[data-state="${state}"]`), async () => {
      await until(() => select(`.connection-setup-card[data-state="${state}"]`), "Setup card did not load");
      select(".connection-setup-card")?.scrollIntoView({ block: "center" });
    }),
    setup: bridge => prepareConnectionSetupFixture(bridge, state),
  })),
  // X-003: a connection that needs a contact email, not a key, names the field and where it goes.
  { ...base("chat-connection-setup-contact", "A connection that needs a contact email, set up from Chat", () => select('.connection-setup-card[data-entry-id="sec.edgar"][data-state="needs_setup"]'), async () => {
    await until(() => select('.connection-setup-card[data-state="needs_setup"]'), "Setup card did not load");
    select(".connection-setup-card")?.scrollIntoView({ block: "center" });
  }), setup: bridge => prepareConnectionSetupFixture(bridge, "needs_setup", "sec.edgar") },

  // The setup dialog for a key source, from its card, in each state a key goes through.
  ...SETUP_DIALOGS.map(({ id, title, entry, key, landmark, prepare }): PreviewScenario => ({
    ...base(id, title, () => select(landmark), async root => {
      await settings(root, "connections");
      await click(`[aria-labelledby="connection-${entry}"] button`);
      if (!key) return;
      const input = await until(() => select(".marketplace-setup-dialog input[type=password]") as HTMLInputElement | null, "Key field did not load");
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, key);
      input.dispatchEvent(new Event("input", { bubbles: true }));
      await click('.marketplace-setup-dialog button[type="submit"]');
    }),
    setup: async bridge => { await prepareFixture(bridge); await prepare?.(bridge); },
  })),

  {
    ...base("connections-off-here", "Connections: a ready connection that is off for this Loupist says so on its card", () => select(".marketplace-off-here"), async root => {
      await settings(root, "connections");
      const note = await until(() => select(".marketplace-off-here"), "The off-here note did not appear");
      if (hasLayout()) note.closest("article")?.scrollIntoView({ block: "center" });
    }),
    setup: async bridge => { await prepareFixture(bridge); await bridge.loupistCapabilityDisable({ loupist_id: "loupist-quality", entry_id: "openfigi.lookup" }); },
  },
];
