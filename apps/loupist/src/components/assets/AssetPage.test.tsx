import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { chooseSelectOption } from "@/test/selectOption";
import i18n from "@/i18n";
import { formatDay, formatTime } from "@/i18n/format";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { commandError } from "../../bridge/mock/commandError";
import { TooltipProvider } from "../ui/tooltip";
import type { ChatThread, LoupistSummary } from "../../types";
import type { AssetPageTarget } from "../../contracts/assets";
import { failedFixture, hanulFixture, nameOnlyFixture, needsConnectionFixture, noCurrencyFixture, noThesisFixture, nvidiaFixture, type AssetFixture } from "../../preview/assets";
import { AssetPage } from "./AssetPage";

const printReader = vi.hoisted(() => vi.fn(async (_source: HTMLElement, _title: string) => undefined));
vi.mock("../print/printDocuments", () => ({ printReader }));
import { InspectorThesis } from "./InspectorThesis";

/** Installs the fixture for the first Loupist and opens its page by id (or by `target`). */
async function fixture(asset: AssetFixture, { target, configure }: { target?: Partial<AssetPageTarget>; configure?: (bridge: MockLoupistBridge) => void } = {}) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList() as LoupistSummary[];
  bridge.assets.installAsset(asset.asset);
  if (asset.page) bridge.assets.install(loupist!.id, asset.asset.assetId, asset.page);
  if (asset.price) bridge.assets.installPrice(loupist!.id, asset.asset.assetId, asset.price);
  const threads: ChatThread[] = [{ id: "preview-chat", loupist_id: loupist!.id, title: "Hanul margin review", updated_at: "2026-10-28T00:00:00Z", use_memory: true, update_memory: true, messages: [] }];
  configure?.(bridge);
  const spies = {
    get: vi.spyOn(bridge, "assetPageGet"), check: vi.spyOn(bridge, "assetPriceCheck"), series: vi.spyOn(bridge, "assetPriceSeries"), seen: vi.spyOn(bridge, "assetPageSeen"),
  };
  const handlers = { onBack: vi.fn(), onAsk: vi.fn(), onOpenChat: vi.fn(), onOpenMap: vi.fn(), onOpenLink: vi.fn(), onOpenSource: vi.fn(), onOpenConnections: vi.fn() };
  render(<TooltipProvider><AssetPage bridge={bridge} loupist={loupist!} target={{ loupistId: loupist!.id, ...(target ?? { assetId: asset.asset.assetId }) }} threads={threads} {...handlers} /></TooltipProvider>);
  return { bridge, loupist: loupist!, handlers, spies, user: userEvent.setup() };
}

const opened = (name: string) => screen.findByRole("region", { name: i18n.t("assets:page.label", { name }) });
const quote = (page: HTMLElement) => page.querySelector<HTMLElement>(".asset-quote")!;

afterEach(() => { Reflect.deleteProperty(window.navigator, "onLine"); });

it("shows the stored price as the replayed result states it: value, currency, stated date, provider and its source", async () => {
  const asset = hanulFixture();
  const { spies, loupist, user } = await fixture(asset);
  const page = await opened("Hanul Precision");
  const price = quote(page);
  expect(price).toHaveAttribute("data-price-read", "ready");
  expect(price).toHaveTextContent("₩39,920");
  expect(price).toHaveTextContent(`As of ${formatDay(asset.price!.price!.date)} · KRX`);
  expect(within(price).getByRole("button", { name: /^Source 1/u })).toBeVisible();
  // This saved observation has no stated kind.
  expect(price).not.toHaveTextContent(/close|intraday/iu);
  // The listing's reference fields, as stored.
  expect(page.querySelector(".asset-page-meta")).toHaveTextContent("HNP·KS·Following");
  // Opening sends the id alone; the page counts as seen and reads its chart. It never checks on its own.
  expect(spies.get).toHaveBeenCalledWith({ loupistId: loupist.id, assetId: asset.asset.assetId });
  await waitFor(() => expect(spies.seen).toHaveBeenCalledWith({ loupistId: loupist.id, assetId: asset.asset.assetId }));
  await waitFor(() => expect(page.querySelector(".price-chart-line")).not.toBeNull());
  expect(spies.check).not.toHaveBeenCalled();
  await user.click(within(price).getByRole("button", { name: /^Source 1/u }));
  const dialog = await screen.findByRole("dialog", { name: "KRX" });
  expect(within(dialog).getByText(/Hanul Precision \(HNP\) · KRX · daily prices/)).toBeVisible();
  const details = within(dialog).getByText("Details").closest("details")!;
  expect(details.open).toBe(false);
  await user.click(within(dialog).getByText("Details"));
  expect(within(details).getByText(asset.price!.price!.dataset.dataset_id)).toBeVisible();
  expect(within(dialog).queryByText("Stored value")).toBeNull();
});

it.each([
  ["open", "Open", "시가"], ["high", "High", "고가"], ["low", "Low", "저가"],
  ["close", "Close", "종가"], ["adjusted_close", "Adjusted close", "수정 종가"],
  ["last", "Last", "최근 체결가"], ["bid", "Bid", "매수 호가"], ["ask", "Ask", "매도 호가"],
] as const)("shows the stated %s beside the quote date in both languages", async (kind, en, ko) => {
  const asset = nvidiaFixture();
  asset.price!.price!.kind = kind;
  asset.price!.price!.date = "2026-11-19";
  asset.asset.exchangeName = "Nasdaq Global Select Market";
  const { user } = await fixture(asset);
  const page = await opened("NVIDIA CORP");
  const line = quote(page);
  expect(line).toHaveTextContent(`${en} · Nov 19 · Yahoo Finance`);
  expect(page.querySelector(".asset-page-meta")).toHaveTextContent("NVDA·Nasdaq Global Select Market");
  await user.click(within(line).getByRole("button"));
  const dialog = await screen.findByRole("dialog", { name: "Yahoo Finance" });
  expect(dialog).toHaveAccessibleDescription(`${en} · Nov 19`);
  try {
    await act(() => i18n.changeLanguage("ko"));
    const date = formatDay("2026-11-19");
    expect(line).toHaveTextContent(`${ko} · ${date} · Yahoo Finance`);
    expect(dialog).toHaveAccessibleDescription(`${ko} · ${date}`);
  } finally { await act(() => i18n.changeLanguage("en")); }
});

it("opens the stored provider URL from the price source", async () => {
  const asset = hanulFixture();
  const sourceUrl = "https://data.krx.co.kr/price";
  asset.price!.price!.sourceUrl = sourceUrl;
  const { handlers, user } = await fixture(asset);
  await user.click(within(quote(await opened("Hanul Precision"))).getByRole("button", { name: /^Source 1/u }));
  expect(handlers.onOpenLink).toHaveBeenCalledWith(sourceUrl);
});

it("uses the localized listing country in the asset header, source and World inspector", async () => {
  const asset = hanulFixture();
  asset.asset.exchangeName = "South Korea";
  const { bridge, loupist, user } = await fixture(asset);
  const page = await opened("Hanul Precision");
  const meta = page.querySelector(".asset-page-meta");
  expect(meta).toHaveTextContent("South Korea");
  await user.click(within(quote(page)).getByRole("button", { name: /^Source 1/u }));
  const dialog = await screen.findByRole("dialog", { name: "KRX" });
  render(<InspectorThesis bridge={bridge} loupistId={loupist.id} entityId={asset.page!.entityId!} />);
  try {
    await act(() => i18n.changeLanguage("ko"));
    expect(meta).toHaveTextContent("대한민국");
    expect(dialog).toHaveTextContent("대한민국");
    await waitFor(() => expect(document.querySelector(".asset-thesis-summary")).toHaveTextContent("대한민국"));
  } finally { await act(() => i18n.changeLanguage("en")); }
});

it("words a provider's stated time and draws a year of history points", async () => {
  const asset = nvidiaFixture();
  await fixture(asset);
  const page = await opened("NVIDIA CORP");
  const stated = asset.price!.price!.date;
  expect(quote(page).querySelector("[data-price-date]")).toHaveTextContent(`As of ${formatDay(stated.slice(0, 10))} ${formatTime(Date.parse(stated))} · Yahoo Finance`);
  const ranges = await within(page).findByRole("group", { name: "Range" });
  expect(within(ranges).getAllByRole("button").map(button => button.textContent)).toEqual(["1W", "1M", "3M", "6M", "1Y"]);
  // A year of points opens at three months.
  expect(within(ranges).getByRole("button", { name: "3 months" })).toHaveAttribute("aria-pressed", "true");
});

it.each([[83_155_048, "83,155,048"], [1093.8299560546875, "1,093.83"], [0.000012345678912, "0.0000123457"]])("bounds the precision of a currencyless price %s and adds no display-currency line", async (value, expected) => {
  const asset = noCurrencyFixture();
  asset.price!.price!.value = value;
  await fixture(asset);
  const page = await opened("Harbor Fund");
  expect(quote(page).querySelector(".market-price")?.textContent).toBe(expected);
  expect(page.querySelector("[data-converted-amount]")).toBeNull();
  // Neither the result nor the listing states a currency: the line says so after the number.
  expect(quote(page).querySelector("[data-price-currency=unknown]")).toHaveTextContent("Currency unknown");
});

it("says the currency is unknown in Korean on the line and in the receipt", async () => {
  await i18n.changeLanguage("ko");
  try {
    const { user } = await fixture(noCurrencyFixture());
    const line = quote(await opened("Harbor Fund"));
    expect(line.querySelector("[data-price-currency=unknown]")).toHaveTextContent("통화 알 수 없음");
    await user.click(within(line).getByRole("button"));
    expect((await screen.findByRole("dialog")).querySelector("[data-price-currency=unknown]")).toHaveTextContent("통화 알 수 없음");
  } finally { await i18n.changeLanguage("en"); }
});

it("names the listing as the currency's source in the receipt when the result states none", async () => {
  const asset = nvidiaFixture();
  asset.price!.price = { ...asset.price!.price!, currency: "USD", currencySource: "listing" };
  const { user } = await fixture(asset);
  const line = quote(await opened("NVIDIA CORP"));
  expect(line.querySelector("[data-price-currency]")).toBeNull();
  await user.click(within(line).getByRole("button"));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByText("Currency")).toBeVisible();
  expect(dialog.querySelector("[data-price-currency=listing]")).toHaveTextContent(/^USD, from the listing on .+; the price result states none\.$/u);
});

it("shows the same localized quote in the receipt with its exact stored value below", async () => {
  const asset = nvidiaFixture();
  asset.price!.price!.value = 1093.8299560546875;
  await i18n.changeLanguage("ko");
  try {
    const { user } = await fixture(asset);
    const line = quote(await opened("NVIDIA CORP"));
    expect(line.querySelector(".market-price")).toHaveTextContent("US$1,093.83");
    await user.click(within(line).getByRole("button"));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("US$1,093.83")).toBeVisible();
    expect(within(dialog).getByText("저장된 값")).toBeVisible();
    expect(within(dialog).getByText("1093.8299560546875")).toBeVisible();
  } finally { await i18n.changeLanguage("en"); }
});

it("replays on Check now without reuse and shows what the replay stated", async () => {
  const asset = nvidiaFixture();
  const { spies, loupist, user } = await fixture(asset);
  const page = await opened("NVIDIA CORP");
  await user.click(within(page).getByRole("button", { name: "Check now" }));
  await waitFor(() => expect(quote(page)).toHaveTextContent("$160.42"));
  expect(spies.check).toHaveBeenCalledTimes(1);
  expect(spies.check).toHaveBeenCalledWith({ loupistId: loupist.id, assetId: asset.asset.assetId });
  // The `assets-changed` the replay emitted reread the page and never started another check.
  await waitFor(() => expect(spies.get).toHaveBeenCalledWith({ loupistId: loupist.id, assetId: asset.asset.assetId, storedOnly: true }));
  await waitFor(() => expect(spies.series).toHaveBeenCalledWith({ loupistId: loupist.id, assetId: asset.asset.assetId, storedOnly: true }));
  expect(spies.check).toHaveBeenCalledTimes(1);
});

it("without a saved read: says so, and Check now starts the Loupist's price setup until a price arrives", async () => {
  const asset = noThesisFixture();
  const { bridge, loupist, user } = await fixture(asset);
  const page = await opened("Corvane Systems");
  expect(quote(page).querySelector('[data-price-note="none"]')).toHaveTextContent(`No price yet. Select Check now or Find a price to have ${loupist.name} look for a way to read it. If it can't find one, it explains why in Bot.`);
  expect(page.querySelector(".price-chart")).toBeNull();
  await user.click(within(page).getByRole("button", { name: "Check now" }));
  const note = await waitFor(() => { const element = quote(page).querySelector<HTMLElement>('[data-price-note="setup"]'); expect(element).not.toBeNull(); return element!; });
  expect(note).toHaveTextContent(`${loupist.name} is looking for a way to read the price of Corvane Systems.`);
  expect(within(page).getByRole("button", { name: "Check now" })).toHaveAttribute("aria-disabled", "true");
  // The line offers no second, disabled way to do what is already running.
  expect(within(note).queryByRole("button")).toBeNull();
  expect(bridge.assets.setups.at(-1)).toEqual({ loupistId: loupist.id, assetIds: [asset.asset.assetId], reason: "check" });
  // The delegated run saved a read; the host's event brings the price and the note goes.
  bridge.assets.installPrice(loupist.id, asset.asset.assetId, { price: { value: 18.4, date: "2026-11-18", currency: "USD", currencySource: "result", provider: "fmp", sourceUrl: null, dataset: { dataset_id: "test-price", revision: 1, digest: "test" } }, savedRead: (await import("../../bridge/mock/assets")).mockSavedRead("ready") });
  act(() => bridge.assets.finishSetup(loupist.id, asset.asset.assetId));
  await waitFor(() => expect(quote(page)).toHaveTextContent("$18.40"));
  expect(quote(page).querySelector("[data-price-note]")).toBeNull();
  expect(within(page).getByRole("button", { name: "Check now" })).not.toHaveAttribute("aria-disabled");
});

it("directs a followed asset's Korean empty state to the price actions and Bot", async () => {
  await i18n.changeLanguage("ko");
  try {
    await fixture(noThesisFixture());
    const note = quote(await opened("Corvane Systems")).querySelector('[data-price-note="none"]');
    expect(note).toHaveTextContent("지금 확인 또는 가격 찾기를 누르면");
    expect(note).toHaveTextContent("방법을 찾지 못하면 봇에서 이유를 알려 드립니다.");
    expect(note).not.toHaveTextContent("관심 자산에 추가");
  } finally {
    await i18n.changeLanguage("en");
  }
});

it("tells the app header the name it shows: a Korean listing's Korean name in Korean", async () => {
  await i18n.changeLanguage("ko");
  try {
    const asset = hanulFixture();
    const onName = vi.fn();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const [loupist] = await bridge.loupistList() as LoupistSummary[];
    bridge.assets.installAsset(asset.asset);
    bridge.assets.install(loupist!.id, asset.asset.assetId, asset.page!);
    const map = await bridge.knowledgeMap({ loupistId: loupist!.id, maxNodes: 1000 });
    vi.spyOn(bridge, "knowledgeMap").mockResolvedValue({ ...map, nodes: [{ id: asset.page!.entityId!, kind: "entity", label: asset.asset.name, localNames: ["한울정밀"], entityKind: "Organization", degree: 1, watched: false }] });
    render(<TooltipProvider><AssetPage bridge={bridge} loupist={loupist!} target={{ loupistId: loupist!.id, assetId: asset.asset.assetId, name: asset.asset.name }} threads={[]}
      onBack={vi.fn()} onAsk={vi.fn()} onOpenChat={vi.fn()} onOpenMap={vi.fn()} onOpenLink={vi.fn()} onOpenSource={vi.fn()} onOpenConnections={vi.fn()} onName={onName} /></TooltipProvider>);
    expect(await screen.findByRole("heading", { level: 1, name: "한울정밀" })).toBeInTheDocument();
    await waitFor(() => expect(onName).toHaveBeenLastCalledWith("한울정밀"));
  } finally {
    await i18n.changeLanguage("en");
  }
});

it("keeps the last good price and tries setup only after the saved read fails again", async () => {
  const asset = failedFixture();
  const { bridge, loupist, user } = await fixture(asset);
  const page = await opened("Hanul Precision");
  expect(quote(page)).toHaveAttribute("data-price-read", "failed");
  expect(quote(page)).toHaveTextContent("₩39,920");
  expect(quote(page).querySelector('[data-price-note="failed"]')).toHaveTextContent("The latest read from");
  await user.click(within(quote(page)).getByRole("button", { name: "Try again" }));
  await waitFor(() => expect(bridge.assets.setups.at(-1)?.reason).toBe("check"));
  expect(within(page).getByRole("button", { name: "Check now" })).toHaveAttribute("aria-disabled", "true");
  act(() => bridge.assets.finishSetup(loupist.id, asset.asset.assetId));
  await waitFor(() => expect(within(page).getByRole("button", { name: "Check now" })).not.toHaveAttribute("aria-disabled"));
});

it("keeps focus in the price line when the action pressed there gives way to the setup note", async () => {
  const asset = failedFixture();
  const { user } = await fixture(asset);
  const page = await opened("Hanul Precision");
  const retry = within(quote(page)).getByRole("button", { name: "Try again" });
  await user.click(retry);
  const note = await waitFor(() => { const element = quote(page).querySelector<HTMLElement>('[data-price-note="setup"]'); expect(element).not.toBeNull(); return element!; });
  await waitFor(() => expect(note).toHaveFocus());
});

it("keeps focus on Check now while it checks", async () => {
  const { bridge, user, spies } = await fixture(nvidiaFixture(), { configure: bridge => { bridge.assets.checkHold = true; } });
  const page = await opened("NVIDIA CORP");
  const check = within(page).getByRole("button", { name: "Check now" });
  await user.click(check);
  await waitFor(() => expect(check).toHaveAttribute("aria-busy", "true"));
  expect(check).toHaveFocus();
  // A second press while busy sends nothing more.
  await user.click(check);
  expect(spies.check).toHaveBeenCalledTimes(1);
  bridge.assets.checkHold = false;
  await waitFor(() => expect(check).not.toHaveAttribute("aria-busy"));
});

it("names the Loupist when a failed saved read never stated a price", async () => {
  const asset = failedFixture();
  asset.price!.price = null;
  const { loupist } = await fixture(asset);
  const page = await opened("Hanul Precision");
  const note = quote(page).querySelector<HTMLElement>('[data-price-note="failed"]')!;
  expect(note).toHaveTextContent(`${loupist.name} couldn't read the price last time.`);
  expect(note).not.toHaveTextContent("{{");
  expect(within(note).getByRole("button", { name: "Try again" })).toBeVisible();
});

it("recovers a failed saved read with one replay and skips price setup", async () => {
  const asset = failedFixture();
  asset.price!.replay = "ready";
  const { bridge, user } = await fixture(asset);
  const page = await opened("Hanul Precision");
  await user.click(within(page).getByRole("button", { name: "Check now" }));
  await waitFor(() => expect(quote(page)).toHaveAttribute("data-price-read", "ready"));
  expect(bridge.assets.setups).toHaveLength(0);
});

it("shows Track daily for an unfollowed asset and says to follow it first", async () => {
  const asset = hanulFixture();
  asset.asset.followed = false;
  await fixture(asset);
  const page = await opened("Hanul Precision");
  expect(await within(page).findByText("Follow Hanul Precision to track it daily.")).toBeVisible();
  expect(within(page).getByRole("switch", { name: "Track daily" })).toBeDisabled();
});

it("asks the user to follow an unfollowed asset when checking its price", async () => {
  const asset = hanulFixture();
  asset.asset.followed = false;
  const { bridge, user } = await fixture(asset);
  const page = await opened("Hanul Precision");
  await user.click(within(page).getByRole("button", { name: "Check now" }));
  expect(await within(quote(page)).findByRole("status")).toHaveTextContent("Follow this asset to check its price.");
  expect(bridge.assets.setups).toHaveLength(0);
});

it("keeps the last price when the read's connection is off, and leads to Connections", async () => {
  const { handlers, user } = await fixture(needsConnectionFixture());
  const page = await opened("NVIDIA CORP");
  const note = quote(page).querySelector<HTMLElement>('[data-price-note="needs-connection"]')!;
  expect(note).toHaveTextContent("Market data is off for this Loupist, so this is the last price it read.");
  await user.click(within(note).getByRole("button", { name: "Open Connections" }));
  expect(handlers.onOpenConnections).toHaveBeenCalledTimes(1);
});

it("says when the Loupist has no AI account to find a price with", async () => {
  const { user, loupist } = await fixture(noThesisFixture(), { configure: bridge => {
    bridge.assetPriceCheck = () => Promise.reject(commandError("provider_unavailable", "No model is connected."));
  } });
  const page = await opened("Corvane Systems");
  await user.click(within(page).getByRole("button", { name: "Check now" }));
  // The settings page as the sidebar names it.
  expect(await within(page).findByRole("alert")).toHaveTextContent(`${loupist.name} needs an AI account to find a price. Connect one in Settings › AI accounts.`);
});

it("offline: shows the stored price and leaves Check now unavailable", async () => {
  Object.defineProperty(window.navigator, "onLine", { configurable: true, get: () => false });
  const { spies } = await fixture(hanulFixture());
  const page = await opened("Hanul Precision");
  expect(page.querySelector(".asset-page-offline")).toBeVisible();
  // "Needs a connection" would read as a data connection; the network is what is missing.
  expect(page.querySelector(".asset-page-offline")).toHaveTextContent("Check now works again when you're back online.");
  expect(within(page.querySelector(".asset-page-header")!).getByRole("button", { name: "Check now" })).toBeDisabled();
  expect(spies.check).not.toHaveBeenCalled();
});

it("opens a saved link by its ticker, or says why it cannot", async () => {
  const asset = hanulFixture();
  const { spies, loupist } = await fixture(asset, { target: { ticker: "HNP" } });
  await opened("Hanul Precision");
  expect(spies.get).toHaveBeenCalledWith({ loupistId: loupist.id, ticker: "HNP" });

  document.body.innerHTML = "";
  await fixture(asset, { target: { ticker: "GONE", name: "Gone Corp" } });
  expect(await screen.findByRole("status")).toHaveTextContent("Loupist doesn't track an asset with the ticker GONE.");

  document.body.innerHTML = "";
  await fixture(asset, { target: { ticker: "HNP" }, configure: bridge => bridge.assets.installAsset({ name: "Hanul Works", ticker: "HNP", exchange: "KQ" }) });
  expect(await screen.findByRole("status")).toHaveTextContent("More than one asset you follow has the ticker HNP.");
});

it("follows an unfollowed asset by its id", async () => {
  const asset = { ...noThesisFixture(), asset: { ...noThesisFixture().asset, followed: false } };
  const { bridge, loupist, user } = await fixture(asset);
  const follow = vi.spyOn(bridge, "assetFollow");
  const page = await opened("Corvane Systems");
  await user.click(within(page).getByRole("button", { name: "Follow" }));
  expect(follow).toHaveBeenCalledWith({ loupistId: loupist.id, assetId: asset.asset.assetId });
  await waitFor(() => expect(page.querySelector(".asset-page-meta")).toHaveTextContent("Following"));
});

it("undoes the Loupist's latest move by its id, and unlinks the page from its World entity", async () => {
  const asset = hanulFixture();
  const { bridge, loupist, user, handlers } = await fixture(asset);
  const undo = vi.spyOn(bridge, "assetVerdictUndo");
  const unlink = vi.spyOn(bridge, "assetEntitySet");
  const page = await opened("Hanul Precision");
  expect(within(page).getByRole("heading", { name: "Thesis history" }).parentElement).toHaveTextContent("Holding up → Weakening · the quarterly report shows operating margin 14.8%");
  await user.click(within(page).getByRole("button", { name: "Undo" }));
  await waitFor(() => expect(undo).toHaveBeenCalledWith({ loupistId: loupist.id, assetId: asset.asset.assetId, moveId: "preview-move-1" }));
  await waitFor(() => expect(within(page).queryByRole("button", { name: "Undo" })).toBeNull());

  expect(page.querySelector(".asset-lines-card")).toHaveTextContent("World · 2 suppliers, 3 customers");
  await user.click(within(page).getByRole("button", { name: "Open" }));
  expect(handlers.onOpenMap).toHaveBeenCalledWith("map-entity-00");
  await user.click(within(page).getByRole("button", { name: "Not this company" }));
  expect(unlink).toHaveBeenCalledWith({ loupistId: loupist.id, assetId: asset.asset.assetId, entityId: null });
  await waitFor(() => expect(page.querySelector(".asset-lines-card")).toBeNull());
});

it("puts a question into Bot's composer naming the listing, or the name alone without one", async () => {
  const { handlers, user } = await fixture(nvidiaFixture());
  const page = await opened("NVIDIA CORP");
  expect(within(page).getByRole("textbox", { name: "Ask about NVIDIA CORP" })).toHaveAttribute("placeholder", "Ask about NVIDIA CORP");
  await user.type(within(page).getByRole("textbox", { name: "Ask about NVIDIA CORP" }), "What moved it?{Enter}");
  expect(handlers.onAsk).toHaveBeenLastCalledWith("NVIDIA CORP (NVDA, US) What moved it?");

  document.body.innerHTML = "";
  const local = await fixture(nameOnlyFixture());
  const named = await opened(nameOnlyFixture().asset.name);
  await local.user.click(within(named).getByRole("button", { name: "What changed recently" }));
  expect(local.handlers.onAsk).toHaveBeenLastCalledWith(`${nameOnlyFixture().asset.name} What changed recently`);
  expect(named.querySelector(".asset-page-meta")).toHaveTextContent(/^Following$/u);
  // A long name: the heading shows it whole under the pointer, and the field's placeholder keeps its purpose in view.
  const longName = nameOnlyFixture().asset.name;
  expect(within(named).getByRole("heading", { level: 1 })).toHaveAttribute("title", longName);
  expect(within(named).getByRole("textbox", { name: `Ask about ${longName}` })).toHaveAttribute("placeholder", "Ask a question about it");
});

it("pauses Track daily when stopping following, with the option checked", async () => {
  const asset = nvidiaFixture();
  const { bridge, loupist, user } = await fixture(asset, { configure: bridge => {
    vi.spyOn(bridge, "scheduledTasksList").mockResolvedValue([{
      kind: "learning", id: "daily", loupist_id: "loupist-quality", revision: 4, title: "Track daily", prompt: "",
      schedule: { kind: "daily", hour: 7, minute: 0, timezone: "UTC" }, target: { kind: "new_chat" }, session_thread_id: null,
      model_profile_id: "", thinking_level: "", run_options: {}, use_memory: true, update_memory: false,
      status: "active", next_run_at_ms: null, last_run_at_ms: null, created_at_ms: 0, updated_at_ms: 0, last_error: null,
      awaiting_confirmation: false, latest_run: null, subjects: [{ asset: asset.asset.assetId }],
    }]);
  } });
  const status = vi.spyOn(bridge, "scheduledTaskSetStatus").mockResolvedValue({} as never);
  const unfollow = vi.spyOn(bridge, "assetUnfollow");
  const page = await opened("NVIDIA CORP");
  await user.click(await within(page).findByRole("button", { name: "Stop following NVIDIA CORP" }));
  const dialog = await screen.findByRole("dialog", { name: "Stop following NVIDIA CORP?" });
  expect(within(dialog).getByRole("checkbox", { name: "Also pause Track daily for NVIDIA CORP?" })).toBeChecked();
  await user.click(within(dialog).getByRole("button", { name: "Stop following" }));
  await waitFor(() => expect(status).toHaveBeenCalledWith({ loupist_id: loupist.id, task_id: "daily", expected_revision: 4, status: "paused" }));
  expect(unfollow).toHaveBeenCalledWith({ loupistId: loupist.id, assetId: asset.asset.assetId });
});

it("shows a loading state for price history while that read is still open", async () => {
  let release: (points: []) => void = () => {};
  const gate = new Promise<[]>(resolve => { release = resolve; });
  await fixture(nvidiaFixture(), { configure: bridge => { vi.spyOn(bridge, "assetPriceSeries").mockReturnValue(gate); } });
  const page = await opened("NVIDIA CORP");
  expect(page.querySelector(".price-chart")).toBeNull();
  expect(await within(page).findByText("Getting price history…")).toBeVisible();
  expect(page.querySelector(".asset-chart-skeleton")).toBeVisible();
  act(() => release([]));
  await waitFor(() => expect(page.querySelector(".asset-chart-skeleton")).toBeNull());
  expect(page).toHaveTextContent("No price history saved yet. Check now reads it.");
});

it("prints the drawn page and shows the briefing's error line when printing fails", async () => {
  const { user } = await fixture(nvidiaFixture());
  const page = await opened("NVIDIA CORP");
  await user.click(within(page).getByRole("button", { name: "Print or save as PDF…" }));
  expect(printReader).toHaveBeenCalledWith(page, expect.stringMatching(/^NVIDIA CORP · /));
  printReader.mockRejectedValueOnce(new Error("blocked"));
  await user.click(within(page).getByRole("button", { name: "Print or save as PDF…" }));
  expect(await within(page).findByRole("alert")).toHaveTextContent("Couldn’t open printing. Try again.");
});

it("withholds the verdict until a thesis has a recorded review", async () => {
  const asset = nameOnlyFixture();
  await fixture(asset);
  const page = await opened(asset.asset.name);
  expect(within(page).getByText("Not reviewed yet")).toBeVisible();
  expect(within(page).queryByText("Holding up")).toBeNull();
});

it("saves the direction picked for a thesis line", async () => {
  const asset = hanulFixture();
  const { bridge, user } = await fixture(asset);
  const create = vi.spyOn(bridge, "journalCreate");
  const page = await opened(asset.asset.name);
  await user.click(within(page).getByRole("button", { name: "Add a line" }));
  const form = within(page).getByRole("form", { name: "Add a line" });
  await user.type(within(form).getByRole("textbox", { name: "What to watch" }), "Margin falls");
  await user.type(within(form).getByRole("textbox", { name: "Metric" }), "Margin");
  await user.type(within(form).getByRole("spinbutton", { name: "Threshold" }), "50");
  await chooseSelectOption(user, within(form).getByRole("combobox", { name: "Direction" }), "Below");
  await user.click(within(form).getByRole("button", { name: "Save entry" }));
  await waitFor(() => expect(create).toHaveBeenCalledWith(expect.objectContaining({ check: expect.objectContaining({ direction: "below", threshold: 50 }) })));
});
