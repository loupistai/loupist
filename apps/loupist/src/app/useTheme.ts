import { useEffect, useState } from "react";
import type { LoupistBridge } from "../types";
import { useConfiguration } from "./useConfiguration";

export type ThemePreference = "light" | "dark" | "system";
type ResolvedTheme = Exclude<ThemePreference, "system">;

const prefersDark = () =>
  window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false;

export function useTheme(bridge: LoupistBridge) {
  const configuration = useConfiguration(bridge, { kind: "global" });
  const preference = configuration.snapshot?.effective.appearance.theme ?? "system";
  const setPreference = (theme: ThemePreference) => { void configuration.update({ appearance: { theme } }); };
  const [systemDark, setSystemDark] = useState(prefersDark);

  useEffect(() => {
    const media = window.matchMedia?.("(prefers-color-scheme: dark)");
    if (!media) return undefined;
    const update = () => setSystemDark(media.matches);
    update();
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);

  const resolved: ResolvedTheme =
    preference === "system" ? (systemDark ? "dark" : "light") : preference;

  useEffect(() => {
    document.documentElement.dataset.theme = resolved;
    // Shared shadcn components still use Tailwind dark: variants.
    document.documentElement.classList.toggle("dark", resolved === "dark");
    document.documentElement.style.colorScheme = resolved;
  }, [preference, resolved]);

  return { preference, resolved, setPreference, configuration };
}
