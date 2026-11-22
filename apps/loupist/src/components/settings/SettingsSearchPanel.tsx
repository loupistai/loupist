import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import type { SettingsSection } from "../../app/navigation";
import { focusSetting, settingControls, type SettingResult } from "./settingSearch";

/** A search reads each available section's actual controls. Inactive sections
 * render into detached elements, outside the window and its accessibility tree. */
export function SettingsSearchPanel({ section, active, searching, target, onIndexed, children }: {
  section: SettingsSection; active: boolean; searching: boolean; target?: SettingResult;
  onIndexed?: (section: SettingsSection, entries: SettingResult[]) => void; children: ReactNode;
}) {
  const [detached] = useState(() => document.createElement("div"));
  const visible = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const root = active ? visible.current : detached;
    if (!root || (!searching && !target)) return;
    let focused = false;
    let signature: string | undefined;
    const update = () => {
      const controls = settingControls(root, section);
      if (searching) {
        const entries = Array.from(controls.values(), item => item.result);
        const next = JSON.stringify(entries);
        if (signature !== next) { signature = next; onIndexed?.(section, entries); }
      }
      const match = active && target ? controls.get(target.label) : undefined;
      if (match && !focused) focused = focusSetting(match.element);
    };
    update();
    const observer = new MutationObserver(update);
    observer.observe(root, { childList: true, characterData: true, subtree: true, attributes: true, attributeFilter: ["hidden", "aria-label", "disabled"] });
    return () => observer.disconnect();
  }, [active, detached, onIndexed, searching, section, target]);
  return active ? <div ref={visible} className="settings-active-panel">{children}</div> : createPortal(children, detached);
}
