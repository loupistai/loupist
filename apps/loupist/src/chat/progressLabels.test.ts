import { describe, expect, it, afterEach } from "vitest";
import i18n from "@/i18n";
import toolCatalog from "../../agent/tool-catalog.json";
import { catalogStepLabel } from "./progressLabels";
import { actionLabel } from "./runStatus";

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

  it("labels a step by the tool it names, whatever the host wrote, and has no label for unknown tools", async () => {
    await i18n.changeLanguage("ko");
    expect(catalogStepLabel("web_search", "running")).toBe("웹 검색 중");
    expect(catalogStepLabel("chart_publish", "done")).toBe("차트 만듦");
    // Two tools share this wording; each step is localized by its own tool.
    expect(catalogStepLabel("dataset_query", "done")).toBe("분석 데이터 살펴봄");
    expect(catalogStepLabel("dataset_describe", "done")).toBe("분석 데이터 살펴봄");
    // The host's own wording ("Reading hanul.example") never reaches a Korean live line for a catalog tool.
    expect(catalogStepLabel("web_fetch", "running")).toBe("웹 자료 읽는 중");
    // Without a catalog entry the host's words stand (the caller keeps them).
    expect(catalogStepLabel("capability_load_unknown", "done")).toBeUndefined();
    expect(catalogStepLabel(undefined, "done")).toBeUndefined();
    expect(catalogStepLabel("constructor", "done")).toBeUndefined();
  });
  it("names a market-data read by what it reads in the interface language, not the host's tool words", async () => {
    const step = (status: "running" | "succeeded") => ({
      id: "read-1", kind: "tool" as const, toolName: "mcp__openbb__equity_fundamental_income", category: "finance" as const, operation: "read" as const,
      action: "Read Equity fundamental income", running: "Reading Equity fundamental income", summary: "", target: "000660.KS", href: null, status, startedAtMs: 0, finishedAtMs: null,
    });
    expect(actionLabel(step("running"))).toBe("Reading income statements");
    expect(actionLabel(step("succeeded"))).toBe("Read income statements");
    await i18n.changeLanguage("ko");
    expect(actionLabel(step("running"))).toBe("손익계산서 읽는 중");
    expect(actionLabel(step("succeeded"))).toBe("손익계산서 읽음");
    // A tool without a known noun keeps the host's wording.
    expect(actionLabel({ ...step("running"), toolName: "mcp__acme__widget_read", running: "Reading widgets" })).toBe("Reading widgets");
  });
});
