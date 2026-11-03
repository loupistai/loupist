import { useCallback, useState, type FormEvent } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import i18n, { resources } from "@/i18n";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { BrowserTabEvent, BrowserTabState } from "../../types";
import type { ChatWorkspaceSession, ChatWorkspaceTab } from "../../chat/workspace";
import { useNativeBrowserTabs } from "./useNativeBrowserTabs";

const session: ChatWorkspaceSession = { tabs: [{ id: "local-browser", kind: "browser", url: "https://example.com/", title: "Example", loading: true }], active_tab_id: "local-browser" };
const native: BrowserTabState = { tab_id: "native-browser", url: "https://example.com/", title: "Example", loading: false };
const message = (key: keyof typeof resources.en.chat.useNativeBrowserTabs) => i18n.t(`chat:useNativeBrowserTabs.${key}`);
afterEach(async () => { await i18n.changeLanguage("en"); });
function setup(bridge: MockLoupistBridge) {
  return renderHook(({ loupistId }) => {
    const [current, setSession] = useState(session);
    const onUpdateTab = useCallback((id: string, update: (tab: ChatWorkspaceTab) => ChatWorkspaceTab) => {
      setSession((value) => ({ ...value, tabs: value.tabs.map((tab) => tab.id === id ? update(tab) : tab) }));
    }, []);
    const active = current.tabs.find((tab) => tab.id === current.active_tab_id);
    const browser = useNativeBrowserTabs({ bridge, loupistId, threadId: "chat", open: true, session: current, activeTab: active?.kind === "browser" ? active : undefined, menuOpen: false, placement: "side", width: 600, height: 400, onUpdateTab, onOpenLink: vi.fn() });
    return { browser, current, setSession };
  }, { initialProps: { loupistId: "loupist-a" } });
}

describe("Native browser lifecycle", () => {
  it("keeps a creation failure visible until explicit retry", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const open = vi.spyOn(bridge, "browserTabOpen").mockRejectedValueOnce(new Error("Native creation failed")).mockResolvedValue(native);
    const { result } = setup(bridge);
    await waitFor(() => expect(result.current.current.tabs[0]).toMatchObject({ error: message("couldNotOpenThisWebPage"), loading: false }));
    expect(open).toHaveBeenCalledOnce();
    await act(async () => result.current.browser.retry());
    await waitFor(() => expect(result.current.current.tabs[0]).toMatchObject({ native_id: "native-browser", error: undefined }));
    expect(open).toHaveBeenCalledTimes(2);
  });

  it("retries the current URL on the existing child and exposes external-open failures", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const open = vi.spyOn(bridge, "browserTabOpen").mockResolvedValue(native);
    const navigate = vi.spyOn(bridge, "browserTabNavigate").mockRejectedValueOnce(new Error("Navigation failed")).mockResolvedValue(undefined);
    vi.spyOn(bridge, "openExternalUrl").mockRejectedValue(new Error("System browser unavailable"));
    const { result } = setup(bridge);
    await waitFor(() => expect(result.current.current.tabs[0]).toMatchObject({ native_id: native.tab_id }));
    act(() => result.current.browser.setAddress("https://unsent.example/"));
    await act(async () => result.current.browser.retry());
    expect(navigate).toHaveBeenLastCalledWith(native.tab_id, native.url);
    expect(result.current.current.tabs[0]).toMatchObject({ error: message("couldNotOpenThisWebPage"), loading: false });
    await act(async () => result.current.browser.retry());
    expect(result.current.current.tabs[0]).toMatchObject({ error: undefined, loading: true });
    expect(open).toHaveBeenCalledOnce();
    await act(async () => result.current.browser.openExternal(native.url));
    expect(result.current.current.tabs[0]).toMatchObject({ error: message("couldNotOpenTheSystemBrowser") });
  });

  it.each(["en", "ko"])("localizes blocked links and keeps the loaded page in %s", async (locale) => {
    await i18n.changeLanguage(locale);
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    let emit!: (event: BrowserTabEvent) => void;
    vi.spyOn(bridge, "browserTabOpen").mockImplementation(async (_request, observer) => {
      emit = observer;
      return native;
    });
    const { result } = setup(bridge);
    await waitFor(() => expect(result.current.current.tabs[0]).toMatchObject({ native_id: native.tab_id }));
    act(() => emit({ type: "navigation_blocked", tab_id: native.tab_id, reason: "unsupported_protocol" }));
    expect(result.current.current.tabs[0]).toMatchObject({ url: native.url, title: native.title, error: message("onlyWebAddresses") });
    // A placeholder must never replace the committed page or erase its recovery message.
    act(() => {
      emit({ type: "load_started", tab_id: native.tab_id, url: "about:blank" });
      emit({ type: "load_finished", tab_id: native.tab_id, url: "about:blank" });
      emit({ type: "load_finished", tab_id: native.tab_id, url: native.url });
    });
    expect(result.current.current.tabs[0]).toMatchObject({ url: native.url, title: native.title, loading: false, error: message("onlyWebAddresses") });
    // A subsequent real main-document navigation recovers normally, including redirects.
    act(() => {
      emit({ type: "load_started", tab_id: native.tab_id, url: "https://example.com/report" });
      emit({ type: "load_finished", tab_id: native.tab_id, url: "https://example.com/final-report" });
    });
    expect(result.current.current.tabs[0]).toMatchObject({ url: "https://example.com/final-report", loading: false, error: undefined });
  });

  it.each([
    ["browser_url_credentials", "addressContainsSignInDetails"],
    ["browser_url_too_long", "addressTooLong"],
    ["browser_url_invalid", "invalidWebAddress"],
    ["browser_url_unsupported_protocol", "onlyWebAddresses"],
    ["internal", "couldNotOpenThisWebPage"],
  ] as const)("localizes the host's %s failure instead of showing its internal message", async (code, key) => {
    await i18n.changeLanguage("ko");
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "browserTabOpen").mockRejectedValue({ code, message: "Raw internal WebKit failure" });
    const { result } = setup(bridge);
    await waitFor(() => expect(result.current.current.tabs[0]).toMatchObject({ error: message(key), loading: false }));
  });

  it.each([
    ["file:///tmp/report", "onlyWebAddresses"],
    ["javascript:alert(1)", "onlyWebAddresses"],
    ["data:text/html,report", "onlyWebAddresses"],
    ["ftp://example.com/report", "onlyWebAddresses"],
    ["about:blank", "onlyWebAddresses"],
    ["https://user:secret@example.com/report", "addressContainsSignInDetails"],
    ["https://user@example.com/report", "addressContainsSignInDetails"],
    ["not a web address", "enterAWebAddressNotASearchQuery"],
    [`https://example.com/${"x".repeat(8 * 1024)}`, "addressTooLong"],
  ] as const)("rejects an unsafe or invalid address before navigating (%#)", async (address, key) => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "browserTabOpen").mockResolvedValue(native);
    const navigate = vi.spyOn(bridge, "browserTabNavigate");
    const { result } = setup(bridge);
    await waitFor(() => expect(result.current.current.tabs[0]).toMatchObject({ native_id: native.tab_id }));
    act(() => result.current.browser.setAddress(address));
    await act(async () => result.current.browser.navigate({ preventDefault: vi.fn() } as unknown as FormEvent));
    expect(navigate).not.toHaveBeenCalled();
    expect(result.current.current.tabs[0]).toMatchObject({ url: native.url, error: message(key) });
  });

  it.each([
    ["newsroom.arm.com/news/results", "https://newsroom.arm.com/news/results"],
    ["http://localhost:1420/report", "http://localhost:1420/report"],
    ["localhost:1420/report", "https://localhost:1420/report"],
    ["example.com:8080/report", "https://example.com:8080/report"],
    ["https://example.com/report%20notes#results", "https://example.com/report%20notes#results"],
  ])("navigates a valid web address (%s)", async (address, expected) => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "browserTabOpen").mockResolvedValue(native);
    const navigate = vi.spyOn(bridge, "browserTabNavigate").mockResolvedValue(undefined);
    const { result } = setup(bridge);
    await waitFor(() => expect(result.current.current.tabs[0]).toMatchObject({ native_id: native.tab_id }));
    act(() => result.current.browser.setAddress(address));
    await act(async () => result.current.browser.navigate({ preventDefault: vi.fn() } as unknown as FormEvent));
    expect(navigate).toHaveBeenCalledWith(native.tab_id, expected);
    expect(result.current.current.tabs[0]).toMatchObject({ url: expected, error: undefined });
  });

  it.each(["close", "switch", "unmount"])("closes a late child after %s", async (action) => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    let resolve!: (value: BrowserTabState) => void;
    vi.spyOn(bridge, "browserTabOpen").mockImplementation(() => new Promise((done) => { resolve = done; }));
    const close = vi.spyOn(bridge, "browserTabClose").mockResolvedValue(undefined);
    const hook = setup(bridge);
    const resolveOriginal = resolve;
    if (action === "close") act(() => hook.result.current.setSession({ tabs: [] }));
    else if (action === "switch") hook.rerender({ loupistId: "loupist-b" });
    else hook.unmount();
    await act(async () => resolveOriginal(native));
    await waitFor(() => expect(close).toHaveBeenCalledWith("native-browser"));
    if (action !== "unmount") expect(hook.result.current.current.tabs.every((tab) => tab.kind !== "browser" || !tab.native_id)).toBe(true);
  });
});
