import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ViewBoundary } from "./ViewBoundary";

describe("ViewBoundary", () => {
  it("keeps sibling navigation usable, focuses recovery, and retries the failed view", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    let failed = true;
    const View = () => {
      if (failed) throw new Error("Internal renderer details");
      return <p>Saved source is readable</p>;
    };
    render(<><button>Memory</button><ViewBoundary name="Chat"><View /></ViewBoundary></>);
    expect(screen.getByRole("button", { name: "Memory" })).toBeEnabled();
    expect(screen.getByRole("alert", { name: "Chat unavailable" })).not.toHaveTextContent("Internal renderer details");
    expect(screen.getByRole("button", { name: "Try again" })).toHaveFocus();
    failed = false;
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(screen.getByText("Saved source is readable")).toBeVisible();
  });

  it("recovers when the user navigates to another view", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const Failed = () => { throw new Error("Failed renderer"); };
    const { rerender } = render(<ViewBoundary name="Chat" resetKey="chat"><Failed /></ViewBoundary>);
    rerender(<ViewBoundary name="Memory" resetKey="memory"><p>Memory opened</p></ViewBoundary>);
    expect(screen.getByText("Memory opened")).toBeVisible();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
