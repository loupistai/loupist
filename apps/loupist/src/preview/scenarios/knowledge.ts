/** Preview scenarios, in the capture order `src/preview/scenarios.ts` assembles. */
import { prepareGrowthReplay } from "../growth-replay";
import { PreviewBridge } from "../fixtures";
import { prepareConflictsFixture } from "../fixtures/conflicts";
import { prepareJournalFixture } from "../fixtures/journal";
import { prepareLearningFixture } from "../fixtures/learning";
import { prepareCoreSchema } from "../core-schema";
import i18n from "../../i18n";
import { base, click, clickMapAction, hasLayout, mapCanvas, navigate, openScheduleRun, select, selectPanel, until, type PreviewScenario } from "./kit";

const i18nHistory = () => i18n.t("knowledge:memoryPage.tabs.history");

/** The Map's growth replay, conflicts and core kinds, the Journal and a learned method. */
export const knowledgeScenarios: PreviewScenario[] = [
  { ...base("growth-replay", "Recorded Semiconductors growth", () => select(".map-replay-bar"), async root => {
    await navigate(root, "map");
    const map = await until(() => select(".map-view"), "Growth Map did not load");
    if (hasLayout()) await until(mapCanvas, "Growth Map canvas did not mount");
    await clickMapAction("[data-map-replay]", map);
    const scrubber = await until(() => map.querySelector<HTMLInputElement>(".map-replay-scrubber"), "Growth replay did not open");
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(scrubber, scrubber.max);
    scrubber.dispatchEvent(new Event("input", { bubbles: true }));
    scrubber.dispatchEvent(new Event("change", { bubbles: true }));
    if (hasLayout()) {
      for (let frame = 0; frame < 3; frame++) await new Promise(requestAnimationFrame);
      await clickMapAction("[data-map-fit]", map);
    }
  }), setup: prepareGrowthReplay },
  { ...base("map-conflicts", "Conflicting evidence", () => select(".map-conflicts"), async root => {
    await navigate(root, "map");
    const input = await until(() => select('.map-view [role="combobox"]'), "Map search missing") as HTMLInputElement;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "Hanul");
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await click('[role="option"]');
    await until(() => select(".map-conflicts"), "Conflicting evidence did not load");
    if (hasLayout()) { await until(mapCanvas, "Map canvas missing"); select(".map-conflicts")?.scrollIntoView({ block: "start" }); }
  }), setup: prepareConflictsFixture },
  { ...base("map-core-schema", "Core World kinds", () => select("#map-inspector-title"), async root => {
    await navigate(root, "map");
    const input = await until(() => select('.map-view [role="combobox"]'), "Map search missing") as HTMLInputElement;
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "Hanbit");
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await click('[role="option"]');
    await until(() => select("#map-inspector-title"), "Core schema inspector did not open");
    if (hasLayout()) {
      await until(mapCanvas, "Core schema canvas did not mount");
      const legend = document.querySelector("details.map-legend");
      if (legend instanceof HTMLDetailsElement && !legend.open) legend.querySelector("summary")?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      await clickMapAction("[data-map-fit]", root);
      for (let frame = 0; frame < 3; frame++) await new Promise(requestAnimationFrame);
    }
  }), setup: prepareCoreSchema },
  { ...base("briefing-conflicts-link", "Briefing links to conflicting evidence", () => select("[data-briefings-conflicts]"), async root => {
    await openScheduleRun(root, "preview-briefing-1");
    const link = await until(() => select("[data-briefings-conflicts]"), "Conflicting evidence link missing");
    if (hasLayout()) link.scrollIntoView({ block: "center" });
  }), setup: prepareConflictsFixture },
  ...["notes-journal", "journal-new-entry", "journal-reader-outcome"].map((id): PreviewScenario => ({
    id, title: "Your journal and research notes", setup: prepareJournalFixture,
    landmark: () => select(id === "journal-new-entry" ? ".journal-check-fields" : id === "journal-reader-outcome" ? ".journal-outcome-card" : ".journal-entry-list"),
    enter: async root => {
      await navigate(root, "map");
      await clickMapAction("[data-map-library]", root);
      await click('[role="tab"][id$="-trigger-note"]', root);
      await until(() => root.querySelectorAll(".journal-library-row").length === 3, "Journal entries did not load");
      if (id === "journal-new-entry") {
        await click(".journal-library-section .source-library-heading button", root);
        await click(".journal-check-toggle", root);
      } else if (id === "journal-reader-outcome") await click('[data-research-id="preview-journal-1"]', root);
      if (hasLayout()) { await until(mapCanvas, "Map canvas did not load"); for (let frame = 0; frame < 3; frame++) await new Promise(requestAnimationFrame);
        const target = select(id === "journal-new-entry" ? ".journal-check-fields" : id === "journal-reader-outcome" ? ".journal-outcome-card" : ".journal-entry-list");
        const viewport = target?.closest<HTMLElement>(id === "journal-new-entry" ? '[role="tabpanel"]' : ".knowledge-research-reader");
        if (target && viewport && target.getBoundingClientRect().bottom > viewport.getBoundingClientRect().bottom) viewport.scrollTop += target.getBoundingClientRect().top - viewport.getBoundingClientRect().top - 12;
      }
    },
  })),
  ...["map-learned-method", "map-learned-toast", "memory-history", "lens-then-now-stacked"].map((id): PreviewScenario => {
    let bridge: PreviewBridge;
    let review: Awaited<ReturnType<typeof prepareLearningFixture>>;
    return {
      id, title: "How your Loupist learned a method",
      setup: async candidate => { bridge = candidate; review = await prepareLearningFixture(candidate, 3); },
      landmark: () => select(id === "map-learned-toast" ? ".learning-toast" : id === "memory-history" ? ".memory-history" : id === "lens-then-now-stacked" ? ".lens-then-now" : ".method-learning"),
      enter: async root => {
        if (id === "memory-history") {
          // Learning history lives on "What {Loupist} knows".
          await until(() => root.querySelector("textarea[data-chat-composer]"), "Chat did not initialize");
          await selectPanel("lens");
          // Choose the tab once the page has loaded; a click during loading can be lost to the first render.
          await until(() => select(".memory-page-body"), "What it knows did not load");
          const history = await until(() => [...document.querySelectorAll<HTMLElement>('.memory-page-tabs [role="tab"]')].find(tab => tab.textContent?.includes(i18nHistory())), "History tab missing");
          history.click();
          await until(() => select(".memory-history"), "History did not load");
          return;
        }
        await navigate(root, "map");
        bridge.emitLearningReview(review);
        await until(() => select(".learning-toast"), "Learning toast did not appear");
        if (id !== "map-learned-toast") {
          await click(".learning-toast button");
          await until(() => select(".lens-then-now"), "Learning details did not load");
          if (id === "lens-then-now-stacked") {
            const inspector = select(".workspace-lens-content .memory-reader")!;
            inspector.style.width = "340px";
            inspector.style.height = "100%";
            inspector.style.left = "auto";
            select(".lens-history")?.scrollIntoView({ block: "start" });
          }
        }
        if (hasLayout() && id === "map-learned-toast") { await until(mapCanvas, "Map canvas did not load"); for (let frame = 0; frame < 3; frame++) await new Promise(requestAnimationFrame); }
      },
    };
  }),
];
