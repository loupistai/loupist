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
  const shell = rect(".app-shell");
  if (!shell) return null; // Onboarding and print have no research shell.
  const navigation = [...document.querySelectorAll('[data-slot="sidebar-container"], .settings-navigation[data-slot="sidebar"], [data-slot="sidebar"][data-mobile="true"]')]
    .flatMap(element => {
      const bounds = box(element);
      return bounds ? [{ bounds, mobile: element.dataset.mobile === "true",
        railButtons: element.closest('[data-collapsible="icon"]') ? [...element.querySelectorAll('.navigation-search,.loupist-home-icon,.loupist-list-heading button,#main-tab-settings')].map(box).filter(Boolean) : [],
        footer: box(element.querySelector('[data-slot="sidebar-footer"]')) }] : [];
    });
  const content = document.querySelector(".conversation-content");
  return {
    viewport: { x: 0, y: 0, right: innerWidth, bottom: innerHeight, width: innerWidth, height: innerHeight },
    shell, titlebar: rect(".app-titlebar"), body: rect(".app-body"), frame: rect(".app-frame"), navigation,
    headerControls: [...document.querySelectorAll(".app-titlebar button:not(.loupist-header-identity), .loupist-header-profiles")].map(box).filter(Boolean),
    activeProfile: rect('.loupist-header-identity[aria-pressed="true"]'), profiles: rect(".loupist-header-profiles"),
    composer: rect(".composer-shell"),
    heading: rect(".workspace-heading"),
    titleColumn: rect(".workspace-chat-title"), readingColumn: box(content),
    headingItems: [...document.querySelectorAll(".workspace-chat-folder, .thread-statuses")].map(box).filter(Boolean),
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
  const { viewport, shell, titlebar, body, frame } = layout;
  check(within(shell, viewport) && near(shell.bottom, viewport.bottom), "shell escapes the viewport", { shell, viewport });
  check(within(titlebar, shell) && within(body, shell) && within(frame, body), "shell regions are clipped", { titlebar, body, frame });
  check(near(titlebar.bottom, body.y) && near(body.bottom, frame.bottom) && near(frame.bottom, viewport.bottom),
    "titlebar and body do not share the available height", { titlebar, body, frame, viewport });
  if (viewport.width >= 900) check(layout.navigation.length === 1, "desktop navigation is missing", layout.navigation);
  for (const navigation of layout.navigation) {
    check(within(navigation.bounds, navigation.mobile ? viewport : body), "navigation escapes its region", navigation);
    if (!navigation.mobile) check(near(navigation.bounds.y, body.y) && near(navigation.bounds.bottom, body.bottom),
      "sidebar does not follow the body height", { navigation, body });
    for (const item of navigation.railButtons) {
      check(within(item, navigation.bounds) && near((item.x + item.right) / 2, (navigation.bounds.x + navigation.bounds.right) / 2),
        "collapsed sidebar control is off-center", { item, rail: navigation.bounds });
    }
    if (navigation.footer) check(within(navigation.footer, navigation.bounds), "navigation footer is clipped", navigation);
  }
  for (const control of layout.headerControls) check(within(control, titlebar), "titlebar control is clipped", { control, titlebar });
  if (layout.activeProfile) check(within(layout.activeProfile, layout.profiles),
    "selected profile is clipped in the header strip", { active: layout.activeProfile, strip: layout.profiles });
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
  if (layout.chatGutter !== null && layout.chatWidth != null) {
    // The compact gutter must respond to chat width, even beside a docked panel.
    const expected = layout.chatWidth <= 760 ? 12 : 20;
    check(near(layout.chatGutter, expected), "chat gutter did not adapt to available width", layout);
    if (layout.titleGutter !== null) check(near(layout.titleGutter, layout.chatGutter), "chat title and reading gutters differ", layout);
  }
}
