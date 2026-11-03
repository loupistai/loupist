import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { SidebarProvider } from "@/components/ui/sidebar";
import type { AttentionItem } from "../../contracts/attention";
import { NeedsAttention } from "./NeedsAttention";

const item = (extra: Partial<AttentionItem>): AttentionItem => ({ kind: "unreadResults", loupistId: "g1", loupistName: "Quality Compounder", title: "", atMs: 1, ...extra });

it("lists what waits across Loupists, names each row, and opens the item", async () => {
  const onOpen = vi.fn();
  const items = [
    item({ kind: "approval", chatId: "chat-1", title: "Filings · read_filing" }),
    item({ kind: "briefingConflict", loupistId: "g2", loupistName: "Semiconductors", count: 2 }),
    item({ kind: "unreadResults", loupistId: "g3", loupistName: "Dividend income", count: 2 }),
    item({ kind: "checkDue", recordId: "j1", title: "Inventory days below 50", dueDate: "2000-01-03" }),
    item({ kind: "checkDue", recordId: "j2", title: "Margin above 30%", dueDate: "2000-01-04" }),
    item({ kind: "checkDue", recordId: "j3", title: "Capex flat", dueDate: "2000-01-05" }),
  ];
  render(<SidebarProvider><NeedsAttention items={items} onOpen={onOpen} /></SidebarProvider>);
  expect(screen.getByText("Needs attention")).toBeVisible();
  const rows = screen.getAllByRole("button");
  expect(rows).toHaveLength(5);
  // Each row names its subject: the Loupist and the connection or scheduled result, or the check's claim.
  expect(rows[0]).toHaveTextContent("Quality Compounder wants your OK to use Filings");
  expect(rows[0]).not.toHaveTextContent("read_filing");
  expect(rows[1]).toHaveTextContent("Sources disagree in a scheduled result from Semiconductors");
  expect(rows[2]).toHaveTextContent("2 unread scheduled results from Dividend income");
  expect(rows[3]).toHaveTextContent(/^Quality Compounder · Overdue since .*: Inventory days below 50$/);
  expect(screen.getByText("1 more")).toBeVisible();
  await userEvent.click(rows[0]!);
  expect(onOpen).toHaveBeenCalledWith(items[0]);
});

it("lists an answer that asked a question as waiting on the user, named by its chat, and opens that chat", async () => {
  const onOpen = vi.fn();
  const items = [
    item({ kind: "question", chatId: "chat-7", title: "Hanul Precision margin" }),
    item({ kind: "question", loupistId: "g2", loupistName: "Semiconductors", chatId: "chat-8", title: "" }),
  ];
  render(<SidebarProvider><NeedsAttention items={items} onOpen={onOpen} /></SidebarProvider>);
  const rows = screen.getAllByRole("button");
  expect(rows[0]).toHaveTextContent("Quality Compounder has a question for you in “Hanul Precision margin”");
  // The same amber "waiting on you" dot as an approval.
  expect(rows[0]).toHaveAttribute("data-kind", "question");
  expect(rows[1]).toHaveTextContent("Semiconductors has a question for you");
  await userEvent.click(rows[0]!);
  expect(onOpen).toHaveBeenCalledWith(items[0]);
});

it("renders nothing when nothing waits", () => {
  const { container } = render(<SidebarProvider><NeedsAttention items={[]} onOpen={vi.fn()} /></SidebarProvider>);
  expect(container.querySelector(".needs-attention")).toBeNull();
});
