import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { PrincipleForm } from "./PrincipleForm";
import { MapInspector } from "./MapInspector";

async function fixture() {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0].id;
  return { bridge, loupistId, user: userEvent.setup() };
}
it("reloads a conflicting revision before editing again", async () => {
  const { bridge, loupistId, user } = await fixture();
  const principle = await bridge.principleCreate({ loupistId, statement: "Compare reporting periods.", origin: "settings" });
  const saved = vi.fn();
  render(<PrincipleForm bridge={bridge} loupistId={loupistId} principle={principle} onSaved={saved} onCancel={vi.fn()} />);
  await bridge.principleUpdate({ loupistId, recordId: principle.recordId, expectedRevisionId: principle.revisionId, statement: "Compare the same reporting periods." });
  await user.type(screen.getByRole("textbox", { name: "Why" }), "A local draft");
  await user.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("This principle changed elsewhere. Reload it to edit.");
  expect(saved).not.toHaveBeenCalled();
  expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  await user.click(screen.getByRole("button", { name: "Reload" }));
  await waitFor(() => expect(screen.getByRole("textbox", { name: "Statement" })).toHaveValue("Compare the same reporting periods."));
  await user.type(screen.getByRole("textbox", { name: "Why" }), "Seasonality matters.");
  await user.click(screen.getByRole("button", { name: "Save" }));
  await waitFor(() => expect(saved).toHaveBeenCalledWith(expect.objectContaining({ why: "Seasonality matters." })));
});
it("preserves a failed draft for retry and gives duplicates a specific message", async () => {
  const { bridge, loupistId, user } = await fixture();
  const create = vi.spyOn(bridge, "principleCreate").mockRejectedValueOnce(new Error("Offline"));
  const saved = vi.fn();
  render(<PrincipleForm bridge={bridge} loupistId={loupistId} initialStatement="Compare cash flows." onSaved={saved} onCancel={vi.fn()} />);
  await user.click(screen.getByRole("button", { name: "Save" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Could not save this change.");
  expect(screen.getByRole("textbox", { name: "Statement" })).toHaveValue("Compare cash flows.");
  create.mockRejectedValueOnce({ code: "duplicate_principle" });
  await user.click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("You already have this principle.");
  expect(screen.queryByRole("button", { name: "Try again" })).not.toBeInTheDocument();
});
it("opens a principle's origin Chat and applications, and edits it in the inspector", async () => {
  const { bridge, loupistId, user } = await fixture();
  const principle = await bridge.principleCreate({ loupistId, statement: "Compare cash conversion.", why: "Profit is not cash.", example: "Review working capital.", origin: "feedback", originChatId: "original-chat", originMessageId: "answer" });
  const map = await bridge.knowledgeMap({ loupistId });
  const node = map.nodes.find(item => item.recordId === principle.recordId)!;
  map.edges.push({ id: "applied", kind: "applies_to", source: node.id, target: map.nodes[0].id });
  const openChat = vi.fn(), select = vi.fn(), close = vi.fn();
  render(<MapInspector bridge={bridge} location={{ loupistId, surface: "world" }} node={node} map={map} onClose={close} onSelect={select} onOpen={vi.fn()} onAsk={vi.fn()} onOpenChat={openChat} />);
  expect(await screen.findByText("Profit is not cash.")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Open conversation" }));
  expect(openChat).toHaveBeenCalledWith("original-chat");
  await user.click(screen.getByRole("button", { name: map.nodes[0].label }));
  expect(select).toHaveBeenCalledWith(map.nodes[0]);
  await user.click(screen.getByRole("button", { name: "Edit" }));
  const form = screen.getByRole("form", { name: "Edit" });
  await user.clear(within(form).getByRole("textbox", { name: "Why" }));
  await user.click(within(form).getByRole("button", { name: "Save" }));
  await waitFor(() => expect(screen.queryByText("Profit is not cash.")).not.toBeInTheDocument());
  await user.click(screen.getByRole("button", { name: "Archive" }));
  await waitFor(() => expect(close).toHaveBeenCalled());
});
