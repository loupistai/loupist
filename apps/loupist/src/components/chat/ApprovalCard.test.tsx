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

it("asks once for a new website after third-party text, with no Always", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const decide = vi.spyOn(bridge, "chatApprovalDecide").mockResolvedValue();
  const site = (extra: Partial<ChatApproval> = {}) => waiting("web", {
    requestId: "req-web", kind: "web_site", connectionName: "Web", toolName: "web_fetch", target: "news.example/markets/morrow", ...extra,
  });
  const { rerender } = render(<ApprovalCards approvals={[site()]} refusals={[]} bridge={bridge} loupistId="loupist-one" />);
  const card = screen.getByRole("region", { name: "Approval to open news.example/markets/morrow" });
  expect(within(card).getByText("This reply read text from other sites. It now wants to open a new site: news.example/markets/morrow.")).toBeVisible();
  expect(within(card).getByText("A page it read may have asked for this. Allow it only if you expect this site.")).toBeVisible();
  expect(within(card).queryByRole("button", { name: "Always allow for this connection" })).not.toBeInTheDocument();
  await user.click(within(card).getByRole("button", { name: "Not now" }));
  expect(decide).toHaveBeenCalledWith({ loupist_id: "loupist-one", request_id: "req-web", decision: "deny" });

  rerender(<ApprovalCards approvals={[site({ state: "declined" })]} refusals={[]} bridge={bridge} loupistId="loupist-one" />);
  expect(screen.getByText("Skipped · news.example/markets/morrow · the Loupist continued without it")).toBeVisible();
  rerender(<ApprovalCards approvals={[site({ state: "allowed_once" })]} refusals={[]} bridge={bridge} loupistId="loupist-one" />);
  expect(screen.getByText("Opened once · news.example/markets/morrow")).toBeVisible();
});

it("shows the whole command and offers it once, for the reply, or not now after this reply read other sites", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const decide = vi.spyOn(bridge, "chatApprovalDecide").mockResolvedValue();
  const command = (extra: Partial<ChatApproval> = {}) => waiting("workspace", {
    requestId: "req-cmd", kind: "command", connectionName: "Workspace", toolName: "bash", target: "python3 build_deck.py",
    detail: "python3 build_deck.py \\\n  --out deck.pptx", grant: "reply", ...extra,
  });
  const { rerender } = render(<ApprovalCards approvals={[command()]} refusals={[]} bridge={bridge} loupistId="loupist-one" chatId="chat-one" />);
  const card = screen.getByRole("region", { name: "Approval to run a command" });
  expect(within(card).getByText("This reply read text from other sites. It now wants to run this command:")).toBeVisible();
  expect(card.querySelector(".approval-card-command")).toHaveTextContent("python3 build_deck.py \\ --out deck.pptx");
  expect(within(card).getByText(/A command can reach your files and the network/)).toBeVisible();
  expect(within(card).queryByRole("button", { name: "Allow commands in this chat" })).not.toBeInTheDocument();
  await user.click(within(card).getByRole("button", { name: "Allow commands for this reply" }));
  expect(decide).toHaveBeenCalledWith({ loupist_id: "loupist-one", request_id: "req-cmd", decision: "always" });

  rerender(<ApprovalCards approvals={[command({ state: "always_allowed" })]} refusals={[]} bridge={bridge} loupistId="loupist-one" chatId="chat-one" />);
  expect(screen.getByText("Commands allowed for this reply")).toBeVisible();
  rerender(<ApprovalCards approvals={[command({ state: "allowed_once" })]} refusals={[]} bridge={bridge} loupistId="loupist-one" chatId="chat-one" />);
  expect(screen.getByText("Ran once · python3 build_deck.py")).toBeVisible();
  rerender(<ApprovalCards approvals={[command({ state: "declined" })]} refusals={[]} bridge={bridge} loupistId="loupist-one" chatId="chat-one" />);
  expect(screen.getByText("Skipped · command · the Loupist continued without it")).toBeVisible();
  rerender(<ApprovalCards approvals={[command({ state: "expired" })]} refusals={[]} bridge={bridge} loupistId="loupist-one" chatId="chat-one" />);
  expect(screen.getByText("No longer needed · command")).toBeVisible();
});

it("offers the chat scope when only earlier replies read other sites, and the settled line can stop it", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const decide = vi.spyOn(bridge, "chatApprovalDecide").mockResolvedValue();
  const revoke = vi.spyOn(bridge, "chatCommandGrantRevoke").mockResolvedValue();
  const command = (extra: Partial<ChatApproval> = {}) => waiting("workspace", {
    requestId: "req-cmd", kind: "command", connectionName: "Workspace", toolName: "bash", target: "ls", detail: "ls", grant: "chat", ...extra,
  });
  const { rerender } = render(<ApprovalCards approvals={[command()]} refusals={[]} bridge={bridge} loupistId="loupist-one" chatId="chat-one" />);
  const card = screen.getByRole("region", { name: "Approval to run a command" });
  expect(within(card).getByText(/An earlier reply in this chat read text from other sites/)).toBeVisible();
  await user.click(within(card).getByRole("button", { name: "Allow commands in this chat" }));
  expect(decide).toHaveBeenCalledWith({ loupist_id: "loupist-one", request_id: "req-cmd", decision: "always" });

  rerender(<ApprovalCards approvals={[command({ state: "always_allowed" })]} refusals={[]} bridge={bridge} loupistId="loupist-one" chatId="chat-one" />);
  expect(screen.getByText(/Commands allowed in this chat/)).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Stop allowing" }));
  expect(revoke).toHaveBeenCalledWith({ loupist_id: "loupist-one", thread_id: "chat-one" });
  expect(await screen.findByText("Commands in this chat ask again")).toBeVisible();
});

it("asks for a file outside the workspace or an instruction file with Allow and Not now only", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const decide = vi.spyOn(bridge, "chatApprovalDecide").mockResolvedValue();
  const file = (kind: "file_read" | "file_write" | "instructions", extra: Partial<ChatApproval> = {}) => waiting("workspace", {
    requestId: `req-${kind}`, kind, connectionName: "Workspace", toolName: kind === "file_read" ? "read" : "write",
    target: kind === "instructions" ? "/Users/me/Research/AGENTS.md" : "/Users/me/Documents/shared.csv", ...extra,
  });
  const { rerender } = render(<ApprovalCards approvals={[file("file_read")]} refusals={[]} bridge={bridge} loupistId="loupist-one" />);
  let card = screen.getByRole("region", { name: "Approval to read /Users/me/Documents/shared.csv" });
  expect(within(card).getByText(/It now wants to read \/Users\/me\/Documents\/shared\.csv\./)).toBeVisible();
  expect(within(card).queryByRole("button", { name: /Always|for this reply|in this chat/ })).not.toBeInTheDocument();
  await user.click(within(card).getByRole("button", { name: "Allow" }));
  expect(decide).toHaveBeenCalledWith({ loupist_id: "loupist-one", request_id: "req-file_read", decision: "once" });

  rerender(<ApprovalCards approvals={[file("file_write")]} refusals={[]} bridge={bridge} loupistId="loupist-one" />);
  card = screen.getByRole("region", { name: "Approval to write /Users/me/Documents/shared.csv" });
  expect(within(card).getByText(/It now wants to write \/Users\/me\/Documents\/shared\.csv\./)).toBeVisible();
  await user.click(within(card).getByRole("button", { name: "Not now" }));
  expect(decide).toHaveBeenCalledWith({ loupist_id: "loupist-one", request_id: "req-file_write", decision: "deny" });

  rerender(<ApprovalCards approvals={[file("instructions")]} refusals={[]} bridge={bridge} loupistId="loupist-one" />);
  card = screen.getByRole("region", { name: "Approval to change /Users/me/Research/AGENTS.md" });
  expect(within(card).getByText(/which becomes instructions for later replies/)).toBeVisible();
  expect(within(card).getByRole("button", { name: "Allow once" })).toBeVisible();

  rerender(<ApprovalCards approvals={[file("file_read", { state: "allowed_once" }), file("file_write", { state: "declined" }), file("instructions", { state: "expired" })]}
    refusals={[]} bridge={bridge} loupistId="loupist-one" />);
  expect(screen.getByText("Read allowed · /Users/me/Documents/shared.csv")).toBeVisible();
  expect(screen.getByText("Skipped · /Users/me/Documents/shared.csv · the Loupist continued without it")).toBeVisible();
  expect(screen.getByText("No longer needed · /Users/me/Research/AGENTS.md")).toBeVisible();
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
