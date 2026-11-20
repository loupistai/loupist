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

it("names the post by what it adds, and moves the keyboard to what happened when Undo goes away", async () => {
  let finish!: () => void;
  const undo = vi.fn(() => new Promise<void>((done) => { finish = done; }));
  render(<ThreadVocabulary message={post()} bridge={{ worldSchemaUndo: undo } as unknown as LoupistBridge} loupistId="loupist-a" loupistName="Chip desk" />);
  expect(screen.getByRole("article", { name: "New on the map: Process node, made on" })).toBeVisible();
  const user = userEvent.setup();
  await user.tab();
  await user.keyboard("{Enter}");
  // While the host undoes it, the button keeps the keyboard and ignores a second press.
  expect(screen.getByRole("button", { name: "Undo" })).toHaveFocus();
  expect(screen.getByRole("button", { name: "Undo" })).toHaveAttribute("aria-disabled", "true");
  await user.keyboard("{Enter}");
  expect(undo).toHaveBeenCalledOnce();
  await act(async () => finish());
  expect(screen.getByRole("status")).toHaveFocus();
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
  expect(screen.getByText("이제 지도가 공정 노드, 제조 공정도 추적합니다.")).toBeVisible();
});
