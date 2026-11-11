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
  const receivesClick = button => {
    if (!button) return null;
    const rect = button.getBoundingClientRect();
    const x = rect.x + rect.width / 2, y = rect.y + rect.height / 2;
    const strip = button.closest('.workspace-tabs')?.getBoundingClientRect();
    if (strip && (x < strip.x || x > strip.right)) return null;
    const hit = document.elementFromPoint(x, y);
    return hit === button || button.contains(hit);
  };
  return {
    tabs: [...document.querySelectorAll('.workspace-tab:not(.workspace-tab-fixed)')].filter(shown).map(tab => ({
      tab: box(tab), label: box(tab.querySelector('[role="tab"] > span')), close: box(closeButtons.get(tab.dataset.tabId)),
      closeReceivesClick: receivesClick(closeButtons.get(tab.dataset.tabId)),
    })),
    readers: [...document.querySelectorAll('.workspace-lens-content [data-testid=memory-page][data-reader-open]')].filter(shown).map(page => ({
      page: box(page), paneWidth: page.parentElement.clientWidth,
      list: box(page.querySelector('.memory-page-list, .memory-history')),
      reader: box(page.querySelector('[data-testid=memory-reader]')),
    })),
  };
}

export function assertReaderLayout(layout, { label = "reader", tolerance = 1.5 } = {}) {
  for (const tab of layout.tabs) {
    if (!tab.label || !tab.close) continue;
    assert.ok(tab.label.right <= tab.close.x + tolerance && tab.label.x >= tab.tab.x - tolerance,
      `${label}: tab title covers its close button ${JSON.stringify(tab)}`);
    assert.notEqual(tab.closeReceivesClick, false, `${label}: a click on a tab's close button lands on another element ${JSON.stringify(tab)}`);
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
