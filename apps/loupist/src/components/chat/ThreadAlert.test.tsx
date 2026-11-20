import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { mockAssetId } from "../../bridge/mock/assets";
import type { AssetMove } from "../../contracts/assets";
import type { ChatMessage, LoupistSummary } from "../../types";
import { ThreadAlert } from "./ThreadAlert";
import { destinations, renderWithOpen } from "../../test/open";
import i18n from "@/i18n";

const DAY = 86_400_000;
const ASSET = mockAssetId("Hanul Precision");
const line = { journalId: "line-1", claim: "One customer above 25% of revenue", metric: "Largest customer share", direction: "above" as const, threshold: 25, unit: "%", observed: 27.4, crossed: true, setAtMs: Date.now() - 32 * DAY };
const crossed: AssetMove = { moveId: "move-crossed", assetId: ASSET, from: "weakening", to: "broken", factDate: "2026-10-30", reason: "The largest customer passed 25% of revenue.",
  evidence: [], line, actor: "loupist", atMs: Date.now() - DAY, undone: false };

/** The home Thread's crossed-line message, as Rust posts it. */
const lineAlert = (): ChatMessage => ({
  id: "alert", role: "assistant", status: "complete", host_composed: true, created_at: new Date(Date.now() - 3_600_000).toISOString(),
  content: "Hanul Precision crossed the line you set.",
  alert: { asset_id: ASSET, move_id: crossed.moveId, name: "Hanul Precision", symbol: "HNP", market: "KRX", claim: line.claim, value: "27.4%", set_at_ms: line.setAtMs, fact_date: "2026-10-30",
    source_url: "https://dart.fss.or.kr/dsaf001/main.do?rcpNo=20261030000117", source_title: "분기보고서 (2026.09)" },
});

async function alertFor(bridge: MockLoupistBridge) {
  const [loupist] = await bridge.loupistList() as LoupistSummary[];
  bridge.assets.installAsset({ assetId: ASSET, name: "Hanul Precision", ticker: "HNP", exchange: "KRX" });
  const reason = { journalId: "reason-1", text: "Customers stay diversified", createdAtMs: Date.now() - 40 * DAY, updatedAtMs: Date.now() - 40 * DAY };
  bridge.assets.install(loupist!.id, ASSET, { reasons: [reason], history: [crossed], read: { verdict: "broken", since: "2026-10-30", started: false, latestMove: crossed, undoMoveId: crossed.moveId } });
  render(<ThreadAlert message={lineAlert()} bridge={bridge} loupistId={loupist!.id} loupistName={loupist!.name} />);
  return { loupist: loupist!, user: userEvent.setup() };
}

it("undoes the crossing by its move and says so", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const { loupist, user } = await alertFor(bridge);
  const undo = vi.spyOn(bridge, "assetVerdictUndo");
  await user.click(screen.getByRole("button", { name: "Undo" }));
  expect(undo).toHaveBeenCalledWith({ loupistId: loupist.id, assetId: ASSET, moveId: "move-crossed" });
  expect(await screen.findByText(/Back to Weakening · you undid the .* change/)).toBeVisible();
  expect(screen.queryByRole("alert")).toBeNull();
});

it("says what it is, and moves the keyboard to what happened when Undo goes away", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const { user } = await alertFor(bridge);
  expect(screen.getByRole("article", { name: "Line crossed: Hanul Precision" })).toBeVisible();
  screen.getByRole("button", { name: "Undo" }).focus();
  await user.keyboard("{Enter}");
  const status = await screen.findByRole("status");
  expect(status).toHaveTextContent(/Back to Weakening/);
  expect(status).toHaveFocus();
});

it("moves the keyboard to the reason when a newer change took Undo away", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const { user } = await alertFor(bridge);
  vi.spyOn(bridge, "assetVerdictUndo").mockRejectedValueOnce(Object.assign(new Error("newer"), { code: "conflict" }));
  screen.getByRole("button", { name: "Undo" }).focus();
  await user.keyboard("{Enter}");
  expect(await screen.findByRole("alert")).toHaveFocus();
});

it("says inline when the Undo fails or a newer change came first", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const { user } = await alertFor(bridge);
  bridge.assets.undoFails = true;
  await user.click(screen.getByRole("button", { name: "Undo" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Couldn't undo this change. Try again.");
  bridge.assets.undoFails = false;
  vi.spyOn(bridge, "assetVerdictUndo").mockRejectedValueOnce(Object.assign(new Error("newer"), { code: "conflict" }));
  await user.click(screen.getByRole("button", { name: "Undo" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("A newer change was made, so this one can't be undone.");
  expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
});

it("reviews the crossed line on its asset's page by id", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList() as LoupistSummary[];
  const view = renderWithOpen(<ThreadAlert message={lineAlert()} bridge={bridge} loupistId={loupist!.id} loupistName={loupist!.name} />);
  await userEvent.setup().click(screen.getByRole("button", { name: i18n.t("chat:redLine.review") }));
  expect(destinations(view.open)).toEqual([{ kind: "asset", target: { loupistId: loupist!.id, assetId: ASSET, name: "Hanul Precision" } }]);
});
