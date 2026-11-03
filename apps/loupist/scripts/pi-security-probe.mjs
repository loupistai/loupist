import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";

/** Run through Pi's own model matcher and configured global HTTP client. */
export async function probePiSecurity(resolveModelScope, baseUrl) {
  const runtime = { getAvailable: async () => [{ id: "test", provider: "fixture" }] };
  const ordinary = await resolveModelScope(["fixture/t*"], runtime);
  assert.equal(ordinary.scopedModels.length, 1);
  for (const pattern of [
    "{".repeat(5_000) + "a,b" + "}".repeat(5_000) + "*",
    "{a,".repeat(5_000) + "z" + "}".repeat(5_000) + "*",
  ]) {
    const result = await resolveModelScope([pattern], runtime);
    assert.equal(result.scopedModels.length, 0);
  }
  assert.deepEqual(await (await fetch(`${baseUrl}/healthy`)).json(), { ok: true });
  await assert.rejects(async () => (await fetch(`${baseUrl}/excessive-encodings`)).text());
  // The vulnerable implementation rejects fetch, then emits an uncaught error
  // on its orphaned response stream after the server closes the connection.
  await delay(200);
  assert.deepEqual(await (await fetch(`${baseUrl}/healthy`)).json(), { ok: true });
  return { patterns: "passed", compressedHttp: "passed", recovery: "passed" };
}
