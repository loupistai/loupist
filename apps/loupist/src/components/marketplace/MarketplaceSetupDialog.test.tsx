import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { MarketplaceConnectorStatus } from "../../marketplace/types";
import { MarketplaceSetupDialog } from "./MarketplaceSetupDialog";

describe("Marketplace setup feedback", () => {
  it("saves an optional OpenFIGI key without a verification call", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const [loupist] = await bridge.loupistList();
    const snapshot = await bridge.marketplaceSnapshot();
    const entry = snapshot.catalog.entries.find((item) => item.id === "openfigi.lookup")!;
    const connector = snapshot.connectors.find((item) => item.entry_id === entry.id)!;
    const verify = vi.spyOn(bridge, "marketplaceCredentialVerify");

    render(<MarketplaceSetupDialog bridge={bridge} entry={entry} connector={connector} onClose={vi.fn()} onChanged={vi.fn()} />);
    // What works without the optional key, said once.
    expect(screen.getByText("Optional key for higher request limits.")).toBeVisible();
    expect(screen.queryByText("Not stored")).toBeNull();
    const save = screen.getByRole("button", { name: "Save API key" });
    expect(save).toBeDisabled();
    await user.type(screen.getByLabelText("OpenFIGI API key (optional)"), "synthetic-openfigi-key");
    await user.click(save);
    expect(await screen.findByText("API key saved")).toBeVisible();
    expect(verify).not.toHaveBeenCalled();
    expect(await bridge.marketplaceCredentialUnlock({ entry_id: entry.id })).toMatchObject([
      { stored: true, active: true, pending: false, verification: "never", verified_at: null },
    ]);

    await bridge.loupistCapabilityEnable({ loupist_id: loupist!.id, entry_id: entry.id });
    await bridge.marketplaceCredentialDelete({ entry_id: entry.id });
    expect(await bridge.loupistCapabilityList(loupist!.id)).toContainEqual(
      expect.objectContaining({ entry_id: entry.id, enabled: true, available: true }),
    );
  });

  it("opens CFTC signup and can verify a token after starting anonymously", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const snapshot = await bridge.marketplaceSnapshot();
    const entry = snapshot.catalog.entries.find(item => item.id === "cftc.commitments")!;
    const connector = snapshot.connectors.find(item => item.entry_id === entry.id)!;
    const open = vi.spyOn(bridge, "openExternalUrl").mockResolvedValue(undefined);
    const verify = vi.spyOn(bridge, "marketplaceCredentialVerify");
    render(<MarketplaceSetupDialog bridge={bridge} entry={entry} connector={connector} onClose={vi.fn()} onChanged={vi.fn()} />);
    expect(connector.readiness).toBe("ready");
    const save = screen.getByRole("button", { name: "Save & verify" });
    expect(save).toBeDisabled();
    await user.click(screen.getByRole("button", { name: /Get a key/ }));
    expect(open).toHaveBeenCalledWith("https://publicreporting.cftc.gov/signup");
    await user.type(screen.getByLabelText("CFTC app token (optional)"), "fixture-token-123");
    await user.click(save);
    await waitFor(() => expect(verify).toHaveBeenCalledWith({ entry_id: entry.id }));
  });

  it("reports the saved KRX key's verified and unapproved markets separately", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const snapshot = await bridge.marketplaceSnapshot();
    const entry = snapshot.catalog.entries.find(item => item.id === "krx.market-data")!;
    const connector = snapshot.connectors.find(item => item.entry_id === entry.id)!;
    const status = { ...connector, credentials: connector.credentials.map(credential => ({
      ...credential, stored: true, active: true, verification: "verified" as const,
      krx_coverage: { kospi: "verified" as const, kosdaq: "needs_approval" as const },
    })) };
    render(<MarketplaceSetupDialog bridge={bridge} entry={entry} connector={status} onClose={vi.fn()} onChanged={vi.fn()} />);
    expect(screen.getByText("KOSPI daily trading: Verified")).toBeVisible();
    expect(screen.getByText("KOSDAQ daily trading: Needs KRX approval")).toBeVisible();
  });
  it("retains entered credentials after client validation so missing fields can be corrected", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const snapshot = await bridge.marketplaceSnapshot();
    const entry = snapshot.catalog.entries.find((item) => item.id === "koreainvestment.market-data")!;
    const connector = snapshot.connectors.find((item) => item.entry_id === entry.id);
    const save = vi.spyOn(bridge, "marketplaceCredentialSave");
    render(<MarketplaceSetupDialog bridge={bridge} entry={entry} connector={connector} onClose={vi.fn()} onChanged={vi.fn().mockResolvedValue(undefined)} />);

    const key = screen.getByLabelText("KIS app key");
    await user.type(key, "test key with spaces");
    await user.type(screen.getByLabelText("KIS app secret"), "test-secret");
    await user.click(screen.getByRole("button", { name: "Save & verify" }));
    expect(screen.getByRole("alert")).toHaveTextContent("KIS app key cannot contain whitespace.");
    expect(key).toHaveValue("test key with spaces");
    expect(screen.getByLabelText("KIS app secret")).toHaveValue("test-secret");
    expect(save).not.toHaveBeenCalled();
  });

  it("opens the account type list and chooses the other option", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const snapshot = await bridge.marketplaceSnapshot();
    const entry = snapshot.catalog.entries.find((item) => item.id === "koreainvestment.market-data")!;
    const connector = snapshot.connectors.find((item) => item.entry_id === entry.id);
    render(<MarketplaceSetupDialog bridge={bridge} entry={entry} connector={connector} onClose={vi.fn()} onChanged={vi.fn()} />);
    const trigger = screen.getByRole("combobox", { name: "Account type" });
    expect(trigger).toHaveTextContent("Live");
    await user.click(trigger);
    const list = await screen.findByRole("listbox");
    expect(within(list).getByRole("option", { name: "Live" })).toBeVisible();
    await user.click(within(list).getByRole("option", { name: "Demo" }));
    expect(trigger).toHaveTextContent("Demo");
    expect(screen.queryByRole("listbox")).toBeNull();
  });

  it("keeps an unsaved key when the device rejects the save", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const snapshot = await bridge.marketplaceSnapshot();
    const entry = snapshot.catalog.entries.find((item) => item.id === "opendart.disclosures")!;
    vi.spyOn(bridge, "marketplaceCredentialSave").mockRejectedValue(new Error("Credential store unavailable"));
    const verify = vi.spyOn(bridge, "marketplaceCredentialVerify");
    render(<MarketplaceSetupDialog bridge={bridge} entry={entry} connector={snapshot.connectors.find((item) => item.entry_id === entry.id)} onClose={vi.fn()} onChanged={vi.fn().mockResolvedValue(undefined)} />);
    const key = screen.getByLabelText("OpenDART API key");
    await user.type(key, "test-key");
    await user.click(screen.getByRole("button", { name: "Save & verify" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not save credentials");
    expect(key).toHaveValue("test-key");
    expect(verify).not.toHaveBeenCalled();
  });

  it("keeps deletion failure inside the confirmation so credentials can be retried", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const snapshot = await bridge.marketplaceSnapshot();
    const entry = snapshot.catalog.entries.find((item) => item.id === "opendart.disclosures")!;
    await bridge.marketplaceCredentialSave({ entry_id: entry.id, secrets: { api_key: "test-key" } });
    const populated = await bridge.marketplaceSnapshot();
    const remove = vi.spyOn(bridge, "marketplaceCredentialDelete").mockRejectedValueOnce(new Error("Credential store unavailable"));
    render(<MarketplaceSetupDialog bridge={bridge} entry={entry} connector={populated.connectors.find((item) => item.entry_id === entry.id)} onClose={vi.fn()} onChanged={vi.fn().mockResolvedValue(undefined)} />);
    await user.click(screen.getByRole("button", { name: "Delete saved credentials" }));
    const confirmation = screen.getByRole("dialog", { name: "Delete saved credentials?" });
    await user.click(within(confirmation).getByRole("button", { name: "Delete credentials" }));
    expect(await within(confirmation).findByRole("alert")).toHaveTextContent("Credential store unavailable");
    await user.click(within(confirmation).getByRole("button", { name: "Delete credentials" }));
    expect(remove).toHaveBeenCalledTimes(2);
    expect(await screen.findByRole("status")).toHaveTextContent("Credentials deleted");
  });
});

describe("checking a key", () => {
  async function savedFred(patch: Partial<MarketplaceConnectorStatus> = {}) {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    await bridge.marketplaceCredentialSave({ entry_id: "fred.macro", secrets: { api_key: "abcdefabcdefabcdefabcdefabcdef12" } });
    const snapshot = await bridge.marketplaceSnapshot();
    const entry = snapshot.catalog.entries.find((item) => item.id === "fred.macro")!;
    const connector = { ...snapshot.connectors.find((item) => item.entry_id === entry.id)!, ...patch };
    return { bridge, entry, connector };
  }

  it("names the button for what it does: Verify again for a saved key, Save & verify once one is typed", async () => {
    const user = userEvent.setup();
    const { bridge, entry, connector } = await savedFred();
    const verify = vi.spyOn(bridge, "marketplaceCredentialVerify");
    const save = vi.spyOn(bridge, "marketplaceCredentialSave");
    render(<MarketplaceSetupDialog bridge={bridge} entry={entry} connector={connector} onClose={vi.fn()} onChanged={vi.fn().mockResolvedValue(undefined)} />);
    const button = screen.getByRole("button", { name: "Verify again" });
    await user.type(screen.getByLabelText("FRED API key"), "x");
    expect(button).toHaveAccessibleName("Save & verify");
    await user.clear(screen.getByLabelText("FRED API key"));
    await user.click(screen.getByRole("button", { name: "Verify again" }));
    await waitFor(() => expect(verify).toHaveBeenCalledWith({ entry_id: "fred.macro" }));
    expect(save).not.toHaveBeenCalled();
  });

  it("says a key could not be checked offline, and to verify again when back online", async () => {
    const user = userEvent.setup();
    const { bridge, entry, connector } = await savedFred();
    vi.spyOn(bridge, "marketplaceCredentialVerify").mockRejectedValueOnce({ code: "provider_unavailable", message: "The provider could not verify these credentials right now." });
    Object.defineProperty(window.navigator, "onLine", { configurable: true, get: () => false });
    try {
      render(<MarketplaceSetupDialog bridge={bridge} entry={entry} connector={connector} onClose={vi.fn()} onChanged={vi.fn().mockResolvedValue(undefined)} />);
      await user.click(screen.getByRole("button", { name: "Verify again" }));
      expect(await screen.findByRole("alert")).toHaveTextContent("You're offline. The key is saved on this Mac but not verified. Select Verify again when you're back online.");
    } finally {
      Reflect.deleteProperty(window.navigator, "onLine");
    }
  });

  it("names KRX approval when KRX rejects a key", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const snapshot = await bridge.marketplaceSnapshot();
    const entry = snapshot.catalog.entries.find((item) => item.id === "krx.market-data")!;
    vi.spyOn(bridge, "marketplaceCredentialVerify").mockRejectedValueOnce({ code: "credential_rejected", message: "The provider rejected these credentials." });
    const connector = snapshot.connectors.find((item) => item.entry_id === entry.id)!;
    const props = { bridge, entry, onClose: vi.fn(), onChanged: vi.fn().mockResolvedValue(undefined) };
    const view = render(<MarketplaceSetupDialog {...props} connector={connector} />);
    await user.type(screen.getByLabelText("KRX API key"), "krx-test-key");
    await user.click(screen.getByRole("button", { name: "Save & verify" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("KRX Open API rejected this key. Check that you copied the whole key, then try again. A new KRX key works only after KRX approves it for KOSPI or KOSDAQ daily trading.");
    // The rejected key stays saved; the open dialog keeps its title and shows the result beside the key, above About.
    view.rerender(<MarketplaceSetupDialog {...props} connector={{ ...connector, credentials: connector.credentials.map((credential) => ({ ...credential, stored: true, pending: true })) }} />);
    expect(screen.getByRole("dialog", { name: "Set up KRX Open API" })).toBeVisible();
    expect(alert.compareDocumentPosition(screen.getByText("About this connection")) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("offers Loupist access once the connection is set up, not before", async () => {
    const { bridge, entry, connector } = await openDartEntry();
    const props = { bridge, entry, onClose: vi.fn(), onChanged: vi.fn(), onManageLoupists: vi.fn() };
    const view = render(<MarketplaceSetupDialog {...props} connector={connector} />);
    expect(screen.queryByRole("button", { name: "Loupist access" })).toBeNull();
    view.rerender(<MarketplaceSetupDialog {...props} connector={{ ...connector, readiness: "ready" }} />);
    expect(screen.getByRole("button", { name: "Loupist access" })).toBeVisible();
  });
});

async function openDartEntry() {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const snapshot = await bridge.marketplaceSnapshot();
  const entry = snapshot.catalog.entries.find((item) => item.id === "opendart.disclosures")!;
  return { bridge, entry, connector: snapshot.connectors.find((item) => item.entry_id === entry.id)! };
}

it("explains an unavailable connection and blocks setup while retaining its details", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const snapshot = await bridge.marketplaceSnapshot();
  const entry = snapshot.catalog.entries.find((item) => item.id === "opendart.disclosures")!;
  const connector = snapshot.connectors.find((item) => item.entry_id === entry.id)!;
  const configure = vi.spyOn(bridge, "marketplaceConnectorConfigure");
  render(<MarketplaceSetupDialog bridge={bridge} entry={entry} connector={{ ...connector, readiness: "runtime_unavailable" }} onClose={vi.fn()} onChanged={vi.fn()} />);
  expect(screen.getByRole("dialog", { name: "OpenDART" })).toBeVisible();
  expect(screen.getByRole("alert")).toHaveTextContent("Loupist will try again on your next question. If this keeps happening, restart Loupist. Reinstall only if restarting doesn’t help.");
  expect(screen.getByLabelText("OpenDART API key")).toBeDisabled();
  expect(screen.queryByRole("button", { name: "Save & verify" })).toBeNull();
  expect(configure).not.toHaveBeenCalled();
});

async function openDart() {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const snapshot = await bridge.marketplaceSnapshot();
  const entry = snapshot.catalog.entries.find((item) => item.id === "opendart.disclosures")!;
  const connector = snapshot.connectors.find((item) => item.entry_id === entry.id)!;
  return { bridge, entry, connector };
}

describe("leaving setup with a typed key", () => {
  it("asks before Cancel discards a key that is typed but not saved", async () => {
    const user = userEvent.setup();
    const { bridge, entry, connector } = await openDart();
    const onClose = vi.fn();
    render(<MarketplaceSetupDialog bridge={bridge} entry={entry} connector={connector} onClose={onClose} onChanged={vi.fn()} />);
    const key = screen.getByLabelText("OpenDART API key");
    await user.type(key, "typed-but-unsaved");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    const confirmation = screen.getByRole("dialog", { name: "Discard the key you typed?" });
    expect(onClose).not.toHaveBeenCalled();
    await user.click(within(confirmation).getByRole("button", { name: "Keep editing" }));
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByLabelText("OpenDART API key")).toHaveValue("typed-but-unsaved");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await user.click(within(screen.getByRole("dialog", { name: "Discard the key you typed?" })).getByRole("button", { name: "Discard" }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("closes without asking when no key was typed", async () => {
    const user = userEvent.setup();
    const { bridge, entry, connector } = await openDart();
    const onClose = vi.fn();
    render(<MarketplaceSetupDialog bridge={bridge} entry={entry} connector={connector} onClose={onClose} onChanged={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledOnce();
    expect(screen.queryByRole("dialog", { name: "Discard the key you typed?" })).toBeNull();
  });

  it("asks before Escape drops a typed key too", async () => {
    const user = userEvent.setup();
    const { bridge, entry, connector } = await openDart();
    const onClose = vi.fn();
    render(<MarketplaceSetupDialog bridge={bridge} entry={entry} connector={connector} onClose={onClose} onChanged={vi.fn()} />);
    await user.type(screen.getByLabelText("OpenDART API key"), "typed");
    await user.keyboard("{Escape}");
    expect(screen.getByRole("dialog", { name: "Discard the key you typed?" })).toBeVisible();
    expect(onClose).not.toHaveBeenCalled();
  });
});

it("shows a localized recovery message for a rejected key instead of the raw provider error", async () => {
  const { bridge, entry, connector } = await openDart();
  const status = { ...connector, credentials: connector.credentials.map((credential) => ({
    ...credential, stored: true, pending: true, verification: "rejected" as const, last_error: "HTTP 401 raw provider payload",
  })) };
  render(<MarketplaceSetupDialog bridge={bridge} entry={entry} connector={status} onClose={vi.fn()} onChanged={vi.fn()} />);
  expect(screen.getByText("The provider rejected these credentials. Check the key and try again.")).toBeVisible();
  expect(screen.queryByText(/raw provider payload/)).toBeNull();
});
