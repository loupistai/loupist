import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { chooseLoupist } from "../renderApp";

const QUALITY = { id: "loupist-quality", name: "Quality Compounder" };
const VALUE = { id: "loupist-value", name: "Contrarian Value" };
const PRICE = { currency: "USD", currencySource: "result" as const, provider: "yfinance", sourceUrl: null, dataset: { dataset_id: "follows", revision: 1, digest: "test" } };

/** Each Loupist follows one asset of the shared catalog, with a stored price. */
function bridgeWithFollows() {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const nvidia = bridge.assets.installAsset({ name: "NVIDIA", ticker: "NVDA", exchange: "US", followedBy: [QUALITY.id] });
  const fund = bridge.assets.installAsset({ name: "Harbor Fund", followedBy: [VALUE.id] });
  bridge.assets.installPrice(QUALITY.id, nvidia.assetId, { price: { ...PRICE, value: 162.85, date: "2026-11-13" } });
  bridge.assets.installPrice(VALUE.id, fund.assetId, { price: { ...PRICE, value: 41.2, date: "2026-11-13" } });
  return bridge;
}

async function openFollowing() {
  await screen.findByRole("navigation", { name: "Loupists" });
  await userEvent.setup().click(within(document.querySelector<HTMLElement>('[data-sidebar-place="following"]')!).getByRole("button"));
  const page = await screen.findByTestId("following-page");
  await waitFor(() => expect(page.querySelector("[data-following-loading]")).toBeNull());
  return page;
}

const followedNames = (page: HTMLElement) =>
  [...page.querySelectorAll(".following-row .following-name")].map(name => name.textContent);

async function greeting(name: string) {
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "Bot" }));
  return screen.findByRole("region", { name: `${name} greets you` });
}

it("Following lists only the selected Loupist's follows and reads them again on a switch", async () => {
  const user = userEvent.setup();
  const bridge = bridgeWithFollows();
  render(<App bridge={bridge} />);
  expect(followedNames(await openFollowing())).toEqual(["NVIDIA"]);

  await chooseLoupist(user, VALUE.name);
  const value = await openFollowing();
  await waitFor(() => expect(followedNames(value)).toEqual(["Harbor Fund"]));

  // A follow here stays with this Loupist.
  await user.type(within(value).getByRole("combobox"), "Persistence Works");
  await user.click(await within(value).findByRole("option", { name: /Follow Persistence Works/u }));
  await waitFor(() => expect(followedNames(value)).toEqual(["Harbor Fund", "Persistence Works"]));

  await chooseLoupist(user, QUALITY.name);
  const quality = await openFollowing();
  await waitFor(() => expect(followedNames(quality)).toEqual(["NVIDIA"]));
  expect((await bridge.assetsList({ loupistId: VALUE.id })).map(row => row.name)).toEqual(["Harbor Fund", "Persistence Works"]);
});

it("Bot's tiles show only the selected Loupist's followed prices and change on a switch", async () => {
  const user = userEvent.setup();
  render(<App bridge={bridgeWithFollows()} />);
  const quality = await greeting(QUALITY.name);
  expect(await within(quality).findByRole("button", { name: "Open NVIDIA" })).toHaveTextContent("$162.85");
  expect(within(quality).queryByRole("button", { name: "Open Harbor Fund" })).toBeNull();
  expect(within(quality).getByRole("button", { name: /Where NVIDIA stands/u })).toBeVisible();

  await chooseLoupist(user, VALUE.name);
  const value = await greeting(VALUE.name);
  expect(await within(value).findByRole("button", { name: "Open Harbor Fund" })).toHaveTextContent("$41.20");
  expect(within(value).queryByRole("button", { name: "Open NVIDIA" })).toBeNull();
  expect(within(value).queryByRole("button", { name: /Where NVIDIA stands/u })).toBeNull();
});
