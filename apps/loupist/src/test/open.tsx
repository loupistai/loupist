import { render, type RenderOptions } from "@testing-library/react";
import type { ReactElement, ReactNode } from "react";
import { vi } from "vitest";
import type { Open } from "../chat/navigation";
import { OpenProvider } from "../components/navigation/Open";

/** An app-level opener that records what a component asked to open. */
export const openSpy = () => vi.fn<Open>(async () => "opened");

/** Renders under an app-level opener, as the shell does. */
export function renderWithOpen(ui: ReactElement, open = openSpy(), options?: RenderOptions) {
  const wrapper = ({ children }: { children: ReactNode }) => <OpenProvider open={open}>{children}</OpenProvider>;
  return { open, ...render(ui, { wrapper, ...options }) };
}
