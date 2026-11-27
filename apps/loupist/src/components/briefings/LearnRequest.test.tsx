import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ModelCatalog } from "../../types";
import { learnBlock, LearnRequest, LearnRequestProvider, type LearnBlock } from "./LearnRequest";

const catalog = (credential: "missing" | "keychain") => ({ providers: [{ credential_source: credential }], models: [] }) as unknown as ModelCatalog;
const on = { use_memory: true, update_memory: true };

it("asks Bot only with an AI account and Memory reading and saving on", () => {
  expect(learnBlock(null, on)).toBe("loading");
  expect(learnBlock(catalog("keychain"), null)).toBe("loading");
  expect(learnBlock(catalog("missing"), on)).toBe("noAccount");
  expect(learnBlock(catalog("keychain"), { use_memory: true, update_memory: false })).toBe("memoryOff");
  expect(learnBlock(catalog("keychain"), { use_memory: false, update_memory: true })).toBe("memoryOff");
  expect(learnBlock(catalog("keychain"), on)).toBeNull();
});

const renderWith = (block: LearnBlock, kind: "world" | "lens", subject: string) => {
  const ask = vi.fn();
  render(<LearnRequestProvider value={{ block, ask }}><LearnRequest kind={kind} subject={subject} /></LearnRequestProvider>);
  return ask;
};

it.each([
  ["world", "Learn about this", "Société Ferroviaire du Nord", "Propose a learning program about Société Ferroviaire du Nord that keeps what you know about it current."],
  ["lens", "Keep this page current", "Monsoon and rice exports", "Propose a learning program that keeps my Lens page “Monsoon and rice exports” current."],
] as const)("sends Bot a request that names the subject from the %s entry point", async (kind, label, subject, request) => {
  const ask = renderWith(null, kind, subject);
  await userEvent.click(screen.getByRole("button", { name: label }));
  expect(ask).toHaveBeenCalledExactlyOnceWith(request);
});

it.each([
  ["noAccount", "Needs an AI account"],
  ["memoryOff", "Memory saving is off"],
] as const)("is disabled with its reason when %s", (block, reason) => {
  renderWith(block, "world", "Kōbe Steelworks");
  const button = screen.getByRole("button", { name: "Learn about this" });
  expect(button).toBeDisabled();
  expect(button).toHaveAccessibleDescription(reason);
});

it("is disabled without a line while the account and settings are read, and absent outside the app", () => {
  renderWith("loading", "lens", "Copper");
  expect(screen.getByRole("button", { name: "Keep this page current" })).toBeDisabled();
  expect(screen.queryByText("Needs an AI account")).toBeNull();
  render(<LearnRequest kind="world" subject="Copper" />);
  expect(screen.getAllByRole("button")).toHaveLength(1);
});
