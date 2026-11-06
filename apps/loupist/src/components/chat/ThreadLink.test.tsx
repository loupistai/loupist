import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "../../i18n";
import type { ChatMessage, LoupistBridge } from "../../types";
import { ThreadLink } from "./ThreadLink";

afterEach(async () => { await i18n.changeLanguage("en"); });

function post(): ChatMessage {
  return {
    id: "link-1",
    role: "assistant",
    content: "New link: Alpha ↔ Gamma through Beta.",
    created_at: "2026-10-30T09:00:00.000Z",
    status: "complete",
    link: {
      connectionId: "con_1",
      seedName: "Alpha",
      farName: "Gamma",
      path: [{ name: "Beta", relation: "SUPPLIES" }, { name: "Gamma", relation: "USES", reversed: true }],
      nodeIds: ["a", "b", "c"],
    },
  } as unknown as ChatMessage;
}

it("renders the path with its relations and opens the Map on it", async () => {
  const onOpenMap = vi.fn();
  render(<ThreadLink message={post()} loupistId="loupist-a" loupistName="Chip desk" onOpenMap={onOpenMap} />);
  expect(screen.getByText("New link: Alpha ↔ Gamma through Beta.")).toBeVisible();
  const path = screen.getByRole("button", { name: "Show this path on the map" });
  expect(path).toHaveTextContent("Alpha→supplies→Beta←uses←Gamma");
  await userEvent.setup().click(path);
  expect(onOpenMap).toHaveBeenCalledWith(["a", "b", "c"]);
});

it("hands Look into it to the same research hand-off and marks the link opened", async () => {
  const researchHandoffStart = vi.fn(async () => ({ chatId: "chat-9", handoffId: "handoff-9" }));
  const knowledgeConnectionOpen = vi.fn(async () => ({}));
  const bridge = { researchHandoffStart, knowledgeConnectionOpen } as unknown as LoupistBridge;
  render(<ThreadLink message={post()} bridge={bridge} loupistId="loupist-a" loupistName="Chip desk" />);
  await userEvent.setup().click(screen.getByRole("button", { name: "Look into it" }));
  expect(researchHandoffStart).toHaveBeenCalledWith(expect.objectContaining({
    loupistId: "loupist-a",
    title: "Link: Alpha and Gamma",
    pins: [{ world: "a" }, { world: "b" }, { world: "c" }],
    requestKey: "link-con_1",
    question: expect.stringContaining("Alpha"),
  }));
  expect(knowledgeConnectionOpen).toHaveBeenCalledWith({ loupistId: "loupist-a", connectionId: "con_1" });
  expect(await screen.findByRole("status")).toHaveTextContent("Researching it.");
  expect(screen.queryByRole("button", { name: "Look into it" })).toBeNull();
});

it("dismisses with Not useful and keeps the card when that fails", async () => {
  const knowledgeConnectionDismiss = vi.fn(async () => { throw new Error("offline"); });
  const bridge = { knowledgeConnectionDismiss } as unknown as LoupistBridge;
  render(<ThreadLink message={post()} bridge={bridge} loupistId="loupist-a" loupistName="Chip desk" />);
  await userEvent.setup().click(screen.getByRole("button", { name: "Not useful" }));
  expect(knowledgeConnectionDismiss).toHaveBeenCalledWith({ loupistId: "loupist-a", connectionId: "con_1" });
  expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't do that.");
  expect(screen.getByRole("button", { name: "Not useful" })).toBeEnabled();
});

it("reads in Korean", async () => {
  await act(async () => { await i18n.changeLanguage("ko"); });
  render(<ThreadLink message={post()} loupistId="loupist-a" loupistName="칩 데스크" />);
  expect(screen.getByText("새 연결: Alpha ↔ Gamma, Beta를 거쳐요.")).toBeVisible();
});
