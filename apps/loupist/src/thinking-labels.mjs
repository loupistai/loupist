// Stored thinking ids stay on the wire. Menus, chips, and E2E compare this label.
const THINKING_LABELS = {
  off: "Off",
  none: "Off",
  minimal: "Minimal",
  low: "Low",
  medium: "Medium",
  high: "High",
  xhigh: "Extra high",
  max: "Max",
};

export function thinkingLabel(level) {
  if (Object.hasOwn(THINKING_LABELS, level)) return THINKING_LABELS[level];
  return level ? `${level.slice(0, 1).toUpperCase()}${level.slice(1)}` : "";
}
