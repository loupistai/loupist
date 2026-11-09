import "../i18n";
import { setPreviewLanguage } from "../i18n/useLanguage";
import { createRoot } from "react-dom/client";
import { App } from "../App";
import { enableSharingPreviewHarness } from "../sharing/releaseVisibility";

enableSharingPreviewHarness();
import { configureTheme, freezePreviewClock, PreviewBridge } from "./fixtures";
import { liveScenarios, scenarios, until, visible } from "./scenarios";
import "../styles.css";

const status = document.documentElement.dataset;
const fail = (reason: unknown) => {
  status.previewReady = "error";
  status.previewError = reason instanceof Error ? reason.message : String(reason);
};
const timeout = window.setTimeout(() => fail("Scenario did not reach its visible landmark within 8 seconds"), 8000);
async function start() {
  const query = new URLSearchParams(location.search);
  const id = query.get("scenario") ?? "chat-empty";
  const theme = query.get("theme") ?? "light";
  const locale = query.get("locale") ?? "en";
  if (locale !== "en" && locale !== "ko" && locale !== "en-XA") throw new Error(`Unknown locale: ${locale}`);
  setPreviewLanguage(locale);
  const live = liveScenarios.find((candidate) => candidate.id === id);
  const scenario = live ?? scenarios.find((candidate) => candidate.id === id);
  if (!scenario) throw new Error(`Unknown scenario: ${id}`);
  if (theme !== "light" && theme !== "dark") throw new Error(`Unknown theme: ${theme}`);
  if (!live) freezePreviewClock();
  const bridge = new PreviewBridge({ delay_ms: 0 });
  await scenario.setup(bridge);
  await configureTheme(bridge, theme);
  status.desktopPlatform = navigator.userAgent.includes("Macintosh") ? "macos" : "other";
  const root = document.getElementById("root")!;
  createRoot(root).render(<App bridge={bridge} />);
  await scenario.enter(root);
  await until(() => visible(scenario.landmark()), `Landmark not visible for ${id}`);
  await document.fonts.ready;
  if (status.previewReady !== "error") status.previewReady = "true";
}
void start().catch(fail).finally(() => window.clearTimeout(timeout));
