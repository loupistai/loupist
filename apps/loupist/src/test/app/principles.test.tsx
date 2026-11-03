import { render, screen, within } from "@testing-library/react";
import { openChat } from "../renderApp";
import userEvent from "@testing-library/user-event";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";

vi.mock('../../components/map/MapCanvas', () => ({ default: ({ selectedId }: { selectedId?: string }) => <div data-testid="principle-map" data-selected={selectedId} /> }));

it("opens a feedback principle on What it knows and returns to the original Chat", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const original = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, 'loupistSelect').mockImplementation(async loupistId => {
    const workspace = await original(loupistId);
    const message = workspace.threads[0].messages.find(item => item.role === 'assistant')!;
    message.feedback = { rating: 'down', correction: 'Review cash conversion over multiple periods.', updatedAt: 1 };
    return workspace;
  });
  const user = userEvent.setup();
  render(<App bridge={bridge} />);
  await openChat();
  await user.click(await screen.findByRole('button', { name: 'Save as a principle' }));
  await user.click(screen.getByRole('button', { name: 'Save' }));
  await screen.findByText('Saved to your principles');
  await user.click(screen.getByRole('button', { name: 'View' }));
  const reader = await screen.findByRole('complementary', { name: 'Review cash conversion over multiple periods.' });
  expect(within(reader).getByText(/You said “Not helpful”/)).toBeVisible();
  await user.click(within(reader).getByRole('button', { name: 'Open the chat' }));
  expect(await screen.findByRole('textbox', { name: 'Message Loupist' })).toHaveValue('');
  await user.click(screen.getByRole('button', { name: 'Undo' }));
  expect(await screen.findByRole('button', { name: 'Save as a principle' })).toBeVisible();
});
