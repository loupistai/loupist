import test from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildFeedbackEvalCase } from '../../src/chat/answerFeedbackCore.ts';
import { assertScenarioContract, isKoreanUserTurn } from './suite-contract.mjs';

test('the feedback core imports and shapes a scenario in plain Node without a loader', () => {
  const coreUrl = new URL('../../src/chat/answerFeedbackCore.ts', import.meta.url).href;
  const script = `
    const { buildFeedbackEvalCase } = await import(${JSON.stringify(coreUrl)});
    console.log(JSON.stringify(buildFeedbackEvalCase('plain-node', 'Compare earnings', 'Use consolidated figures')));
  `;
  const scenario = JSON.parse(execFileSync(process.execPath, ['--input-type=module', '--eval', script], {
    encoding: 'utf8',
    env: { ...process.env, NODE_OPTIONS: '' },
  }));
  assert.equal(scenario.title, 'User-corrected answer');
  assert.equal(scenario.sessions[0].turns[0].user, 'Compare earnings');
  assertScenarioContract(scenario, { suite: 'feedback', label: 'plain Node feedback' });
  assert.throws(() => buildFeedbackEvalCase('empty', '  ', 'Correction'), /An original request and saved correction are required/);
});

test('an exported feedback scenario passes the shared suite contract', () => {
  const request = '영업이익을 비교해 주세요';
  const correction = '연결 기준으로 비교하세요.';
  const scenario = buildFeedbackEvalCase('assistant-1234', request, correction);
  assertScenarioContract(scenario, { suite: 'feedback', label: 'feedback-v1.json' });
  assert.equal(scenario.tags.includes('ko'), isKoreanUserTurn(request + correction));
  const namedRequest = 'Please compare Value Hanbit operating profit in the filing';
  const namedCorrection = '한빛은 연결 기준';
  const named = buildFeedbackEvalCase('named', namedRequest, namedCorrection);
  assert.equal(isKoreanUserTurn(namedRequest + namedCorrection), false);
  assert.equal(named.tags.includes('ko'), false);
  assertScenarioContract(named, { suite: 'feedback', label: 'feedback-v1.json' });
});

test('the developer exporter writes only a scrubbed scenario and refuses overwrite', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'loupist-feedback-'));
  try {
    const input = join(dir, 'input.json');
    const output = join(dir, 'output.json');
    await writeFile(input, JSON.stringify({ messageId: 'answer', request: 'Compare consolidated earnings at https://example.com/filing', correction: 'Use consolidated figures.', answer: 'Do not export answer bodies.' }));
    const command = [fileURLToPath(new URL('./export-feedback.mjs', import.meta.url)), input, output];
    execFileSync(process.execPath, command, { stdio: 'pipe' });
    const bytes = await readFile(output, 'utf8');
    assertScenarioContract(JSON.parse(bytes), { suite: 'feedback', label: 'exported feedback' });
    assert.ok(!bytes.includes('https://example.com'));
    assert.ok(!bytes.includes('Do not export answer bodies'));
    assert.throws(() => execFileSync(process.execPath, command, { stdio: 'pipe' }));
    assert.equal(await readFile(output, 'utf8'), bytes);
  } finally {
    await rm(dir, { recursive: true, force: true });
  }
});

test('optional saved principle becomes a scrubbed cross-Chat regression candidate', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'loupist-principle-feedback-'));
  try {
    const input = join(dir, 'input.json'), output = join(dir, 'output.json');
    await writeFile(input, JSON.stringify({ messageId: 'principle-answer', request: 'Compare fictional cash flow', correction: 'Use cash conversion.', principle: 'Check cash conversion before profit. https://private.example/secret' }));
    execFileSync(process.execPath, [fileURLToPath(new URL('./export-feedback.mjs', import.meta.url)), input, output], { stdio: 'pipe' });
    const scenario = JSON.parse(await readFile(output, 'utf8'));
    assertScenarioContract(scenario, { suite: 'feedback' });
    assert.equal(scenario.sessions.length, 2);
    assert.equal(scenario.sessions[0].memory, 'remember');
    assert.equal(scenario.sessions[0].turns[0].expect.memory[0].kind, 'principle');
    assert.equal(scenario.sessions[1].memory, 'read_only');
    assert.equal(scenario.sessions[1].turns[0].expect.rubric[0].metric, 'carry_forward');
    assert.ok(!JSON.stringify(scenario).includes('private.example'));
  } finally { await rm(dir, { recursive: true, force: true }); }
});
