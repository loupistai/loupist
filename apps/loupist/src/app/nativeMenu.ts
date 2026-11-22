import menu from "./nativeMenu.json";
import { isMacPlatform } from "../lib/platform";

type NativeMenuRow = {
  id?: string;
  role?: string;
  accelerator?: string;
  macAccelerator?: string;
  displayKey?: string;
  macOnly?: boolean;
  destination?: boolean;
};

const rows: readonly NativeMenuRow[] = menu.flatMap<NativeMenuRow>(group => group.items);
export const nativeDestinations = rows.filter(row => row.destination);

const menuRow = (id: string) => rows.find(item => item.id === id);

export function menuShortcutKey(id: string) {
  const row = menuRow(id);
  return (row?.displayKey ?? row?.accelerator?.split("+").at(-1) ?? "").toLowerCase();
}

export function menuShortcut(id: string, mac = isMacPlatform()) {
  const row = menuRow(id);
  if (!row?.accelerator || (row.macOnly && !mac)) return "";
  const [modifier, ...parts] = row.accelerator.split("+");
  if (mac && row.macAccelerator) return row.macAccelerator.replace("Ctrl+Cmd+", "⌃⌘");
  const key = row.displayKey ?? parts.at(-1) ?? "";
  if (modifier === "CmdOrCtrl") return (mac ? "⌘" : "Ctrl+") + key;
  if (modifier === "Cmd") return "⌘" + key;
  return row.accelerator;
}
