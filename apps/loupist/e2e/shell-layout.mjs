import assert from "node:assert/strict";

/** Shared browser callback: measure the rendered shell in Chrome or WKWebView. */
export function measureShellLayout() {
  const box = element => {
    if (!element) return null;
    const r = element.getBoundingClientRect();
    const style = getComputedStyle(element);
    if (!r.width || !r.height || style.visibility === "hidden" || style.display === "none") return null;
    return { x: r.x, y: r.y, right: r.right, bottom: r.bottom, width: r.width, height: r.height };
  };
  const rect = selector => box(document.querySelector(selector));
  const shell = rect("[data-testid=app-shell]");
  if (!shell) return null; // Onboarding and print have no research shell.
  const navigation = [...document.querySelectorAll('[data-slot="sidebar-container"], .settings-navigation[data-slot="sidebar"], [data-slot="sidebar"][data-mobile="true"]')]
    .flatMap(element => {
      const bounds = box(element);
      return bounds ? [{ bounds, mobile: element.dataset.mobile === "true",
        railButtons: element.closest('[data-collapsible="icon"]') ? [...element.querySelectorAll('[data-testid=loupist-header-identity],.navigation-search,[data-testid=loupist-chat-row][data-home] button[data-testid=loupist-chat-button],[data-sidebar-place] button,[data-testid=navigation-new],#main-tab-settings')].map(box).filter(Boolean) : [],
        footer: box(element.querySelector('[data-slot="sidebar-footer"]')),
        resizeHandle: box(element.querySelector("[data-sidebar-resize]")) }] : [];
    });
  const content = document.querySelector(".conversation-content");
  // Lists in the sidebar and the side panels whose rows pair a one-line title with a time, count or
  // status at the row's end: [row, title, what sits at its end].
  const titledRows = [
    [".activity-earlier > li", ".activity-step-label", ":scope > time"],
    [".loupist-chat-row", ".loupist-chat-title",
      ".loupist-draft-dot, .thread-running, .loupist-status-dot, .loupist-private-mark, .shell-unread"],
    [".schedule-running > li", ".schedule-running-title", ":scope > time"],
    [".workspace-file-entry", ":scope > span", ":scope > small"],
  ].flatMap(([row, title, end]) => [...document.querySelectorAll(row)].flatMap(element => {
    const titleBox = box(element.querySelector(title));
    const ends = [...element.querySelectorAll(end)].map(box).filter(Boolean);
    return titleBox && ends.length ? [{ list: row, text: element.querySelector(title).textContent.trim().slice(0, 40),
      row: box(element), title: titleBox, end: ends.reduce((first, next) => next.x < first.x ? next : first) }] : [];
  }));
  return {
    viewport: { x: 0, y: 0, right: innerWidth, bottom: innerHeight, width: innerWidth, height: innerHeight },
    shell, header: rect(".app-header"), body: rect(".app-body"), frame: rect(".app-frame"), navigation,
    headerControls: [...document.querySelectorAll(".app-header button")].map(box).filter(Boolean),
    identity: rect("[data-testid=loupist-header-identity]"),
    composer: rect(".composer-shell"),
    heading: rect(".workspace-heading"),
    titleColumn: rect(".workspace-chat-title"), readingColumn: box(content),
    headingItems: [...document.querySelectorAll(".thread-statuses")].map(box).filter(Boolean),
    titledRows,
    chatWidth: rect(".chat-workspace")?.width,
    chatGutter: content ? parseFloat(getComputedStyle(content).paddingLeft) : null,
    titleGutter: document.querySelector(".workspace-chat-title")
      ? parseFloat(getComputedStyle(document.querySelector(".workspace-chat-title")).paddingLeft) : null,
  };
}

export function assertShellLayout(layout, { label = "shell", tolerance = 1.5 } = {}) {
  if (!layout) return;
  const check = (condition, reason, evidence) => assert.ok(condition,
    `${label}: ${reason} ${JSON.stringify(evidence)}`);
  const near = (a, b) => Math.abs(a - b) <= tolerance;
  const within = (child, parent) => child && parent && child.x >= parent.x - tolerance
    && child.right <= parent.right + tolerance && child.y >= parent.y - tolerance && child.bottom <= parent.bottom + tolerance;
  const { viewport, shell, header, body, frame } = layout;
  check(within(shell, viewport) && near(shell.bottom, viewport.bottom), "shell escapes the viewport", { shell, viewport });
  check(within(body, shell) && within(frame, body), "shell regions are clipped", { body, frame });
  // No title bar row: the sidebar and the page column both run the full height of the window.
  check(near(body.y, shell.y) && near(frame.y, body.y) && near(body.bottom, frame.bottom) && near(frame.bottom, viewport.bottom),
    "body and page column do not take the full height", { body, frame, viewport });
  if (header) check(within(header, frame) && near(header.y, frame.y), "page header is not the first row of the page column", { header, frame });
  if (viewport.width >= 900) check(layout.navigation.length === 1, "desktop navigation is missing", layout.navigation);
  for (const navigation of layout.navigation) {
    check(within(navigation.bounds, navigation.mobile ? viewport : body), "navigation escapes its region", navigation);
    if (!navigation.mobile) check(near(navigation.bounds.y, body.y) && near(navigation.bounds.bottom, body.bottom),
      "sidebar does not run the full height", { navigation, body });
    for (const item of navigation.railButtons) {
      check(within(item, navigation.bounds) && near((item.x + item.right) / 2, (navigation.bounds.x + navigation.bounds.right) / 2),
        "collapsed sidebar control is off-center", { item, rail: navigation.bounds });
    }
    if (navigation.footer) check(within(navigation.footer, navigation.bounds), "navigation footer is clipped", navigation);
    // The sidebar's width is the user's; its resize handle always sits on its right edge, inside it.
    if (navigation.resizeHandle) check(within(navigation.resizeHandle, navigation.bounds) && near(navigation.resizeHandle.right, navigation.bounds.right),
      "sidebar resize handle is not on the sidebar's right edge", { handle: navigation.resizeHandle, sidebar: navigation.bounds });
  }
  for (const control of layout.headerControls) check(within(control, header), "page header control is clipped", { control, header });
  const sidebar = layout.navigation.find(item => !item.mobile);
  if (layout.identity && sidebar) check(within(layout.identity, sidebar.bounds), "Loupist identity is clipped in the sidebar", { identity: layout.identity, sidebar: sidebar.bounds });
  if (layout.composer) check(within(layout.composer, frame), "composer is clipped below the frame", { composer: layout.composer, frame });
  if (layout.titleColumn && layout.readingColumn) {
    check(near(layout.titleColumn.x, layout.readingColumn.x) && near(layout.titleColumn.right, layout.readingColumn.right),
      "chat title is shifted from the reading column", { title: layout.titleColumn, reading: layout.readingColumn });
  }
  for (const [index, item] of layout.headingItems.entries()) {
    check(within(item, layout.heading), "chat header item is clipped", { item, heading: layout.heading });
    for (const next of layout.headingItems.slice(index + 1)) {
      check(Math.min(item.right, next.right) - Math.max(item.x, next.x) <= tolerance,
        "chat header items overlap", { item, next });
    }
  }
  // A long title ends in an ellipsis before the time or badge at its row's end; neither runs past the row.
  for (const item of layout.titledRows ?? []) {
    check(item.title.right <= item.end.x + tolerance, "row title runs under its time or badge", item);
    check(item.title.x >= item.row.x - tolerance && item.end.right <= item.row.right + tolerance, "row title or its time escapes the row", item);
  }
  if (layout.chatGutter !== null && layout.chatWidth != null) {
    // The compact gutter must respond to chat width, even beside a docked panel.
    const expected = layout.chatWidth <= 760 ? 12 : 20;
    check(near(layout.chatGutter, expected), "chat gutter did not adapt to available width", layout);
    if (layout.titleGutter !== null) check(near(layout.titleGutter, layout.chatGutter), "chat title and reading gutters differ", layout);
  }
}
