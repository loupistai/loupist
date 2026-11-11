import type { CommandError, CommandErrorCode } from "@/contracts/generated";

/**
 * What a Rust command rejects with: a `CommandError` `{ code, message }`. It is an
 * `Error` too, so preview stack traces and test matchers keep working.
 */
export const commandError = (code: CommandErrorCode, message: string): CommandError & Error =>
  Object.assign(new Error(message), { code });

/** Throws a coded command failure from a mock command. */
export const rejectCommand = (code: CommandErrorCode, message: string): never => {
  throw commandError(code, message);
};
