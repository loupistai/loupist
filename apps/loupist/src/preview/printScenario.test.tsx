import { act, cleanup, render, waitFor } from "@testing-library/react";
import { App } from "../App";
import { endPrint } from "../components/print/printView";
import { setPreviewLanguage } from "../i18n/useLanguage";
import { configureTheme, PreviewBridge } from "./fixtures";
import { scenarios } from "./scenarios";

const originalPrint = window.print;

afterEach(() => {
  act(() => endPrint());
  cleanup();
  localStorage.clear();
  setPreviewLanguage("en");
  window.print = originalPrint;
});

it.each(["en", "ko", "en-XA"] as const)("prints the chat preview through the palette in %s", async locale => {
  setPreviewLanguage(locale);
  const scenario = scenarios.find(candidate => candidate.id === "chat-print-view")!;
  const bridge = new PreviewBridge({ delay_ms: 0 });
  await scenario.setup(bridge);
  await configureTheme(bridge, "light");
  const view = render(<App bridge={bridge} />);
  const entering = scenario.enter(view.container);
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });
  await entering;
  await waitFor(() => expect(scenario.landmark()).toBeVisible());
  const paper = document.querySelector(".print-sheet .print-document")!;
  expect(paper).toHaveAttribute("lang", locale);
  expect(paper.querySelector("[data-streamdown]")?.textContent?.trim()).toBeTruthy();
  expect(document.documentElement).toHaveAttribute("data-printing");
  expect(document.querySelector(".command-palette-input")).toBeNull();
});
