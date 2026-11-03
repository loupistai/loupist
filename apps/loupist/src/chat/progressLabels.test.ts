import { describe, expect, it, afterEach } from "vitest";
import i18n from "@/i18n";
import toolCatalog from "../../agent/tool-catalog.json";
import { localizedProgressLabel } from "./progressLabels";

const catalog = toolCatalog as unknown as { tools: Array<{ name: string; progress?: { running: string; done: string } }> };

describe("tool progress labels", () => {
  afterEach(async () => { await i18n.changeLanguage("en"); });

  it("covers every catalog tool with the catalog's exact English labels", () => {
    const english = i18n.getResourceBundle("en", "chat").toolProgress as Record<string, { running: string; done: string }>;
    for (const tool of catalog.tools) {
      if (!tool.progress) continue;
      expect(english[tool.name], tool.name).toEqual({ running: tool.progress.running, done: tool.progress.done });
    }
    expect(Object.keys(english).sort()).toEqual(catalog.tools.filter((tool) => tool.progress).map((tool) => tool.name).sort());
  });

  it("translates a step by the tool it names, and passes other labels through", async () => {
    await i18n.changeLanguage("ko");
    expect(localizedProgressLabel("Searching the web", "web_search")).toBe("웹 검색 중");
    expect(localizedProgressLabel("Made a chart", "chart_publish")).toBe("차트 만듦");
    // Two tools share this wording; each step is localized by its own tool.
    expect(localizedProgressLabel("Inspected analysis data", "dataset_query")).toBe("분석 데이터 살펴봄");
    expect(localizedProgressLabel("Inspected analysis data", "dataset_describe")).toBe("분석 데이터 살펴봄");
    // Wording the host composed for the step, and steps without a tool name, stay as sent.
    expect(localizedProgressLabel("Opened Morrow research tools", "capability_load")).toBe("Opened Morrow research tools");
    expect(localizedProgressLabel("Reading hanul.example", "web_fetch")).toBe("Reading hanul.example");
    expect(localizedProgressLabel("Made a chart", undefined)).toBe("Made a chart");
    expect(localizedProgressLabel("Made a chart", "constructor")).toBe("Made a chart");
    // A label is never mapped back to a tool: another tool's wording is not translated.
    expect(localizedProgressLabel("Made a chart", "grid_publish")).toBe("Made a chart");
  });
});
