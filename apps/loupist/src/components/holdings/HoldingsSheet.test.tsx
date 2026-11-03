import { useState } from "react";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "@/bridge/mockBridge";
import { HoldingsSheet } from "./HoldingsSheet";
import { detectNetwork, looksLikeSecret } from "./walletAddress";

function Harness({ bridge, onImported }: { bridge: MockLoupistBridge; onImported?: Parameters<typeof HoldingsSheet>[0]["onImported"] }) {
  const [open, setOpen] = useState(true);
  return <HoldingsSheet bridge={bridge} open={open} onOpenChange={setOpen} onImported={onImported} />;
}

const setup = () => ({ bridge: new MockLoupistBridge({ delay_ms: 0 }), user: userEvent.setup() });

it("shows each source as not imported, then imports a Kraken file with quantities only in the preview", async () => {
  const { bridge, user } = setup();
  const imported = vi.fn();
  vi.spyOn(bridge, "pickFile").mockResolvedValue("/mock/kraken-ledger.csv");
  render(<Harness bridge={bridge} onImported={imported} />);
  const sheet = await screen.findByRole("dialog", { name: "Your holdings" });
  expect(within(sheet).getAllByText("not imported")).toHaveLength(2);
  expect(within(sheet).getByText("not connected")).toBeVisible();
  expect(within(sheet).getByRole("button", { name: "Import" })).toBeDisabled();
  expect(within(sheet).getByText("Connect Korea Investment in Settings > Connections")).toBeVisible();

  const crypto = within(sheet).getAllByRole("button", { name: "Import file" })[1]!;
  await user.click(crypto);
  expect(await within(sheet).findByText(/^Kraken ledger · 412 rows · balances as of/)).toBeVisible();
  const table = within(sheet).getByRole("table");
  expect(within(table).getByText("0.4271")).toBeVisible();
  expect(within(table).getByText("XETH, ETH2.S")).toBeVisible();
  expect(within(sheet).getByText("Skipped: USD, EUR (cash) · DOGE (zero balance)")).toBeVisible();
  await user.click(within(sheet).getByRole("button", { name: "Import 3 assets" }));
  await waitFor(() => expect(within(sheet).getByText(/^imported .* · 3 assets$/)).toBeVisible());
  expect(imported).toHaveBeenCalledWith(expect.arrayContaining([{ symbol: "BTC", market: "CRYPTO", displayName: null }]));
  // After import the sheet shows counts and dates, never quantities.
  expect(sheet).not.toHaveTextContent("0.4271");
});

it("asks for the asset and balance columns, and refuses a file of trades", async () => {
  const { bridge, user } = setup();
  const map = vi.spyOn(bridge, "holdingsMapColumns");
  vi.spyOn(bridge, "pickFile").mockResolvedValueOnce("/mock/my-columns.csv").mockResolvedValueOnce("/mock/trades.csv");
  render(<Harness bridge={bridge} />);
  const sheet = await screen.findByRole("dialog", { name: "Your holdings" });
  await user.click(within(sheet).getAllByRole("button", { name: "Import file" })[1]!);
  const asset = await within(sheet).findByRole("combobox", { name: "Which column is the asset?" });
  const balance = within(sheet).getByRole("combobox", { name: "Which column is the balance?" });
  const next = within(sheet).getByRole("button", { name: "Continue" });
  expect(next).toBeDisabled();
  await user.selectOptions(asset, "Token");
  await user.selectOptions(balance, "Token");
  expect(next).toBeDisabled();
  await user.selectOptions(balance, "Holding");
  await user.click(next);
  expect(map).toHaveBeenCalledWith(expect.objectContaining({ asset: "Token", balance: "Holding" }));
  expect(await within(sheet).findByText(/^Your columns/)).toBeVisible();
  await user.click(within(sheet).getByRole("button", { name: "Cancel" }));

  await user.click(within(sheet).getAllByRole("button", { name: "Import file" })[1]!);
  expect(await within(sheet).findByRole("alert")).toHaveTextContent("This file lists trades, not balances.");
  expect(within(sheet).queryByRole("button", { name: /^Import \d/ })).toBeNull();
  // The refused file names itself and offers both ways forward; the first action has focus, not the dialog.
  expect(within(sheet).getByText("trades.csv")).toBeVisible();
  expect(within(sheet).getByRole("button", { name: "Choose another file" })).toBeVisible();
  await waitFor(() => expect(within(sheet).getByRole("button", { name: "Cancel" })).toHaveFocus());
  await user.click(within(sheet).getByRole("button", { name: "Add a wallet address" }));
  expect(await within(sheet).findByRole("textbox", { name: "Address" })).toBeVisible();
});

it("names the file whose columns it asks about", async () => {
  const { bridge, user } = setup();
  vi.spyOn(bridge, "pickFile").mockResolvedValue("/mock/my-columns.csv");
  render(<Harness bridge={bridge} />);
  const sheet = await screen.findByRole("dialog", { name: "Your holdings" });
  await user.click(within(sheet).getAllByRole("button", { name: "Import file" })[1]!);
  expect(await within(sheet).findByText("File: my-columns.csv")).toBeVisible();
  await waitFor(() => expect(within(sheet).getByRole("combobox", { name: "Which column is the asset?" })).toHaveFocus());
});

it("clears a pasted recovery phrase, detects networks and shows wallets only masked", async () => {
  const { bridge, user } = setup();
  const add = vi.spyOn(bridge, "walletAdd");
  render(<Harness bridge={bridge} />);
  const sheet = await screen.findByRole("dialog", { name: "Your holdings" });
  await user.click(within(sheet).getByRole("button", { name: "Add a wallet address" }));
  const field = within(sheet).getByRole("textbox", { name: "Address" });
  fireEvent.change(field, { target: { value: "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about" } });
  expect(field).toHaveValue("");
  expect(within(sheet).getByRole("alert")).toHaveTextContent("This looks like a recovery phrase or private key.");
  // The warning sits right under the Address field and describes it.
  expect(field.closest("label")!.nextElementSibling).toHaveAttribute("data-wallet-secret");
  expect(field).toHaveAttribute("aria-invalid", "true");
  expect(field).toHaveAccessibleDescription(/This looks like a recovery phrase/);
  expect(add).not.toHaveBeenCalled();

  fireEvent.change(field, { target: { value: "not-an-address" } });
  expect(within(sheet).getByText("Not a supported address")).toBeVisible();
  expect(within(sheet).getByRole("button", { name: "Add and read" })).toBeDisabled();

  const address = "0x71C7656EC7ab88b098defB751B7401B5f6d8976F";
  fireEvent.change(field, { target: { value: address } });
  expect(within(sheet).getByText("Detected: Ethereum, Base, Arbitrum, Optimism and Polygon")).toBeVisible();
  await user.type(within(sheet).getByRole("textbox", { name: "Name (optional)" }), "Cold storage");
  await user.click(within(sheet).getByRole("button", { name: "Add and read" }));
  expect(add).toHaveBeenCalledWith({ address, name: "Cold storage" });
  const row = await within(sheet).findByLabelText("Ethereum + 4 networks wallet ending 976F");
  expect(row).toHaveTextContent("0x71C7…976F");
  expect(sheet).not.toHaveTextContent(address);
  expect(within(sheet).getByText(/4 unlisted tokens skipped/)).toBeVisible();
});

it("keeps the last good read when a refresh fails and removes a wallet after confirming", async () => {
  const { bridge, user } = setup();
  await bridge.walletAdd({ address: "bc1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qccfmv3" });
  const [wallet] = await bridge.walletsList();
  vi.spyOn(bridge, "walletRefresh").mockResolvedValue([{ ...wallet!, status: "failed", lastReadAtMs: Date.now() }]);
  const remove = vi.spyOn(bridge, "walletRemove");
  render(<Harness bridge={bridge} />);
  const sheet = await screen.findByRole("dialog", { name: "Your holdings" });
  await user.click(await within(sheet).findByRole("button", { name: /^Refresh · Bitcoin wallet ending/ }));
  expect(await within(sheet).findByText(/^Couldn't read · last read/)).toBeVisible();
  await user.click(within(sheet).getByRole("button", { name: /^More for Bitcoin wallet/ }));
  await user.click(await screen.findByRole("menuitem", { name: "Remove" }));
  expect(within(sheet).getByText("Remove this wallet? Its balances are deleted from this computer.")).toBeVisible();
  await user.click(within(sheet).getByRole("button", { name: "Remove" }));
  expect(remove).toHaveBeenCalledWith({ walletId: wallet!.id });
});

it("replaces Add with the limit at ten wallets", async () => {
  const { bridge } = setup();
  for (let index = 1; index <= 10; index++) await bridge.walletAdd({ address: `0x${index.toString(16).padStart(40, "0")}` });
  render(<Harness bridge={bridge} />);
  const sheet = await screen.findByRole("dialog", { name: "Your holdings" });
  expect(await within(sheet).findByText("You can add up to 10 wallets.")).toBeVisible();
  expect(within(sheet).queryByRole("button", { name: "Add a wallet address" })).toBeNull();
});

it("detects networks and secrets the way Rust checks them", () => {
  expect(detectNetwork("bc1qrp33g0q5c5txsp9arysrx4k6zdkfs4nce4xj0gdcccefvpysxf3qccfmv3")).toBe("bitcoin");
  expect(detectNetwork("1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa")).toBe("bitcoin");
  expect(detectNetwork("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed")).toBe("evm");
  expect(detectNetwork("9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM")).toBe("solana");
  expect(detectNetwork("hello")).toBeNull();
  expect(looksLikeSecret("0c28fca386c7a227600b2fe50b7cae11ec86d3bf1fbe471be89827e19d72aa1d")).toBe(true);
  expect(looksLikeSecret("5HueCGU8rMjxEXxiPuD5BDku4MkFqeZyd4dZ1jvhTVqvbTLvyTJ")).toBe(true);
  expect(looksLikeSecret("0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed")).toBe(false);
});
