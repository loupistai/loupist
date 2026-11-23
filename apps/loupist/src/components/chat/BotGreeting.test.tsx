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
    { assetId: NVIDIA, price: { value: 162.85, date: "2026-11-13", currency: "USD", currencySource: "result", provider: "yfinance", sourceUrl: null, dataset: { dataset_id: "a", revision: 1, digest: "test" } } },
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
  // A stated currency needs no marker.
  expect(tile.querySelector("[data-price-currency]")).toBeNull();
});

it("names and prices a tile as the asset page does: a Korean listing's Korean name and every stated digit", async () => {
  await i18n.changeLanguage("ko");
  try {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const hanul = `ast_${"4".repeat(16)}`;
    vi.spyOn(bridge, "assetsList").mockResolvedValue([{ assetId: hanul, name: "Hanul Precision", reference: { figi: null, ticker: "HNP", exchange: "KS", securityType: null, origin: "imported" }, followedAtMs: 0, createdAtMs: 0 }]);
    // A price nothing states a currency for keeps its own precision and says the currency is unknown, as on the asset page.
    vi.spyOn(bridge, "watchlistQuotes").mockResolvedValue([
      { assetId: hanul, price: { value: 39920, date: "2026-11-13", currency: null, currencySource: "none", provider: "krx", sourceUrl: null, dataset: { dataset_id: "a", revision: 1, digest: "test" } } },
    ]);
    vi.spyOn(bridge, "principleList").mockResolvedValue([]);
    const map = await bridge.knowledgeMap({ loupistId: loupist.id, maxNodes: 1000 });
    vi.spyOn(bridge, "knowledgeMap").mockResolvedValue({ ...map, nodes: [{ id: "e-hanul", kind: "entity", label: "Hanul Precision", localNames: ["한울정밀"], entityKind: "Organization", degree: 1, watched: false }] });
    renderWithOpen(<BotGreeting bridge={bridge} loupistId={loupist.id} loupist={loupist} onAsk={vi.fn()} />);
    const tile = await screen.findByRole("button", { name: "한울정밀 열기" });
    expect(tile.querySelector(".bot-tile-name")).toHaveTextContent("한울정밀");
    expect(tile.querySelector(".bot-tile-price")!.textContent).toBe("39,920");
    expect(tile.querySelector("[data-price-currency=unknown]")).toHaveTextContent("통화 알 수 없음");
    expect(tile).toHaveAccessibleDescription(/^39,920 통화 알 수 없음 /u);
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

describe("the greeting follows the Mac's local clock", () => {
  const at = (hour: number, minute: number) => new Date(2026, 10, 23, hour, minute).getTime();
  const shownGreeting = async () => {
    const region = await screen.findByRole("region", { name: "Quality Compounder greets you" });
    return region.querySelector(".bot-greeting-top strong")!.textContent;
  };
  afterEach(() => vi.restoreAllMocks());

  it.each([
    [0, 0, "What would you like to research?"], [3, 38, "What would you like to research?"], [4, 59, "What would you like to research?"],
    [5, 0, "Good morning"], [11, 59, "Good morning"], [12, 0, "Good afternoon"], [17, 59, "Good afternoon"],
    [18, 0, "Good evening"], [23, 59, "Good evening"],
  ])("at %i:%i it reads %s", async (hour, minute, expected) => {
    vi.spyOn(Date, "now").mockReturnValue(at(hour, minute));
    renderWithOpen(<BotGreeting bridge={setup()} loupistId={loupist.id} loupist={loupist} onAsk={vi.fn()} />);
    expect(await shownGreeting()).toBe(expected);
  });

  it("greets anew each time Bot is shown", async () => {
    const now = vi.spyOn(Date, "now").mockReturnValue(at(23, 50));
    const bridge = setup();
    const view = renderWithOpen(<BotGreeting bridge={bridge} loupistId={loupist.id} loupist={loupist} onAsk={vi.fn()} refreshRevision={0} />);
    expect(await shownGreeting()).toBe("Good evening");
    now.mockReturnValue(at(7, 30));
    view.rerender(<BotGreeting bridge={bridge} loupistId={loupist.id} loupist={loupist} onAsk={vi.fn()} refreshRevision={1} />);
    await waitFor(async () => expect(await shownGreeting()).toBe("Good morning"));
  });

  it("uses Korean's neutral line rather than an evening greeting after midnight", async () => {
    vi.spyOn(Date, "now").mockReturnValue(at(3, 38));
    await i18n.changeLanguage("ko");
    try {
      renderWithOpen(<BotGreeting bridge={setup()} loupistId={loupist.id} loupist={loupist} onAsk={vi.fn()} />);
      const region = await screen.findByRole("region", { name: "Quality Compounder의 인사" });
      expect(region.querySelector(".bot-greeting-top strong")).toHaveTextContent("무엇을 리서치하시겠습니까?");
    } finally { await i18n.changeLanguage("en"); }
  });
});
