import { act, render } from "@testing-library/react";
import { useEffect, useState } from "react";

const renderer = vi.hoisted(() => ({ warm: undefined as undefined | (() => void) }));

// The real renderer loads lazily: plain text first, Streamdown's table frame on a later commit.
vi.mock("./SafeMessageResponse", () => ({
  SafeMessageResponse: () => {
    const [ready, setReady] = useState(false);
    useEffect(() => { renderer.warm = () => setReady(true); }, []);
    if (!ready) return <p>Segment results</p>;
    return <div data-streamdown="table-wrapper"><div data-testid="frame"><table data-streamdown="table"><tbody><tr><td>1,204.5</td></tr></tbody></table></div></div>;
  },
}));

it("fades a table that mounts after the answer and scrolls inside Streamdown's own frame", async () => {
  const { ProseSegment } = await import("./ProseSegment");
  const view = render(<ProseSegment text="| a |" streaming={false} tone="answer" />);
  const scrollWidth = vi.spyOn(HTMLElement.prototype, "scrollWidth", "get").mockImplementation(function (this: HTMLElement) { return this.dataset.testid === "frame" ? 948 : 760; });
  const clientWidth = vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(760);
  try {
    await act(async () => { renderer.warm?.(); });
    const wrapper = view.container.querySelector('[data-streamdown="table-wrapper"]');
    expect(wrapper).toHaveAttribute("data-overflows");
  } finally {
    scrollWidth.mockRestore();
    clientWidth.mockRestore();
  }
});
