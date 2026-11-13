import { render } from "@testing-library/react";
import { MessageResponse } from "@/components/ai-elements/message-response";
import { LIVE_ANSWER } from "../preview/liveScenario";
import { stableMarkdown } from "./streamingMarkdown";

const evidenceId = `ev_${"c".repeat(32)}`;

describe("stableMarkdown", () => {
  it("holds a table whole until a line that is not a row follows it, with a placeholder meanwhile", () => {
    const head = "Text.\n\n| Driver | Q2 |";
    expect(stableMarkdown(head, true)).toEqual({ text: "Text.\n", pending: "table" });
    expect(stableMarkdown(`${head}\n| --- | ---: |`, true)).toEqual({ text: "Text.\n", pending: "table" });
    const rows = `${head}\n| --- | ---: |\n| Utilization | 78% |\n`;
    // Rows never arrive one by one: a finished row is still held while another may follow.
    expect(stableMarkdown(rows, true)).toEqual({ text: "Text.\n", pending: "table" });
    expect(stableMarkdown(`${rows}| Inventory | 4`, true).pending).toBe("table");
    expect(stableMarkdown(`${rows}| Inventory | 47 |\n\n`, true)).toEqual({ text: `${rows}| Inventory | 47 |\n\n`, pending: null });
    expect(stableMarkdown(`${rows}| Inventory | 47 |\nAfter.`, true)).toEqual({ text: `${rows}| Inventory | 47 |\nAfter.`, pending: null });
    expect(stableMarkdown(rows, false)).toEqual({ text: rows, pending: null });
  });

  it("holds back links, math and markers that just opened and leaves bold to the renderer", () => {
    expect(stableMarkdown("Fell to 14.8% [", true).text).toBe("Fell to 14.8%");
    expect(stableMarkdown("Fell to 14.8% [1]", true).text).toBe("Fell to 14.8%");
    expect(stableMarkdown("Fell to 14.8% [1](#evidence-ev_1", true).text).toBe("Fell to 14.8%");
    expect(stableMarkdown(`See [1](#evidence-${evidenceId}) and`, true).text).toBe(`See [1](#evidence-${evidenceId}) and`);
    expect(stableMarkdown("Where $$x = 1", true).text).toBe("Where");
    expect(stableMarkdown("Fell **16.2% to", true).text).toBe("Fell **16.2% to");
    expect(stableMarkdown("Fell **", true).text).toBe("Fell");
    expect(stableMarkdown("Fell.\n\n#", true).text).toBe("Fell.\n\n");
    // A heading shows whole once its line ends.
    expect(stableMarkdown("Fell.\n\n### What moved", true).text).toBe("Fell.\n\n");
    expect(stableMarkdown("Fell.\n\n### What moved\n", true).text).toBe("Fell.\n\n### What moved\n");
    expect(stableMarkdown("Fell.\n\n- ", true).text).toBe("Fell.\n\n");
    expect(stableMarkdown("Fell.\n\n1.", true).text).toBe("Fell.\n\n");
    expect(stableMarkdown("Fell.\n\n- **Sign-off.**", true).text).toBe("Fell.\n\n- **Sign-off.**");
  });

  it("leaves lines inside a code fence to append and draws no caret", () => {
    expect(stableMarkdown("```py\n| a |\n### b", true)).toEqual({ text: "```py\n| a |\n### b", pending: null });
    expect(stableMarkdown("Done.", false)).toEqual({ text: "Done.", pending: null });
  });

  it("never shortens the rendered answer as the live reply grows, character by character", () => {
    const view = render(<MessageResponse isAnimating>{""}</MessageResponse>);
    let previous = "";
    for (let end = 1; end <= LIVE_ANSWER.length; end += 1) {
      const { text } = stableMarkdown(LIVE_ANSWER.slice(0, end), true);
      view.rerender(<MessageResponse isAnimating>{text}</MessageResponse>);
      const rendered = view.container.textContent ?? "";
      expect(rendered.length, `after ${end} characters`).toBeGreaterThanOrEqual(previous.length);
      previous = rendered;
    }
    expect(previous).toContain("Check the next filing");
    expect(view.container.querySelectorAll("table")).toHaveLength(1);
    expect(view.container.querySelector("[class*='caret']")).toBeNull();
  });

  it("adds a held table to the page in one step, with all its rows", () => {
    const view = render(<MessageResponse isAnimating>{""}</MessageResponse>);
    const rowCounts = new Set<number>();
    for (let end = 1; end <= LIVE_ANSWER.length; end += 1) {
      view.rerender(<MessageResponse isAnimating>{stableMarkdown(LIVE_ANSWER.slice(0, end), true).text}</MessageResponse>);
      const table = view.container.querySelector("table");
      if (table) rowCounts.add(table.querySelectorAll("tbody tr").length);
    }
    expect([...rowCounts]).toEqual([3]);
  });
});
