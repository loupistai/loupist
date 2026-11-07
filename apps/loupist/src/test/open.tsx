import { render, type RenderOptions } from "@testing-library/react";
import type { ReactElement, ReactNode } from "react";
import { vi, type Mock } from "vitest";
import type { ChatDestination, Open } from "../chat/navigation";
import { OpenProvider } from "../components/navigation/Open";

/** An app-level opener that records what a component asked to open. */
export const openSpy = () => vi.fn<Open>(async () => undefined);

/** The destinations a component asked an opener to open, in order. */
export const destinations = (open: Mock<Open>): ChatDestination[] => open.mock.calls.map(([to]) => to);

/** Renders under an app-level opener, as the shell does. */
export function renderWithOpen(ui: ReactElement, open = openSpy(), options?: RenderOptions) {
  const wrapper = ({ children }: { children: ReactNode }) => <OpenProvider open={open}>{children}</OpenProvider>;
  return { open, ...render(ui, { wrapper, ...options }) };
}
