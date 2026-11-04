import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { createMockMarketplaceSnapshot } from "../../bridge/mock/marketplace";
import { mainTab, openApp } from "../renderApp";

type User = ReturnType<typeof userEvent.setup>;
const card = (name: string) => screen.getByRole("heading", { name }).closest("article")!;
const openSetup = async (user: User, name: string) => {
  await user.click(await mainTab(/Connections/));
  await user.click(await screen.findByRole("button", { name: `Set up ${name}` }));
  return screen.getByRole("dialog", { name: `Set up ${name}` });
};
const enabledFor = async (bridge: MockLoupistBridge, entryId: string) =>
  (await bridge.loupistCapabilityList("loupist-quality")).find((binding) => binding.entry_id === entryId);

describe("Loupist · Connections", () => {
  it("shows onboarding before the global catalog when no Loupist exists", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "loupistList").mockResolvedValueOnce([]);
    render(<App bridge={bridge} />);
    expect(await screen.findByRole("button", { name: "Get started" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Connections" })).toBeNull();
  });

  it("enables global data for a new Loupist and groups optional Korea services", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const loupist = await bridge.loupistCreate({ name: "Global research" });
    const bindings = await bridge.loupistCapabilityList(loupist.id);
    expect(bindings.filter((binding) => binding.enabled).map((binding) => binding.entry_id).sort()).toEqual([
      "community.web-research", "defillama.tvl", "openbb.platform", "world-bank.indicators",
    ]);
    await openApp(bridge);
    const user = userEvent.setup();
    await user.click(await mainTab(/Connections/));
    const korea = within((await screen.findByRole("heading", { name: /^Korea/, level: 2 })).closest("section")!);
    for (const name of ["Korea Investment Open Trading API", "KRX Open API", "OpenDART"]) expect(korea.getByRole("heading", { name, level: 3 })).toBeVisible();
    expect(within(card("Market data platform")).getByText("Ready")).toBeVisible();
  });

  it("filters the catalog by search text and access type", async () => {
    const user = userEvent.setup();
    await openApp();
    await user.click(await mainTab(/Connections/));
    const search = screen.getByRole("searchbox", { name: "Search connections" });
    await user.type(search, "Korean");
    expect(screen.getByRole("heading", { name: "OpenDART" })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "SEC EDGAR" })).toBeNull();
    await user.clear(search);
    await user.click(screen.getByRole("combobox", { name: "Filter connections by access" }));
    await user.click(screen.getByRole("option", { name: "No key" }));
    expect(screen.getByRole("heading", { name: "World Bank Indicators" })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "OpenDART" })).toBeNull();
  });

  it("shows a missing bundled runtime instead of a dead setup action", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    vi.spyOn(bridge, "marketplaceSnapshot").mockResolvedValue(createMockMarketplaceSnapshot(
      new Set(), new Set(), new Map(), new Map(), new Map(), new Set(["openbb.platform"]),
    ));
    await user.click(await mainTab(/Connections/));
    await screen.findByRole("heading", { name: "Market data platform" });
    const openbb = within(card("Market data platform"));
    expect(openbb.getByText("Connection unavailable")).toBeVisible();
    await user.click(openbb.getByRole("button", { name: "Details Market data platform" }));
    const dialog = screen.getByRole("dialog", { name: "Market data platform" });
    expect(within(dialog).getByText(/Update or reinstall/)).toBeVisible();
    expect(within(dialog).queryByRole("button", { name: /Save/ })).not.toBeInTheDocument();
  });

  it("saves Web Research routing without changing any Loupist binding", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const configure = vi.spyOn(bridge, "marketplaceConnectorConfigure");
    const enable = vi.spyOn(bridge, "loupistCapabilityEnable");
    const disable = vi.spyOn(bridge, "loupistCapabilityDisable");
    await user.click(await mainTab(/Connections/));
    await user.click(await screen.findByRole("button", { name: "Settings Web Research" }));
    const dialog = screen.getByRole("dialog", { name: "Web Research settings" });
    await user.click(within(dialog).getByRole("combobox", { name: "Search provider" }));
    await user.click(screen.getByRole("option", { name: "Model search only" }));
    await user.click(within(dialog).getByRole("button", { name: "Save setup" }));
    await waitFor(() => expect(configure).toHaveBeenCalledWith({ entry_id: "community.web-research", config: { search_policy: "model_only" } }));
    expect(await within(dialog).findByText("Settings saved")).toBeVisible();
    expect(enable).not.toHaveBeenCalled();
    expect(disable).not.toHaveBeenCalled();
  });

  it("labels a partially configured connection as unfinished", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    await bridge.marketplaceConnectorConfigure({ entry_id: "koreainvestment.market-data", config: { environment: "demo" } });
    await user.click(await mainTab(/Connections/));
    expect(await screen.findByRole("button", { name: "Continue setup Korea Investment Open Trading API" })).toBeVisible();
  });

  it("enables every Loupist when setup first makes a connection ready", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const configure = vi.spyOn(bridge, "marketplaceConnectorConfigure");
    const dialog = await openSetup(user, "SEC EDGAR");
    await user.type(within(dialog).getByRole("textbox", { name: "SEC contact email" }), "research@example.com");
    await user.click(within(dialog).getByRole("button", { name: "Save setup" }));
    await waitFor(() => expect(configure).toHaveBeenCalledWith({ entry_id: "sec.edgar", config: { contact_email: "research@example.com" } }));
    expect(await within(dialog).findByText("SEC EDGAR is ready and enabled for every Loupist.")).toBeVisible();
    expect(await enabledFor(bridge, "sec.edgar")).toMatchObject({ enabled: true, available: true });
  });

  it("keeps API keys write-only across setup, verification and deletion", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const save = vi.spyOn(bridge, "marketplaceCredentialSave");
    const verify = vi.spyOn(bridge, "marketplaceCredentialVerify");
    const deleteOnDevice = bridge.marketplaceCredentialDelete.bind(bridge);
    let releaseDelete!: () => void;
    const pending = new Promise<void>((resolve) => { releaseDelete = resolve; });
    const remove = vi.spyOn(bridge, "marketplaceCredentialDelete").mockImplementation(async (request) => { await pending; return deleteOnDevice(request); });
    const help = vi.spyOn(bridge, "openExternalUrl").mockResolvedValue(undefined);

    let dialog = await openSetup(user, "OpenDART");
    await user.type(within(dialog).getByLabelText("OpenDART API key"), "discard-this-secret");
    await user.click(within(dialog).getByRole("button", { name: "Close" }));
    await user.click(screen.getByRole("button", { name: "Set up OpenDART" }));
    dialog = screen.getByRole("dialog", { name: "Set up OpenDART" });
    const apiKey = within(dialog).getByLabelText("OpenDART API key");
    expect(apiKey).toHaveAttribute("type", "password");
    expect(apiKey).toHaveValue("");
    await user.click(within(dialog).getByRole("button", { name: /Open OpenDART API key setup help/ }));
    expect(help).toHaveBeenCalledWith("https://opendart.fss.or.kr/uss/umt/EgovMberInsertView.do");

    await user.type(apiKey, "open-dart-test-key");
    await user.click(within(dialog).getByRole("button", { name: "Save & verify" }));
    await waitFor(() => expect(save).toHaveBeenCalledWith({ entry_id: "opendart.disclosures", secrets: { api_key: "open-dart-test-key" } }));
    expect(verify).toHaveBeenCalledWith({ entry_id: "opendart.disclosures" });
    expect(await within(dialog).findByText("OpenDART is ready and enabled for every Loupist.")).toBeVisible();
    expect(apiKey).toHaveValue("");
    expect(screen.queryByDisplayValue("open-dart-test-key")).toBeNull();
    expect(await enabledFor(bridge, "opendart.disclosures")).toMatchObject({ enabled: true, available: true });

    await user.click(within(dialog).getByRole("button", { name: "Delete saved credentials" }));
    const confirmation = screen.getByRole("dialog", { name: "Delete saved credentials?" });
    expect(remove).not.toHaveBeenCalled();
    await user.click(within(confirmation).getByRole("button", { name: "Delete credentials" }));
    await waitFor(() => expect(remove).toHaveBeenCalledWith({ entry_id: "opendart.disclosures" }));
    await user.keyboard("{Escape}");
    expect(confirmation).toBeVisible();
    await act(async () => releaseDelete());
    expect(await within(dialog).findByText("Not stored")).toBeVisible();
  });

  it("stages required KIS credentials and optional profile values without exposing saved values", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const configure = vi.spyOn(bridge, "marketplaceConnectorConfigure");
    const save = vi.spyOn(bridge, "marketplaceCredentialSave");
    const verify = vi.spyOn(bridge, "marketplaceCredentialVerify");
    const dialog = await openSetup(user, "Korea Investment Open Trading API");
    const field = (label: string) => within(dialog).getByLabelText(label);
    await user.type(field("KIS app key"), "  test-app-key  ");
    await user.click(within(dialog).getByRole("button", { name: "Save & verify" }));
    expect(save).not.toHaveBeenCalled();
    expect(field("KIS app secret")).toBeInvalid();

    await user.type(field("KIS app secret"), "  test-app-secret  ");
    await user.click(within(dialog).getByRole("button", { name: "Save & verify" }));
    await waitFor(() => expect(save).toHaveBeenCalledWith({ entry_id: "koreainvestment.market-data", secrets: { app_key: "test-app-key", app_secret: "test-app-secret" } }));
    expect(configure).toHaveBeenCalledWith({ entry_id: "koreainvestment.market-data", config: { environment: "real" } });
    expect(await within(dialog).findByText("Korea Investment Open Trading API is ready and enabled for every Loupist.")).toBeVisible();

    await user.type(field("KIS account number (optional)"), "12345678");
    await user.type(field("KIS account product code (optional)"), "01");
    await user.click(within(dialog).getByRole("button", { name: "Save & verify" }));
    await waitFor(() => expect(save).toHaveBeenLastCalledWith({ entry_id: "koreainvestment.market-data", secrets: { account_number: "12345678", account_product_code: "01" } }));
    expect(verify).toHaveBeenCalledTimes(2);
    expect(screen.queryByDisplayValue("12345678")).toBeNull();
  });

  it("shows the safe provider diagnostic when verification fails", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    vi.spyOn(bridge, "marketplaceCredentialVerify").mockRejectedValueOnce({
      code: "credential_rejected", message: "KIS error · EGW00123: The app key and secret do not match.",
    });
    const dialog = await openSetup(user, "Korea Investment Open Trading API");
    await user.type(within(dialog).getByLabelText("KIS app key"), "test-app-key");
    await user.type(within(dialog).getByLabelText("KIS app secret"), "test-app-secret");
    await user.click(within(dialog).getByRole("button", { name: "Save & verify" }));
    const alert = await within(dialog).findByRole("alert");
    expect(alert).toHaveTextContent("KIS error · EGW00123: The app key and secret do not match.");
    expect(alert).not.toHaveTextContent("test-app-secret");
  });
});
