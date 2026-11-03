import { act, renderHook, waitFor } from "@testing-library/react";
import { MockLoupistBridge } from "../bridge/mockBridge";
import { useTheme } from "./useTheme";

it("persists the theme in global configuration and reloads external changes on focus", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const first = renderHook(() => useTheme(bridge));
  await waitFor(() => expect(first.result.current.configuration.snapshot).not.toBeNull());
  act(() => first.result.current.setPreference("dark"));
  await waitFor(() => expect(first.result.current.preference).toBe("dark"));
  first.unmount();
  const second = renderHook(() => useTheme(bridge));
  await waitFor(() => expect(second.result.current.preference).toBe("dark"));
  const current = await bridge.configRead({ scope: { kind: "global" } });
  await bridge.configUpdate({ scope: current.scope, expectedRevision: current.revision, set: { appearance: { theme: "light" } }, unset: [] });
  act(() => window.dispatchEvent(new Event("focus")));
  await waitFor(() => expect(second.result.current.preference).toBe("light"));
  expect(document.documentElement).toHaveAttribute("data-theme", "light");
});
