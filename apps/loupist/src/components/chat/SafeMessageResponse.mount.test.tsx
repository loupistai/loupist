import { act, render, screen } from "@testing-library/react";
import type { ReactNode } from "react";

const load = vi.hoisted(() => {
  let release!: () => void;
  const ready = new Promise<void>(resolve => { release = resolve; });
  return { ready, release };
});

vi.mock("@/components/ai-elements/message-response", async () => {
  await load.ready;
  return { MessageResponse: ({ children }: { children: ReactNode }) => <button type="button">{children}</button> };
});

it("preserves the focused citation when a cold renderer becomes warm and the answer updates", async () => {
  const { SafeMessageResponse } = await import("./SafeMessageResponse");
  const view = render(<SafeMessageResponse text="Source before" isAnimating={false} />);
  expect(screen.getByText("Source before")).toHaveClass("message-response-fallback");
  await act(async () => { load.release(); });
  const citation = await screen.findByRole("button", { name: "Source before" });
  citation.focus();
  view.rerender(<SafeMessageResponse text="Source after" isAnimating={false} />);
  expect(screen.getByRole("button", { name: "Source after" })).toBe(citation);
  expect(citation).toHaveFocus();
});
