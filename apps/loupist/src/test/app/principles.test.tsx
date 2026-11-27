import { render, screen, within } from "@testing-library/react";
import { mainTab, openChat } from "../renderApp";
import userEvent from "@testing-library/user-event";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";

vi.mock('../../components/map/MapCanvas', () => ({ default: ({ selectedId }: { selectedId?: string }) => <div data-testid="principle-map" data-selected={selectedId} /> }));

it("opens an existing feedback principle in Lens and returns to its original Chat", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = "loupist-quality";
  const [origin] = (await bridge.loupistSelect(loupistId)).threads;
  const message = origin.messages.find(item => item.role === "assistant")!;
  const statement = "Review cash conversion over multiple periods.";
  await bridge.principleCreate({
    loupistId, statement, origin: "feedback", originChatId: origin.id, originMessageId: message.id,
  });
  const user = userEvent.setup();
  render(<App bridge={bridge} />);
  await openChat();
  await user.click(await mainTab(/Lens/));
  await user.click(await screen.findByRole("button", { name: /Review cash conversion over multiple periods/ }));
  const reader = await screen.findByRole("complementary", { name: statement });
  expect(within(reader).getByText(/You said “Not helpful”/)).toBeVisible();
  await user.click(within(reader).getByRole('button', { name: 'Open the chat' }));
  expect(await screen.findByRole('textbox', { name: 'Message Loupist' })).toHaveValue('');
  expect(screen.getByText(message.content)).toBeVisible();
});
