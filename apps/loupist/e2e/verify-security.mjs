import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const e2eRoot = dirname(fileURLToPath(import.meta.url));
const appRoot = resolve(e2eRoot, "..");
const read = (path) => readFileSync(path, "utf8");

assert.match(read(resolve(e2eRoot, ".npmrc")), /^ignore-scripts=true$/m);

const packageManifest = JSON.parse(read(resolve(e2eRoot, "package.json")));
assert.equal(packageManifest.devDependencies.webdriverio, "9.32.0");
assert.equal(packageManifest.scripts.mcp, undefined);

const appLauncher = read(resolve(e2eRoot, "run-app.sh"));
assert.match(appLauncher, /\/usr\/bin\/env -i/);
assert.match(appLauncher, /LOUPIST_E2E_APP_DATA_DIR/);
assert.doesNotMatch(appLauncher, /LOUPIST_E2E_APPROVE_NATIVE/);
assert.doesNotMatch(appLauncher, /LOUPIST_AGENT_E2E_KEYCHAIN/);

const windowsSmokeLauncher = read(resolve(e2eRoot, "run-native-smoke.ps1"));
assert.match(windowsSmokeLauncher, /ProcessStartInfo/);
assert.match(windowsSmokeLauncher, /\.Environment\.Clear\(\)/);
assert.match(windowsSmokeLauncher, /LOUPIST_E2E_APP_DATA_DIR/);
assert.match(windowsSmokeLauncher, /Get-NetTCPConnection/);
assert.match(windowsSmokeLauncher, /Get-CimInstance/);
assert.doesNotMatch(windowsSmokeLauncher, /LOUPIST_E2E_APPROVE_NATIVE/);
assert.doesNotMatch(windowsSmokeLauncher, /LOUPIST_AGENT_E2E_KEYCHAIN/);

const devLauncher = read(resolve(e2eRoot, "run-dev-app.sh"));
assert.match(devLauncher, /\/usr\/bin\/env -i/);
assert.doesNotMatch(devLauncher, /LOUPIST_AGENT_E2E_KEYCHAIN/);

// Guard the remaining OS credential store. The ordinary debug integration
// test in src-tauri/tests/debug_credentials.rs verifies behavior without the
// library's cfg(test) override and rejects every attempted keyring entry.
for (const [path, nativeFactory] of [
  ["src-tauri/src/finance_credentials.rs", "native_entry"],
]) {
  const source = read(resolve(appRoot, path));
  assert.match(
    source,
    /#\[cfg\(any\(debug_assertions,\s*test,\s*feature = "webdriver"\)\)\]\s*fn read_blob\(/,
    `${path}: all debug and automated builds must use session credentials`,
  );
  const factory = source.indexOf(`fn ${nativeFactory}(`);
  assert.notEqual(factory, -1, `${path}: native credential boundary is missing`);
  const gate = source.slice(source.lastIndexOf("#[cfg(", factory), factory);
  assert.doesNotMatch(gate, /\b(?:fn|const)\b/, `${path}: native factory must be directly gated`);
  assert.ok(
    gate.replace(/\s+/g, "").startsWith(
      '#[cfg(all(not(any(debug_assertions,test,feature="webdriver")),',
    ),
    `${path}: native credentials must be excluded from debug and automated builds`,
  );
  assert.doesNotMatch(source, /LOUPIST_AGENT_E2E_KEYCHAIN/);
}

// World requests go through the shared host-owned provider service. The graph
// bridge cannot read provider secrets or bypass that service with direct HTTP.
const graphInference = read(resolve(appRoot, "src-tauri/src/knowledge/graph_inference.rs"));
assert.match(graphInference, /ProviderInferenceService/);
assert.doesNotMatch(
  graphInference,
  /graph_credentials|helper_key|keyring::|provider_credential_from_environment|read_auth_map|auth\.json|reqwest::|ureq::/,
  "World inference must keep provider authentication inside the shared provider service",
);
const providerInference = read(resolve(appRoot, "src-tauri/src/providers/inference.rs"));
const inferenceRequest = providerInference.match(/pub struct ProviderInferenceRequest\s*\{([^}]+)\}/)?.[1];
assert.ok(inferenceRequest, "The isolated inference request boundary must exist");
assert.doesNotMatch(
  inferenceRequest,
  /\b(?:api_key|access_token|refresh_token|credentials?|password|auth_path|agent_data_dir)\b/,
  "Inference request files must not carry provider credentials or credential storage paths",
);
const noScriptOrCookies = /executeScript|eval\(|getCookies|setCookies/;
assert.doesNotMatch(read(resolve(e2eRoot, "agent-driver.mjs")), noScriptOrCookies);
assert.doesNotMatch(read(resolve(e2eRoot, "window-geometry.mjs")), noScriptOrCookies);
assert.doesNotMatch(read(resolve(e2eRoot, "detach-launch.mjs")), noScriptOrCookies);
assert.doesNotMatch(read(resolve(e2eRoot, "wait-session-lib.mjs")), noScriptOrCookies);
assert.doesNotMatch(read(resolve(appRoot, "scripts/tauri.mjs")), noScriptOrCookies);

assert.doesNotMatch(read(resolve(e2eRoot, "down.sh")), /pkill/);

const noCredentialIo = /auth\.json|copyFile|cpSync|readdirSync|readdir\(/;
for (const name of [
  "live-preset.mjs",
  "env-diagnostic.mjs",
  "up.sh",
  "run-native-live-journeys.sh",
]) {
  assert.doesNotMatch(read(resolve(e2eRoot, name)), noCredentialIo, name);
  assert.doesNotMatch(read(resolve(e2eRoot, name)), noScriptOrCookies, name);
}
for (const name of [
  "run-native-smoke.sh",
  "run-native-persistence.sh",
  "run-native-smoke.ps1",
]) {
  assert.doesNotMatch(
    read(resolve(e2eRoot, name)),
    /LOUPIST_LIVE_PI_AGENT_DATA_DIR/,
    `${name} must stay credential-free`,
  );
}

const cargoManifest = read(resolve(appRoot, "src-tauri/Cargo.toml"));
assert.match(cargoManifest, /^webdriver = \["dep:tauri-plugin-wdio-webdriver"\]$/m);
assert.match(cargoManifest, /^e2e = \["webdriver"\]$/m);
assert.match(cargoManifest, /^tauri-plugin-wdio-webdriver = \{ version = "=1\.3\.0", path = "vendor\/tauri-plugin-wdio-webdriver", optional = true \}$/m);

const driverRoot = resolve(appRoot, "src-tauri/vendor/tauri-plugin-wdio-webdriver");
const driverManifest = read(resolve(driverRoot, "Cargo.toml"));
assert.match(driverManifest, /^version = "1\.3\.0"$/m);
assert.match(driverManifest, /^license = "MIT"$/m);
assert.match(read(resolve(driverRoot, "LICENSE")), /Copyright \(c\) 2024 WebdriverIO Community/);
assert.equal(createHash("sha256").update(read(resolve(driverRoot, "LICENSE"))).digest("hex"),
  "20c84663dbfa685b230f362509fdbaea7819f5da166afea9857c4d96dcc7e0bc");
const driverServer = read(resolve(driverRoot, "src/server/mod.rs"));
assert.match(driverServer, /#\[cfg\(not\(target_os = "android"\)\)\]\s*let addr = SocketAddr::from\(\(\[127, 0, 0, 1\], port\)\)/);
assert.match(read(resolve(e2eRoot, "wait-session-lib.mjs")), /"wdio:tauriServiceOptions": \{ windowLabel: "main" \}/);
const mainCapability = JSON.parse(read(resolve(appRoot, "src-tauri/capabilities/default.json")));
assert.deepEqual(mainCapability.webviews, ["main"]);
assert.equal(mainCapability.remote, undefined);

assert.match(
  read(resolve(appRoot, "src-tauri/build.rs")),
  /WebDriver is forbidden in release builds/,
);
assert.match(
  read(resolve(appRoot, "src-tauri/src/lib.rs")),
  /compile_error!\("WebDriver is forbidden in release builds"\)/,
);
assert.match(
  read(resolve(appRoot, "src-tauri/src/lib.rs")),
  /com\.monarchjuno\.loupist\.e2e/,
);
assert.doesNotMatch(
  read(resolve(appRoot, "src-tauri/src/app.rs")),
  /LOUPIST_E2E_APPROVE_NATIVE/,
);

const tauriConfig = JSON.parse(
  read(resolve(appRoot, "src-tauri/tauri.e2e.conf.json")),
);
assert.equal(tauriConfig.identifier, "com.monarchjuno.loupist.e2e");
assert.equal(tauriConfig.bundle.active, false);

console.log("Loupist E2E security invariants passed.");
