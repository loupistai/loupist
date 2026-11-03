import { useState } from "react";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { WatchlistAddRequest } from "@/contracts/userContext";
import { LoupistTemplateFields, templateDraft, type LoupistDraft } from "./LoupistTemplateFields";

function Harness({ onFollow }: { onFollow?: (entry: WatchlistAddRequest) => Promise<void> }) {
  const [draft, setDraft] = useState<LoupistDraft>(() => templateDraft("portfolio"));
  const [followed, setFollowed] = useState<WatchlistAddRequest[]>([]);
  return <LoupistTemplateFields draft={draft} onChange={setDraft} followed={followed}
    onFollow={onFollow && (async entry => { await onFollow(entry); setFollowed(current => [...current, entry]); })} />;
}

it("offers nine templates in order, with no icons on the cards", () => {
  render(<Harness />);
  const cards = within(screen.getByRole("group", { name: /template/i })).getAllByRole("button");
  expect(cards.map(card => card.querySelector("strong")?.textContent)).toEqual([
    "My portfolio", "Semiconductors", "Dividend income", "US big tech", "Crypto", "Macro & rates", "Commodities", "Real assets", "Custom",
  ]);
  expect(cards.every(card => !card.querySelector("svg"))).toBe(true);
});

it("fills a class template's persona and offers starters that are added only when tapped", async () => {
  const user = userEvent.setup();
  const follow = vi.fn(async () => {});
  render(<Harness onFollow={follow} />);
  expect(screen.queryByText("Tap to follow:")).toBeNull();
  await user.click(screen.getByRole("button", { name: /^Macro & rates/ }));
  expect(screen.getByLabelText("Name")).toHaveValue("Macro Loupist");
  expect(screen.getByLabelText("Focus")).toHaveValue("Interest rates, inflation, currencies and central banks");
  expect(screen.getByText("Click to follow:")).toBeVisible();
  expect(follow).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Follow US 10-year" }));
  expect(follow).toHaveBeenCalledWith({ symbol: "US10Y", market: "RATES", displayName: "US 10-year Treasury" });
  await waitFor(() => expect(screen.getByRole("button", { name: "Follow US 10-year" })).toHaveAttribute("aria-pressed", "true"));
  await user.click(screen.getByRole("button", { name: /^Crypto/ }));
  expect(screen.getAllByRole("button", { name: /^Follow / }).map(chip => chip.textContent)).toEqual(["Bitcoin", "Ethereum", "Solana", "USD Coin"]);
  await user.click(screen.getByRole("button", { name: /^Real assets/ }));
  expect(screen.queryByRole("button", { name: /^Follow / })).toBeNull();
  expect(screen.getByText("Add a source about a property or fund")).toBeVisible();
});
