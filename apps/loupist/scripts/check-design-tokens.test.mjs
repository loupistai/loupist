import test from "node:test";
import assert from "node:assert/strict";
import { copyFile, mkdtemp, mkdir, rm, symlink, writeFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { inspectDesignTokens, checkDesignTokens } from "./check-design-tokens.mjs";

test("hex and color functions are found in CSS, TypeScript strings and JSX", () => {
  for (const value of ["#abc", "#abcd", "#abcdef", "#abcdef12", "rgb(0, 0, 0)", "rgba(0,0,0,.5)", "hsl(0 0% 0%)", "hsla(0,0%,0%,.5)", "oklch(.5 .1 30)"]) {
    for (const [file, source] of [["src/a.css", `a { color: ${value}; }`], ["src/a.ts", `const color = "${value}";`], ["src/a.tsx", `<div style={{color: "${value}"}} />`]]) {
      assert.equal(inspectDesignTokens(file, source)[0].rule, "design-tokens/raw-color", `${file}: ${value}`);
    }
  }
  assert.deepEqual(inspectDesignTokens("src/a.ts", 'class C { #abc = 1; } // #fff'), []);
});

test("tokens own raw values but legacy aliases remain forbidden everywhere", () => {
  assert.deepEqual(inspectDesignTokens("src/styles/tokens.css", ":root { --gg-color-a: #abc; --gg-z-a: 10; }"), []);
  const aliases = ["ink", "ink-soft", "muted", "muted-strong", "line", "line-strong", "app-bg", "navigation-bg", "paper", "paper-soft", "surface-raised", "user-bubble", "overlay", "primary-bg", "primary-ink", "danger", "danger-soft", "warning-soft", "code-bg", "focus-ring", "accent-soft", "shadow"];
  for (const alias of aliases) assert.equal(inspectDesignTokens("src/styles/tokens.css", `--${alias}: var(--gg-color-a);`)[0].rule, "design-tokens/legacy-alias");
  assert.deepEqual(inspectDesignTokens("src/a.css", "a { color: var(--gg-color-text); box-shadow: var(--gg-shadow-sm); --paperwork: 1; }"), []);
});

test("literal CSS and React z-index values are caught, token expressions and auto pass", () => {
  assert.equal(inspectDesignTokens("src/a.css", "a {\n z-index: -10;\n}")[0].line, 2);
  for (const value of ["10", "-1", '"100"']) assert.equal(inspectDesignTokens("src/a.tsx", `<div style={{zIndex: ${value}}} />`)[0].rule, "design-tokens/literal-z-index");
  assert.deepEqual(inspectDesignTokens("src/a.css", "a { z-index: var(--gg-z-dialog); } b { z-index: auto; }"), []);
  assert.deepEqual(inspectDesignTokens("src/a.ts", 'const style = {zIndex: layer};'), []);
});

test("same-line justified comments suppress raw values; empty, fake and other-line comments do not", () => {
  assert.deepEqual(inspectDesignTokens("src/a.css", "a { color: #fff; } /* token-exception: vendor palette */"), []);
  assert.deepEqual(inspectDesignTokens("src/a.tsx", '<div color="#fff" /> // token-exception: exported illustration'), []);
  assert.deepEqual(inspectDesignTokens("src/a.ts", 'const color = "#fff"; /* token-exception: export */'), []);
  assert.deepEqual(inspectDesignTokens("src/a.ts", 'const color = `rgba(${r}, 0, 0, 1)`; // token-exception: computed user color'), []);
  assert.deepEqual(inspectDesignTokens("src/a.tsx", '<div color="#fff">{/* token-exception: illustration */}</div>'), []);
  assert.equal(inspectDesignTokens("src/a.css", "/* token-exception: previous line */\na { color: #fff; }").length, 1);
  assert.equal(inspectDesignTokens("src/a.css", "a { color: #fff; } /* token-exception:  */").length, 1);
  assert.equal(inspectDesignTokens("src/a.ts", 'const color = "#fff"; const fake = "// token-exception: forged";').length, 1);
  assert.equal(inspectDesignTokens("src/a.css", "a { color: var(--ink); } /* token-exception: no alias bypass */").length, 1);
});

test("CLI report succeeds with findings, enforce fails, and malformed arguments are rejected", async () => {
  const root = await mkdtemp(join(tmpdir(), "design-token-cli-"));
  try {
    await mkdir(join(root, "src"));
    await mkdir(join(root, "scripts"));
    await copyFile(new URL("./check-design-tokens.mjs", import.meta.url), join(root, "scripts/check-design-tokens.mjs"));
    await symlink(fileURLToPath(new URL("../node_modules", import.meta.url)), join(root, "node_modules"), "dir");
    await writeFile(join(root, "src/a.css"), "a { color: #fff; }");
    const run = (...args) => spawnSync(process.execPath, [join(root, "scripts/check-design-tokens.mjs"), ...args], { encoding: "utf8" });
    const report = run("--report");
    assert.equal(report.status, 0, report.stderr);
    assert.match(report.stdout, /src\/a.css:1: design-tokens\/raw-color/);
    assert.equal(run("--enforce").status, 1);
    assert.equal(run("--unknown").status, 2);
    assert.equal(run("--report", "extra").status, 2);
    await writeFile(join(root, "src/a.css"), "a { color: #fff; } /* token-exception: fixture */");
    assert.equal(run("--enforce").status, 0);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test("svg rules sized at 20px or more are decorative icons; small icons and exceptions pass", () => {
  for (const value of ["24px", "var(--gg-size-control-sm)", "var(--gg-space-5)"]) {
    assert.equal(inspectDesignTokens("src/a.css", `.empty > svg { width: ${value}; }`)[0].rule, "design-tokens/decorative-icon-size");
  }
  assert.deepEqual(inspectDesignTokens("src/a.css", ".row svg { width: 16px; height: var(--gg-space-4); }"), []);
  assert.deepEqual(inspectDesignTokens("src/a.css", ".hero img { width: 64px; }"), []);
  assert.deepEqual(inspectDesignTokens("src/a.css", ".thumb > svg { width: 42px; /* token-exception: file thumbnail */ }"), []);
});

test("filesystem scan includes CSS, TS and TSX but excludes tests and unrelated files", async () => {
  const root = await mkdtemp(join(tmpdir(), "design-token-check-"));
  try {
    await mkdir(join(root, "src"));
    await writeFile(join(root, "src/a.css"), "a { color: #fff; }");
    await writeFile(join(root, "src/a.test.ts"), 'const color = "#fff";');
    await writeFile(join(root, "src/a.tsx"), '<div color="#fff" />');
    await writeFile(join(root, "src/a.json"), '{"color":"#fff"}');
    assert.deepEqual((await checkDesignTokens(root)).map((hit) => hit.file), ["src/a.css", "src/a.tsx"]);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
