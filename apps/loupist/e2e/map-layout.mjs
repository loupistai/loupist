import assert from "node:assert/strict";

/**
 * Browser callback shared by design previews and native layout probes: each
 * shown Map's stage, the panels that cover part of it (the Library, What's new,
 * an inspector or reader sheet) and the controls that float over its canvas.
 */
export function measureMapOverlays() {
  const box = element => {
    const rect = element.getBoundingClientRect();
    return { x: rect.x, y: rect.y, right: rect.right, bottom: rect.bottom, width: rect.width, height: rect.height };
  };
  const shown = element => !element.closest('[hidden], [inert], [aria-hidden="true"]')
    && getComputedStyle(element).visibility === "visible" && getComputedStyle(element).display !== "none"
    && element.getBoundingClientRect().width > 0 && element.getBoundingClientRect().height > 0;
  const name = element => [element.tagName.toLowerCase(), ...element.classList].join(".");
  return [...document.querySelectorAll("[data-testid=map-view]")].flatMap(view => {
    const stage = view.querySelector(":scope > .map-body > .map-stage");
    if (!stage || !shown(stage)) return [];
    // A side inspector shrinks the stage; only a panel laid over it covers part of the canvas.
    const covers = [...view.querySelectorAll(":scope > .map-body > :is(.map-library, .map-inspector)")]
      .filter(panel => shown(panel) && getComputedStyle(panel).position === "absolute")
      .map(panel => ({ name: name(panel), box: box(panel) }));
    const overlays = [...stage.querySelectorAll(".map-stats, .map-overlay-pill, .map-pick-hint, .map-no-neighbors, details.map-legend, .map-fit, .map-tooltip, .map-empty > *")]
      .filter(shown).map(element => ({ name: name(element), box: box(element) }));
    return [{ stage: box(stage), covers, overlays }];
  });
}

/** The part of the stage no panel covers. A panel spans the stage's height on one side, or its width at the bottom. */
export function mapFreeArea({ stage, covers }, tolerance = 1.5) {
  const free = { x: stage.x, y: stage.y, right: stage.right, bottom: stage.bottom };
  for (const { box } of covers) {
    const tall = box.y <= stage.y + tolerance && box.bottom >= stage.bottom - tolerance;
    const wide = box.x <= stage.x + tolerance && box.right >= stage.right - tolerance;
    if (wide && box.y > stage.y + tolerance) free.bottom = Math.min(free.bottom, box.y);
    else if (wide && tall) return null;
    else if (tall && box.x <= stage.x + tolerance) free.x = Math.max(free.x, box.right);
    else if (tall) free.right = Math.min(free.right, box.x);
  }
  return free.right - free.x > tolerance && free.bottom - free.y > tolerance ? free : null;
}

export function assertMapOverlays(maps, { label = "map", tolerance = 1.5 } = {}) {
  for (const map of maps) {
    const free = mapFreeArea(map, tolerance);
    // A panel over the whole canvas leaves no free area; what floats over the canvas is under it.
    if (!free) continue;
    for (const overlay of map.overlays) {
      const { box } = overlay;
      assert.ok(box.x >= free.x - tolerance && box.right <= free.right + tolerance && box.y >= free.y - tolerance && box.bottom <= free.bottom + tolerance,
        `${label}: ${overlay.name} is not inside the free part of the Map ${JSON.stringify({ overlay: box, free, covers: map.covers })}`);
    }
  }
}
