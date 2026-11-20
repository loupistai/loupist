import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "@/i18n";
import { MockLoupistBridge } from "@/bridge/mockBridge";
import type { ProviderConnectFailure } from "@/types";
import { ConnectStep } from "../onboarding/ConnectStep";

const rejectedCopy = {
  en: "You approved the browser sign-in, but ChatGPT didn't finish connecting and gave no reason. Try again for a new sign-in, which may work, or use an OpenAI API key.",
  ko: "브라우저 로그인은 승인했지만 ChatGPT가 이유를 알리지 않고 연결을 완료하지 않았습니다. 다시 시도해 새로 로그인하면 연결될 수도 있고, OpenAI API 키로 연결할 수도 있습니다.",
};

afterEach(async () => { await i18n.changeLanguage("en"); });

it.each(["en", "ko"] as const)("offers a key and retry after a rejected ChatGPT exchange in %s", async language => {
  await i18n.changeLanguage(language);
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "providerConnect").mockRejectedValue({ code: "provider_connect_failed", message: "chatgpt_connection_rejected" });
  const user = userEvent.setup();
  render(<ConnectStep bridge={bridge} catalog={await bridge.modelCatalogGet()} connected={null} onCatalogChange={vi.fn()} onConnected={vi.fn()} onWaitingChange={vi.fn()} />);
  await user.click(screen.getByRole("button", { name: i18n.t("wizard.chatgpt", { ns: "onboarding" }) }));
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent(rejectedCopy[language]);
  expect(alert).toHaveAttribute("data-tone", "warning");
  expect(within(alert).getByRole("button", { name: i18n.t("wizard.retry", { ns: "onboarding" }) })).toBeEnabled();
  await user.click(within(alert).getByRole("button", { name: i18n.t("wizard.useApiKey", { ns: "onboarding" }) }));
  expect(within(await screen.findByRole("dialog")).getByLabelText(i18n.t("providers.apiKey", { ns: "settings" }), { exact: false })).toBeVisible();
});

it.each(["denied", "plan_permission_declined", "chatgpt_connection_rejected", "other"] as ProviderConnectFailure[])("releases waiting after %s and allows another provider", async reason => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  let fail!: () => void;
  const connect = vi.spyOn(bridge, "providerConnect").mockImplementationOnce((_provider, observer) => {
    observer({ type: "opening_browser", opened: true, url: null, message: "" });
    return new Promise((_resolve, reject) => { fail = () => { observer({ type: "failed", reason }); reject({ code: "provider_connect_failed", message: reason }); }; });
  });
  const onWaitingChange = vi.fn();
  const onConnected = vi.fn();
  const user = userEvent.setup();
  render(<ConnectStep bridge={bridge} catalog={await bridge.modelCatalogGet()} connected={null} onCatalogChange={vi.fn()} onConnected={onConnected} onWaitingChange={onWaitingChange} />);
  await user.click(screen.getByRole("button", { name: "Continue with ChatGPT" }));
  expect(await screen.findByText("Finish signing in to ChatGPT in your browser")).toBeVisible();
  await act(async () => { fail(); });
  const alert = await screen.findByRole("alert");
  expect(screen.queryByText("Finish signing in to ChatGPT in your browser")).toBeNull();
  await waitFor(() => expect(onWaitingChange).toHaveBeenLastCalledWith(false));
  if (reason !== "other") expect(within(alert).getByRole("button", { name: "Use an OpenAI API key" })).toBeEnabled();
  await user.click(screen.getByRole("button", { name: "Continue with SuperGrok" }));
  await waitFor(() => expect(onConnected).toHaveBeenCalledWith("SuperGrok"));
  expect(connect).toHaveBeenLastCalledWith("xai", expect.any(Function));
});
