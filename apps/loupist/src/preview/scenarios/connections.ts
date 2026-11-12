/** Preview scenarios, in the capture order `src/preview/scenarios.ts` assembles. */
import { prepareConnectionSetupFixture, prepareCustomConnectionFixture } from "../fixtures/connections";
import { base, click, hasLayout, select, settings, until, type PreviewScenario } from "./kit";

/** Custom connections and a connection set up from Chat. */
export const connectionScenarios: PreviewScenario[] = [
  ...["connections-custom-empty", "connections-custom-list", "connection-add-url-signin", "connection-add-json-review", "connection-review-tools", "connection-detail", "chat-needs-approval"].map((id): PreviewScenario => ({
    id, title: id, setup: bridge => prepareCustomConnectionFixture(bridge, id),
    landmark: () => select(id === "chat-needs-approval" ? ".approval-cards" : id === "connection-detail" ? ".custom-detail" : id === "connection-add-url-signin" ? "[data-custom-signing-in]" : id === "connection-add-json-review" || id === "connection-review-tools" ? ".custom-add .custom-tools" : ".custom-connections"),
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
];
