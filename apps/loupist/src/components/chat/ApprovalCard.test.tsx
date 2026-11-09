import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatApproval } from "../../contracts/chat";
import { ApprovalCards } from "./ApprovalCard";

const waiting = (connectionId: string, extra: Partial<ChatApproval> = {}): ChatApproval => ({
  requestId: "req-1", connectionId, connectionName: "Company research", toolName: "read_filings",
  target: "Hanul Precision · 2026 Q3", state: "waiting", requestedAtMs: Date.now() - 12_000, ...extra,
});

it("asks for one read-only tool and sends the user's choice to the host", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const decide = vi.spyOn(bridge, "chatApprovalDecide").mockResolvedValue();
  const { rerender } = render(<ApprovalCards approvals={[waiting("c1")]} refusals={[]} bridge={bridge} loupistId="loupist-one" />);
  const card = screen.getByRole("region", { name: "Approval for Read filings" });
  expect(within(card).getByText("Company research wants to use “Read filings” for Hanul Precision · 2026 Q3.")).toBeVisible();
  expect(within(card).getByText(/^waiting 0:1\d$/)).toBeVisible();
  expect(within(card).getByText("This connection receives the request. The tool can only read data.")).toBeVisible();
  await user.click(within(card).getByRole("button", { name: "Allow once" }));
  expect(decide).toHaveBeenCalledWith({ loupist_id: "loupist-one", request_id: "req-1", decision: "once" });

  // The host's next progress patch settles the card into one line.
  rerender(<ApprovalCards approvals={[waiting("c1", { state: "allowed_once" })]} refusals={[]} bridge={bridge} loupistId="loupist-one" />);
  expect(screen.queryByRole("region", { name: "Approval for Read filings" })).not.toBeInTheDocument();
  expect(screen.getByText("Allowed once · Read filings")).toBeVisible();
  rerender(<ApprovalCards approvals={[waiting("c1", { state: "declined" })]} refusals={[]} bridge={bridge} loupistId="loupist-one" />);
  expect(screen.getByText("Skipped · Read filings · the Loupist continued without it")).toBeVisible();
});

it("says when a card is no longer waiting", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "chatApprovalDecide").mockRejectedValue(new Error("This approval is no longer waiting"));
  render(<ApprovalCards approvals={[waiting("c1")]} refusals={[]} bridge={bridge} loupistId="loupist-one" />);
  await user.click(screen.getByRole("button", { name: "Always allow for this connection" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("This approval is no longer waiting.");
});

it("opens the connection's settings to change an Always or review a refused tool", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await bridge.customConnectionAddUrl({ url: "https://other.example", name: "Other connection" });
  const target = await bridge.customConnectionAddUrl({ url: "https://research.example", name: "Company research" });
  const list = vi.spyOn(bridge, "customConnectionList");
  render(<ApprovalCards approvals={[waiting(target.id, { state: "always_allowed" })]}
    refusals={[{ connection_id: target.id, connection_name: target.name, tool_name: "read_filings", reason: "blocked_unattended" }]} bridge={bridge} loupistId="loupist-one" />);
  expect(screen.getByText(/Always allowed · Read filings \(Company research\)/)).toBeVisible();
  expect(screen.getByText("“Read filings” from Company research needs your approval in Settings.")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Change in Settings" }));
  const sheet = await screen.findByRole("dialog", { name: "Company research" });
  expect(within(sheet).getByRole("heading", { name: "Company research" })).toBeVisible();
  expect(list).toHaveBeenCalledWith({ loupist_id: "loupist-one" });
});
