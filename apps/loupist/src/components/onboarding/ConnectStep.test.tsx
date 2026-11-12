import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "@/bridge/mockBridge";
import type { ModelCatalog, ProviderConnectFailure, ProviderConnectionEvent } from "@/types";
import { ConnectStep } from "./ConnectStep";

async function setup() {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  for (const provider of (await bridge.modelCatalogGet()).providers) await bridge.providerDisconnect(provider.id);
  const catalog = await bridge.modelCatalogGet();
  const onConnected = vi.fn();
  const onWaitingChange = vi.fn();
  const onCatalogChange = vi.fn<(catalog: ModelCatalog) => void>();
  const user = userEvent.setup();
  render(<ConnectStep bridge={bridge} catalog={catalog} onCatalogChange={onCatalogChange} connected={null} onConnected={onConnected} onWaitingChange={onWaitingChange} />);
  return { bridge, user, onConnected, onWaitingChange, onCatalogChange };
}

/** A sign-in that emits events, then waits until the test ends it. */
function pendingConnect(bridge: MockLoupistBridge, events: ProviderConnectionEvent[]) {
  return vi.spyOn(bridge, "providerConnect").mockImplementation((_provider, observer) => {
    for (const event of events) observer(event);
    return new Promise(() => {});
  });
}

function failingConnect(bridge: MockLoupistBridge, reason: ProviderConnectFailure) {
  return vi.spyOn(bridge, "providerConnect").mockImplementation(async (_provider, observer) => {
    observer({ type: "failed", reason });
    throw { code: "provider_connect_failed", message: reason };
  });
}

/** The waiting or device card, once its status line is announced. */
async function waitingCard() {
  await screen.findAllByRole("status");
  const card = document.querySelector<HTMLElement>("[data-signin]");
  expect(card).not.toBeNull();
  return card!;
}

it("signs in with ChatGPT through the openai provider and reports the connected name", async () => {
  const { bridge, user, onConnected, onCatalogChange } = await setup();
  const connect = vi.spyOn(bridge, "providerConnect");
  expect(screen.getByText("Uses the plan you already pay for. Loupist doesn't charge you.")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
  await waitFor(() => expect(onConnected).toHaveBeenCalledWith("ChatGPT"));
  expect(connect).toHaveBeenCalledWith("openai", expect.any(Function));
  expect(onCatalogChange).toHaveBeenCalled();
});

it("replaces the button with a waiting card, offers the link and checks a pasted address", async () => {
  const { bridge, user, onWaitingChange } = await setup();
  pendingConnect(bridge, [
    { type: "opening_browser", message: "", opened: false, url: "https://auth.openai.com/api/accounts/authorize?state=fixture" },
    { type: "manual_entry" },
  ]);
  const submit = vi.spyOn(bridge, "providerConnectSubmitCode");
  await user.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
  const card = await waitingCard();
  // Only the status line is live; the actions and the paste form are not announced.
  expect(screen.getByRole("status")).toHaveTextContent("Finish signing in to ChatGPT in your browser");
  expect(card).not.toHaveAttribute("aria-live");
  expect(card).toHaveTextContent("ChatGPT shows this connection as Loupist.");
  expect(screen.queryByRole("button", { name: "Continue with ChatGPT" })).toBeNull();
  await waitFor(() => expect(onWaitingChange).toHaveBeenLastCalledWith(true));
  // The browser did not open, so the link and the paste route are offered at once.
  expect(within(card).getByRole("button", { name: "Copy the sign-in link" })).toBeVisible();
  await user.click(within(card).getByRole("button", { name: "Browser didn't come back?" }));
  const field = within(card).getByLabelText("Paste the address your browser shows after signing in");
  expect(field.closest("[aria-live]")).toBeNull();
  await user.type(field, "https://example.com/not-the-callback");
  await user.click(within(card).getByRole("button", { name: "Continue" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("That isn't the sign-in address. Copy the whole address from your browser's address bar.");
  expect(submit).toHaveBeenCalledWith({ value: "https://example.com/not-the-callback" });
  await user.clear(field);
  await user.type(field, "http://127.0.0.1:1455/auth/callback?code=abc&state=xyz");
  await user.click(within(card).getByRole("button", { name: "Continue" }));
  await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
});

it("waits 20 seconds before offering the paste route when the browser opened", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  try {
    const { bridge, user } = await setup();
    pendingConnect(bridge, [{ type: "opening_browser", message: "", opened: true, url: null }, { type: "manual_entry" }]);
    await user.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
    await screen.findByRole("status");
    expect(screen.queryByRole("button", { name: "Browser didn't come back?" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Copy the sign-in link" })).toBeNull();
    await act(async () => { vi.advanceTimersByTime(20_000); });
    expect(screen.getByRole("button", { name: "Browser didn't come back?" })).toBeVisible();
  } finally { vi.useRealTimers(); }
});

it("shows SuperGrok's device code without a paste route", async () => {
  const { bridge, user } = await setup();
  pendingConnect(bridge, [{ type: "device_code", user_code: "WDJB-MJHT", verification_uri: "https://accounts.x.ai/oauth2/device", expires_in_s: 900 }]);
  const openAgain = vi.spyOn(bridge, "providerConnectOpenBrowser");
  const writeText = vi.fn(async () => {});
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
  expect(screen.getByText("Uses the plan you already pay for. Loupist doesn't charge you.")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Continue with SuperGrok" }));
  const card = await waitingCard();
  expect(card).toHaveTextContent("Enter this code on the xAI page");
  // The code is announced as a status, spelled character by character.
  const code = within(card).getByLabelText("W D J B - M J H T");
  expect(code.tagName).toBe("OUTPUT");
  expect(code).toHaveTextContent("WDJB-MJHT");
  expect(card).not.toHaveAttribute("aria-live");
  await user.click(within(card).getByRole("button", { name: "Copy code" }));
  expect(writeText).toHaveBeenCalledWith("WDJB-MJHT");
  expect(await within(card).findByText("Code copied")).toBeInTheDocument();
  await user.click(within(card).getByRole("button", { name: "Open the page again" }));
  expect(openAgain).toHaveBeenCalled();
  expect(within(card).queryByRole("button", { name: "Browser didn't come back?" })).toBeNull();
});

it.each([
  ["plan_permission_declined", "You didn't allow Loupist to use your ChatGPT plan, so nothing was connected.", "warning"],
  ["port_in_use", "Another sign-in is already waiting, often the Codex app or a terminal.", "warning"],
  ["timed_out", "Sign-in took too long. Try again.", "warning"],
  ["other", "Could not connect your account.", "negative"],
] as const)("names one next step when sign-in fails with %s", async (reason, text, tone) => {
  const { bridge, user, onConnected } = await setup();
  const connect = failingConnect(bridge, reason);
  await user.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent(text);
  expect(alert).toHaveAttribute("data-tone", tone);
  connect.mockRestore();
  await user.click(within(alert).getByRole("button", { name: "Try again" }));
  await waitFor(() => expect(onConnected).toHaveBeenCalledWith("ChatGPT"));
  expect(screen.queryByRole("alert")).toBeNull();
});

it("offers an OpenAI key when the ChatGPT plan is not eligible", async () => {
  const { bridge, user } = await setup();
  failingConnect(bridge, "plan_not_eligible");
  await user.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("This ChatGPT plan can't be used in other apps.");
  await user.click(within(alert).getByRole("button", { name: "Use an OpenAI API key" }));
  const sheet = await screen.findByRole("dialog");
  expect(within(sheet).getByRole("heading", { name: "OpenAI" })).toBeVisible();
  expect(within(sheet).getByLabelText("API key")).toBeVisible();
});

it("cancels a waiting sign-in with Cancel or Escape and shows no error", async () => {
  const { bridge, user, onWaitingChange } = await setup();
  pendingConnect(bridge, [{ type: "opening_browser", message: "", opened: true, url: null }]);
  const cancel = vi.spyOn(bridge, "providerConnectCancel");
  await user.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
  await user.click(within(await waitingCard()).getByRole("button", { name: "Cancel" }));
  expect(cancel).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("button", { name: "Continue with ChatGPT" })).toBeEnabled();
  expect(screen.queryByRole("alert")).toBeNull();
  await waitFor(() => expect(onWaitingChange).toHaveBeenLastCalledWith(false));
  await user.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
  await screen.findByRole("status");
  await user.keyboard("{Escape}");
  expect(cancel).toHaveBeenCalledTimes(2);
  expect(screen.queryByRole("status")).toBeNull();
});

it("lists every other way and opens key, local and custom rows in the connect sheet", async () => {
  const { user } = await setup();
  await user.click(screen.getByText("Other ways to connect"));
  for (const name of ["Anthropic", "OpenAI", "Google Gemini", "xAI", /Upstage Solar/, /OpenRouter/, /Use a model on this Mac/, "Custom endpoint"]) {
    expect(screen.getByRole("button", { name })).toBeVisible();
  }
  // ChatGPT and SuperGrok sign-in are the buttons above the list, not rows in it.
  const other = document.querySelector<HTMLElement>("details.onboarding-other")!;
  expect(within(other).queryByRole("button", { name: /ChatGPT/ })).toBeNull();
  expect(within(other).queryByRole("button", { name: /SuperGrok/ })).toBeNull();
  // Anthropic does not permit third-party apps to offer Claude sign-in, so Claude is only a key row.
  expect(screen.queryByRole("button", { name: /Sign in with Claude/ })).toBeNull();
  expect(screen.getByText("Your key stays on this device. Requests go straight to the provider.")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Google Gemini" }));
  const sheet = await screen.findByRole("dialog");
  expect(within(sheet).getByRole("heading", { name: "Google Gemini" })).toBeVisible();
  // The list stays open behind the sheet.
  expect(document.querySelector("details.onboarding-other")).toHaveAttribute("open");
});
