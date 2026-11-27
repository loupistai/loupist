import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { commandError } from "@/bridge/mock/commandError";
import i18n from "@/i18n";
import type { LoupistBridge, ScheduledTask } from "@/types";
import { LearningProgramEditor } from "./LearningProgramEditor";

const program = (patch: Partial<ScheduledTask> = {}): ScheduledTask => ({
  id: "learn-rioja", loupist_id: "loupist-a", kind: "learning", title: "Exportaciones de vino de Rioja", prompt: "Track Rioja wine export volumes and prices each month.",
  schedule: { kind: "weekly", days: [1, 2, 3, 4, 5], hour: 6, minute: 30, timezone: "Europe/Madrid" }, target: { kind: "dedicated_chat" },
  model_profile_id: "", thinking_level: "", run_options: {}, use_memory: true, update_memory: true, status: "active", revision: 7,
  next_run_at_ms: null, last_run_at_ms: null, created_at_ms: 0, updated_at_ms: 0, last_error: null, session_thread_id: null, awaiting_confirmation: false,
  program: { depth: "standard", scope: { connections: "all_enabled", web: true }, keeps: { sources: true, lens: true, asset_reads: false } },
  subjects: [{ name: "Rioja DOCa" }, { asset: "ast-harborline" }], report: "when_new", delivery: { to: "bot" },
  ...patch,
});
const names = new Map([["ast-harborline", "Harborline Logistics"], ["ast-nordlys", "Nordlys Kraft"]]);

function setup(task = program(), bridge: Partial<LoupistBridge> = {}) {
  const calls = {
    learningProgramUpdate: vi.fn(async () => ({ ...task, revision: task.revision + 1 })),
    scheduledTaskGet: vi.fn(async () => ({ ...task, revision: task.revision + 1, title: "Rioja exports, renamed elsewhere" })),
    ...bridge,
  };
  const onOpenChange = vi.fn(), onSaved = vi.fn();
  const view = render(<LearningProgramEditor bridge={calls as unknown as LoupistBridge} task={task} names={names} open onOpenChange={onOpenChange} onSaved={onSaved} />);
  return { calls, onOpenChange, onSaved, view, user: userEvent.setup() };
}

const save = () => screen.getByRole("button", { name: "Save" });

it("keeps Save off while nothing changed, and sends only the changed fields with the opened revision", async () => {
  const { calls, onSaved, onOpenChange, user } = setup();
  expect(screen.getByRole("dialog", { name: "Edit learning program" })).toBeVisible();
  expect(save()).toBeDisabled();
  await user.click(screen.getByRole("radio", { name: "Deep" }));
  expect(save()).toBeEnabled();
  await user.click(save());
  expect(calls.learningProgramUpdate).toHaveBeenCalledExactlyOnceWith({
    loupistId: "loupist-a", taskId: "learn-rioja", revision: 7,
    patch: { depth: "deep" },
  });
  expect(onSaved).toHaveBeenCalledWith(expect.objectContaining({ revision: 8 }));
  expect(onOpenChange).toHaveBeenLastCalledWith(false);
});

it("saves with Enter in the title: Save is the default action", async () => {
  const { calls, user } = setup();
  const title = screen.getByRole("textbox", { name: "Title" });
  await user.clear(title);
  await user.type(title, "Rioja exports{Enter}");
  expect(calls.learningProgramUpdate).toHaveBeenCalledWith(expect.objectContaining({ patch: { title: "Rioja exports" } }));
});

it("changes the time and days of a weekly program", async () => {
  const { calls, user } = setup();
  await user.click(screen.getByRole("button", { name: "Sat", pressed: false }));
  await user.click(screen.getByRole("combobox", { name: "Time" }));
  await user.click(await screen.findByRole("option", { name: "6:45 PM" }));
  await user.click(save());
  expect(calls.learningProgramUpdate).toHaveBeenCalledWith(expect.objectContaining({
    patch: { schedule: { kind: "weekly", days: [1, 2, 3, 4, 5, 6], hour: 18, minute: 45, timezone: "Europe/Madrid" } },
  }));
});

it("shows saving while the host works, then closes", async () => {
  let finish!: (task: ScheduledTask) => void;
  const { user, onOpenChange } = setup(program(), { learningProgramUpdate: vi.fn(() => new Promise<ScheduledTask>(resolve => { finish = resolve; })) });
  await user.type(screen.getByRole("textbox", { name: "What to learn" }), " Note harvest reports.");
  await user.click(save());
  expect(screen.getByRole("button", { name: /Saving/ })).toBeDisabled();
  expect(screen.getByRole("textbox", { name: "Title" })).toBeDisabled();
  await act(async () => { finish(program({ revision: 8 })); });
  expect(onOpenChange).toHaveBeenLastCalledWith(false);
});

it("puts a host refusal under the field it names, and clears it when that field changes", async () => {
  const { user } = setup(program(), { learningProgramUpdate: vi.fn(async () => { throw commandError("invalid_request", "A program can use only the connections this Loupist allows", { field: "sources", reason: "A program can use only the connections this Loupist allows" }); }) });
  await user.click(screen.getByRole("switch", { name: "Web research" }));
  await user.click(save());
  const sources = screen.getByRole("group", { name: "Sources" });
  expect(await within(sources).findByRole("alert")).toHaveTextContent("This Loupist doesn't allow these sources. Choose from the ones it allows.");
  expect(within(screen.getByRole("group", { name: "Keeps" })).queryByRole("alert")).toBeNull();
  await user.click(screen.getByRole("switch", { name: "Web research" }));
  expect(within(sources).queryByRole("alert")).toBeNull();
});

it("says what is wrong before Save: an over-long brief and more than eight subjects", async () => {
  const { user } = setup(program({ prompt: "한".repeat(2730) }));
  await user.type(screen.getByRole("textbox", { name: "What to learn" }), "국");
  expect(within(screen.getByRole("textbox", { name: "What to learn" }).closest("[data-program-field]")! as HTMLElement).getByRole("alert")).toHaveTextContent("Shorten this to 8 KB or less.");
  expect(save()).toBeDisabled();
  const name = screen.getByRole("textbox", { name: "Add a name" });
  for (const subject of ["Ningbo", "Rotterdam", "Santos", "Mombasa", "Busan", "Valparaíso", "Gdańsk"]) await user.type(name, `${subject}{Enter}`);
  expect(within(screen.getByRole("group", { name: "About" })).getByRole("alert")).toHaveTextContent("Choose up to 8 subjects.");
});

it("says the program changed when the revision is stale, and reloads it", async () => {
  const { calls, user } = setup(program(), { learningProgramUpdate: vi.fn(async () => { throw commandError("stale_revision", "changed"); }) });
  await user.click(screen.getByRole("radio", { name: "Light" }));
  await user.click(save());
  expect(await screen.findByText(/This program changed\. Reload/)).toBeVisible();
  expect(save()).toBeDisabled();
  await user.click(screen.getByRole("button", { name: "Reload" }));
  expect(calls.scheduledTaskGet).toHaveBeenCalledWith({ loupist_id: "loupist-a", task_id: "learn-rioja" });
  expect(await screen.findByDisplayValue("Rioja exports, renamed elsewhere")).toBeEnabled();
  expect(screen.queryByText(/This program changed/)).toBeNull();
});

it("stays editable while paused or without an AI account, and says why it will not run", () => {
  setup(program({ status: "paused", model_selection: { unavailable_reason: "no_connected_account" } as ScheduledTask["model_selection"] }));
  expect(screen.getByText("This program is paused. Your changes apply when you resume it.")).toBeVisible();
  expect(screen.getByText("This program can't run until an AI account is connected. You can still edit it.")).toBeVisible();
  expect(screen.getByRole("textbox", { name: "Title" })).toBeEnabled();
});

it("closes with Escape when unchanged, and asks before discarding changes", async () => {
  const clean = setup();
  await clean.user.keyboard("{Escape}");
  expect(clean.onOpenChange).toHaveBeenLastCalledWith(false);
  clean.view.unmount();

  const { user, onOpenChange } = setup();
  await user.type(screen.getByRole("textbox", { name: "Title" }), " 2027");
  await user.keyboard("{Escape}");
  const prompt = await screen.findByRole("dialog", { name: "Discard your changes?" });
  expect(onOpenChange).not.toHaveBeenCalledWith(false);
  await user.click(within(prompt).getByRole("button", { name: "Keep editing" }));
  expect(screen.getByRole("textbox", { name: "Title" })).toHaveValue("Exportaciones de vino de Rioja 2027");
  await user.keyboard("{Escape}");
  await user.click(within(await screen.findByRole("dialog", { name: "Discard your changes?" })).getByRole("button", { name: "Discard" }));
  await waitFor(() => expect(onOpenChange).toHaveBeenLastCalledWith(false));
});

it("adds and removes subjects, followed assets by their names", async () => {
  const { calls, user } = setup();
  await user.click(screen.getByRole("button", { name: "Remove Rioja DOCa" }));
  await user.click(screen.getByRole("combobox", { name: "Add a followed asset" }));
  await user.click(await screen.findByRole("option", { name: "Nordlys Kraft" }));
  await user.click(save());
  expect(calls.learningProgramUpdate).toHaveBeenCalledWith(expect.objectContaining({ patch: { subjects: [{ asset: "ast-harborline" }, { asset: "ast-nordlys" }] } }));
});

it("reads in Korean with the same fields", async () => {
  await i18n.changeLanguage("ko");
  try {
    setup();
    expect(screen.getByRole("dialog", { name: "학습 프로그램 편집" })).toBeVisible();
    expect(screen.getByRole("button", { name: "저장" })).toBeDisabled();
  } finally {
    await act(async () => { await i18n.changeLanguage("en"); });
  }
});
