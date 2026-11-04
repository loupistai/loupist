import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { EarningsSnapshot } from "../../contracts/earnings";
import { morrowSnapshot, solsupSnapshot } from "../../preview/finance";
import { noThesisFixture } from "../../preview/assets";
import { morrowWithGrid } from "../../preview/korea";
import type { ChatThread, LoupistSummary } from "../../types";
import { TooltipProvider } from "../ui/tooltip";
import { AssetPage } from "./AssetPage";
import { EarningsCard } from "./EarningsCard";

async function card(snapshot: EarningsSnapshot, options: { onAsk?: (text: string) => void; onOpenConnections?: () => void } = {}) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList() as LoupistSummary[];
  bridge.earnings.install(loupist!.id, snapshot);
  const onOpenEvidence = vi.fn();
  const view = render(<TooltipProvider><EarningsCard bridge={bridge} loupistId={loupist!.id} loupistName="Quality" snapshot={snapshot} name={snapshot.name} onOpenEvidence={onOpenEvidence} onAsk={options.onAsk ?? vi.fn()} onOpenConnections={options.onOpenConnections} /></TooltipProvider>);
  const region = within(view.container).getByRole("region", { name: `Results · ${snapshot.period}` });
  return { bridge, loupist: loupist!, region, onOpenEvidence, user: userEvent.setup() };
}

/** The window is 700 px wide or narrower while `narrow` is true. */
function windowWidth(narrow: boolean) {
  const original = window.matchMedia;
  Object.defineProperty(window, "matchMedia", { configurable: true, value: (query: string) => ({ matches: narrow && query.includes("max-width: 700px"), media: query, onchange: null, addEventListener: vi.fn(), removeEventListener: vi.fn(), addListener: vi.fn(), removeListener: vi.fn(), dispatchEvent: vi.fn() }) });
  return () => { Object.defineProperty(window, "matchMedia", { configurable: true, value: original }); };
}

afterEach(() => { Reflect.deleteProperty(window.navigator, "onLine"); });

it("before a report: says when expectations will be captured, and Prepare now captures them through Rust", async () => {
  const { bridge, loupist, region, user } = await card(morrowSnapshot("upcoming"));
  expect(within(region).getByText(/^Expected [A-Z]/)).toBeVisible();
  expect(within(region).getByText(/^Will be captured /)).toBeVisible();
  expect(within(region).getByText("Not reported yet")).toBeVisible();
  const prepare = vi.spyOn(bridge, "earningsPrepare");
  bridge.earnings.prepareHold = true;
  await user.click(within(region).getByRole("button", { name: "Prepare now" }));
  // Preparing: the button says so with a spinner and the change is announced.
  expect(await within(region).findByRole("button", { name: /Capturing what's expected…/ })).toHaveAttribute("aria-busy", "true");
  expect(within(region).getByText("Capturing expectations for Morrow Components")).toBeInTheDocument();
  bridge.earnings.prepareHold = false;
  await waitFor(() => expect(region).toHaveAttribute("data-status", "before_captured"));
  expect(prepare).toHaveBeenCalledWith({ loupistId: loupist.id, assetId: morrowSnapshot("upcoming").assetId, eventId: morrowSnapshot("upcoming").eventId });
  expect(within(region).getByText("Expectations captured")).toBeInTheDocument();
  expect(within(region).queryByRole("button", { name: "Prepare now" })).toBeNull();
});

it("a snapshot Rust is still capturing shows the preparing state", async () => {
  const { region } = await card(morrowSnapshot("preparing"));
  expect(within(region).getByRole("button", { name: /Capturing what's expected…/ })).toBeDisabled();
});

it("before captured: expected values with their sources, nothing reported yet", async () => {
  const { region } = await card(morrowSnapshot("before"));
  const table = within(region).getByRole("table", { name: "Expected versus reported" });
  expect(within(table).getByRole("columnheader", { name: /^Expected · / })).toBeVisible();
  const revenue = within(table).getByRole("row", { name: /^Revenue/ });
  expect(within(revenue).getByText(/Company guidance · /)).toBeVisible();
  expect(within(revenue).getByText("Analyst consensus · FMP")).toBeVisible();
  expect(within(revenue).getByText("Not reported yet")).toBeVisible();
  expect(within(region).getByText(/^Before: captured /)).toBeVisible();
  // PRICE IMPLIED is one calculation chip with its formula.
  expect(within(region).getByText(/yearly cash-flow growth for 5 years/)).toBeVisible();
  expect(within(region).getByRole("button", { name: "6.1%–9.8%, calculated. Show the math" })).toBeVisible();
});

it("after: Rust's differences in words first, the reaction alone in price color, the call's tone and the filed chain", async () => {
  const { region } = await card(morrowSnapshot("after"));
  const table = within(region).getByRole("table", { name: "Expected versus reported" });
  const revenue = within(table).getAllByRole("row", { name: /^Revenue/ })[0]!;
  expect(within(revenue).getByText("Above the range")).toBeVisible();
  expect(within(revenue).getByRole("button", { name: "+1.6% over the top, calculated. Show the math" })).toHaveTextContent("+1.6% over the top");
  expect(within(revenue).getByRole("button", { name: "+3.3%, calculated. Show the math" })).toBeVisible();
  expect(within(table).getByRole("row", { name: /^Operating margin/ })).toHaveTextContent("−1.2 pp");
  const guidance = within(table).getByRole("row", { name: /^Revenue · Q4 2026/ });
  expect(within(guidance).getByText("New")).toBeVisible();
  // Every value carries a source chip.
  for (const value of region.querySelectorAll(".earnings-value")) expect(value.querySelector(".loupist-citation")).not.toBeNull();
  // Only the reaction percentage uses the price colors.
  expect(region.querySelector('.earnings-reaction[data-direction="down"]')).not.toBeNull();
  expect(region.querySelectorAll("[data-direction]")).toHaveLength(1);
  expect(within(region).getByText(/Quality's read of the call:/)).toBeVisible();
  const chain = region.querySelector(".earnings-chain")!;
  expect(within(chain as HTMLElement).getAllByText("filed")).toHaveLength(2);
  expect(within(region).getByText("Numbers unchanged in the final report")).toBeVisible();
  expect(region.textContent).not.toMatch(/\b(beat|miss)\b/i);
});

it("final with a revision names the changed number; Korea shows the preliminary results and the legal deadline", async () => {
  const { region } = await card(morrowSnapshot("revised"));
  expect(within(region).getByText(/^Revised in the final report: Revenue /)).toBeVisible();
  const { region: korea } = await card(solsupSnapshot());
  const steps = within(korea.querySelector(".earnings-chain") as HTMLElement).getAllByRole("listitem");
  expect(steps[0]).toHaveTextContent(/filed.*Preliminary results/);
  expect(steps[1]).toHaveTextContent(/not filed yet.*Quarterly report · due by .* \(legal deadline\)/);
  // No transcript: the tone row says how to get one.
  expect(within(korea).getByText("No call transcript. Add one as a source to get a tone line.")).toBeVisible();
});

it("late capture and no expectations say what could not be captured", async () => {
  const { region } = await card(morrowSnapshot("late"));
  expect(within(region).getByText(/Captured after the fact, using only what was published by /)).toBeVisible();
  expect(within(region).getByText("Consensus wasn't captured before the report.")).toBeVisible();
  const onOpenConnections = vi.fn();
  const none = await card(morrowSnapshot("none"), { onOpenConnections });
  expect(within(none.region).getByText("No company guidance found.")).toBeVisible();
  expect(within(none.region).getByText(/Analyst consensus needs a market-data key \(FMP or Intrinio\)\./)).toBeVisible();
  await none.user.click(within(none.region).getByRole("button", { name: "Open Connections" }));
  expect(onOpenConnections).toHaveBeenCalled();
});

it("waits for the next close, keeps the last good state when a capture failed, and is offline-aware", async () => {
  const waiting = await card({ ...morrowSnapshot("after"), reaction: undefined, reactionWaiting: true });
  expect(within(waiting.region).getByText("Waiting for the next close")).toBeVisible();
  const failed = await card({ ...morrowSnapshot("upcoming"), failed: { side: "before", atMs: Date.now() } });
  expect(within(failed.region).getByRole("alert")).toHaveTextContent("Couldn't capture expectations for Morrow Components before results.");
  const retry = vi.spyOn(failed.bridge, "earningsPrepare");
  await failed.user.click(within(failed.region).getByRole("button", { name: "Try again" }));
  await waitFor(() => expect(retry).toHaveBeenCalled());
  Object.defineProperty(window.navigator, "onLine", { configurable: true, get: () => false });
  const offline = await card(morrowSnapshot("upcoming"));
  expect(within(offline.region).getByRole("button", { name: "Prepare now" })).toBeDisabled();
  expect(within(offline.region).getByRole("button", { name: "Prepare now" }).parentElement).toHaveAttribute("title", "You're offline");
});

it("shows six measures, then Show more", async () => {
  const after = morrowSnapshot("after");
  const many = { ...after, measures: Array.from({ length: 8 }, (_, index) => ({ ...after.measures[1]!, label: `Segment ${index + 1}`, key: undefined })) };
  const { region, user } = await card(many);
  expect(within(region).getAllByRole("row").length).toBe(7);
  await user.click(within(region).getByRole("button", { name: "Show 2 more" }));
  expect(within(region).getAllByRole("row").length).toBe(9);
});

it("stacks one block per measure at 700 px", async () => {
  const restore = windowWidth(true);
  try {
    const { region } = await card(morrowSnapshot("after"));
    expect(within(region).queryByRole("table", { name: "Expected versus reported" })).toBeNull();
    const blocks = region.querySelectorAll(".earnings-stack-measure");
    expect(blocks).toHaveLength(3);
    expect(within(blocks[0] as HTMLElement).getByRole("heading", { name: "Revenue" })).toBeVisible();
    expect(within(blocks[0] as HTMLElement).getByText(/^Expected · /)).toBeVisible();
    expect(within(blocks[0] as HTMLElement).getByText(/^Reported · /)).toBeVisible();
    expect(within(blocks[0] as HTMLElement).getByText("Difference")).toBeVisible();
  } finally { restore(); }
});

it("Ask about these results fills a composer with the asset and never sends", async () => {
  const onAsk = vi.fn();
  const { bridge, region, user } = await card(morrowSnapshot("after"), { onAsk });
  const send = vi.spyOn(bridge, "chatSend");
  await user.click(within(region).getByRole("button", { name: "Ask about these results" }));
  expect(onAsk).toHaveBeenCalledWith("$MORR:NASDAQ Walk me through Morrow Components's Q3 2026 results against what was expected.");
  expect(send).not.toHaveBeenCalled();
});

it("Open the grid shows the whole reverse-DCF grid in a dialog; Change the rates fills a composer and never sends", async () => {
  expect(within((await card(morrowSnapshot("after"))).region).queryByRole("button", { name: "Open the grid" })).toBeNull();
  const onAsk = vi.fn();
  const snapshot = morrowWithGrid();
  const { bridge, region, user, onOpenEvidence } = await card(snapshot, { onAsk });
  const send = vi.spyOn(bridge, "chatSend");
  const implied = within(region).getByText("Price implied").nextElementSibling as HTMLElement;
  await user.click(within(implied).getByRole("button", { name: "Open the grid" }));
  const dialog = await screen.findByRole("dialog", { name: /^What the price implies · Morrow Components · \$41\.20 on /u });
  // As in an answer's card: the dated close beside the price, the statements and their year beside the cash flow.
  const title = within(dialog).getByRole("heading", { name: /^What the price implies/u });
  const [close, statements] = snapshot.implied!.evidence;
  await user.click(within(title).getByRole("button", { name: new RegExp(`^Source \\d+: ${close!.title}`, "u") }));
  expect(onOpenEvidence).toHaveBeenCalledWith(close);
  const inputs = dialog.querySelector<HTMLElement>(".sensitivity-inputs")!;
  expect(inputs).toHaveTextContent(/Free cash flow FY2025 \$402m/u);
  expect(within(inputs).getByRole("button", { name: new RegExp(`^Source \\d+: ${statements!.title}`, "u") })).toBeVisible();
  expect(inputs).toHaveTextContent(/182\.4m shares/u);
  const table = within(dialog).getByRole("table");
  expect(within(table).getAllByRole("row")).toHaveLength(4);
  expect(table.querySelector("[data-base]")).toHaveTextContent("7.9%");
  // No reference: nothing shaded and no legend.
  expect(table.querySelectorAll("[data-above]")).toHaveLength(0);
  await user.click(within(dialog).getByRole("button", { name: "Change the rates · Morrow Components" }));
  expect(onAsk).toHaveBeenCalledWith("$MORR:NASDAQ Recalculate with discount rates 9%, 10%, 11% and long-run growth 2.0%, 2.5%, 3.0%.");
  expect(send).not.toHaveBeenCalled();
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
});

it("the asset page places the card before What changed and folds other events into Earlier results", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList() as LoupistSummary[];
  const fixture = noThesisFixture();
  bridge.assets.install(loupist!.id, fixture.page, fixture.delta);
  await bridge.watchlistAdd({ symbol: fixture.page.symbol, market: fixture.page.market, displayName: fixture.page.name });
  bridge.earnings.install(loupist!.id, morrowSnapshot("upcoming"));
  bridge.earnings.install(loupist!.id, { ...morrowSnapshot("after"), eventId: "evt-morr-q2-2026", period: "Q2 2026", reportDate: "2026-07-30" }, { current: false });
  const threads: ChatThread[] = [];
  const onAskInNewChat = vi.fn();
  render(<TooltipProvider><AssetPage bridge={bridge} loupist={loupist!} target={{ loupistId: loupist!.id, symbol: "MORR", market: "NASDAQ" }} threads={threads}
    onBack={vi.fn()} onAsk={vi.fn()} onAskInNewChat={onAskInNewChat} onOpenChat={vi.fn()} onOpenMap={vi.fn()} onOpenLink={vi.fn()} onOpenSource={vi.fn()} onOpenConnections={vi.fn()} onAddSource={vi.fn()} /></TooltipProvider>);
  const page = await screen.findByRole("region", { name: "Morrow Components asset page" });
  const card = await within(page).findByRole("region", { name: "Results · Q3 2026" });
  const changed = within(page).getByRole("heading", { name: "What changed" });
  expect(card.compareDocumentPosition(changed) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  const earlier = await within(page).findByRole("heading", { name: "Earlier results" });
  const user = userEvent.setup();
  const row = within(earlier.parentElement!).getByRole("button", { name: /Results · Q2 2026/ });
  await user.click(row);
  expect(await within(page).findByRole("region", { name: "Results · Q2 2026" })).toBeVisible();
  expect(row).toHaveAttribute("aria-expanded", "true");
  // "Ask about these results" goes to a new topic Chat.
  await user.click(within(card).getByRole("button", { name: "Ask about these results" }));
  expect(onAskInNewChat).toHaveBeenCalledWith(expect.stringMatching(/^\$MORR:NASDAQ /));
});
