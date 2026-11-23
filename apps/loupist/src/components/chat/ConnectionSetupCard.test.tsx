import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "@/i18n";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { ConnectionSetupCard, setupNeeds } from "./ConnectionSetupCard";

const user = () => userEvent.setup({ skipHover: true });

function renderCard(bridge: MockLoupistBridge, entryId: string, onAskAgain?: () => void) {
  return render(<ConnectionSetupCard bridge={bridge} loupistId="loupist-quality" entryId={entryId} onAskAgain={onAskAgain} />);
}

it("offers setup for a connection without its key, opens the Connections dialog, and refreshes when it closes", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const snapshot = vi.spyOn(bridge, "marketplaceSnapshot");
  renderCard(bridge, "fred.macro");
  const card = await screen.findByRole("region", { name: "Set up FRED" });
  expect(await within(card).findByText("FRED needs its setup before this Loupist can use it.")).toBeVisible();
  expect(within(card).getByText("Any key goes in the setup dialog and stays on this Mac; the Loupist never sees it.")).toBeVisible();
  expect(within(card).queryByText(/with each request/)).toBeNull();
  expect(card).toHaveAttribute("data-state", "needs_setup");
  await user().click(within(card).getByRole("button", { name: "Set up" }));
  const dialog = await screen.findByRole("dialog");
  expect(within(dialog).getByLabelText("FRED API key")).toBeVisible();

  // The key is entered in the dialog, never in Chat; once verified, the card reads the new state.
  await user().type(within(dialog).getByLabelText("FRED API key"), "test-fred-key-0123");
  await user().click(within(dialog).getByRole("button", { name: "Save & verify" }));
  await within(dialog).findByText("Verification successful");
  const reads = snapshot.mock.calls.length;
  await user().click(within(dialog).getByRole("button", { name: "Done" }));
  expect(await within(card).findByText("FRED is ready. It works from your next message.")).toBeVisible();
  expect(snapshot.mock.calls.length).toBeGreaterThan(reads);
  expect(card).toHaveAttribute("data-state", "ready");
  // Set up went away with the state it belonged to: the keyboard is on the card, not the page.
  await waitFor(() => expect(card).toHaveFocus());
});

it("keeps the keyboard on the card when Use in this Loupist turns into Ask again", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await bridge.loupistCapabilityDisable({ loupist_id: "loupist-quality", entry_id: "world-bank.indicators" });
  renderCard(bridge, "world-bank.indicators", vi.fn());
  const card = await screen.findByRole("region", { name: "Set up World Bank Indicators" });
  (await within(card).findByRole("button", { name: "Use in this Loupist" })).focus();
  await user().keyboard("{Enter}");
  await waitFor(() => expect(within(card).getByRole("button", { name: "Ask again" })).toHaveFocus());
});

it("turns a set-up connection on for this Loupist and then offers to ask again", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await bridge.loupistCapabilityDisable({ loupist_id: "loupist-quality", entry_id: "world-bank.indicators" });
  const enable = vi.spyOn(bridge, "loupistCapabilityEnable");
  const onAskAgain = vi.fn();
  renderCard(bridge, "world-bank.indicators", onAskAgain);
  const card = await screen.findByRole("region", { name: "Set up World Bank Indicators" });
  expect(await within(card).findByText("World Bank Indicators is set up but off for this Loupist.")).toBeVisible();
  await user().click(within(card).getByRole("button", { name: "Use in this Loupist" }));
  expect(enable).toHaveBeenCalledWith({ loupist_id: "loupist-quality", entry_id: "world-bank.indicators" });
  expect(await within(card).findByText("World Bank Indicators is ready. It works from your next message.")).toBeVisible();
  await user().click(within(card).getByRole("button", { name: "Ask again" }));
  expect(onAskAgain).toHaveBeenCalledTimes(1);
});

it("says when the connection is gone or its runtime is missing, and reports a failed enable", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  renderCard(bridge, "gone.entry");
  const gone = await screen.findByRole("region", { name: "Set up gone.entry" });
  expect(await within(gone).findByText("gone.entry is not available in this app.")).toBeVisible();
  expect(gone).toHaveAttribute("data-state", "unavailable");

  await bridge.loupistCapabilityDisable({ loupist_id: "loupist-quality", entry_id: "world-bank.indicators" });
  vi.spyOn(bridge, "loupistCapabilityEnable").mockRejectedValue(new Error("Loupist is busy"));
  renderCard(bridge, "world-bank.indicators");
  const card = await screen.findByRole("region", { name: "Set up World Bank Indicators" });
  await user().click(await within(card).findByRole("button", { name: "Use in this Loupist" }));
  expect(await within(card).findByRole("alert")).toHaveTextContent("Could not change this connection. Try again.");
  expect(card).toHaveAttribute("data-state", "off_here");
});

describe("a connection that needs a value that is not a key", () => {
  afterEach(async () => { await act(async () => { await i18n.changeLanguage("en"); }); });

  it("names SEC EDGAR's contact email and says where it goes, with no word of a key", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    renderCard(bridge, "sec.edgar");
    const card = await screen.findByRole("region", { name: "Set up SEC EDGAR" });
    expect(await within(card).findByText("Before this Loupist can use SEC EDGAR, it needs: SEC contact email.")).toBeVisible();
    expect(within(card).getByText("Enter it in the setup window. It is stored on this Mac and sent to SEC EDGAR with each request.")).toBeVisible();
    expect(card).not.toHaveTextContent(/key/i);

    await act(async () => { await i18n.changeLanguage("ko"); });
    expect(within(card).getByText("SEC EDGAR 연결을 쓰려면 먼저 SEC 연락처 이메일 설정이 필요합니다.")).toBeVisible();
    expect(within(card).getByText("설정 창에 입력하십시오. 이 Mac에 저장되며 요청마다 SEC EDGAR에 전송됩니다.")).toBeVisible();
    expect(card).not.toHaveTextContent("키");
  });

  it("reads what each bundled connection needs from its own fields", async () => {
    const { catalog } = await new MockLoupistBridge({ delay_ms: 0 }).marketplaceSnapshot();
    const needs = Object.fromEntries(catalog.entries.map((entry) => {
      const { key, values } = setupNeeds(entry);
      return [entry.id, [key ? "key" : null, ...values].filter(Boolean).join(" + ") || "nothing"];
    }));
    expect(needs).toEqual({
      "alpha-vantage.market-data": "key", "benzinga.news": "key", "biztoc.news": "key", "bls.macro": "key",
      "cftc.commitments": "key", "community.web-research": "nothing", "congress-gov.legislation": "key",
      "defillama.tvl": "nothing", "econdb.macro": "key", "eia.energy": "key", "fmp.market-data": "key", "fred.macro": "key",
      "intrinio.market-data": "key", "koreainvestment.market-data": "key", "krx.market-data": "key",
      "nasdaq.data-link": "key", "openbb.platform": "nothing", "opendart.disclosures": "key", "openfigi.lookup": "key",
      "sec.edgar": "SEC contact email", "tiingo.market-data": "key", "tradier.market-data": "key",
      "tradingeconomics.macro": "key", "world-bank.indicators": "nothing",
    });
  });
});
