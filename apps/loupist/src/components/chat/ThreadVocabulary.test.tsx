import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "../../i18n";
import type { ChatMessage, LoupistBridge } from "../../types";
import { ThreadVocabulary } from "./ThreadVocabulary";

afterEach(async () => { await i18n.changeLanguage("en"); });

function post(): ChatMessage {
  return {
    id: "vocabulary-1",
    role: "assistant",
    content: "Your map now also tracks Process node.",
    created_at: "2026-10-30T09:00:00.000Z",
    status: "complete",
    vocabulary: { changeId: "change-7", producer: "preset", labels: ["Process node", "made on"], labelsKo: ["공정 노드", "제조 공정"] },
  } as unknown as ChatMessage;
}

it("says what the map now tracks in plain words and undoes only that change", async () => {
  const undo = vi.fn(async () => ({}));
  render(<ThreadVocabulary message={post()} bridge={{ worldSchemaUndo: undo } as unknown as LoupistBridge} loupistId="loupist-a" loupistName="Chip desk" />);
  expect(screen.getByText("Your map now also tracks Process node, made on.")).toBeVisible();
  await userEvent.setup().click(screen.getByRole("button", { name: "Undo" }));
  expect(undo).toHaveBeenCalledWith({ loupistId: "loupist-a", changeId: "change-7" });
  expect(await screen.findByRole("status")).toHaveTextContent("Undone.");
  expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
});

it("keeps the card and says so when the undo fails", async () => {
  const undo = vi.fn(async () => { throw new Error("nothing left"); });
  render(<ThreadVocabulary message={post()} bridge={{ worldSchemaUndo: undo } as unknown as LoupistBridge} loupistId="loupist-a" loupistName="Chip desk" />);
  await userEvent.setup().click(screen.getByRole("button", { name: "Undo" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't undo this.");
  expect(screen.getByRole("button", { name: "Undo" })).toBeEnabled();
});

it("reads the Korean labels in Korean", async () => {
  await act(async () => { await i18n.changeLanguage("ko"); });
  render(<ThreadVocabulary message={post()} loupistId="loupist-a" loupistName="칩 데스크" />);
  expect(screen.getByText("이제 지도가 공정 노드, 제조 공정도 추적해요.")).toBeVisible();
});
