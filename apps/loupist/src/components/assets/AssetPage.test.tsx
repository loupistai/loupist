import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "@/i18n";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { TooltipProvider } from "../ui/tooltip";
import type { ChatThread, LoupistSummary } from "../../types";
import { cryptoFixture, hanulFixture, noThesisFixture, partialFixture, privateFixture, type AssetFixture } from "../../preview/assets";
import { capitalRowsFixture, koreaDepthFixture, koreaNeedsKeyFixture } from "../../preview/korea";
import { AssetPage } from "./AssetPage";

async function fixture(asset: AssetFixture, visitedDaysAgo?: number) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList() as LoupistSummary[];
  bridge.assets.install(loupist!.id, asset.page, asset.delta);
  if (visitedDaysAgo !== undefined) localStorage.setItem("loupist.assetVisit.v1", JSON.stringify({ [`${loupist!.id}:${asset.page.assetId}`]: Date.now() - visitedDaysAgo * 86_400_000 }));
  const threads: ChatThread[] = [{ id: "preview-chat", loupist_id: loupist!.id, title: "Hanul margin review", updated_at: "2026-10-28T00:00:00Z", use_memory: true, update_memory: true, messages: [] }];
  const handlers = { onBack: vi.fn(), onAsk: vi.fn(), onOpenChat: vi.fn(), onOpenMap: vi.fn(), onOpenLink: vi.fn(), onOpenSource: vi.fn(), onOpenConnections: vi.fn(), onAddSource: vi.fn() };
  render(<TooltipProvider><AssetPage bridge={bridge} loupist={loupist!} target={{ loupistId: loupist!.id, symbol: asset.page.symbol, market: asset.page.market }} threads={threads} {...handlers} /></TooltipProvider>);
  const page = await screen.findByRole("region", { name: `${asset.page.name} asset page` });
  return { bridge, loupist: loupist!, handlers, page, user: userEvent.setup() };
}

afterEach(() => localStorage.clear());

it("shows the read, the user's reasons and lines, and changes that each carry a date and a source", async () => {
  const { loupist, page } = await fixture(hanulFixture(), 3);
  expect(within(page).getByRole("img", { name: `${loupist.name}'s read: Weakening, step 2 of 4` })).toBeVisible();
  expect(within(page).getByText(/Weakening since .* — the quarterly report shows operating margin 14\.8%/)).toBeVisible();
  const reasons = within(page).getByRole("heading", { name: "Why you follow it" }).nextElementSibling as HTMLElement;
  expect(within(reasons).getAllByRole("listitem")).toHaveLength(2);
  expect(within(page).getByText(/Not crossed · last 21% \(/)).toBeVisible();
  await waitFor(() => expect(page.querySelectorAll(".asset-changes .asset-change").length).toBeGreaterThanOrEqual(4));
  const rows = page.querySelectorAll(".asset-changes .asset-change");
  for (const row of rows) expect(row.querySelector(".loupist-citation")).not.toBeNull();
  expect(within(page).getByText("Risk factors: 2 added · 1 removed · 1 changed")).toBeVisible();
  // The DART report reads in English and names its form once.
  expect(within(page).getByText(/^Quarterly report \(2026\.09\) filed/)).toBeVisible();
  // One number per source across the page: the filing behind the read keeps its number in What changed.
  const chips = [...page.querySelectorAll(".loupist-citation")].map(chip => chip.textContent);
  expect(page.querySelector(".asset-read-sentence .loupist-citation")?.textContent).toBe("1");
  expect(rows[0]!.querySelector(".loupist-citation")?.textContent).toBe("1");
  expect(Math.max(...chips.map(Number))).toBe(new Set(chips).size);
  expect(within(page).getByText(/Operating margin 14\.8% for Q3 2026, from 16\.2%/)).toBeVisible();
  expect(within(page).getByRole("button", { name: "Since you last looked" })).toHaveAttribute("aria-pressed", "true");
  expect(within(page).getByText("AI can be wrong. Not a licensed financial adviser.")).toBeVisible();
});

it("undoes the Loupist's latest move by its id, even after a refresh changed the page", async () => {
  const { bridge, loupist, page, user } = await fixture(hanulFixture(), 3);
  const undo = vi.spyOn(bridge, "assetVerdictUndo");
  // Check now bumps the page's revision; it must not stale the Undo.
  await user.click(within(page).getByRole("button", { name: "Check now" }));
  await waitFor(() => expect(within(page).getByRole("button", { name: "Check now" })).toBeEnabled());
  await user.click(within(page).getByRole("button", { name: "Undo" }));
  await waitFor(() => expect(undo).toHaveBeenCalledWith({ loupistId: loupist.id, assetId: hanulFixture().page.assetId, moveId: "preview-move-1", sinceMs: expect.any(Number) }));
  expect(await within(page).findByRole("img", { name: `${loupist.name}'s read: Holding up, step 1 of 4` })).toBeVisible();
  expect(within(page).getByText(/Back to Holding up · you undid the .* change/)).toBeVisible();
  expect(within(page).queryByRole("button", { name: "Undo" })).toBeNull();
  expect(within(page).queryByRole("alert")).toBeNull();
});

it("sends Rust only the request fields its asset commands accept", async () => {
  // Rust's request structs deny unknown fields; its test deserializes these same examples.
  const examples = (await import("../../contracts/assetRequests.example.json")).default as Record<string, Record<string, unknown>[]>;
  const { bridge, page, user } = await fixture(hanulFixture(), 3);
  const calls: [string, Record<string, unknown>][] = [];
  const methods = { assetPageGet: "asset_page_get", assetDelta: "asset_delta", assetRefresh: "asset_refresh", assetVerdictUndo: "asset_verdict_undo", assetPageSeen: "asset_page_seen", assetClassSet: "asset_class_set" } as const;
  for (const [method, command] of Object.entries(methods)) {
    const original = (bridge[method as keyof typeof methods] as (request: unknown) => Promise<unknown>).bind(bridge);
    vi.spyOn(bridge, method as keyof typeof methods).mockImplementation(async (request: unknown) => { calls.push([command, request as Record<string, unknown>]); return original(request) as never; });
  }
  await user.click(within(page).getByRole("button", { name: "Check now" }));
  await waitFor(() => expect(within(page).getByRole("button", { name: "Check now" })).toBeEnabled());
  await user.click(within(page).getByRole("button", { name: "Undo" }));
  await waitFor(() => expect(calls.some(([command]) => command === "asset_verdict_undo")).toBe(true));
  expect(new Set(calls.map(([command]) => command))).toEqual(new Set(["asset_refresh", "asset_delta", "asset_verdict_undo"]));
  for (const [command, request] of calls) {
    const accepted = new Set((examples[command] ?? []).flatMap(example => Object.keys(example)));
    const sent = Object.entries(request).filter(([, value]) => value !== undefined).map(([key]) => key);
    expect(sent.filter(key => !accepted.has(key)), command).toEqual([]);
  }
});

it("says so inline when Check now, Undo or Follow fails", async () => {
  const { bridge, page, user } = await fixture(hanulFixture(), 3);
  bridge.assets.refreshFails = true;
  await user.click(within(page).getByRole("button", { name: "Check now" }));
  expect(await within(page).findByRole("alert")).toHaveTextContent("Couldn't check Hanul Precision now. Try again in a moment.");
  bridge.assets.undoFails = true;
  await user.click(within(page).getByRole("button", { name: "Undo" }));
  expect(await within(page).findByRole("alert")).toHaveTextContent("Couldn't undo this change. Try again.");
  // A newer move than the one shown: the page reloads and says why.
  bridge.assets.undoFails = false;
  vi.spyOn(bridge, "assetVerdictUndo").mockRejectedValueOnce(Object.assign(new Error("A newer change was made."), { code: "conflict" }));
  await user.click(within(page).getByRole("button", { name: "Undo" }));
  expect(await within(page).findByRole("alert")).toHaveTextContent("A newer change was made, so this one can't be undone.");

  const notFollowed = noThesisFixture();
  notFollowed.page.watch = "none";
  const unfollowed = await fixture(notFollowed);
  vi.spyOn(unfollowed.bridge, "watchlistAdd").mockRejectedValueOnce(new Error("store failed"));
  await unfollowed.user.click(within(unfollowed.page).getByRole("button", { name: "Follow" }));
  expect(await within(unfollowed.page).findByRole("alert")).toHaveTextContent("Couldn't add Morrow Components to what you follow. Try again.");
});

it("asks for a first reason and writes it as a Journal entry tagged with the asset", async () => {
  const { bridge, loupist, page, user, handlers } = await fixture(noThesisFixture());
  expect(within(page).getByText(/You haven't written why you follow Morrow Components/)).toBeVisible();
  expect(within(page).queryByRole("img", { name: /read:/ })).toBeNull();
  expect(within(page).getByText("First look · showing the last 30 days")).toBeVisible();
  await user.click(within(page).getByRole("button", { name: "Work it out in Chat" }));
  expect(handlers.onAsk).toHaveBeenCalledWith("Help me write down why I follow $MORR and what would change my mind.");
  const create = vi.spyOn(bridge, "journalCreate");
  await user.click(within(page).getAllByRole("button", { name: "Add a reason" })[0]!);
  await user.type(within(page).getByRole("textbox", { name: "Your reason" }), "Morrow keeps winning HBM substrate qualifications.");
  await user.click(within(page).getByRole("button", { name: "Save entry" }));
  await waitFor(() => expect(create).toHaveBeenCalledWith(expect.objectContaining({ loupistId: loupist.id, judgment: "Morrow keeps winning HBM substrate qualifications.", thesis: { assetId: noThesisFixture().page.assetId, role: "reason" } })));
  expect(await within(page).findByText("Morrow keeps winning HBM substrate qualifications.")).toBeVisible();
  expect(within(page).getByRole("img", { name: `${loupist.name}'s read: Holding up, step 1 of 4` })).toBeVisible();
});

it("says what the last check read, what failed, what needs a connection and what it cannot check yet", async () => {
  const { page, user, handlers } = await fixture(partialFixture(), 3);
  // Only what was read is named as a source; the switched-off connection is not.
  expect(within(page).getByText("Filings · from OpenDART")).toBeVisible();
  expect(within(page).getByText("Not checked yet: credit balances, short-sale balances, lock-up schedules")).toBeVisible();
  expect(within(page).getByText(/Couldn't check results · OpenDART didn't answer/)).toBeVisible();
  // Values the check could not refresh keep their date.
  const glance = within(page).getByText("At a glance").parentElement!;
  expect(within(glance).getByText("Price").nextElementSibling).toHaveTextContent(/as of /);
  expect(within(glance).getByText("Price").nextElementSibling).toHaveAttribute("data-stale");
  expect(within(glance).getByText("Last filing").nextElementSibling).not.toHaveAttribute("data-stale");
  await user.click(within(page).getByRole("button", { name: "Turn on KRX in Connections to check price" }));
  expect(handlers.onOpenConnections).toHaveBeenCalled();
});

it("follows a private company through Sources and a crypto asset through price and drawdown", async () => {
  const privatePage = await fixture(privateFixture());
  expect(within(privatePage.page).getByText("Nothing to check automatically for this kind of asset. Add sources to follow it.")).toBeVisible();
  // Nothing to check automatically: Check now is off, and the kind of asset is named once.
  expect(within(privatePage.page).getByRole("button", { name: "Check now" })).toBeDisabled();
  expect(privatePage.page.querySelector(".asset-page-meta")).not.toHaveTextContent(/PRIVATE/);
  expect(within(privatePage.page).queryByRole("heading", { name: "Thesis history" })).toBeNull();
  await privatePage.user.click(within(privatePage.page).getByRole("button", { name: "Add a source" }));
  expect(privatePage.handlers.onAddSource).toHaveBeenCalled();
});

it("shows the crypto page's drawdown and what token data is not checked", async () => {
  const { page } = await fixture(cryptoFixture(), 2);
  expect(within(page).getByText("From 90-day high")).toBeVisible();
  expect(within(page).getByText("Not checked yet: token unlocks, TVL, governance, exchange flows")).toBeVisible();
});

it("shows a Korean stock's investor flows as calculations with their lag, never in price colors", async () => {
  const { page } = await fixture(koreaDepthFixture(), 3);
  const flows = within(page).getByRole("region", { name: "Investor flows" });
  expect(within(flows).getByText(/^10 trading days to /u)).toBeVisible();
  const rows = flows.querySelectorAll(".asset-flows-rows > div");
  expect([...rows].map(row => row.querySelector("dt")?.textContent)).toEqual(["Foreign", "Institutions", "Individuals", "Short selling", "Lending balance"]);
  // "Net buying" heads the investor groups; each value is a calculation chip that says what its sign means.
  expect(within(flows).getByText("Net buying")).toBeVisible();
  expect(within(flows).getByRole("button", { name: "net buying 12.3 billion won, calculated. Show the math" })).toHaveTextContent("+₩12.3bn");
  expect(within(flows).getByRole("button", { name: "net selling 4.1 billion won, calculated. Show the math" })).toHaveTextContent("−₩4.1bn");
  expect(rows[3]).toHaveTextContent("4.2% of volume");
  expect(rows[4]).toHaveTextContent(/^Lending balance−3\.1% to .*Shares lent out, often for short selling$/u);
  // Flows are not prices: no direction attribute and no price color class anywhere in the section.
  expect(flows.querySelector("[data-direction], .asset-price")).toBeNull();
  expect(within(flows).getByText("Final after the close. Lending balances settle two business days later.")).toBeVisible();
  expect(within(flows).getByText("From Korea Investment")).toBeVisible();
  // Korean dividends name their record date; the old Korea "not checked" line is replaced.
  expect(within(page).getByText(/^₩300 · record date /u)).toBeVisible();
  expect(within(page).getByText("Not checked yet: credit balances, short-sale balances, lock-up schedules")).toBeVisible();
});

it("says investor flows need Korea Investment once, as a link to Connections", async () => {
  const { page, user, handlers } = await fixture(koreaNeedsKeyFixture(), 3);
  const flows = within(page).getByRole("region", { name: "Investor flows" });
  expect(flows.querySelector(".asset-flows-rows")).toBeNull();
  // One statement: the section's heading names flows, the line says how to get them.
  expect(flows).toHaveTextContent(/^Investor flowsConnect Korea Investment to see investor flows$/u);
  // The checks section does not repeat it.
  expect(within(page).queryByRole("button", { name: /to check investor flows/u })).toBeNull();
  expect(within(page).getByText(/^₩300 · record date in the notice$/u)).toBeVisible();
  await user.click(within(flows).getByRole("button", { name: "Connect Korea Investment to see investor flows" }));
  expect(handlers.onOpenConnections).toHaveBeenCalled();
});

it("words capital decisions plainly: a calculated dilution is a chip, a filed one is text", async () => {
  const { page } = await fixture(capitalRowsFixture(), 3);
  await waitFor(() => expect(page.querySelectorAll('.asset-change[data-kind="capital"]')).toHaveLength(4));
  const [bonds, rights, warrants, buyback] = [...page.querySelectorAll<HTMLElement>('.asset-change[data-kind="capital"]')];
  // A reset floor: the most shares at the floor, Rust's calculation behind the chip.
  expect(bonds).toHaveTextContent(/Capital.*Convertible bonds ₩30bn · converts at ₩9,100 · up to 8\.7% more shares if reset to ₩6,370/u);
  expect(within(bonds!).getByRole("button", { name: "8.7%, calculated. Show the math" })).toHaveTextContent("8.7%");
  expect(rights).toHaveTextContent("Rights offering: 4.2m new shares, 7.8% more shares · price and payment date in the notice");
  // No reset floor: the filed share at the starting price, as text.
  expect(warrants).toHaveTextContent("Bonds with warrants ₩15bn · 3.2% more shares at ₩8,800");
  expect(within(warrants!).queryByRole("button", { name: /calculated/u })).toBeNull();
  expect(warrants).not.toHaveTextContent(/reset/u);
  expect(buyback).toHaveTextContent(/Share buyback decided: ₩5bn by /u);
  for (const row of [bonds, rights, warrants, buyback]) expect(row!.querySelector(".loupist-citation")).not.toBeNull();
});

it("words capital decisions with Korean amounts and share counts", async () => {
  await i18n.changeLanguage("ko");
  try {
    const asset = capitalRowsFixture();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const [loupist] = await bridge.loupistList() as LoupistSummary[];
    bridge.assets.install(loupist!.id, asset.page, asset.delta);
    render(<TooltipProvider><AssetPage bridge={bridge} loupist={loupist!} target={{ loupistId: loupist!.id, symbol: asset.page.symbol, market: asset.page.market }} threads={[]}
      onBack={vi.fn()} onAsk={vi.fn()} onOpenChat={vi.fn()} onOpenMap={vi.fn()} onOpenLink={vi.fn()} onOpenSource={vi.fn()} onOpenConnections={vi.fn()} onAddSource={vi.fn()} /></TooltipProvider>);
    const page = await screen.findByRole("region", { name: i18n.t("assets:page.label", { name: asset.page.name }) });
    await waitFor(() => expect(page.querySelectorAll('.asset-change[data-kind="capital"]')).toHaveLength(4));
    const [bonds, rights] = [...page.querySelectorAll<HTMLElement>('.asset-change[data-kind="capital"]')];
    expect(bonds).toHaveTextContent("전환사채 300억 원 · 전환가 9,100원 · 6,370원까지 조정되면 주식 수 최대 8.7% 증가");
    // Under 1,000만 Korean keeps every digit, as the shared formatter does.
    expect(rights).toHaveTextContent("유상증자: 신주 4,200,000주, 주식 수 7.8% 증가");
  } finally {
    await i18n.changeLanguage("en");
  }
});

it("keeps the header and offers Try again when the page cannot open", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList() as LoupistSummary[];
  bridge.assets.pageFails = true;
  render(<TooltipProvider><AssetPage bridge={bridge} loupist={loupist!} target={{ loupistId: loupist!.id, symbol: "HANU", market: "KRX", name: "Hanul Precision" }} threads={[]}
    onBack={vi.fn()} onAsk={vi.fn()} onOpenChat={vi.fn()} onOpenMap={vi.fn()} onOpenLink={vi.fn()} onOpenSource={vi.fn()} onOpenConnections={vi.fn()} onAddSource={vi.fn()} /></TooltipProvider>);
  expect(await screen.findByText("Couldn't open this page.")).toBeVisible();
  expect(screen.getByRole("heading", { level: 1, name: "Hanul Precision" })).toBeVisible();
  bridge.assets.pageFails = false;
  bridge.assets.install(loupist!.id, hanulFixture().page, hanulFixture().delta);
  await userEvent.setup().click(screen.getByRole("button", { name: "Try again" }));
  expect(await screen.findByRole("region", { name: "Hanul Precision asset page" })).toBeVisible();
});
