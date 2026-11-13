import { act, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";

const mermaid = vi.hoisted(() => ({ load: vi.fn() }));

vi.mock("./mermaidPlugin", () => ({
  hasCompleteMermaidFence: (text: string) => text === "closed-diagram",
  loadMermaidPlugin: mermaid.load,
}));

vi.mock("@/components/ai-elements/message-response", () => ({
  MessageResponse: ({ children, mermaidPlugin, holdDiagrams }: { children: ReactNode; mermaidPlugin?: unknown; holdDiagrams?: boolean }) => {
    if (children === "broken renderer") throw new Error("renderer unavailable");
    return <div data-mermaid={mermaidPlugin ? "loaded" : "base"} data-hold={holdDiagrams ? "" : undefined} data-testid="response">{children}</div>;
  },
}));

import { plainResponseText, SafeMessageResponse } from "./SafeMessageResponse";

const response = (text: string, isAnimating = false) => <SafeMessageResponse isAnimating={isAnimating} text={text} />;

beforeEach(() => mermaid.load.mockReset());

it("keeps response text visible when rich rendering fails", async () => {
  const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
  try {
    render(response("broken renderer"));
    expect(await screen.findByText("broken renderer")).toBeVisible();
  } finally {
    consoleError.mockRestore();
  }
});

it("loads Mermaid only for a completed diagram after streaming stops", async () => {
  mermaid.load.mockResolvedValue({ name: "mermaid" });
  const view = render(response("ordinary Markdown"));
  expect(await screen.findByTestId("response")).toHaveAttribute("data-mermaid", "base");
  view.rerender(response("closed-diagram", true));
  expect(mermaid.load).not.toHaveBeenCalled();
  view.rerender(response("closed-diagram"));
  await waitFor(() => expect(mermaid.load).toHaveBeenCalledOnce());
  await waitFor(() => expect(screen.getByTestId("response")).toHaveAttribute("data-mermaid", "loaded"));
});

it("holds a diagram's frame while the reply streams and until its renderer arrives, then lets it render", async () => {
  let resolve!: (plugin: unknown) => void;
  mermaid.load.mockReturnValue(new Promise((done) => { resolve = done; }));
  const view = render(response("closed-diagram", true));
  expect(await screen.findByTestId("response")).toHaveAttribute("data-hold");
  view.rerender(response("closed-diagram"));
  // Settled, but the renderer is still loading: the frame stays, so the code never shows in its place.
  expect(screen.getByTestId("response")).toHaveAttribute("data-hold");
  await act(async () => { resolve({ name: "mermaid" }); });
  expect(screen.getByTestId("response")).not.toHaveAttribute("data-hold");
  expect(screen.getByTestId("response")).toHaveAttribute("data-mermaid", "loaded");
});

it("keeps the base renderer when the deferred plugin cannot load", async () => {
  mermaid.load.mockRejectedValueOnce(new Error("unavailable"));
  render(response("closed-diagram"));
  const rendered = await screen.findByTestId("response");
  await waitFor(() => expect(mermaid.load).toHaveBeenCalledOnce());
  await act(async () => { await Promise.resolve(); });
  expect(rendered).toHaveAttribute("data-mermaid", "base");
  // The frame gives way to the code rather than staying empty.
  expect(rendered).not.toHaveAttribute("data-hold");
});

it("shows a readable projection without Markdown markers while the renderer loads", () => {
  expect(plainResponseText("Token **18420** on __12 March__. [Attached note](#evidence-ev_1) and `code`"))
    .toBe("Token 18420 on 12 March. Attached note and code");
  expect(plainResponseText("## Outline\n> quoted")).toBe("Outline\nquoted");
});
