import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const scripts = dirname(fileURLToPath(import.meta.url));
const app = dirname(scripts);
const profile = JSON.parse(readFileSync(join(scripts, "pi-security.json"), "utf8"));

function installedPackage(name, from) {
  const require = createRequire(join(from, "package.json"));
  for (const base of require.resolve.paths(name) ?? []) {
    const root = join(base, name);
    const manifestPath = join(root, "package.json");
    if (existsSync(manifestPath)) {
      const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
      if (manifest.name === name) return { root, manifest };
    }
  }
  throw new Error(`Cannot locate installed package ${name}`);
}

/** Check the SDK's actual resolution: old Pi shrinkwrap ignored root overrides. */
export function verifyPiDependencies() {
  const pi = installedPackage("@earendil-works/pi-coding-agent", app);
  const undici = installedPackage("undici", pi.root);
  const minimatch = installedPackage("minimatch", pi.root);
  const braces = installedPackage("brace-expansion", minimatch.root);
  const lock = JSON.parse(readFileSync(join(app, "package-lock.json"), "utf8"));
  for (const [pkg, expected] of [
    [pi, profile.pi], [undici, profile.undici], [braces, profile["brace-expansion"]],
  ]) {
    assert.equal(pkg.manifest.version, expected, `${pkg.manifest.name} must match the security pin; run npm ci`);
    const key = relative(app, pkg.root).split(sep).join("/");
    assert.equal(lock.packages[key]?.version, expected, `${pkg.manifest.name} differs from the lockfile`);
    assert.ok(lock.packages[key]?.integrity, `${pkg.manifest.name} needs an integrity pin`);
  }
  assert.equal(existsSync(join(pi.root, "npm-shrinkwrap.json")), false,
    "Pi shrinkwrap prevents security overrides from reaching installed dependencies");
  return { pi, undici, braces };
}
