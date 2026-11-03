import { compile, type TopLevelSpec } from "vega-lite";
import { parse, View } from "vega";
import { assembleAnalyticSpec } from "./analyticAdapter";

it.each([
  ["0.0000001", "0.0000002"],
  ["-0.00000025", "0.0000004"],
  ["2.5", "7.5"],
  ["3500000000", "4500000000"],
])("renders distinct quantitative ticks for decimal values %s and %s", async (first, second) => {
  const spec = assembleAnalyticSpec({
    kind: "line", x: "period", y: ["amount"],
    semantic_types: { period: "Category", amount: "Number" },
  }, [{ period: "earlier", amount: first }, { period: "later", amount: second }], "light", 520, 360);
  const view = new View(parse(compile(spec as unknown as TopLevelSpec).spec), { renderer: "none" });
  try {
    await view.runAsync();
    const ticks: Array<{ value: number; text: string }> = [];
    const visit = (node: unknown) => {
      if (!node || typeof node !== "object") return;
      const item = node as { role?: string; items?: Array<{ datum?: { value?: unknown }; text?: string }> };
      if (item.role === "axis-label") {
        for (const label of item.items ?? []) {
          if (typeof label.datum?.value === "number") ticks.push({ value: label.datum.value, text: label.text ?? "" });
        }
      }
      for (const child of item.items ?? []) visit(child);
    };
    const scenegraph = view.scenegraph() as unknown as { root: unknown };
    visit(scenegraph.root);
    expect(ticks.length).toBeGreaterThan(2);
    expect(new Set(ticks.map((tick) => tick.text)).size).toBe(ticks.length);
    for (const tick of ticks) {
      const displayed = Number(tick.text.replaceAll(",", "").replace("−", "-"));
      expect(Number.isFinite(displayed)).toBe(true);
      if (tick.value !== 0) {
        expect(displayed).not.toBe(0);
        expect(Math.abs((displayed - tick.value) / tick.value)).toBeLessThan(1e-8);
      } else expect(displayed).toBe(0);
    }
  } finally {
    view.finalize();
  }
});
