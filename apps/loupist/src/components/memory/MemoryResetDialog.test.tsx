import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { MemoryResetDialog } from "./MemoryResetDialog";

const counts = { principles: 2, methods: 3, names: 4, relationships: 5, sources: 6 };
function fixture() {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "knowledgeMemoryResetPreview").mockResolvedValue(counts);
  const reset = vi.spyOn(bridge, "knowledgeMemoryReset").mockResolvedValue(counts);
  const onOpenChange = vi.fn();
  const onChanged = vi.fn();
  render(<MemoryResetDialog bridge={bridge} loupistId="loupist-quality" name="Quality" open onOpenChange={onOpenChange} onChanged={onChanged} />);
  return { bridge, reset, onChanged, onOpenChange, user: userEvent.setup() };
}

it("shows host counts and what stays, then requires the typed confirmation", async () => {
  const { reset, onChanged, onOpenChange, user } = fixture();
  const dialog = screen.getByRole("dialog", { name: "Reset what Quality learned?" });
  const confirm = within(dialog).getByRole("button", { name: "Reset" });
  expect(confirm).toBeDisabled();
  expect(await within(dialog).findByText("6")).toBeVisible();
  expect(within(dialog).getByText(/Your writing, shared items/)).toBeVisible();
  const input = within(dialog).getByRole("textbox", { name: "Type RESET to confirm" });
  await user.type(input, "reset");
  expect(confirm).toBeDisabled();
  await user.clear(input); await user.type(input, "RESET");
  await user.click(confirm);
  expect(reset).toHaveBeenCalledWith({ loupistId: "loupist-quality", confirm: true, expected: counts });
  expect(onChanged).toHaveBeenCalledOnce();
  expect(onOpenChange).toHaveBeenCalledWith(false);
});

it("keeps the dialog open when the host refuses a busy World", async () => {
  const { reset, onChanged, onOpenChange, user } = fixture();
  reset.mockRejectedValue(new Error("World processing is busy"));
  const input = screen.getByRole("textbox", { name: "Type RESET to confirm" });
  await waitFor(() => expect(input).toBeEnabled());
  await user.type(input, "RESET"); await user.click(screen.getByRole("button", { name: "Reset" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("World processing is busy");
  expect(onChanged).not.toHaveBeenCalled();
  expect(onOpenChange).not.toHaveBeenCalled();
});
