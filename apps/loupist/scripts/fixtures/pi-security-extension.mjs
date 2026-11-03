import { writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { resolveModelScopeWithDiagnostics } from "@earendil-works/pi-coding-agent";
import { probePiSecurity } from "../pi-security-probe.mjs";

export default function register(pi) {
  pi.registerCommand("loupist-security-probe", {
    description: "Credential-free runtime security regression probe",
    handler: async () => {
      let result;
      try {
        const httpModule = createRequire(import.meta.url).resolve("undici");
        if (httpModule !== "undici" || !process.versions.bun) {
          throw new Error("Packaged Pi must use Bun's built-in HTTP implementation");
        }
        result = await probePiSecurity(resolveModelScopeWithDiagnostics, process.env.LOUPIST_SECURITY_URL);
        result.httpImplementation = "bun-builtin";
      } catch (error) {
        result = { error: error.name, message: error.message };
      }
      writeFileSync(process.env.LOUPIST_SECURITY_RESULT, JSON.stringify(result));
    },
  });
}
