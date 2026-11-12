import { App } from "../../App";
import { act, cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { chooseLoupist, openApp, selectedLoupistButton } from "../renderApp";

it("keeps the research draft while moving between a session, Bot and its read-only schedule history", async () => {
  const user = userEvent.setup();
  await openApp();
  const sessionTitle = "How should we read the margin decline?";
  const composer = () => screen.getByRole("textbox", { name: "Message Loupist" });
  await user.type(composer(), "Compare inventory days");
  await user.click(screen.getByRole("button", { name: "Bot" }));
  expect(composer()).not.toHaveValue("Compare inventory days");
  await user.click(screen.getByRole("button", { name: "Options for Bot" }));
  await user.click(screen.getByRole("menuitem", { name: "Scheduled tasks" }));
  const history = await screen.findByRole("region", { name: "Schedule" });
  expect(within(history).queryByRole("textbox")).toBeNull();
  await user.click(screen.getByRole("button", { name: sessionTitle }));
  expect(composer()).toHaveValue("Compare inventory days");
  expect(screen.getByRole("button", { name: "New Loupist" })).toBeVisible();
});

it("restores the last session and unsent text when reopening the app", async () => {
  const user = userEvent.setup();
  const bridge = await openApp();
  const title = "How should we read the margin decline?";
  await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "Unsaved inventory question");
  cleanup();
  render(<App bridge={bridge} />);
  expect(await screen.findByRole("heading", { name: title })).toBeVisible();
  expect(await screen.findByDisplayValue("Unsaved inventory question")).toBeVisible();
});

it("opens the chosen Loupist's Bot and preserves the previous Loupist's session draft", async () => {
  const user = userEvent.setup();
  await openApp();
  const composer = () => screen.getByRole("textbox", { name: "Message Loupist" });
  await user.type(composer(), "Compare yesterday's filing");
  await chooseLoupist(user, "Cycle Reader");
  expect(await screen.findByRole("heading", { name: "Bot" })).toBeVisible();
  expect(selectedLoupistButton()).toHaveTextContent("Cycle Reader");
  expect(composer()).toHaveValue("");
  await chooseLoupist(user, "Quality Compounder");
  await user.click(await screen.findByRole("button", { name: "How should we read the margin decline?" }));
  expect(composer()).toHaveValue("Compare yesterday's filing");
});

it("opens a new user's Bot after the notice and skips watchlist and scheduling setup", async () => {
  const { MockLoupistBridge } = await import("../../bridge/mockBridge");
  const bridge = new MockLoupistBridge({ delay_ms: 0, notice_acknowledged_version: 0 });
  for (const profile of await bridge.loupistList()) await bridge.loupistDelete({ loupist_id: profile.id });
  const create = vi.spyOn(bridge, "loupistCreate");
  const schedule = vi.spyOn(bridge, "briefingSettingsSet");
  const user = userEvent.setup();
  render(<App bridge={bridge} />);
  await user.click(await screen.findByRole("checkbox", { name: "I understand" }));
  await user.click(screen.getByRole("button", { name: "Get started" }));
  await user.click(await screen.findByRole("button", { name: "Create Loupist" }));
  expect(await screen.findByRole("heading", { name: "Bot" })).toBeVisible();
  expect(screen.getByRole("textbox", { name: "Message Loupist" })).toBeEnabled();
  expect(create).toHaveBeenCalledTimes(1);
  expect(schedule).not.toHaveBeenCalled();
});

it("restores attachment bytes and removes them from the saved draft when discarded", async () => {
  const user = userEvent.setup();
  const bridge = await openApp();
  const identity = {loupist_id:"loupist-quality", thread_id:"thread-margin"};
  const file = new File(["weekly inventory"], "inventory.txt", {type:"text/plain"});
  await user.upload(document.querySelector<HTMLInputElement>('input[type="file"]')!, file);
  await vi.waitFor(async () => expect(await bridge.chatDraftFilesRead(identity)).toMatchObject([{filename:"inventory.txt",url:`data:text/plain;base64,${btoa("weekly inventory")}`}]));
  cleanup();
  render(<App bridge={bridge} />);
  const tray = await screen.findByLabelText("1 attachment");
  expect(tray).toHaveTextContent("inventory.txt");
  await user.click(within(tray).getByRole("button", {name:/remove/i}));
  await vi.waitFor(async () => expect(await bridge.chatDraftFilesRead(identity)).toEqual([]));
});

it("reopens an inline file reference in its sentence and sends its original path", async () => {
  const user = userEvent.setup();
  const bridge = await openApp();
  const identity = { loupist_id: "loupist-quality", thread_id: "thread-margin" };
  const visible = "@report.md";
  const start = "Compare ".length;
  const prompt = "Compare " + visible + " with last year.";
  cleanup();
  await bridge.chatDraftWrite(identity, { prompt, context: {
    chips: [{ key: "file:report", kind: "file", id: "reports/report.md", label: "report.md" }],
    inlineReferences: [{ key: "chip:file:report", kind: "file", text: visible, start, end: start + visible.length }],
    pins: [],
  } });
  const send = vi.spyOn(bridge, "chatSend");
  let release!: () => void;
  const ready = new Promise<void>(resolve => { release = resolve; });
  const read = bridge.chatDraftRead.bind(bridge);
  vi.spyOn(bridge, "chatDraftRead").mockImplementation(async request => { await ready; return read(request); });
  render(<App bridge={bridge} />);
  await screen.findByRole("textbox", { name: "Message Loupist" });
  await act(async () => { release(); });
  await screen.findByDisplayValue(prompt);
  await user.click(screen.getByRole("button", { name: "Send" }));
  await vi.waitFor(() => expect(send.mock.calls[0]?.[0].prompt).toBe("Compare reports/report.md with last year."));
});

it("opens Schedule from its sidebar row under Bot", async () => {
  const user = userEvent.setup();
  await openApp();
  const row = screen.getByRole("button", { name: "Schedule" });
  expect(row).not.toHaveAttribute("aria-current");
  await user.click(row);
  expect(await screen.findByRole("region", { name: "Schedule" })).toBeVisible();
  expect(screen.getByRole("button", { name: "Schedule" })).toHaveAttribute("aria-current", "page");
});
