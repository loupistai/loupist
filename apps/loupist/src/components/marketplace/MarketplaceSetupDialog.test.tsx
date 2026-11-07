import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { MarketplaceSetupDialog } from "./MarketplaceSetupDialog";

describe("Marketplace setup feedback", () => {
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

it("explains an unavailable connection and blocks setup while retaining its details", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const snapshot = await bridge.marketplaceSnapshot();
  const entry = snapshot.catalog.entries.find((item) => item.id === "opendart.disclosures")!;
  const connector = snapshot.connectors.find((item) => item.entry_id === entry.id)!;
  const configure = vi.spyOn(bridge, "marketplaceConnectorConfigure");
  render(<MarketplaceSetupDialog bridge={bridge} entry={entry} connector={{ ...connector, readiness: "runtime_unavailable" }} onClose={vi.fn()} onChanged={vi.fn()} />);
  expect(screen.getByRole("dialog", { name: "OpenDART" })).toBeVisible();
  expect(screen.getByRole("alert")).toHaveTextContent("Update or reinstall Loupist");
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
  it("asks before Done discards a key that is typed but not saved", async () => {
    const user = userEvent.setup();
    const { bridge, entry, connector } = await openDart();
    const onClose = vi.fn();
    render(<MarketplaceSetupDialog bridge={bridge} entry={entry} connector={connector} onClose={onClose} onChanged={vi.fn()} />);
    const key = screen.getByLabelText("OpenDART API key");
    await user.type(key, "typed-but-unsaved");
    await user.click(screen.getByRole("button", { name: "Done" }));
    const confirmation = screen.getByRole("dialog", { name: "Discard the key you typed?" });
    expect(onClose).not.toHaveBeenCalled();
    await user.click(within(confirmation).getByRole("button", { name: "Keep editing" }));
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByLabelText("OpenDART API key")).toHaveValue("typed-but-unsaved");
    await user.click(screen.getByRole("button", { name: "Done" }));
    await user.click(within(screen.getByRole("dialog", { name: "Discard the key you typed?" })).getByRole("button", { name: "Discard" }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("closes without asking when no key was typed", async () => {
    const user = userEvent.setup();
    const { bridge, entry, connector } = await openDart();
    const onClose = vi.fn();
    render(<MarketplaceSetupDialog bridge={bridge} entry={entry} connector={connector} onClose={onClose} onChanged={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: "Done" }));
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
