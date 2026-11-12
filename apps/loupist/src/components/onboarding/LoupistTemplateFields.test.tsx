import { useState } from "react";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { WatchlistAddRequest } from "@/contracts/userContext";
import { LoupistTemplateFields, templateDraft, type LoupistDraft } from "./LoupistTemplateFields";

function Harness({ onFollow }: { onFollow?: (entry: WatchlistAddRequest) => Promise<void> }) {
  const [draft, setDraft] = useState<LoupistDraft>(() => templateDraft("portfolio"));
  return <LoupistTemplateFields draft={draft} onChange={setDraft} onFollow={onFollow} />;
}

it("offers eight templates in order, with no icons on the cards", () => {
  render(<Harness />);
  const cards = within(screen.getByRole("group", { name: /template/i })).getAllByRole("button");
  expect(cards.map(card => card.querySelector("strong")?.textContent)).toEqual([
    "My portfolio", "Semiconductors", "Dividend income", "US big tech", "Crypto", "Macro & rates", "Commodities", "Custom",
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
  await user.click(screen.getByRole("button", { name: /^Custom/ }));
  expect(screen.queryByRole("button", { name: /^Follow / })).toBeNull();
});

it("leaves a starter unfollowed and says so when following fails", async () => {
  const user = userEvent.setup();
  render(<Harness onFollow={vi.fn(async () => { throw new Error("offline"); })} />);
  await user.click(screen.getByRole("button", { name: /^Crypto/ }));
  await user.click(screen.getByRole("button", { name: "Follow Bitcoin" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Couldn’t follow that. Try again.");
  expect(screen.getByRole("button", { name: "Follow Bitcoin" })).toHaveAttribute("aria-pressed", "false");
});

it("keeps onboarding to the essentials: templates and Name, plus Focus for Custom", async () => {
  const user = userEvent.setup();
  function Essentials() {
    const [draft, setDraft] = useState<LoupistDraft>(() => templateDraft("portfolio"));
    return <LoupistTemplateFields draft={draft} onChange={setDraft} essentials />;
  }
  render(<Essentials />);
  expect(screen.getByLabelText("Name")).toBeVisible();
  expect(screen.queryByLabelText("Focus")).toBeNull();
  expect(screen.queryByRole("group", { name: "Avatar" })).toBeNull();
  expect(screen.queryAllByRole("radio")).toHaveLength(0);
  expect(screen.getByText("You can change its avatar, focus and tone later in Loupist settings.")).toBeVisible();
  await user.click(screen.getByRole("button", { name: /^Custom/ }));
  expect(screen.getByLabelText("Focus")).toBeVisible();
});
