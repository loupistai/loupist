// Start the production extension against the host's JSON, with no Pi session
// or provider. An invalid context exits with the same failure as on launch.
import extension from "../../../../../agent/loupist-extension.mjs";

extension({
  on() {},
  registerCommand() {},
  registerTool() {},
  setActiveTools() {},
});
