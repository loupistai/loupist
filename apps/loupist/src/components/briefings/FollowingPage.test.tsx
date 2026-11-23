import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "@/i18n";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { LoupistSummary } from "../../types";
import { AssetPageOpenerProvider, type OpenAssetPage } from "../assets/assetOpener";
import { App } from "../../App";
import { FollowingPage, type FollowingPageProps } from "./FollowingPage";

type Setup = (bridge: MockLoupistBridge, loupist: LoupistSummary) => Promise<unknown> | void;

async function fixture(setup?: Setup, props: Partial<FollowingPageProps> = {}) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList() as LoupistSummary[];
  await setup?.(bridge, loupist!);
  const lookup = vi.spyOn(bridge, "assetLookup");
  const followName = vi.spyOn(bridge, "assetFollowName");
  const openAsset = vi.fn<OpenAssetPage>();
  const view = render(<AssetPageOpenerProvider value={openAsset}><FollowingPage bridge={bridge} loupist={loupist!} {...props} /></AssetPageOpenerProvider>);
  await screen.findByTestId("following-page");
  await waitFor(() => expect(document.querySelector("[data-following-loading]")).toBeNull());
  return { bridge, loupist: loupist!, openAsset, view, lookup, followName, user: userEvent.setup() };
}
const NVIDIA = { name: "NVIDIA CORP", ticker: "NVDA", exchange: "US", figi: "BBG000BBJQV0", securityType: "Common Stock" };
const field = () => screen.getByRole("combobox", { name: "Find or add by name or ticker" });
const results = () => screen.getByRole("listbox", { name: "Search results" });
const options = () => within(results()).getAllByRole("option");
const row = (name: string) => screen.getByText(name, { selector: ".following-name" }).closest("li") as HTMLElement;
const settle = () => act(() => new Promise(resolve => setTimeout(resolve, 300)));
/** The line under the results for one lookup state, once it shows. */
const lookupLine = (state: string) => waitFor(() => { const line = document.querySelector<HTMLElement>(`[data-lookup="${state}"]`); expect(line).not.toBeNull(); return line!; });

afterEach(() => { cleanup(); Reflect.deleteProperty(window.navigator, "onLine"); });

it("lists each followed asset by its name and reference fields, or Name only, sorted by name", async () => {
  await fixture(bridge => {
    bridge.assets.installAsset({ name: "Zeta Instruments", ticker: "ZETA", exchange: "UW" });
    bridge.assets.installAsset(NVIDIA);
    bridge.assets.installAsset({ name: "Alpine office stake" });
  });
  const list = screen.getByRole("list", { name: "Followed assets" });
  expect(within(list).getAllByRole("listitem").map(item => item.querySelector(".following-name")?.textContent)).toEqual(["Alpine office stake", "NVIDIA CORP", "Zeta Instruments"]);
  expect(row("NVIDIA CORP").querySelector(".following-identity")).toHaveTextContent("NVDA · US · Common Stock");
  expect(row("Alpine office stake").querySelector(".following-identity")).toHaveTextContent("Name only");
  expect(within(row("Zeta Instruments")).getByRole("button", { name: "Stop following Zeta Instruments" })).toBeVisible();
  expect(screen.getByTestId("following-page")).not.toHaveTextContent(/\$|%|Alerts|Watching|Kind/iu);
});

it("shows each available stored price beside its followed asset", async () => {
  await fixture((bridge, loupist) => {
    const asset = bridge.assets.installAsset(NVIDIA);
    bridge.assets.installAsset({ name: "No stored quote" });
    bridge.assets.installPrice(loupist.id, asset.assetId, { price: {
      value: 1093.8299560546875, currency: "USD", currencySource: "result", date: "2026-11-19", provider: "yfinance",
      sourceUrl: null, dataset: { dataset_id: "quote", revision: 1, digest: "test" },
    } });
  });
  expect(await within(row("NVIDIA CORP")).findByText("$1,093.83")).toBeVisible();
  expect(row("No stored quote")).not.toHaveTextContent("$");
});

it("opens an asset page from a row by its id", async () => {
  const { openAsset, loupist, user, bridge } = await fixture(bridge => { bridge.assets.installAsset(NVIDIA); });
  const [asset] = await bridge.assetsList({ loupistId: loupist.id });
  await user.click(screen.getByRole("button", { name: /^NVIDIA CORP/u }));
  expect(openAsset).toHaveBeenCalledWith({ loupistId: loupist.id, assetId: asset!.assetId, name: "NVIDIA CORP" });
});

it("says when nothing is followed and focuses the field", async () => {
  await fixture();
  expect(screen.getByRole("heading", { name: "No assets followed" })).toBeVisible();
  await waitFor(() => expect(field()).toHaveFocus());
});

it("filters what is followed while typing and sends nothing until Enter", async () => {
  const { lookup, user } = await fixture(bridge => { bridge.assets.installAsset(NVIDIA); });
  await user.type(field(), "nvid");
  await settle();
  expect(options().map(item => item.dataset.kind)).toEqual(["followed", "search", "typed"]);
  expect(within(results()).getByRole("option", { name: "Open NVIDIA CORP, already followed" })).toBeVisible();
  expect(within(results()).getByRole("option", { name: "Search listings for nvid" })).toBeVisible();
  expect(lookup).not.toHaveBeenCalled();
  await user.keyboard("{Enter}");
  await waitFor(() => expect(lookup).toHaveBeenCalledTimes(1));
  expect(lookup).toHaveBeenCalledWith(expect.objectContaining({ query: "nvid" }));
});

it("searches listings once per submitted text, marks the followed one and keeps Follow as typed last", async () => {
  const { lookup, user } = await fixture(bridge => { bridge.assets.installAsset(NVIDIA); });
  await user.type(field(), "nvidia{Enter}");
  await screen.findByText("Listings from OpenFIGI. Your search text was sent to OpenFIGI.");
  // The followed listing shows once, as the followed match typing found.
  expect(options().map(item => [item.dataset.kind, item.dataset.figi ?? ""])).toEqual([["followed", ""], ["candidate", "BBG000BBJR79"], ["typed", ""]]);
  expect(within(results()).getByRole("option", { name: "Follow NVIDIA CORP, NVDA · UW · Common Stock" })).toBeVisible();
  // Enter again on the same answered text sends nothing more.
  await user.keyboard("{Enter}");
  await settle();
  expect(lookup).toHaveBeenCalledTimes(1);
});

it("follows a picked listing with its fields as the reference, clears the field and announces it", async () => {
  const { followName, loupist, user, bridge } = await fixture();
  await user.type(field(), "apple{Enter}");
  await user.click(await screen.findByRole("option", { name: "Follow APPLE INC, AAPL · US · Common Stock" }));
  expect(followName).toHaveBeenCalledWith({ loupistId: loupist.id, name: "APPLE INC", reference: { figi: "BBG000B9XRY4", ticker: "AAPL", exchange: "US", securityType: "Common Stock" } });
  await waitFor(() => expect(row("APPLE INC")).toBeVisible());
  expect(field()).toHaveValue("");
  expect(field()).toHaveFocus();
  expect(screen.getByText(`Following APPLE INC. ${loupist.name} looks for its price.`)).toBeInTheDocument();
  // The host starts one price setup for the burst of follows.
  expect(bridge.assets.setups.at(-1)?.reason).toBe("follow");
});

it("keeps the host's ranking and carries the selected exchange name into the followed row", async () => {
  const { followName, loupist, lookup, user } = await fixture();
  const first = { ...NVIDIA, name: "Zeta Corp", exchange: "UB", exchangeName: "Example Exchange", marketSector: "Equity" };
  const second = { ...NVIDIA, name: "Alpine Fund", figi: "BBG000BBJR79", marketSector: "Equity" };
  lookup.mockResolvedValue({ candidates: [first, second], savedKeyUnread: false });
  await user.type(field(), "NVDA{Enter}");
  const top = await screen.findByRole("option", { name: "Follow Zeta Corp, NVDA · Example Exchange · Common Stock" });
  expect(options().filter(item => item.dataset.kind === "candidate").map(item => item.dataset.figi)).toEqual([first.figi, second.figi]);
  await user.click(top);
  expect(followName).toHaveBeenCalledWith({ loupistId: loupist.id, name: first.name,
    reference: { figi: first.figi, ticker: first.ticker, exchange: "UB", exchangeName: "Example Exchange", securityType: first.securityType } });
  expect(row(first.name).querySelector(".following-identity")).toHaveTextContent("NVDA · Example Exchange · Common Stock");
});

it("localizes generic listing labels and accessible names while saving the provider fields", async () => {
  await i18n.changeLanguage("ko");
  try {
    const { followName, loupist, lookup, user } = await fixture();
    const domestic = {
      name: "SAMSUNG ELECTRONICS CO LTD", ticker: "005930", figi: "BBG000BDTF76",
      exchange: "KS", exchangeName: "South Korea", securityType: "Common Stock",
    };
    const otc = {
      ...domestic, ticker: "00593EUR", figi: "BBG019T9FJM3",
      exchange: "XL", exchangeName: "TradEcho-LSE OTC", securityType: "Provider Type",
    };
    lookup.mockResolvedValue({ candidates: [{ ...domestic, marketSector: "Equity" }, { ...otc, marketSector: "Equity" }], savedKeyUnread: false });
    await user.type(screen.getByRole("combobox"), "samsung{Enter}");
    const country = await screen.findByRole("option", { name: /005930 · 대한민국 · 보통주/u });
    expect(country).toHaveTextContent("005930 · 대한민국 · 보통주");
    expect(screen.getByRole("option", { name: /00593EUR · TradEcho-LSE OTC · Provider Type/u })).toBeVisible();
    await user.click(country);
    const { name, ...reference } = domestic;
    expect(followName).toHaveBeenCalledWith({ loupistId: loupist.id, name, reference });
    expect(row(name).querySelector(".following-identity")).toHaveTextContent("005930 · 대한민국 · 보통주");
    await act(async () => { await i18n.changeLanguage("en"); });
    expect(row(name).querySelector(".following-identity")).toHaveTextContent("005930 · South Korea · Common Stock");
  } finally {
    cleanup();
    await i18n.changeLanguage("en");
  }
});

it("follows the text as typed by name alone, from the last option or with the arrows and Enter", async () => {
  const { followName, loupist, lookup, user } = await fixture();
  await user.type(field(), "Seongsu office stake");
  await user.keyboard("{ArrowDown}{ArrowDown}");
  expect(field().getAttribute("aria-activedescendant")).toBe(within(results()).getByRole("option", { name: "Follow Seongsu office stake as typed" }).id);
  await user.keyboard("{Enter}");
  expect(followName).toHaveBeenCalledWith({ loupistId: loupist.id, name: "Seongsu office stake" });
  await waitFor(() => expect(row("Seongsu office stake").querySelector(".following-identity")).toHaveTextContent("Name only"));
  expect(lookup).not.toHaveBeenCalled();
});

it.each([NVIDIA, { name: "NVIDIA CORP" }])("opens the followed asset from the exact-name typed option ($figi)", async asset => {
  const { bridge, followName, lookup, loupist, openAsset, user } = await fixture(bridge => {
    bridge.assets.installAsset(asset);
  });
  const [existing] = await bridge.assetsList({ loupistId: loupist.id });
  await user.type(field(), "  nvidia corp  ");
  const typed = options().at(-1);
  expect(typed).toHaveAttribute("data-kind", "typed");
  expect(typed).toHaveAccessibleName("Open NVIDIA CORP, already followed");
  expect(typed).toHaveTextContent("Already followed");
  await user.keyboard("{ArrowUp}");
  expect(field()).toHaveAttribute("aria-activedescendant", typed!.id);
  await user.keyboard("{Enter}");
  expect(openAsset).toHaveBeenCalledWith({ loupistId: loupist.id, assetId: existing!.assetId, name: asset.name });
  expect(followName).not.toHaveBeenCalled();
  expect(lookup).not.toHaveBeenCalled();
});

it("shows Adding on the picked row, and on failure keeps the query and results with Retry", async () => {
  const { bridge, user } = await fixture();
  await user.type(field(), "apple{Enter}");
  const pick = await screen.findByRole("option", { name: /^Follow APPLE INC/u });
  let fail: (error: Error) => void = () => {};
  bridge.assetFollowName = () => new Promise((_, reject) => { fail = reject; });
  await user.click(pick);
  expect(pick).toHaveAttribute("data-pick", "adding");
  act(() => fail(new Error("locked")));
  await waitFor(() => expect(pick).toHaveAttribute("data-pick", "failed"));
  expect(pick).toHaveTextContent("Couldn't add · Try again");
  expect(field()).toHaveValue("apple");
});

it("says no listing matched, keeps Follow as typed, and Ask Bot drafts the request", async () => {
  const onAskInChat = vi.fn();
  const { user } = await fixture(undefined, { onAskInChat });
  await user.type(field(), "Hanbit Heavy");
  // Typing only filters; the box says plainly that Enter searches listings.
  expect(screen.getByText("Press Enter to search listings on OpenFIGI.")).toBeVisible();
  await user.keyboard("{Enter}");
  expect(await screen.findByText("No listing matched. OpenFIGI matches English names and tickers, such as Apple or AAPL. You can still follow it as typed.")).toBeVisible();
  expect(options().map(item => item.dataset.kind)).toEqual(["typed"]);
  await user.click(screen.getByRole("button", { name: "Ask Bot to follow it" }));
  expect(onAskInChat).toHaveBeenCalledWith("Follow “Hanbit Heavy”.");
});

it("words each lookup failure with the one thing to do about it", async () => {
  const onOpenConnections = vi.fn();
  const { bridge, user, lookup } = await fixture(undefined, { onOpenConnections });
  bridge.assets.lookupNeedsConnection = true;
  await user.type(field(), "nvidia{Enter}");
  expect(await lookupLine("connection")).toHaveTextContent("Asset lookup is off for this Loupist. Turn it on in Connections to search listings.");
  await user.click(screen.getByRole("button", { name: "Open Connections" }));
  expect(onOpenConnections).toHaveBeenCalledWith("openfigi.lookup");

  bridge.assets.lookupNeedsConnection = false;
  bridge.assets.lookupRateLimited = true;
  await user.clear(field());
  await user.type(field(), "nvidia{Enter}");
  expect(await lookupLine("busy")).toHaveTextContent("OpenFIGI asked to wait. Try again in a minute.");
  bridge.assets.lookupRateLimited = false;
  await user.click(screen.getByRole("button", { name: "Try again" }));
  await waitFor(() => expect(options().filter(item => item.dataset.kind === "candidate")).toHaveLength(2));
  expect(lookup).toHaveBeenCalledTimes(3);
});

it("sends a rejected saved key to Connections, not to Try again", async () => {
  const onOpenConnections = vi.fn();
  const { lookup, user } = await fixture(undefined, { onOpenConnections });
  lookup.mockRejectedValueOnce({ code: "credential_rejected", message: "OpenFIGI rejected the key." });
  await user.type(field(), "nvidia{Enter}");
  const line = await lookupLine("keyRejected");
  expect(line).toHaveTextContent("OpenFIGI rejected the saved API key. Replace or remove it in Connections.");
  // Searching again would fail the same way until the key is fixed.
  expect(within(line).queryByRole("button", { name: "Try again" })).toBeNull();
  await user.click(within(line).getByRole("button", { name: "Open Connections" }));
  expect(onOpenConnections).toHaveBeenCalledWith("openfigi.lookup");
});

it("lists the public route's listings and notes a saved key it could not read", async () => {
  const onOpenConnections = vi.fn();
  const { bridge, user } = await fixture(undefined, { onOpenConnections });
  bridge.assets.lookupSavedKeyUnread = true;
  await user.type(field(), "nvidia{Enter}");
  await waitFor(() => expect(options().filter(item => item.dataset.kind === "candidate")).toHaveLength(2));
  expect(await lookupLine("done")).toHaveTextContent("Listings from OpenFIGI.");
  const note = document.querySelector<HTMLElement>("[data-lookup-key=unread]")!;
  expect(note).toHaveTextContent("Loupist can't use the saved OpenFIGI API key. Allow access to it, or save it again, in Connections.");
  expect(within(note).queryByRole("button", { name: "Try again" })).toBeNull();
  await user.click(within(note).getByRole("button", { name: "Open Connections" }));
  expect(onOpenConnections).toHaveBeenCalledWith("openfigi.lookup");

  // Without a saved key there is nothing to report. (The same text is never sent twice.)
  bridge.assets.lookupSavedKeyUnread = false;
  await user.clear(field());
  await user.type(field(), "nvda{Enter}");
  await waitFor(() => expect(options().filter(item => item.dataset.kind === "candidate").length).toBeGreaterThan(0));
  expect(document.querySelector("[data-lookup-key=unread]")).toBeNull();
});

it("describes the field with what typing and Enter do, and shows a long name whole on hover", async () => {
  const long = "Northern Lights Hydroelectric Infrastructure and Transmission Holdings Corporation";
  await fixture(bridge => { bridge.assets.installAsset({ name: long, ticker: "NLHT", exchange: "US" }); });
  expect(field()).toHaveAccessibleDescription("Typing filters what you follow. Press Enter to search listings on OpenFIGI.");
  expect(field()).not.toHaveAttribute("aria-controls");
  expect(row(long).querySelector(".following-name")).toHaveAttribute("title", long);
});

it("sends nothing offline and says so", async () => {
  Object.defineProperty(window.navigator, "onLine", { configurable: true, get: () => false });
  const { lookup, user } = await fixture();
  await user.type(field(), "nvidia{Enter}");
  expect(await lookupLine("offline")).toHaveTextContent("Listing search is unavailable offline. You can still follow it as typed.");
  expect(lookup).not.toHaveBeenCalled();
});

it("leaves Enter to the input method while it composes", async () => {
  const { lookup } = await fixture();
  const input = field();
  fireEvent.compositionStart(input);
  fireEvent.change(input, { target: { value: "엔비디아" } });
  fireEvent.keyDown(input, { key: "Enter", isComposing: true });
  await settle();
  expect(lookup).not.toHaveBeenCalled();
  fireEvent.compositionEnd(input);
  fireEvent.keyDown(input, { key: "Enter" });
  await waitFor(() => expect(lookup).toHaveBeenCalledTimes(1));
});

it("drops an answer for text that was cleared before it arrived", async () => {
  const { bridge, user } = await fixture();
  bridge.assets.lookupHold = true;
  await user.type(field(), "nvidia{Enter}");
  expect(await screen.findByText("Searching listings on OpenFIGI…")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Clear search" }));
  bridge.assets.lookupHold = false;
  await settle();
  await user.type(field(), "nvidia");
  expect(options().map(item => item.dataset.kind)).toEqual(["search", "typed"]);
});

it("stops following at once, moving focus to the next row, then the field; a failure says so and retries", async () => {
  const { bridge, user, loupist } = await fixture(bridge => {
    bridge.assets.installAsset({ name: "Alpha", ticker: "ALPH" });
    bridge.assets.installAsset({ name: "Beta", ticker: "BETA" });
  });
  const unfollow = vi.spyOn(bridge, "assetUnfollow");
  await user.click(screen.getByRole("button", { name: "Stop following Alpha" }));
  await waitFor(() => expect(screen.queryByText("Alpha", { selector: ".following-name" })).toBeNull());
  expect(unfollow).toHaveBeenCalledWith({ loupistId: loupist.id, assetId: expect.stringMatching(/^ast_[0-9a-f]{16}$/u) });
  await waitFor(() => expect(within(row("Beta")).getByRole("button", { name: /^Beta/u })).toHaveFocus());
  bridge.assets.unfollowFails = true;
  await user.click(screen.getByRole("button", { name: "Stop following Beta" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't stop following Beta.");
  bridge.assets.unfollowFails = false;
  await user.click(within(row("Beta")).getByRole("button", { name: "Try again" }));
  await waitFor(() => expect(field()).toHaveFocus());
  expect(await bridge.assetsList({ loupistId: loupist.id })).toEqual([]);
});

it("reads the list again when the host says assets changed", async () => {
  const { bridge } = await fixture();
  bridge.assets.installAsset(NVIDIA);
  act(() => bridge.assets.changed());
  expect(await screen.findByText("NVIDIA CORP", { selector: ".following-name" })).toBeVisible();
});

describe("in the app", () => {
  it("opens from Bot's Add assets to follow card with the field focused, and Ask Bot drafts into the composer", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const user = userEvent.setup();
    render(<App bridge={bridge} />);
    const card = await waitFor(() => {
      const button = document.querySelector<HTMLButtonElement>('.bot-greeting [data-ask="follow"]');
      expect(button).not.toBeNull();
      return button!;
    }, { timeout: 4000 });
    await user.click(card);
    await waitFor(() => expect(field()).toHaveFocus());
    await user.type(field(), "Hanbit Heavy{Enter}");
    await user.click(await screen.findByRole("button", { name: "Ask Bot to follow it" }, { timeout: 2000 }));
    await waitFor(() => expect(document.querySelector<HTMLTextAreaElement>("textarea[data-chat-composer]")?.value).toContain("Follow “Hanbit Heavy”."));
    const [loupist] = await bridge.loupistList();
    expect(await bridge.assetsList({ loupistId: loupist!.id })).toEqual([]);
  });
});
