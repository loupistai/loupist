import { act, cleanup, fireEvent, render, renderHook, screen, waitFor, within } from "@testing-library/react";
import { MockLoupistBridge } from "../bridge/mockBridge";
import { useConfiguration } from "../app/useConfiguration";
import { AppearanceSettings } from "../components/settings/AppearanceSettings";
import i18n, { resources, useTranslation } from "./index";
import { autoPriceLocale, resolveLanguage, resolvePriceColors, useLanguage } from "./useLanguage";
import { formatTime } from "./format";

afterEach(async () => { cleanup(); await i18n.changeLanguage("en"); });

it("resolves explicit preference before the ordered OS language list, then English", () => {
  expect(resolveLanguage("en", ["ko-KR"])).toBe("en");
  expect(resolveLanguage("system", ["fr-FR", "ko-KR", "en-US"])).toBe("ko");
  expect(resolveLanguage("system", ["fr-FR"])).toBe("en");
});

it("switches subscribed UI and document language immediately after saving, and persists across mounts", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  function Consumer() {
    const config = useConfiguration(bridge, { kind: "global" });
    const language = useLanguage(config);
    const { i18n: active } = useTranslation();
    return <button disabled={!config.snapshot} onClick={() => language.setPreference("ko")}>{active.language}</button>;
  }
  const first = render(<Consumer />);
  await waitFor(() => expect(screen.getByRole("button")).toBeEnabled());
  fireEvent.click(screen.getByRole("button"));
  await waitFor(() => expect(screen.getByRole("button")).toHaveTextContent("ko"));
  expect(document.documentElement.lang).toBe("ko");
  first.unmount();
  const second = renderHook(() => useLanguage(useConfiguration(bridge, { kind: "global" })));
  await waitFor(() => expect(second.result.current.preference).toBe("ko"));
  const snapshot = await bridge.configRead({ scope: { kind: "global" } });
  await bridge.configUpdate({ scope: snapshot.scope, expectedRevision: snapshot.revision, set: { appearance: { language: "en" } }, unset: [] });
  act(() => window.dispatchEvent(new Event("focus")));
  await waitFor(() => expect(document.documentElement.lang).toBe("en"));
});

it("colours prices by the stored choice, and Automatic by the interface language", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const hook = renderHook(() => {
    const config = useConfiguration(bridge, { kind: "global" });
    return { config, language: useLanguage(config) };
  });
  await waitFor(() => expect(hook.result.current.config.snapshot?.effective.appearance.price_colors).toBe("auto"));
  await act(() => hook.result.current.language.setPreference("ko"));
  await waitFor(() => expect(document.documentElement.dataset.priceColors).toBe("red-up"));
  await act(() => hook.result.current.config.update({ appearance: { price_colors: "green-up" } }));
  await waitFor(() => expect(document.documentElement.dataset.priceColors).toBe("green-up"));
  hook.unmount();
  // The choice is stored by the host: a new mount reads it back once the settings load,
  // even when the language says otherwise (English with red-up).
  const snapshot = await bridge.configRead({ scope: { kind: "global" } });
  await bridge.configUpdate({ scope: snapshot.scope, expectedRevision: snapshot.revision, set: { appearance: { language: "en", price_colors: "red-up" } }, unset: [] });
  const again = renderHook(() => {
    const config = useConfiguration(bridge, { kind: "global" });
    return { config, language: useLanguage(config) };
  });
  await waitFor(() => expect(again.result.current.config.snapshot?.effective.appearance.price_colors).toBe("red-up"));
  await waitFor(() => expect(document.documentElement.lang).toBe("en"));
  expect(document.documentElement.dataset.priceColors).toBe("red-up");
});

it("resolves Automatic price colours from the locale and keeps an explicit choice", () => {
  expect(resolvePriceColors("auto", "ko-KR")).toBe("red-up");
  expect(resolvePriceColors("auto", "ja")).toBe("red-up");
  expect(resolvePriceColors("auto", "zh-Hant-TW")).toBe("red-up");
  expect(resolvePriceColors("auto", "en-US")).toBe("green-up");
  expect(resolvePriceColors("auto", "de-DE")).toBe("green-up");
  expect(resolvePriceColors("green-up", "ko")).toBe("green-up");
  expect(resolvePriceColors("red-up", "en")).toBe("red-up");
  // A Japanese Mac in English still follows the system's convention.
  expect(autoPriceLocale("system", ["ja-JP", "en-US"], "en")).toBe("ja-JP");
  expect(autoPriceLocale("en", ["ja-JP"], "en")).toBe("en");
});

it("offers the price colour choices in Appearance, each sampled in its own colours", () => {
  const change = vi.fn();
  render(<AppearanceSettings theme="system" onThemeChange={vi.fn()} language="en" onLanguageChange={vi.fn()} priceColors="auto" onPriceColorsChange={change} />);
  const group = screen.getByRole("group", { name: resources.en.settings.appearance.priceColors.label });
  expect(within(group).getByRole("button", { name: new RegExp(resources.en.settings.appearance.priceColors.auto) })).toHaveAttribute("aria-pressed", "true");
  fireEvent.click(within(group).getByRole("button", { name: resources.en.settings.appearance.priceColors.redUp }));
  expect(change).toHaveBeenCalledWith("red-up");
  expect(group.querySelector('[data-price-choice="red-up"] [data-price-preview="red-up"]')).not.toBeNull();
  // Automatic samples what it resolves to for English.
  expect(group.querySelector('[data-price-choice="auto"] [data-price-preview="green-up"]')).not.toBeNull();
});

it("pseudo-localizes copy while preserving interpolation", async () => {
  await i18n.changeLanguage("en-XA");
  expect(i18n.t("appearance.theme.optionLabel", { ns: "settings", theme: "DARK" })).toContain("DARK");
  expect(i18n.t("appearance.theme.label", { ns: "settings" })).toMatch(/^⟦.*⟧$/);
});
it("renders Korean appearance with English placeholder values and English fallback for missing copy", async () => {
  await i18n.changeLanguage("ko");
  const change = vi.fn();
  const ko = i18n.getResourceBundle("ko", "settings");
  i18n.removeResourceBundle("ko", "settings");
  render(<AppearanceSettings theme="system" onThemeChange={vi.fn()} language="ko" onLanguageChange={change} />);
  expect(screen.getByRole("heading", { name: resources.en.settings.appearance.theme.label })).toBeInTheDocument();
  expect(screen.getByText(resources.en.settings.appearance.language.description)).toBeInTheDocument();
  const group = screen.getByRole("group", { name: "Language" });
  expect(within(group).getByRole("button", { name: "한국어" })).toHaveAttribute("aria-pressed", "true");
  fireEvent.click(within(group).getByRole("button", { name: "English" }));
  expect(change).toHaveBeenCalledWith("en");
  i18n.addResourceBundle("ko", "settings", ko);
  expect(i18n.t("appearance.theme.label", { ns: "settings" })).toBe(resources.ko.settings.appearance.theme.label);
});

it.each([
  ["en", "9:00 AM"],
  ["ko", "오전 9:00"],
])("formats the local clock in %s without a leading hour zero", async (locale, expected) => {
  await i18n.changeLanguage(locale);
  const morning = new Date(2026, 9, 28, 9, 0);
  expect(formatTime(morning)).toBe(expected);
  expect(formatTime(morning.getTime())).toBe(expected);
});

it.each([
  ["AM", "오전"], ["오후", "오후"],
])("normalizes only English Korean clock markers from ICU: %s", async (marker, expected) => {
  await i18n.changeLanguage("ko");
  const parts = vi.spyOn(Intl.DateTimeFormat.prototype, "formatToParts").mockReturnValue([
    { type: "dayPeriod", value: marker }, { type: "literal", value: " " },
    { type: "hour", value: "9" }, { type: "literal", value: ":" }, { type: "minute", value: "00" },
  ]);
  try {
    expect(formatTime(new Date(2026, 9, 28, 9))).toBe(`${expected} 9:00`);
  } finally {
    parts.mockRestore();
  }
});

it("names methods one way in both languages", () => {
  const text = JSON.stringify(resources);
  expect(text).not.toContain("작업 방식");
  expect(text).not.toMatch(/ways? of working/i);
});
