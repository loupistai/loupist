import { fireEvent, render, screen } from "@testing-library/react";
import { ComposerAssistMenu, type AssistOption } from "./ComposerAssist";

const options: AssistOption[] = ["dividend-income", "fund-compare", "why-it-moved", "goal-math"].map(id => ({
  id: `skill:${id}`, kind: "skill", label: id, name: id, detail: `${id} blurb`, insert: "", group: "Skills",
}));

/** jsdom has no layout: the list is 300 px tall over 600 px of rows, scrolled to `top`. */
function layout(list: HTMLElement, top: number) {
  Object.defineProperty(list, "scrollHeight", { configurable: true, value: 600 });
  Object.defineProperty(list, "clientHeight", { configurable: true, value: 300 });
  Object.defineProperty(list, "scrollTop", { configurable: true, writable: true, value: top });
}

it("fades the list's edge while rows are below the fold, and stops at the end", () => {
  const view = render(<ComposerAssistMenu id="assist" menuRef={null} trigger={{ symbol: "/", query: "", start: 0, end: 1 }} options={options}
    activeOption={0} skillCount={4} onHover={vi.fn()} onSelect={vi.fn()} />);
  expect(screen.getAllByRole("option")).toHaveLength(4);
  const list = view.container.querySelector<HTMLElement>(".composer-assist-scroll")!;
  layout(list, 0);
  fireEvent.scroll(list);
  expect(list).toHaveAttribute("data-more-below");
  layout(list, 300);
  fireEvent.scroll(list);
  expect(list).not.toHaveAttribute("data-more-below");
});
