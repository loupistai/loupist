import { useState } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SelectField, SelectItem } from "./select";

it("supports keyboard navigation, type-ahead, Escape and native form data", async () => {
  const onChange = vi.fn();
  function Form() {
    const [value, setValue] = useState("a");
    return <form aria-label="Filters">
      <SelectField aria-label="Sort" name="sort" value={value} onValueChange={next => { onChange(next); setValue(next); }}>
        <SelectItem value="a">Alphabetical</SelectItem>
        <SelectItem value="d" disabled>Default</SelectItem>
        <SelectItem value="r">Recent</SelectItem>
      </SelectField>
    </form>;
  }
  render(<Form />);
  const user = userEvent.setup();
  const trigger = screen.getByRole("combobox", { name: "Sort" });
  trigger.focus();
  await user.keyboard("{ArrowDown}");
  expect(await screen.findByRole("option", { name: "Default" })).toHaveAttribute("aria-disabled", "true");
  await user.keyboard("{Escape}");
  expect(trigger).toHaveFocus();
  expect(onChange).not.toHaveBeenCalled();
  await user.keyboard("r");
  await waitFor(() => expect(trigger).toHaveTextContent("Recent"));
  expect(onChange).toHaveBeenCalledExactlyOnceWith("r");
  expect(new FormData(screen.getByRole<HTMLFormElement>("form", { name: "Filters" })).get("sort")).toBe("r");
});

it("opens its options as the app's menus do: inset so a highlighted option is never clipped, offset by Radix, arrows move and Enter picks", async () => {
  const onChange = vi.fn();
  function Delivery() {
    const [value, setValue] = useState("bot");
    return <SelectField compact aria-label="Posts to" value={value} onValueChange={next => { onChange(next); setValue(next); }}>
      <SelectItem value="bot">Bot</SelectItem>
      <SelectItem value="none">Schedule only</SelectItem>
    </SelectField>;
  }
  render(<Delivery />);
  const user = userEvent.setup();
  const trigger = screen.getByRole("combobox", { name: "Posts to" });
  trigger.focus();
  await user.keyboard("{ArrowDown}");
  const list = await screen.findByRole("listbox");
  const content = list.closest<HTMLElement>("[data-slot=select-content]")!;
  // The same inset as DropdownMenuContent; the offset comes from sideOffset, not a translate that fights the open animation.
  expect(content.className.split(/\s+/u)).toContain("p-1");
  expect(content.className).not.toMatch(/translate-[xy]-1/u);
  await waitFor(() => expect(screen.getByRole("option", { name: "Bot" })).toHaveFocus());
  await user.keyboard("{ArrowDown}");
  await waitFor(() => expect(screen.getByRole("option", { name: "Schedule only" })).toHaveFocus());
  await user.keyboard("{Enter}");
  await waitFor(() => expect(trigger).toHaveTextContent("Schedule only"));
  expect(onChange).toHaveBeenCalledExactlyOnceWith("none");
  expect(trigger).toHaveFocus();
});
