import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { LoupistAvailabilityBoundary } from "./LoupistAvailabilityBoundary";

function renderBoundary(status: "newer_version" | "workspace_unavailable") {
  const handlers = { onRecover: vi.fn(), onReconnect: vi.fn(), onRemove: vi.fn() };
  render(
    <LoupistAvailabilityBoundary
      availability={{ status }}
      busy={false}
      error={null}
      {...handlers}
    />,
  );
  return handlers;
}

describe("LoupistAvailabilityBoundary", () => {
  it("asks for an update for a folder a newer Loupist saved, not a reconnect", async () => {
    const { onReconnect, onRemove } = renderBoundary("newer_version");
    const refusal = screen.getByRole("alert", { name: "Saved by a newer version" });
    expect(
      within(refusal).getByText(
        "This Loupist was saved by a newer version of Loupist and can’t be opened here. Update Loupist to open it; its folder is unchanged.",
      ),
    ).toBeVisible();
    expect(within(refusal).queryByRole("button", { name: "Reconnect folder" })).toBeNull();
    await userEvent.click(within(refusal).getByRole("button", { name: "Remove Loupist" }));
    expect(onRemove).toHaveBeenCalledOnce();
    expect(onReconnect).not.toHaveBeenCalled();
  });

  it("keeps a moved folder distinct and offers reconnecting it", async () => {
    const { onReconnect, onRemove } = renderBoundary("workspace_unavailable");
    const unavailable = screen.getByRole("alert", { name: "Workspace folder unavailable" });
    expect(within(unavailable).queryByText(/newer version/)).toBeNull();
    await userEvent.click(within(unavailable).getByRole("button", { name: "Reconnect folder" }));
    expect(onReconnect).toHaveBeenCalledOnce();
    expect(onRemove).not.toHaveBeenCalled();
  });
});
