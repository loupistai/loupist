// The release renderer: the `vite build` output served with the exact Content-Security-Policy
// from tauri.conf.json. Desktop `tauri dev` loads devUrl directly, and Tauri attaches the policy
// only to its own tauri:// responses, so a Vite dev server runs with no policy at all. This server
// lets a native suite, the layout suite and the preview matrix run what a signed build enforces.
import { createServer } from "node:http";
import { appendFileSync, readFileSync, realpathSync } from "node:fs";
import { once } from "node:events";
import { mkdir, mkdtemp, readFile, rename, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, extname, join, normalize, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const appRoot = dirname(dirname(fileURLToPath(import.meta.url)));

/** The policy a signed build serves with every page, read from the shipped configuration. */
export function releasePolicy() {
  const config = JSON.parse(readFileSync(join(appRoot, "src-tauri/tauri.conf.json"), "utf8"));
  const csp = config.app?.security?.csp;
  if (typeof csp !== "string" || !csp.trim()) throw new Error("tauri.conf.json has no app.security.csp string");
  return csp.trim();
}

export const RECORDER_PATH = "/__loupist-csp-recorder.js";
export const REPORT_PATH = "/__loupist-csp-report";
export const VIOLATIONS_KEY = "loupist.cspViolations";

// Runs before every other script of the page. A violation is kept in the page and in
// sessionStorage, which survives the reloads a suite makes in the same window.
const recorder = `(() => {
  const key = ${JSON.stringify(VIOLATIONS_KEY)};
  const keep = (entry) => {
    (window.__loupistCspViolations ||= []).push(entry);
    try {
      const saved = JSON.parse(sessionStorage.getItem(key) || "[]");
      saved.push(entry);
      sessionStorage.setItem(key, JSON.stringify(saved.slice(-200)));
    } catch {}
  };
  document.addEventListener("securitypolicyviolation", (event) => keep({
    directive: event.effectiveDirective || event.violatedDirective,
    blocked: event.blockedURI,
    source: event.sourceFile,
    line: event.lineNumber,
    sample: event.sample,
    page: location.pathname + location.search,
  }), true);
})();
`;

const types = {
  ".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8", ".json": "application/json", ".map": "application/json", ".svg": "image/svg+xml",
  ".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg", ".gif": "image/gif", ".webp": "image/webp",
  ".ico": "image/x-icon", ".woff": "font/woff", ".woff2": "font/woff2", ".ttf": "font/ttf", ".otf": "font/otf",
  ".wasm": "application/wasm", ".txt": "text/plain; charset=utf-8",
};

// Source modules a suite imports by their dev-server path (native-browser.mjs reaches the bridge so).
// They are built as entries with their exports, and the server answers the source path with the chunk.
const SOURCE_ENTRIES = ["src/bridge/tauriBridge.ts"];

/** Builds the shipped page and the design preview (which the layout suite and map probes open). */
export async function buildReleaseRenderer(outDir) {
  const { build } = await import("vite");
  await mkdir(outDir, { recursive: true });
  await build({
    root: appRoot,
    configFile: join(appRoot, "vite.config.ts"),
    logLevel: "warn",
    build: {
      outDir,
      emptyOutDir: true,
      manifest: true,
      rollupOptions: {
        input: {
          index: join(appRoot, "index.html"),
          preview: join(appRoot, "preview.html"),
          ...Object.fromEntries(SOURCE_ENTRIES.map((path) => [path.replace(/\W+/g, "-"), join(appRoot, path)])),
        },
        preserveEntrySignatures: "exports-only",
      },
    },
  });
  return outDir;
}

function sourceEntryMap(root) {
  try {
    const manifest = JSON.parse(readFileSync(join(root, ".vite/manifest.json"), "utf8"));
    return new Map(SOURCE_ENTRIES.filter((path) => manifest[path]?.file).map((path) => [`/${path}`, `/${manifest[path].file}`]));
  } catch {
    return new Map();
  }
}

/**
 * Serves a built renderer. Pages get the release policy plus a report-uri, which changes no
 * source list: what is allowed and blocked is the shipped policy. Reports land in `reportFile`.
 */
export async function serveReleaseRenderer(root, { port = 0, reportFile } = {}) {
  const policy = `${releasePolicy()}; report-uri ${REPORT_PATH}`;
  const base = realpathSync(root);
  const sources = sourceEntryMap(base);
  const listener = createServer(async (request, response) => {
    try {
      const url = new URL(request.url ?? "/", "http://127.0.0.1");
      if (request.method === "POST" && url.pathname === REPORT_PATH) {
        const chunks = [];
        for await (const chunk of request) chunks.push(chunk);
        if (reportFile) appendFileSync(reportFile, `${Buffer.concat(chunks).toString("utf8").replace(/\s*\n\s*/g, " ")}\n`, { mode: 0o600 });
        response.writeHead(204).end();
        return;
      }
      if (request.method !== "GET" && request.method !== "HEAD") { response.writeHead(405).end(); return; }
      if (url.pathname === RECORDER_PATH) {
        response.writeHead(200, { "Content-Type": types[".js"], "Cache-Control": "no-store" }).end(recorder);
        return;
      }
      const built = sources.get(url.pathname);
      if (built) { response.writeHead(302, { Location: built, "Cache-Control": "no-store" }).end(); return; }
      const relative = normalize(decodeURIComponent(url.pathname === "/" ? "/index.html" : url.pathname)).replace(/^[/\\]+/, "");
      const path = resolve(base, relative);
      if (path !== base && !path.startsWith(base + sep)) { response.writeHead(403).end(); return; }
      if (!(await stat(path).catch(() => null))?.isFile()) { response.writeHead(404).end(); return; }
      const type = types[extname(path).toLowerCase()] ?? "application/octet-stream";
      if (type.startsWith("text/html")) {
        const html = (await readFile(path, "utf8")).replace(/<head>/i, `<head><script src="${RECORDER_PATH}"></script>`);
        response.writeHead(200, { "Content-Type": type, "Content-Security-Policy": policy, "Cache-Control": "no-store" });
        response.end(request.method === "HEAD" ? undefined : html);
        return;
      }
      response.writeHead(200, { "Content-Type": type, "Cache-Control": "no-store" });
      response.end(request.method === "HEAD" ? undefined : await readFile(path));
    } catch (error) {
      response.writeHead(500).end(String(error?.message ?? error));
    }
  });
  listener.listen(port, "127.0.0.1");
  await once(listener, "listening");
  const close = () => new Promise((done) => { listener.closeAllConnections(); listener.close(() => done()); });
  return { url: `http://127.0.0.1:${listener.address().port}`, close, policy };
}

/** Reads the violations a report-uri delivered, one JSON report per line. */
export function readViolationReports(reportFile) {
  let text = "";
  try { text = readFileSync(reportFile, "utf8"); } catch (error) { if (error.code === "ENOENT") return []; throw error; }
  return text.split("\n").filter(Boolean).map((line) => {
    try {
      const report = JSON.parse(line)["csp-report"] ?? JSON.parse(line);
      return { directive: report["effective-directive"] ?? report["violated-directive"], blocked: report["blocked-uri"], source: report["source-file"], line: report["line-number"], page: report["document-uri"] };
    } catch {
      return { raw: line };
    }
  });
}

/** One line per distinct blocked source, for a failure message or a survey. */
export function describeViolations(violations) {
  const seen = new Map();
  for (const entry of violations) {
    const key = `${entry.directive} ${entry.blocked} ${entry.source ?? ""}`;
    const page = entry.page ? String(entry.page).replace(/^https?:\/\/[^/]+/, "") : "";
    if (!seen.has(key)) seen.set(key, { ...entry, pages: new Set() });
    if (page) seen.get(key).pages.add(page);
  }
  return [...seen.values()].map((entry) => `${entry.directive} blocked ${entry.blocked || "(inline)"}${entry.source ? ` from ${entry.source}${entry.line ? `:${entry.line}` : ""}` : ""}${entry.sample ? ` "${String(entry.sample).slice(0, 60)}"` : ""}${entry.pages.size ? ` on ${[...entry.pages].slice(0, 4).join(", ")}` : ""}`);
}

if (process.argv[1] && realpathSync(resolve(process.argv[1])) === fileURLToPath(import.meta.url)) {
  const [mode, value, receipt, reportFile] = process.argv.slice(2);
  if (mode === "check" && value) {
    const lines = describeViolations(readViolationReports(value));
    for (const [index, line] of lines.entries()) console.error(`  ${index + 1}. ${line}`);
    if (lines.length) {
      console.error(`The shipped Content-Security-Policy reported ${lines.length} distinct violation(s).`);
      process.exit(1);
    }
    console.log("No Content-Security-Policy violation was reported.");
    process.exit(0);
  }
  if (mode === "build" && value) {
    await buildReleaseRenderer(resolve(value));
    process.exit(0);
  }
  if (mode !== "start" || !receipt || !reportFile) {
    throw new Error("usage: release-renderer.mjs start <port> <receipt> <report-file> | build <dir> | check <report-file>");
  }
  // A runner that launches several times builds once (LOUPIST_E2E_RELEASE_RENDERER_DIR); otherwise this launch builds.
  const prebuilt = process.env.LOUPIST_E2E_RELEASE_RENDERER_DIR;
  const outDir = prebuilt || await mkdtemp(join(tmpdir(), "loupist-release-renderer-"));
  if (!prebuilt) await buildReleaseRenderer(outDir);
  const server = await serveReleaseRenderer(outDir, { port: Number(value), reportFile });
  // The same receipt shape as dev-server.mjs, so the launcher waits for either one.
  await writeFile(`${receipt}.tmp`, JSON.stringify({ pid: process.pid, url: server.url }), { mode: 0o600 });
  await rename(`${receipt}.tmp`, receipt);
  for (const signal of ["SIGTERM", "SIGINT", "SIGHUP"]) {
    process.once(signal, async () => { await server.close(); if (!prebuilt) await rm(outDir, { recursive: true, force: true }); process.exit(0); });
  }
}
