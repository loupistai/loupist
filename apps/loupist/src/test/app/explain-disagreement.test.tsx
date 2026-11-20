import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../../App";
import { configureTheme, PreviewBridge } from "../../preview/fixtures";
import { scenarios } from "../../preview/scenarios";

/** finance-korea-retail-disagreement.md §2.3: the Map inspector's action fills a new topic Chat and never sends. */
it("Explain the disagreement opens a new chat with inline entity and relation references and the question, unsent", async () => {
  const scenario = scenarios.find((item) => item.id === "map-inspector-explain-disagreement")!;
  // WebKit puts the caret at a field's start on a scripted focus when it has no remembered place.
  const focus = HTMLTextAreaElement.prototype.focus;
  vi.spyOn(HTMLTextAreaElement.prototype, "focus").mockImplementation(function (this: HTMLTextAreaElement, options?: FocusOptions) {
    focus.call(this, options);
    this.setSelectionRange(0, 0);
  });
  const bridge = new PreviewBridge({ delay_ms: 0 });
  await scenario.setup(bridge);
  await configureTheme(bridge, "light");
  const send = vi.spyOn(bridge, "chatSend");
  const create = vi.spyOn(bridge, "chatCreate");
  const view = render(<App bridge={bridge} />);
  const entering = scenario.enter(view.container);
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 0)); });
  await entering;
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: /Explain the disagreement · Hanul Precision/u }));

  const composer = await screen.findByRole("textbox", { name: /message/iu });
  await waitFor(() => expect(composer).toHaveValue("⟦Hanul Precision⟧ ⟦Hanul Precision expanded supplier capacity by 18% in the third quarter.⟧ Why do these sources disagree, and which should I rely on?"));
  // The person types on after the question: the caret is after everything the action put in.
  await waitFor(() => expect(composer).toHaveFocus());
  expect((composer as HTMLTextAreaElement).selectionStart).toBe((composer as HTMLTextAreaElement).value.length);
  expect(create).toHaveBeenCalledTimes(1);
  expect(document.querySelector('[data-inline-reference="world_node"]')).toHaveTextContent("Hanul Precision");
  expect(document.querySelector('[data-inline-reference="world_claim"]')).toHaveTextContent("expanded supplier capacity by 18%");
  expect(send).not.toHaveBeenCalled();
});
