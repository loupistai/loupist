import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { evidenceFixture } from "../../test/evidenceFixtures";
import { ChatToolResult } from "./ChatToolResult";

it("labels a tool result by its actual purpose", () => {
  render(<ChatToolResult evidence={evidenceFixture({ kind: "derived", tool_name: "dataset_query" })} expanded={false} onToggle={vi.fn()} />);
  expect(screen.getByRole("button")).toHaveTextContent("Selected data");
});

it("copies exactly the retained output and leaves a keyboard-scrollable preview", async () => {
  const user = userEvent.setup();
  const copy = vi.spyOn(navigator.clipboard, "writeText");
  const excerpt = '{"value":391,"unit":"KRW"}';
  render(<ChatToolResult evidence={evidenceFixture({ kind: "derived", tool_name: "dataset_import", excerpt })} expanded onToggle={vi.fn()} />);
  await user.click(screen.getByRole("button", { name: "Copy output" }));
  expect(copy).toHaveBeenCalledExactlyOnceWith(excerpt);
  expect(await screen.findByRole("button", { name: "output copied" })).toBeVisible();
  expect(screen.getByLabelText("Result output")).toHaveAttribute("tabindex", "0");
});
