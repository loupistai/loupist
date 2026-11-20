import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { mockAssetId } from "../../bridge/mock/assets";
import { renderWithOpen } from "../../test/open";
import i18n from "../../i18n";
import { BotGreeting } from "./BotGreeting";

const loupist = { id: "loupist-quality", name: "Quality Compounder", avatar: "loupist-01" as const };
const followedRow = (assetId: string, name: string) => ({ assetId, name, reference: null, followedAtMs: 0, createdAtMs: 0 });
const NVIDIA = mockAssetId("NVIDIA");
const APPLE = mockAssetId("Apple");

function setup() {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "assetsList").mockResolvedValue([followedRow(NVIDIA, "NVIDIA"), followedRow(APPLE, "Apple")]);
  vi.spyOn(bridge, "watchlistQuotes").mockResolvedValue([
    { assetId: NVIDIA, price: { value: 162.85, date: "2026-11-13", currency: "USD", provider: "yfinance", sourceUrl: null, dataset: { dataset_id: "a", revision: 1, digest: "test" } } },
  ]);
  vi.spyOn(bridge, "principleList").mockResolvedValue([]);
  return bridge;
}

afterEach(() => localStorage.clear());

it("names a tile by where it goes and reads its price and stated date after the name", async () => {
  renderWithOpen(<BotGreeting bridge={setup()} loupistId={loupist.id} loupist={loupist} onAsk={vi.fn()} />);
  const tile = await screen.findByRole("button", { name: "Open NVIDIA" });
  expect(tile).toHaveAccessibleDescription(/^\$162\.85 As of Nov 13 ·\s+Yahoo Finance$/u);
  // The separator travels with the provider after it, so the line never ends on "·".
  expect(tile.querySelector(".bot-tile-foot")!.textContent).toBe("As of Nov 13 · Yahoo Finance");
});

it("names and prices a tile as the asset page does: a Korean listing's Korean name and every stated digit", async () => {
  await i18n.changeLanguage("ko");
  try {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const hanul = `ast_${"4".repeat(16)}`;
    vi.spyOn(bridge, "assetsList").mockResolvedValue([{ assetId: hanul, name: "Hanul Precision", reference: { figi: null, ticker: "HNP", exchange: "KS", securityType: null, origin: "imported" }, followedAtMs: 0, createdAtMs: 0 }]);
    // A price the replay stated without a currency keeps its own precision, as on the asset page.
    vi.spyOn(bridge, "watchlistQuotes").mockResolvedValue([
      { assetId: hanul, price: { value: 39920, date: "2026-11-13", currency: null, provider: "krx", sourceUrl: null, dataset: { dataset_id: "a", revision: 1, digest: "test" } } },
    ]);
    vi.spyOn(bridge, "principleList").mockResolvedValue([]);
    const map = await bridge.knowledgeMap({ loupistId: loupist.id, maxNodes: 1000 });
    vi.spyOn(bridge, "knowledgeMap").mockResolvedValue({ ...map, nodes: [{ id: "e-hanul", kind: "entity", label: "Hanul Precision", localNames: ["한울정밀"], entityKind: "Organization", degree: 1, watched: false }] });
    renderWithOpen(<BotGreeting bridge={bridge} loupistId={loupist.id} loupist={loupist} onAsk={vi.fn()} />);
    const tile = await screen.findByRole("button", { name: "한울정밀 열기" });
    expect(tile.querySelector(".bot-tile-name")).toHaveTextContent("한울정밀");
    expect(tile.querySelector(".bot-tile-price")!.textContent).toBe("39,920");
  } finally { await i18n.changeLanguage("en"); }
});

it("hides the principle suggestion for good and keeps the keyboard on the cards above it", async () => {
  const bridge = setup();
  const view = renderWithOpen(<BotGreeting bridge={bridge} loupistId={loupist.id} loupist={loupist} onAsk={vi.fn()} />);
  const greeting = await screen.findByRole("region", { name: "Quality Compounder greets you" });
  await waitFor(() => expect(greeting).toHaveAttribute("aria-busy", "false"));
  const hide = within(greeting).getByRole("button", { name: "Hide this suggestion" });
  hide.focus();
  await userEvent.setup().keyboard("{Enter}");
  expect(within(greeting).queryByRole("button", { name: "Hide this suggestion" })).toBeNull();
  const cards = within(within(greeting).getByRole("region", { name: "Suggested questions" })).getAllByRole("button");
  expect(cards.at(-1)).toHaveFocus();
  view.unmount();
  renderWithOpen(<BotGreeting bridge={bridge} loupistId={loupist.id} loupist={loupist} onAsk={vi.fn()} />);
  await waitFor(() => expect(screen.getByRole("region", { name: "Quality Compounder greets you" })).toHaveAttribute("aria-busy", "false"));
  expect(screen.queryByRole("button", { name: "Hide this suggestion" })).toBeNull();
});
