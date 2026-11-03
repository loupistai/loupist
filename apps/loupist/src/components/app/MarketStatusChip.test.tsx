import { act, render, screen } from "@testing-library/react";
import i18n from "@/i18n";
import { MarketStatusChip } from "./MarketStatusChip";

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(Date.parse("2026-10-28T13:00:00Z")); });
afterEach(async () => { vi.useRealTimers(); await act(async () => { await i18n.changeLanguage("en"); }); });

it("shows at most two followed markets with their state and says which calendars it knows", async () => {
  render(<MarketStatusChip markets={["US", "KRX", "CRYPTO"]} />);
  const status = screen.getByRole("group", { name: "Market status" });
  expect([...status.children].map((item) => item.textContent)).toEqual(["US opens in 30m", "Korea closed", "+1 more"]);
  expect(status).toHaveAttribute("title", "Holidays and early closes as the exchanges publish them (US through 2027, Korea through 2026). Other markets and later dates use regular trading hours.");
  // The third market is counted, not dropped silently.
  expect(screen.queryByText("Crypto 24h")).not.toBeInTheDocument();
  await act(async () => { await i18n.changeLanguage("ko"); });
  expect(screen.getByText("미국 30분 뒤 개장")).toBeInTheDocument();
});

it("says a published holiday and an early close in the reader's language", async () => {
  // 10:00 in New York on the day after Thanksgiving, which closes at 1:00 PM.
  vi.setSystemTime(Date.parse("2026-11-27T15:00:00Z"));
  const { unmount } = render(<MarketStatusChip markets={["US"]} />);
  expect(screen.getByText(/^US closes early at /)).toHaveAttribute("data-state", "open");
  unmount();
  // Hangeul Day in Seoul.
  vi.setSystemTime(Date.parse("2026-10-09T01:00:00Z"));
  render(<MarketStatusChip markets={["KRX"]} />);
  expect(screen.getByText("Korea closed today (holiday)")).toHaveAttribute("data-state", "closed");
  await act(async () => { await i18n.changeLanguage("ko"); });
  expect(screen.getByText("한국 오늘 휴장")).toBeInTheDocument();
});

it("renders nothing for markets it does not know", () => {
  const { container } = render(<MarketStatusChip markets={[]} />);
  expect(container).toBeEmptyDOMElement();
});
