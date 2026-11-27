import { chooseSelectOption, openSelect } from "@/test/selectOption";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n, { resources } from "@/i18n";
import { formatDay } from "@/i18n/format";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { FX_DISPLAY_FIXTURES as fixtures } from "../../bridge/mock/fxDisplay";
import { useConfiguration } from "../../app/useConfiguration";
import { DisplayFxProvider, useDisplayFx } from "../../app/useDisplayFx";
import type { ConfigUpdateRequest, FxDisplay } from "../../types";
import { SettingsView } from "../SettingsView";
import { DISPLAY_CURRENCIES, displayCurrencyName } from "./DisplayCurrencySetting";

const QUALITY = "loupist-quality";
const VALUE = "loupist-value";
const en = resources.en.settings.appearance.displayCurrency;
const ko = resources.ko.settings.appearance.displayCurrency;
const fill = (text: string, values: Record<string, string>) => text.replace(/\{\{(\w+)\}\}/g, (_, key: string) => values[key] ?? "");

/** Settings › Appearance as the shell renders it, with configuration updates recorded. */
async function settings({ loupistId = QUALITY as string | null, configured, prepare }: { loupistId?: string | null; configured?: string; prepare?: (bridge: MockLoupistBridge) => unknown } = {}) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  if (configured) {
    const current = await bridge.configRead({ scope: { kind: "global" } });
    await bridge.configUpdate({ scope: current.scope, expectedRevision: current.revision, set: { appearance: { display_currency: configured } }, unset: [] });
  }
  await prepare?.(bridge);
  const updates: ConfigUpdateRequest[] = [];
  const update = bridge.configUpdate.bind(bridge);
  const recorded = vi.spyOn(bridge, "configUpdate").mockImplementation(async (request) => { updates.push(structuredClone(request)); return update(request); });
  const reads = vi.spyOn(bridge, "fxDisplay");
  const openConnections = vi.fn();
  function Shell() {
    const configuration = useConfiguration(bridge, { kind: "global" });
    const fx = useDisplayFx(bridge, loupistId, configuration);
    return <DisplayFxProvider value={fx}>
      <SettingsView bridge={bridge} loupists={[]} currentLoupistId={loupistId} section="appearance" appearanceConfiguration={configuration}
        catalog={null} theme="system" language="en" onLanguageChange={vi.fn()} loadError={null} onThemeChange={vi.fn()} onManageProviders={vi.fn()}
        onOpenLoupistConnections={openConnections} providerActions={{} as never} update={{} as never} />
    </DisplayFxProvider>;
  }
  render(<Shell />);
  const select = await screen.findByRole("combobox", { name: i18n.t("settings:appearance.displayCurrency.label") });
  await waitFor(() => expect(select).toBeEnabled());
  return { bridge, select, updates, recorded, reads, openConnections, user: userEvent.setup() };
}

const status = () => document.querySelector<HTMLElement>("[data-fx-status]")!;

afterEach(async () => { cleanup(); vi.restoreAllMocks(); await i18n.changeLanguage("en"); });

it("offers As listed and the 30 reference currencies by code and localized name, in English and Korean", async () => {
  const { select, user } = await settings();
  expect(select).toHaveTextContent(en.asListed);
  const options = (await openSelect(user, select)).getAllByRole("option");
  expect(options.map(option => option.textContent?.split(" — ")[0])).toEqual([en.asListed, ...DISPLAY_CURRENCIES]);
  expect(options.find(option => option.textContent?.startsWith("KRW"))).toHaveTextContent(fill(en.option, { code: "KRW", name: displayCurrencyName("KRW", "en") }));
  expect(displayCurrencyName("KRW", "en")).not.toBe("KRW");
  await user.keyboard("{Escape}");
  expect(select).toHaveFocus();
  expect(status()).toBeEmptyDOMElement();

  await act(async () => { await i18n.changeLanguage("ko"); });
  const korean = await openSelect(user, select);
  expect(korean.getByRole("option", { name: ko.asListed })).toBeVisible();
  expect(korean.getByRole("option", { name: fill(ko.option, { code: "KRW", name: displayCurrencyName("KRW", "ko") }) })).toBeVisible();
  expect(displayCurrencyName("KRW", "ko")).not.toBe(displayCurrencyName("KRW", "en"));
  expect(displayCurrencyName("ZZZ", "ko")).toBe("ZZZ");
});

it("saves a code to global appearance only, resets through the unset path, and shows the status for this Loupist", async () => {
  const { bridge, select, updates, user } = await settings();
  await chooseSelectOption(user, select, /^KRW/);
  await waitFor(() => expect(select).toHaveTextContent(/^KRW/));
  expect(updates).toEqual([{ scope: { kind: "global" }, expectedRevision: "0", set: { appearance: { display_currency: "KRW" } }, unset: [] }]);
  await waitFor(() => expect(status()).toHaveTextContent(en.status.waiting));

  // The cold read completes while Settings stays open; focus stays on the select.
  select.focus();
  act(() => bridge.fxDisplayControl.publish(fixtures.ready));
  await waitFor(() => expect(status()).toHaveTextContent(fill(en.status.ready, { source: fixtures.ready.source, date: formatDay(fixtures.ready.date) })));
  expect(select).toHaveFocus();

  await chooseSelectOption(user, select, en.asListed);
  await waitFor(() => expect(select).toHaveTextContent(en.asListed));
  expect(updates[1]).toEqual({ scope: { kind: "global" }, expectedRevision: "1", set: {}, unset: [["appearance", "display_currency"]] });
  expect(status()).toBeEmptyDOMElement();
  const stored = await bridge.configRead({ scope: { kind: "global" } });
  expect(stored.explicit?.appearance).not.toHaveProperty("display_currency");
});

it("saves with no Loupist open, without inventing one or reading rates", async () => {
  const { bridge, select, updates, reads, user } = await settings({ loupistId: null });
  await chooseSelectOption(user, select, /^CAD/);
  await waitFor(() => expect(select).toHaveTextContent(/^CAD/));
  expect(updates[0]?.scope).toEqual({ kind: "global" });
  expect(reads).not.toHaveBeenCalled();
  expect(bridge.fxDisplayControl.rateReads).toBe(0);
  expect(status()).toBeEmptyDOMElement();
  expect(screen.queryByRole("button", { name: en.connect })).toBeNull();
});

it("keeps and names a configured code outside the table until another is chosen", async () => {
  const { select, updates, user } = await settings({ configured: "XAU" });
  await waitFor(() => expect(select).toHaveTextContent(/^XAU/));
  const choices = await openSelect(user, select);
  expect(choices.getByRole("option", { name: fill(en.unsupported, { code: "XAU" }) })).toBeInTheDocument();
  expect(choices.getAllByRole("option")).toHaveLength(32);
  await user.click(choices.getByRole("option", { name: /^USD/ }));
  await waitFor(() => expect(select).toHaveTextContent(/^USD/));
  expect(updates[0]?.set).toEqual({ appearance: { display_currency: "USD" } });
  expect((await openSelect(user, select)).queryByRole("option", { name: fill(en.unsupported, { code: "XAU" }) })).toBeNull();
});

const date = formatDay("2026-11-16");
const source = fixtures.ready.source;
it.each<[string, FxDisplay, string]>([
  ["waiting", fixtures.waiting, en.status.waiting],
  ["ready", fixtures.ready, fill(en.status.ready, { source, date })],
  ["behind offline", fixtures.behindOffline, fill(en.status.behindOffline, { source, date })],
  ["behind provider", fixtures.behindProviderFailed, fill(en.status.behindProviderFailed, { source, date })],
  ["offline", fixtures.offline, en.status.offline],
  ["offline with a date", { state: "unavailable", currency: "KRW", reason: "offline", lastDate: "2026-11-12" }, fill(en.status.offlineSince, { date: formatDay("2026-11-12") })],
  ["provider failed", fixtures.providerFailed, en.status.providerFailed],
  ["no rate", fixtures.noRate, fill(en.status.noRateOn, { code: "KRW", date })],
  ["too old", fixtures.tooOld, fill(en.status.tooOldSince, { date: formatDay("2026-11-08") })],
])("words the %s state as Rust returned it, with no Connect", async (_name, fixture, text) => {
  const { bridge } = await settings({ configured: "KRW" });
  act(() => bridge.fxDisplayControl.publish(fixture));
  await waitFor(() => expect(status()).toHaveTextContent(text));
  expect(status()).toHaveAttribute("role", "status");
  expect(screen.queryByRole("button", { name: en.connect })).toBeNull();
});

it("offers Connect to this Loupist's Connections only when Market data is off for it", async () => {
  const off = await settings({ configured: "KRW", loupistId: VALUE, prepare: bridge => bridge.loupistCapabilityDisable({ loupist_id: VALUE, entry_id: "openbb.platform" }) });
  await waitFor(() => expect(status()).toHaveTextContent(en.status.connectionOff));
  await off.user.click(within(status()).getByRole("button", { name: en.connect }));
  expect(off.openConnections).toHaveBeenCalledExactlyOnceWith(VALUE);
  // A stored table stays usable for this Loupist; the line says why it is not refreshed.
  act(() => off.bridge.fxDisplayControl.publish(fixtures.ready));
  await waitFor(() => expect(status()).toHaveTextContent(fill(en.status.behindConnectionOff, { source, date })));
  expect(within(status()).getByRole("button", { name: en.connect })).toBeVisible();
});

it("drops Connect once Market data is enabled for this Loupist and the host sends its grant hint", async () => {
  const { bridge, openConnections, user } = await settings({ configured: "KRW", loupistId: VALUE, prepare: bridge => bridge.loupistCapabilityDisable({ loupist_id: VALUE, entry_id: "openbb.platform" }) });
  await waitFor(() => expect(status()).toHaveTextContent(en.status.connectionOff));
  await user.click(within(status()).getByRole("button", { name: en.connect }));
  expect(openConnections).toHaveBeenCalledExactlyOnceWith(VALUE);
  // Enabled in this Loupist's Connections: the host sends its hint for the grant change, and no
  // rate is published. That hint alone moves the shared query.
  const reads = vi.mocked(bridge.fxDisplay).mock.calls.length;
  await act(() => bridge.loupistCapabilityEnable({ loupist_id: VALUE, entry_id: "openbb.platform" }));
  expect(vi.mocked(bridge.fxDisplay).mock.calls.length).toBeGreaterThan(reads);
  await waitFor(() => expect(status()).toHaveTextContent(en.status.waiting));
  expect(screen.queryByRole("button", { name: en.connect })).toBeNull();
});

it("keeps the saved choice when a save fails, and recovers a failed rate read with Retry", async () => {
  const { bridge, select, recorded, reads, user } = await settings({ configured: "KRW" });
  await waitFor(() => expect(select).toHaveTextContent(/^KRW/));
  recorded.mockRejectedValueOnce(new Error("disk full"));
  await chooseSelectOption(user, select, /^JPY/);
  expect(await screen.findByRole("alert")).toHaveTextContent("disk full");
  expect(select).toHaveTextContent(/^KRW/);
  expect((await bridge.configRead({ scope: { kind: "global" } })).effective.appearance.display_currency).toBe("KRW");

  reads.mockRejectedValueOnce(new Error("bridge closed"));
  act(() => bridge.fxDisplayControl.publish(fixtures.ready));
  await waitFor(() => expect(status()).toHaveTextContent(en.status.readFailed));
  await user.click(within(status()).getByRole("button", { name: en.retry }));
  await waitFor(() => expect(status()).toHaveTextContent(fill(en.status.ready, { source, date })));
});
