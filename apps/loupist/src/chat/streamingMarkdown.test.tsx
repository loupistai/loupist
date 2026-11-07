import { render } from "@testing-library/react";
import { MessageResponse } from "@/components/ai-elements/message-response";
import { LIVE_ANSWER } from "../preview/liveScenario";
import { stableMarkdown } from "./streamingMarkdown";

const evidenceId = `ev_${"c".repeat(32)}`;

describe("stableMarkdown", () => {
  it("holds back a table until its separator row has arrived, then whole rows only", () => {
    const head = "Text.\n\n| Driver | Q2 |";
    expect(stableMarkdown(head, true).text).toBe("Text.\n");
    expect(stableMarkdown(`${head}\n| --- | ---: |`, true).text).toBe("Text.\n");
    expect(stableMarkdown(`${head}\n| --- | ---: |\n| Util`, true).text).toBe(`${head}\n| --- | ---: |\n`);
    const rows = `${head}\n| --- | ---: |\n| Utilization | 78% |\n`;
    expect(stableMarkdown(rows, true).text).toBe(rows);
    expect(stableMarkdown(`${rows}| Inventory | 4`, true).text).toBe(rows);
    expect(stableMarkdown(`${rows}| Inventory | 47 |\n\nAfter.`, true).text).toBe(`${rows}| Inventory | 47 |\n\nAfter.`);
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
    expect(stableMarkdown("Fell.\n\n### What moved", true).text).toBe("Fell.\n\n### What moved");
    expect(stableMarkdown("Fell.\n\n- ", true).text).toBe("Fell.\n\n");
    expect(stableMarkdown("Fell.\n\n1.", true).text).toBe("Fell.\n\n");
    expect(stableMarkdown("Fell.\n\n- **Sign-off.**", true).text).toBe("Fell.\n\n- **Sign-off.**");
  });

  it("offers a caret after a paragraph or list item only", () => {
    expect(stableMarkdown("A sentence", true).caret).toBe(true);
    expect(stableMarkdown("- **Sign-off.** If", true).caret).toBe(true);
    expect(stableMarkdown("### Heading", true).caret).toBe(false);
    expect(stableMarkdown("| a | b |\n| - | - |\n| 1 | 2 |\n", true).caret).toBe(false);
    expect(stableMarkdown("```py\nprint(1)", true).caret).toBe(false);
    expect(stableMarkdown("Done.", false)).toEqual({ text: "Done.", caret: false });
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
    expect(previous).toContain("Fixed costs spread over fewer units");
    expect(view.container.querySelectorAll("table")).toHaveLength(1);
  });
});
