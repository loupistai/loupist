import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "@/i18n";
import { MockLoupistBridge } from "@/bridge/mockBridge";
import type { ProviderConnectFailure } from "@/types";
import { ConnectStep } from "../onboarding/ConnectStep";

const rejectedCopy = {
  en: "OpenAI approved the sign-in in the browser but did not let this account connect. This can happen when the ChatGPT plan or workspace isn’t allowed to be used in other apps. Try again, or connect with an OpenAI API key or another account.",
  ko: "브라우저에서는 승인됐지만 OpenAI가 이 계정의 연결을 허용하지 않았습니다. ChatGPT 플랜이나 워크스페이스가 다른 앱에서의 사용을 허용하지 않을 때 이렇게 될 수 있습니다. 다시 시도하거나 OpenAI API 키 또는 다른 계정으로 연결하십시오.",
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
