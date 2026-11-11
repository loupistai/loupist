/** Mock in-app browser tabs: Rust's twelve-tab limit and the load events a tab emits. */
import type * as Contract from "../../types";
import { httpAddressError, parseCredentialFreeHttpUrl } from "../../lib/credentialFreeUrl";

/** A credential-free http(s) address of at most 8 KiB, as Rust validates it. */
export function validBrowserUrl(raw: string) {
  if (new TextEncoder().encode(raw).byteLength > 8 * 1024) {
    throw new Error("External link is too long.");
  }
  const parsed = parseCredentialFreeHttpUrl(raw);
  if (!parsed) {
    throw new Error(httpAddressError);
  }
  return parsed;
}

export class MockBrowserTabs {
  private readonly tabs = new Map<string, { state: Contract.BrowserTabState; observer: Contract.StreamObserver<Contract.BrowserTabEvent> }>();

  async open(
    request: Contract.BrowserTabOpenRequest,
    observer: Contract.StreamObserver<Contract.BrowserTabEvent>,
  ) {
    if (this.tabs.size >= 12) {
      throw new Error("Close a browser tab before opening another one.");
    }
    const parsed = validBrowserUrl(request.url);
    const tab_id = crypto.randomUUID();
    const state: Contract.BrowserTabState = {
      tab_id,
      url: parsed.href,
      title: parsed.hostname,
      loading: false,
    };
    this.tabs.set(tab_id, { state, observer });
    queueMicrotask(() => {
      observer({ type: "load_finished", tab_id, url: parsed.href });
      observer({ type: "title_changed", tab_id, title: parsed.hostname });
    });
    return structuredClone(state);
  }

  async navigate(tab_id: string, url: string) {
    const tab = this.tabs.get(tab_id);
    if (!tab) throw new Error("Browser tab was not found.");
    const parsed = validBrowserUrl(url);
    tab.state = { ...tab.state, url: parsed.href, title: parsed.hostname };
    tab.observer({ type: "load_started", tab_id, url: parsed.href });
    tab.observer({ type: "load_finished", tab_id, url: parsed.href });
    tab.observer({ type: "title_changed", tab_id, title: parsed.hostname });
  }

  async history(
    tab_id: string,
    _direction: Contract.BrowserHistoryDirection,
  ) {
    if (!this.tabs.has(tab_id))
      throw new Error("Browser tab was not found.");
  }

  async reload(tab_id: string) {
    const tab = this.tabs.get(tab_id);
    if (!tab) throw new Error("Browser tab was not found.");
    tab.observer({ type: "load_started", tab_id, url: tab.state.url });
    tab.observer({ type: "load_finished", tab_id, url: tab.state.url });
  }

  async setBounds(request: Contract.BrowserTabBoundsRequest) {
    if (!this.tabs.has(request.tab_id)) {
      throw new Error("Browser tab was not found.");
    }
  }

  async close(tab_id: string) {
    if (!this.tabs.delete(tab_id))
      throw new Error("Browser tab was not found.");
  }

  async reset() {
    this.tabs.clear();
  }
}
