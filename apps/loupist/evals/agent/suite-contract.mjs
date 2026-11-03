import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { dirname, join, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const catalogPath = join(here, '../../agent/tool-catalog.json');

import { TRACE_ASSERTS, MEMORY_ASSERTS, MEMORY_KINDS, ANSWER_ASSERTS, answerPatterns, citationAssertion, validDate,
  numbersLinkedAssertion, asOfLeftOutAssertion, asOfRefusedAssertion, calculateInputsAssertion, gridCellsAssertion, metricTolerance,
  toolCalledAssertion, sensitivityCardAssertion, planProgressAssertion, calculationChipAssertion } from './eval-contract.mjs';
const METRICS = new Set([
  'carry_forward',
  'negative_transfer',
  'save_recall',
  'save_precision',
  'revision_disclosure',
  'tool_validity',
  'connection_honesty',
  'prediction_structure',
  'task_success',
  'retrieval',
  'efficiency',
  'boundary',
]);
const CONNECTIONS = new Set(['opendart', 'krx', 'koreainvestment', 'world-bank', 'web-research']);
const ROLES = new Set(['train', 'probe']);
const MEMORY_MODES = new Set(['remember', 'read_only', 'off']);
const NATIVE_TOOLS = ['read', 'write', 'edit', 'bash', 'ls', 'find', 'grep'];

const catalog = JSON.parse(await readFile(catalogPath, 'utf8'));
export const toolNames = new Set(catalog.tools.map((tool) => tool.name));
for (const name of NATIVE_TOOLS) toolNames.add(name);
const componentIds = new Set(catalog.components.map((component) => component.id));

function stringsOf(value, out = []) {
  if (typeof value === 'string') out.push(value);
  else if (Array.isArray(value)) value.forEach((item) => stringsOf(item, out));
  else if (value && typeof value === 'object') Object.values(value).forEach((item) => stringsOf(item, out));
  return out;
}

function leaves(value, path = '', out = []) {
  if (value === null || typeof value !== 'object') {
    out.push({ path, value });
    return out;
  }
  if (Array.isArray(value)) {
    value.forEach((item, index) => leaves(item, `${path}[${index}]`, out));
    return out;
  }
  for (const [key, item] of Object.entries(value)) leaves(item, path ? `${path}.${key}` : key, out);
  return out;
}

function numberPresent(corpus, value) {
  const form = JSON.stringify(value);
  const escaped = form.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return new RegExp(`(?<![0-9.])${escaped}(?![0-9.])`).test(corpus);
}

export function expectations(turn) {
  const expect = turn.expect ?? {};
  return [
    ...(expect.trace ?? []).map((item) => ({ bucket: 'trace', item })),
    ...(expect.answer ?? []).map((item) => ({ bucket: 'answer', item })),
    ...(expect.memory ?? []).map((item) => ({ bucket: 'memory', item })),
    ...(expect.rubric ?? []).map((item) => ({ bucket: 'rubric', item })),
  ];
}

export function userCorpus(scenario) {
  const chunks = [];
  for (const session of scenario.sessions ?? []) {
    for (const turn of session.turns ?? []) {
      chunks.push(turn.user ?? '');
      for (const attachment of turn.attachments ?? []) chunks.push(attachment.text ?? '');
    }
  }
  for (const value of Object.values(scenario.workspace_files ?? {})) chunks.push(value);
  return chunks.join('\n');
}

/** Hangul is dominant when it reaches the suite threshold and exceeds Latin letters. */
export function isKoreanUserTurn(text) {
  const hangul = (text.match(/[가-힣]/g) ?? []).length;
  const latin = (text.match(/[A-Za-z]/g) ?? []).length;
  return hangul >= 12 && hangul > latin;
}

/** Location is part of the split contract, including for temporary suite fixtures. */
export function isHeldoutSuitePath(filePath) {
  return typeof filePath === 'string' && resolve(filePath).split(sep).join('/').includes('/evals/agent/heldout/');
}

export function assertScenarioSplit(scenario, filePath) {
  const allowed = isHeldoutSuitePath(filePath) ? ['heldout'] : ['capability', 'regression'];
  assert.ok(allowed.includes(scenario.split), `${scenario.id}: split ${scenario.split} is not allowed here; heldout scenarios belong only under evals/agent/heldout/`);
}

/** Scenario-level checks from the agent-suite contract. File-level checks stay with the suite test. */
export function assertScenarioContract(scenario, { suite, label = suite, seenIds = new Map(), filePath } = {}) {
  const name = label;
  assert.equal(typeof scenario.id, 'string');
  assert.ok(scenario.id.length > 0, name);
  assert.equal(seenIds.has(scenario.id), false, `duplicate scenario id ${scenario.id}`);
  seenIds.set(scenario.id, name);
  assert.equal(typeof scenario.title, 'string');
  assert.ok(Array.isArray(scenario.tags), scenario.id);
  assert.equal(new Set(scenario.tags).size, scenario.tags.length, scenario.id);
  assertScenarioSplit(scenario, filePath);
  if (scenario.connections !== undefined) {
    assert.ok(Array.isArray(scenario.connections), scenario.id);
    for (const connection of scenario.connections) assert.ok(CONNECTIONS.has(connection), `${scenario.id} ${connection}`);
  }
  if ((scenario.connections ?? []).length > 0 || (scenario.tags ?? []).includes('needs-replay') || scenario.replay) {
    assert.equal(typeof scenario.replay, 'string', scenario.id);
    assert.match(scenario.replay, /^replay\/[a-z0-9]+(?:-[a-z0-9]+)*\.json$/u, scenario.id);
    assert.ok((scenario.tags ?? []).includes('needs-replay'), scenario.id);
    assert.equal(typeof scenario.replay_needs, 'string', scenario.id);
    assert.ok(scenario.replay_needs.length > 40, scenario.id);
  }
  if (scenario.workspace_files !== undefined) {
    assert.equal(typeof scenario.workspace_files, 'object', scenario.id);
    for (const [path, body] of Object.entries(scenario.workspace_files)) {
      assert.equal(typeof body, 'string', `${scenario.id} ${path}`);
      assert.equal(path.startsWith('/'), false, path);
      assert.equal(path.split('/').includes('..'), false, path);
    }
  }
  assert.ok(Array.isArray(scenario.sessions) && scenario.sessions.length > 0, scenario.id);
  const sessionIds = new Set();
  for (const session of scenario.sessions) {
    assert.equal(sessionIds.has(session.id), false, session.id);
    sessionIds.add(session.id);
    assert.ok(ROLES.has(session.role), `${scenario.id} ${session.role}`);
    assert.ok(MEMORY_MODES.has(session.memory), `${scenario.id} ${session.memory}`);
    assert.ok(Array.isArray(session.turns) && session.turns.length > 0, session.id);
    for (const [index, turn] of session.turns.entries()) {
      const where = `${scenario.id} ${session.id}#${index}`;
      assert.equal(typeof turn.user, 'string', where);
      assert.ok(turn.user.trim().length > 0, where);
      if (turn.attachments !== undefined) {
        assert.ok(Array.isArray(turn.attachments) && turn.attachments.length <= 4, `${where} attachments`);
        for (const attachment of turn.attachments) {
          assert.deepEqual(Object.keys(attachment).sort(), ['filename', 'media_type', 'text'], where);
          assert.equal(typeof attachment.filename, 'string', where);
          assert.ok(attachment.filename.trim().length > 0 && Buffer.byteLength(attachment.filename) <= 255, where);
          assert.doesNotMatch(attachment.filename, /[/\\\p{Cc}]/u, where);
          assert.match(attachment.media_type, /^(?:text\/[a-z0-9.+-]+|application\/(?:json|xml|csv))$/u, where);
          assert.equal(typeof attachment.text, 'string', where);
        }
      }
      if (turn.as_of !== undefined) {
        assert.ok(validDate(turn.as_of), `${where} as_of must be YYYY-MM-DD`);
        assert.ok(scenario.tags.includes('as-of'), `${where} cutoff scenarios carry the as-of tag`);
      }
      const entries = expectations(turn);
      assert.ok(entries.length > 0, `${where} needs an expectation`);
      for (const { bucket, item } of entries) {
        assert.ok(METRICS.has(item.metric), `${where} metric ${item.metric}`);
        if (bucket === 'trace') {
          assert.ok(TRACE_ASSERTS.has(item.assert), `${where} ${item.assert}`);
          if (item.assert === 'tool_args_not_matching') {
            answerPatterns(item);
            if (item.tool !== undefined) assert.ok(toolNames.has(item.tool), where);
          } else if (item.assert === 'scheduled_tasks') {
            assert.ok(Number.isInteger(item.count) && item.count >= 0, where);
            assert.ok(Array.isArray(item.tasks ?? []), where);
            assert.ok((item.tasks ?? []).length <= item.count, where);
            for (const task of item.tasks ?? []) {
              assert.ok(task && typeof task === 'object' && !Array.isArray(task), where);
              if (task.prompt_includes !== undefined) assert.ok(Array.isArray(task.prompt_includes) && task.prompt_includes.every((text) => typeof text === 'string' && text.length > 0), where);
            }
          } else if (item.assert === 'tool_called' || item.assert === 'tool_not_called') {
            assert.ok(toolNames.has(item.tool), `${where} unknown tool ${item.tool}`);
            if (item.assert === 'tool_called') toolCalledAssertion(item);
            else assert.equal(item.args, undefined, `${where} tool_not_called takes no args`);
          } else if (item.assert === 'tool_called_before') {
            assert.ok(Array.isArray(item.tools) && item.tools.length === 2, where);
            for (const tool of item.tools) assert.ok(toolNames.has(tool), `${where} ${tool}`);
          } else if (item.assert === 'max_tool_calls' || item.assert === 'max_tool_errors') {
            assert.equal(Number.isInteger(item.n) && item.n >= 0, true, where);
          } else if (item.assert === 'capability_loaded') {
            assert.ok(componentIds.has(item.id), `${where} component ${item.id}`);
          } else if (item.assert === 'as_of_left_out' || item.assert === 'as_of_refused') {
            assert.ok(scenario.tags.includes('as-of'), `${where} ${item.assert} belongs to an as-of scenario`);
            if (item.assert === 'as_of_left_out') asOfLeftOutAssertion(item);
            else assert.ok(toolNames.has(asOfRefusedAssertion(item).tool), `${where} unknown tool ${item.tool}`);
          } else if (item.assert === 'calculate_inputs_cited') {
            calculateInputsAssertion(item);
          } else if (item.assert === 'grid_cells_linked') {
            gridCellsAssertion(item);
          } else if (item.assert === 'sensitivity_card') {
            sensitivityCardAssertion(item);
          } else if (item.assert === 'plan_progress') {
            planProgressAssertion(item);
          }
        } else if (bucket === 'memory') {
          assert.ok(MEMORY_ASSERTS.has(item.assert), where);
          const kinds = Array.isArray(item.kind) ? item.kind : [item.kind];
          assert.ok(kinds.length > 0 && kinds.every((kind) => MEMORY_KINDS.has(kind)), `${where} ${item.kind}`);
          if (item.min !== undefined) assert.equal(Number.isInteger(item.min) && item.min >= 1, true, where);
          if (item.title_contains !== undefined) assert.equal(typeof item.title_contains, 'string', where);
        } else if (bucket === 'answer') {
          assert.ok(ANSWER_ASSERTS.has(item.assert), where);
          if (item.assert === 'metrics') {
            assert.equal(typeof item.values, 'object', where);
            const keys = Object.keys(item.values);
            assert.ok(keys.length > 0, where);
            for (const key of keys) {
              assert.equal(Number.isFinite(item.values[key]), true, `${where} ${key}`);
              assert.equal(numberPresent(turn.user, item.values[key]), false, `${where} gold ${key}=${item.values[key]} is already in the user turn`);
            }
            metricTolerance(item);
          } else if (item.assert === 'prediction') {
            assert.equal(item.values, undefined, where);
          } else if (item.assert === 'answer_not_matching') {
            answerPatterns(item);
          } else if (item.assert === 'cites_result') {
            const { tool } = citationAssertion(item);
            assert.ok(toolNames.has(tool), `${where} unknown tool ${tool}`);
          } else if (item.assert === 'numbers_linked') {
            numbersLinkedAssertion(item);
          } else if (item.assert === 'calculation_chip') {
            calculationChipAssertion(item);
          } else {
            assert.ok(Array.isArray(item.values) && item.values.length > 0, where);
            for (const value of item.values) assert.equal(typeof value, 'string', where);
          }
        } else {
          assert.equal(typeof item.id, 'string', where);
          assert.equal(typeof item.criterion, 'string', where);
          assert.ok(item.criterion.length >= 40, `${where} rubric ${item.id} is too vague`);
        }
      }
    }
  }
  const corpus = userCorpus(scenario);
  for (const text of stringsOf(scenario)) {
    for (const match of text.match(/https?:\/\/[^\s"'<>)\]]+/gu) ?? []) {
      const url = new URL(match);
      assert.equal(url.hostname, 'fictional.example', `${scenario.id} ${match}`);
    }
  }
  if (scenario.hidden !== undefined) {
    for (const leaf of leaves(scenario.hidden)) {
      if (leaf.path === 'outcome.metric') continue;
      if (typeof leaf.value === 'number') {
        assert.equal(numberPresent(corpus, leaf.value), false, `${scenario.id} hidden number ${leaf.value} leaked`);
      } else if (typeof leaf.value === 'string' && leaf.value.length > 0) {
        assert.equal(corpus.includes(leaf.value), false, `${scenario.id} hidden string ${leaf.path} leaked`);
      }
    }
  }
}
