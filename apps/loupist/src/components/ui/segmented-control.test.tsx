import { useState } from "react";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SegmentedControl, SegmentedControlItem } from "./segmented-control";

it("selects filters with the arrow keys and leaves the group with Tab", async () => {
  function Filters() {
    const [value, setValue] = useState("all");
    return <><SegmentedControl value={value} onValueChange={setValue} aria-label="Collection">
      <SegmentedControlItem value="all">All</SegmentedControlItem>
      <SegmentedControlItem value="ready">Ready</SegmentedControlItem>
    </SegmentedControl><button type="button">Next action</button></>;
  }
  const user = userEvent.setup();
  render(<Filters />);
  await user.tab();
  expect(screen.getByRole("radio", { name: "All" })).toHaveFocus();
  await user.keyboard("{ArrowRight>}");
  const ready = screen.getByRole("radio", { name: "Ready" });
  await waitFor(() => expect(ready).toHaveFocus());
  expect(ready).toHaveAttribute("aria-checked", "true");
  expect(screen.getByRole("radio", { name: "All" })).toHaveAttribute("aria-checked", "false");
  await user.keyboard("{/ArrowRight}");
  await user.tab();
  expect(screen.getByRole("button", { name: "Next action" })).toHaveFocus();
});
