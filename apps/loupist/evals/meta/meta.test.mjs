import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { applyPatch, diffSurface, validateChange, surfaceHash } from './surface.mjs';
import { partition, adaptiveSelection, acceptance, archiveCandidate, score, redact } from './core.mjs';
import { addCommittedLocks, loadSuite, proposerArgs, report } from './optimize.mjs';

const prompt = 'Helpful prose.\n<!-- safety:locked -->\nNever trade.\n<!-- /safety:locked -->\nAdjacent workflow.\n';
const assets = { 'SYSTEM.md': prompt, 'skills/lens/SKILL.md': '---\nname: lens\ndescription: A method\n---\nWorkflow.\n',
  'tool-catalog.json': '{"tools":[{"name":"read","description":"Read data","parameters":{"type":"object"}}]}\n',
  'context-policy.json': '{"reserve_tokens":16384,"keep_recent_tokens":20000}\n' };
const check = (path, text) => applyPatch(assets, diffSurface(assets, { ...assets, [path]: text }));

test('hash partition is stable across order and excludes regressions', () => {
  const scenarios = Array.from({ length: 100 }, (_, n) => ({ id: `case-${n}`, split: 'capability' }));
  const first = partition({ scenarios: [...scenarios, { id: 'regression', split: 'regression' }] });
  assert.deepEqual(first, partition({ scenarios: [...scenarios].reverse() }));
  assert.equal(first.train.length + first.validation.length, 100);
  assert.equal(first.train.filter(id => first.validation.includes(id)).length, 0);
  assert.throws(() => partition({ scenarios: [{ id: 'secret', split: 'heldout' }, ...scenarios] }), /Held-out/u);
});
test('adaptive selection keeps failures, excludes judge unknowns, and samples 30 percent of passes', () => {
  const scenarios = { fail: false, missing: null, ...Object.fromEntries(Array.from({ length: 10 }, (_, n) => [`pass-${n}`, true])) };
  const ids = adaptiveSelection({ scenarios }, 1, 'seed');
  assert.equal(ids.length, 4); assert.ok(ids.includes('fail')); assert.ok(!ids.includes('missing'));
  assert.deepEqual(ids, adaptiveSelection({ scenarios }, 1, 'seed'));
});
test('unified diffs apply exact adjacent edits and protect lock contents and markers', () => {
  assert.equal(check('SYSTEM.md', prompt.replace('Adjacent workflow.', 'Improved workflow.'))['SYSTEM.md'], prompt.replace('Adjacent workflow.', 'Improved workflow.'));
  assert.throws(() => check('SYSTEM.md', prompt.replace('Never trade.', 'Trade.')), /Locked/u);
  assert.throws(() => check('SYSTEM.md', prompt.replace('<!-- safety:locked -->\n', '')), /lock/iu);
  assert.throws(() => check('SYSTEM.md', prompt.replace('<!-- /safety:locked -->', '<!-- safety:locked -->')), /lock/iu);
  assert.throws(() => applyPatch(assets, '--- a/../SYSTEM.md\n+++ b/../SYSTEM.md\n@@ -1 +1 @@\n-x\n+y\n'), /path/u);
  for (const path of ['optimizer.mjs', 'src-tauri/src/pi.rs', 'agent/heldout/a.json', '/tmp/file', 'skills/lens/../../other']) {
    assert.throws(() => applyPatch(assets, `--- a/${path}\n+++ b/${path}\n@@ -1 +1 @@\n-x\n+y\n`), /path/u);
  }
  const patch = diffSurface(assets, { ...assets, 'SYSTEM.md': prompt + 'New line.\n' });
  assert.throws(() => applyPatch(assets, patch.replace('-Helpful prose.', '-Wrong context.')), /mismatch/u);
  assert.throws(() => applyPatch(assets, patch + patch), /duplicate/u);
});
test('Skill frontmatter, schemas, and context-policy bounds cannot be changed', () => {
  assert.ok(check('skills/lens/SKILL.md', assets['skills/lens/SKILL.md'] + 'Check facts.\n'));
  assert.throws(() => check('skills/lens/SKILL.md', assets['skills/lens/SKILL.md'].replace('name: lens', 'name: other')), /frontmatter/u);
  assert.ok(check('tool-catalog.json', assets['tool-catalog.json'].replace('Read data', 'Read the original data')));
  assert.throws(() => check('tool-catalog.json', assets['tool-catalog.json'].replace('"name":"read"', '"name":"write"')), /description/u);
  assert.throws(() => check('tool-catalog.json', assets['tool-catalog.json'].replace('"type":"object"', '"type":"string"')), /description/u);
  assert.throws(() => validateChange('context-policy.json', assets['context-policy.json'], '{"reserve_tokens":1,"keep_recent_tokens":20000}'), /50%/u);
  assert.throws(() => validateChange('context-policy.json', assets['context-policy.json'], '{"reserve_tokens":8192,"keep_recent_tokens":20000,"retry":true}'), /two/u);
});
const result = (passed, total = 4) => ({ passed, total, scenarios: Object.fromEntries(Array.from({ length: total }, (_, i) => [`case-${i}`, i < passed])), complete: true, checks: 10, checks_passed: 10, deterministic_failures: [] });
const smoke = () => ({ 'principles-v1': result(4), 'security-v1': result(4) });
test('acceptance requires one complete train scenario gain and regression checks outrank scores', () => {
  assert.equal(acceptance(result(1), result(2), smoke(), smoke()).accepted, true);
  assert.equal(acceptance(result(1), result(1), smoke(), smoke()).accepted, false);
  assert.equal(acceptance(result(1), { ...result(4), complete: false }, smoke(), smoke()).accepted, false);
  const drop = smoke(); drop['principles-v1'].checks_passed--;
  assert.equal(acceptance(result(1), result(4), smoke(), drop).accepted, false);
  const previous = smoke(), offset = smoke();
  previous['principles-v1'].metrics = { carry: { passed: 2, total: 2 }, saves: { passed: 0, total: 2 } };
  offset['principles-v1'].metrics = { carry: { passed: 1, total: 2 }, saves: { passed: 1, total: 2 } };
  assert.equal(acceptance(result(1), result(4), previous, offset).accepted, false);
  const boundary = smoke(); boundary['security-v1'].deterministic_failures.push({ pass: false });
  assert.equal(acceptance(result(1), result(4), boundary, boundary).accepted, false);
});
test('archive persists the four required candidate artifacts with an actual asset digest', async () => {
  const root = await mkdtemp(join(tmpdir(), 'meta-test-'));
  try {
    const directory = await archiveCandidate(root, 2, { diff: 'patch\n', assets, scores: result(2), traces: [{ run_id: 'example' }] });
    assert.deepEqual((await readdir(directory)).sort(), ['assets.sha256', 'diff.patch', 'scores.json', 'traces.jsonl']);
    assert.equal((await readFile(join(directory, 'assets.sha256'), 'utf8')).trim(), surfaceHash(assets));
    assert.equal(JSON.parse(await readFile(join(directory, 'scores.json'), 'utf8')).passed, 2);
    assert.equal(JSON.parse((await readFile(join(directory, 'traces.jsonl'), 'utf8')).trim()).run_id, 'example');
  } finally { await rm(root, { recursive: true, force: true }); }
});
test('missing turns never score as passing and held-out paths fail before reading', async () => {
  const suite = { schema: 'loupist-agent-suite/1', version: 1, suite: 'fixture', scenarios: [{ id: 'A', split: 'capability', title: 'A', tags: [], connections: [], sessions: [{ id: 's', role: 'probe', memory: 'off', turns: [{ user: '2 + 2?', expect: { answer: [{ assert: 'contains_any', values: ['4'], metric: 'task_success' }] } }] }] }] };
  const scores = score(suite, [], [], 2);
  assert.equal(scores.passed, 0); assert.equal(scores.complete, false);
  await assert.rejects(loadSuite('/missing/heldout/secret.json'), /Held-out/u);
});
test('committed baseline gains only locks while excluding uncommitted wording', () => {
  const committed = 'Workflow.\nNever trade.\n';
  const marked = 'New uncommitted workflow.\n<!-- safety:locked -->Never trade.<!-- /safety:locked -->\n';
  assert.equal(addCommittedLocks(committed, marked), 'Workflow.\n<!-- safety:locked -->Never trade.<!-- /safety:locked -->\n');
});
test('proposer controls pin required model and effort, and excerpts redact credential patterns', () => {
  const codex = proposerArgs('codex', '/scratch', '/scratch/result.patch');
  assert.ok(codex.includes('gpt-6-astra')); assert.ok(codex.includes('model_reasoning_effort="high"'));
  assert.ok(codex.includes('--ignore-user-config')); assert.ok(codex.includes('features.shell_tool=false'));
  const grok = proposerArgs('grok', '/scratch', '/scratch/result.patch');
  assert.ok(grok.includes('grok-4.7-build-fast')); assert.ok(grok.includes('high'));
  assert.deepEqual(redact({ token: 'sensitive', answer: 'Bearer secret_value sk-abcdefghijklmnop' }), { token: '[REDACTED]', answer: 'Bearer [REDACTED] [REDACTED]' });
});


test('adoption report shows measured adaptive scores even without full-train promotion', () => {
  const text = report({ commit: 'abc', split: { train: ['a', 'b'], validation: ['c'] }, baseline: { passed: 1, total: 2 }, best: 0,
    iterations: [{ n: 1, subset: { passed: 0, total: 1 }, reason: 'No gain' }],
    validation: { baseline: { passed: 0, eligible_total: 1, scenarios: { c: false } }, best: { passed: 0, eligible_total: 1, scenarios: { c: false } } }, original: assets, assets, usage: [] });
  assert.match(text, /\| 1 \| 0\/1 \| not scored \| No gain \|/u);
  assert.match(text, /human adoption pending/u);
  assert.match(text, /Mean provider input tokens/u);
});
