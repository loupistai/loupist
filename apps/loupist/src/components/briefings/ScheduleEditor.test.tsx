import { act, render, screen, within } from "@testing-library/react";
import i18n from "@/i18n";
import type { BriefingSettings, LoupistBridge } from "@/types";
import { ScheduleEditor } from "./ScheduleEditor";

const settings = { enabled: true, task: null, schedule: { kind: "weekly", days: [1, 2, 3, 4, 5], hour: 8, minute: 0, timezone: "Asia/Seoul" } } satisfies BriefingSettings;
const bridge = { briefingResidentSettingsGet: async () => ({ keepRunning: false }) } as unknown as LoupistBridge;

afterEach(async () => { await act(async () => { await i18n.changeLanguage("en"); }); });

it("names the schedule hour in the app language", async () => {
  await act(async () => { await i18n.changeLanguage("en"); });
  const view = render(<ScheduleEditor bridge={bridge} loupistId="loupist" settings={settings} onClose={() => {}} onSaved={() => {}} />);
  expect(document.querySelector("input[type=time]")).toBeNull();
  expect(within(screen.getByRole("combobox", { name: "Hour" })).getByRole("option", { selected: true })).toHaveTextContent(/8.*AM|AM.*8/);
  await act(async () => { await i18n.changeLanguage("ko"); });
  view.rerender(<ScheduleEditor bridge={bridge} loupistId="loupist" settings={settings} onClose={() => {}} onSaved={() => {}} />);
  expect(within(screen.getByRole("combobox", { name: "시" })).getByRole("option", { selected: true })).toHaveTextContent("오전");
});
