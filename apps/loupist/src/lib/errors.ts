import type { CommandErrorCode } from "@/contracts/generated";

export type { CommandErrorCode };

export const errorMessage = (cause: unknown, fallback: string) => {
  if (cause instanceof Error && cause.message) return cause.message;
  if (typeof cause === "string" && cause.trim()) return cause;
  if (
    cause !== null &&
    typeof cause === "object" &&
    "message" in cause &&
    typeof cause.message === "string" &&
    cause.message.trim()
  ) {
    return cause.message;
  }
  return fallback;
};

/**
 * The code of a rejected native command (Rust's `CommandError`), or null for
 * any other failure. Codes are the generated `CommandErrorCode` union, so a
 * comparison with a code Rust never sends fails to compile.
 */
export const commandErrorCode = (cause: unknown): CommandErrorCode | null =>
  cause !== null && typeof cause === "object" && "code" in cause && typeof cause.code === "string"
    ? (cause.code as CommandErrorCode)
    : null;
