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
    expect(screen.getByText("KOSPI daily trading (stk_bydd_trd): Verified")).toBeVisible();
    expect(screen.getByText("KOSDAQ daily trading (ksq_bydd_trd): Needs KRX approval")).toBeVisible();
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
