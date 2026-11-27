import { commandError } from "@/bridge/mock/commandError";
import type { ScheduledTask } from "@/types";
import { draftProblems, programDraft, programPatch, saveRefusal } from "./learningProgramDraft";

const task = {
  title: "Inflación subyacente", prompt: "Core inflation releases.", subjects: [{ name: "Banco de México" }],
  schedule: { kind: "interval", every_minutes: 720 }, report: "every_run", delivery: { to: "bot", notify: "digest" },
  program: { depth: "light", scope: { connections: ["fixture.stats"], web: false }, keeps: { sources: true, lens: false, asset_reads: false } },
} as unknown as ScheduledTask;

it("patches only what changed, and keeps the delivery's notify choice", () => {
  const draft = programDraft(task);
  expect(programPatch(task, draft)).toEqual({});
  expect(programPatch(task, { ...draft, title: "  Inflación subyacente  " })).toEqual({});
  expect(programPatch(task, { ...draft, delivery: "briefing" })).toEqual({ delivery: { to: "briefing", notify: "digest" } });
  expect(programPatch(task, { ...draft, web: true })).toEqual({ sources: { connections: ["fixture.stats"], web: true } });
  expect(programPatch(task, { ...draft, depth: "deep", keeps: { sources: true, lens: true, asset_reads: false } })).toEqual({ depth: "deep", keeps: { sources: true, lens: true, asset_reads: false } });
});

it("counts the brief in UTF-8 bytes, as the host does", () => {
  const draft = programDraft(task);
  expect(draftProblems({ ...draft, prompt: "€".repeat(2730) })).toEqual({});
  expect(draftProblems({ ...draft, prompt: "€".repeat(2731) })).toEqual({ prompt: "too_long" });
  expect(draftProblems({ ...draft, keeps: { ...draft.keeps, asset_reads: true } })).toEqual({ keeps: "invalid" });
  expect(draftProblems({ ...draft, title: "Two\nlines" })).toEqual({ title: "invalid" });
});

it("reads a refusal as stale, a named field, or neither", () => {
  expect(saveRefusal(commandError("stale_revision", "changed"))).toEqual({ kind: "stale" });
  expect(saveRefusal(commandError("invalid_request", "x", { field: "schedule", reason: "an active one-time schedule must be in the future" }))).toEqual({ kind: "field", field: "schedule" });
  expect(saveRefusal(commandError("invalid_request", "x", { field: "model", reason: "invalid" }))).toEqual({ kind: "other" });
  expect(saveRefusal(new Error("offline"))).toEqual({ kind: "other" });
});
