/** Confirm the effective private Chat settings without Pi's set_auto_* RPCs:
 * those setters persist into its shared authentication directory. This command
 * reads the public settings API and never sends a model prompt or writes state.
 */
export function verifyPiSettings(actual, expected) {
  const integer = (value, max) => Number.isSafeInteger(value) && value >= 0 && value <= max;
  if (!expected || expected.transport !== "auto" || expected.httpIdleTimeoutMs !== 300000
      || expected.compaction?.enabled !== true || expected.compaction.modelOverrides !== null
      || !integer(expected.compaction.reserveTokens, Number.MAX_SAFE_INTEGER)
      || !integer(expected.compaction.keepRecentTokens, Number.MAX_SAFE_INTEGER)
      || typeof expected.retry?.enabled !== "boolean" || !integer(expected.retry.maxRetries, 10)
      || !integer(expected.retry.baseDelayMs, 300000) || !integer(expected.retry.maxAgentDelayMs, 300000)
      || expected.retry.provider?.maxRetries !== 0 || expected.retry.provider.timeoutMs !== 300000
      || expected.retry.provider.maxRetryDelayMs !== 60000) {
    throw new Error("Chat Pi settings are invalid");
  }
  const matches = (value, pattern) => Object.entries(pattern).every(([key, expectedValue]) =>
    expectedValue && typeof expectedValue === "object"
      ? value?.[key] && matches(value[key], expectedValue)
      : value?.[key] === expectedValue);
  if (!matches(actual, expected)) throw new Error("Chat Pi settings changed before execution");
}

export function registerPiSettingsControl(pi) {
  pi.registerCommand("loupist-verify-settings", {
    description: "Confirm app-owned Chat execution settings",
    handler: async (args) => {
      try {
        if (args.length > 4096) throw new Error("Invalid settings control");
        verifyPiSettings(pi.getSettings(), JSON.parse(args));
      } catch {
        // Pi swallows command exceptions. Exit before it can acknowledge a
        // mismatched snapshot and before any provider request is possible.
        process.stderr.write("Loupist Pi settings changed; start a new turn.\n");
        process.exit(76);
      }
    },
  });
}
