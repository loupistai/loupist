import test from "node:test";
import assert from "node:assert/strict";
import { inspectBoundaries } from "./check-boundaries.mjs";

test("Tauri imports, exports, dynamic imports, require and type imports stay in bridge", () => {
  const source = [
    'import { invoke } from "@tauri-apps/api/core";',
    'export { open } from "@tauri-apps/plugin-dialog";',
    'void import("@tauri-apps/api/event");',
    'const fs = require("@tauri-apps/plugin-fs");',
    'import core = require("@tauri-apps/api/core");',
    'type Core = import("@tauri-apps/api/core").Core;',
  ].join("\n");
  const hits = inspectBoundaries("src/App.tsx", source);
  assert.equal(hits.length, 6);
  assert.deepEqual(hits.map((hit) => hit.line), [1, 2, 3, 4, 5, 6]);
  assert.ok(hits.every((hit) => hit.rule === "boundaries/tauri-only-in-bridge"));
  assert.deepEqual(inspectBoundaries("src/bridge/events.ts", source), []);
  assert.equal(inspectBoundaries("src/bridge.ts", source).length, 6);
});

test("components use the typed bridge, including alias and relative imports", () => {
  for (const path of ["../../bridge/tauriBridge", "@/bridge/tauriBridge.ts", "src/bridge/tauriBridge"]) {
    const hits = inspectBoundaries("src/components/ui/Widget.tsx", `import { Bridge } from "${path}";`);
    assert.equal(hits[0].rule, "boundaries/components-typed-bridge");
  }
  assert.deepEqual(inspectBoundaries("src/components/Widget.tsx", 'import { bridge } from "../bridge";'), []);
  assert.deepEqual(inspectBoundaries("src/bridge.ts", 'import { Bridge } from "./bridge/tauriBridge";'), []);
});

test("preview and mock imports cannot enter production, with exact directory matching", () => {
  for (const path of ["./preview/fixtures", "@/preview/scenarios", "./bridge/mock/state", "@/bridge/mock", "./bridge/mockBridge", "@/bridge/mockBridge.ts"]) {
    const hits = inspectBoundaries("src/App.tsx", `export * from "${path}";`);
    assert.equal(hits[0].rule, "boundaries/no-preview-in-production");
  }
  for (const file of ["src/App.test.tsx", "src/App.spec.ts", "src/test/setup.ts", "src/__tests__/fixture.ts", "src/preview/main.tsx", "src/bridge/mock/state.ts", "src/bridge/mockBridge.ts"]) {
    assert.deepEqual(inspectBoundaries(file, 'import { state } from "@/bridge/mock/state";'), []);
  }
  assert.deepEqual(inspectBoundaries("src/App.tsx", 'import "./previewer"; import "./bridge/mockery";'), []);
});

test("comments and ordinary strings are not imports", () => {
  assert.deepEqual(inspectBoundaries("src/App.tsx", '// import "@tauri-apps/api/core";\nconst example = \'import("@tauri-apps/api/event")\';'), []);
});
