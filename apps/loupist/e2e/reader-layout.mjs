import assert from "node:assert/strict";

/** Browser callback shared by design previews and native layout probes. */
export function measureReaderLayout() {
  const box = element => {
    if (!element) return null;
    const rect = element.getBoundingClientRect();
    return { x: rect.x, y: rect.y, right: rect.right, bottom: rect.bottom, width: rect.width, height: rect.height };
  };
  const shown = element => element && !element.closest('[hidden], [inert], [aria-hidden="true"]') && element.getBoundingClientRect().width > 0;
  const closeButtons = new Map([...document.querySelectorAll('[data-tab-close]')].map(button => [button.dataset.tabClose, button]));
  return {
    tabs: [...document.querySelectorAll('.workspace-tab:not(.workspace-tab-fixed)')].filter(shown).map(tab => ({
      tab: box(tab), label: box(tab.querySelector('[role="tab"] > span')), close: box(closeButtons.get(tab.dataset.tabId)),
    })),
    readers: [...document.querySelectorAll('.workspace-lens-content .memory-page[data-reader-open]')].filter(shown).map(page => ({
      page: box(page), paneWidth: page.parentElement.clientWidth,
      list: box(page.querySelector('.memory-page-list, .memory-history')),
      reader: box(page.querySelector('.memory-reader')),
    })),
  };
}

export function assertReaderLayout(layout, { label = "reader", tolerance = 1.5 } = {}) {
  for (const tab of layout.tabs) {
    if (!tab.label || !tab.close) continue;
    assert.ok(tab.label.right <= tab.close.x + tolerance && tab.label.x >= tab.tab.x - tolerance,
      `${label}: tab title covers its close button ${JSON.stringify(tab)}`);
  }
  for (const item of layout.readers) {
    if (!item.reader) continue;
    assert.ok(item.reader.height >= 100 && item.reader.bottom <= item.page.bottom + tolerance,
      `${label}: open document has no usable reading area ${JSON.stringify(item)}`);
    if (item.paneWidth > 971 && item.list?.width > 0) {
      assert.ok(item.list.right <= item.reader.x + tolerance,
        `${label}: wide Lens must keep its list beside the document ${JSON.stringify(item)}`);
    }
  }
}
