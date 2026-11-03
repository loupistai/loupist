import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { grade } from './grade.mjs';
import { assertScenarioContract, userCorpus } from './suite-contract.mjs';

const suite = JSON.parse(await readFile(new URL('./suites/citations-v1.json', import.meta.url), 'utf8'));

test('a correct note answer and a saved Source do not substitute for an inline citation', () => {
  for (const scenario of suite.scenarios) {
    const session = scenario.sessions[0];
    const citation = '#evidence-ev_' + 'a'.repeat(32);
    const record = {
      schema: 'loupist-agent-trace/1', run_id: 'note-fixture', suite: suite.suite,
      scenario_id: scenario.id, session_id: session.id, turn_index: 0, trial: 0,
      status: 'completed', session_role: session.role, memory_mode: session.memory,
      user: session.turns[0].user,
      tool_calls: [{ name: 'workspace_capture', outcome: 'ok', result_redacted: false,
        result: { content: [{ type: 'text', text: JSON.stringify({ citation_url: citation, content: 'The token is 18420.' }) }] } }],
      memory_commits: [{ kind: 'source', title: 'Northwind Loom note' }],
    };
    for (const [answer, expected] of [
      ['Northwind Loom recorded token **18420** on 12 March 2026.', false],
      ['Northwind Loom recorded token [18420](#evidence-invented).', false],
      [`Northwind Loom recorded token **18420**. ${citation}`, false],
      [`Northwind Loom recorded token \`[18420](${citation})\`.`, false],
      [`\`\`\`markdown\nNorthwind Loom recorded token [18420](${citation}).\n\`\`\``, false],
      [`    Northwind Loom recorded token [18420](${citation}).`, false],
      [`Northwind Loom recorded token ![18420](${citation}).`, false],
      [`Northwind Loom recorded token ![18420][note].\n\n[note]: ${citation}`, false],
      [`Northwind Loom recorded token \\[18420](${citation}).`, false],
      [`Northwind Loom recorded token [18420](${citation}extra).`, false],
      [`Northwind Loom recorded token [18420](${citation}a).`, false],
      [`Northwind Loom recorded token [18420](${citation}?value=1).`, false],
      [`Northwind Loom recorded token [18420](https://example.com/${citation}).`, false],
      [`Northwind Loom recorded token [18420](https://example.com/ "${citation}").`, false],
      [`Northwind Loom recorded token 18420.\n\nSource: [note](${citation}).`, false],
      [`[Note][source]\n\n[source]: ${citation} "18420"`, false],
      [`Northwind Loom recorded token [18420][note].\n\n[note]: #evidence-ev_${'b'.repeat(32)}\n[note]: ${citation}`, false],
      [`Northwind Loom recorded token [18420](${citation}) on 12 March 2026.`, true],
      [`Northwind Loom recorded token **[18420](${citation})**.`, true],
      [`Northwind Loom recorded token [18420](<${citation}>).`, true],
      [`Northwind Loom recorded token [18420][Note].\n\n[note]: ${citation}`, true],
      [`Northwind Loom recorded token [18420][].\n\n[18420]: ${citation}`, true],
      [`Northwind Loom recorded token [18420].\n\n[18420]: ${citation}`, true],
      [`Northwind Loom recorded token [18420][note].\n\n[note]: ${citation}\n[note]: #evidence-ev_${'b'.repeat(32)}`, true],
    ]) {
      const graded = grade(suite, [{ ...record, answer }]).results;
      assert.equal(graded.find(row => row.assert === 'cites_result').pass, expected, `${scenario.id}: ${answer}`);
      assert.equal(graded.find(row => row.assert === 'committed').pass, true);
    }
  }
});

test('attachment fixtures participate in the scenario corpus and reject non-text intake', () => {
  const scenario = suite.scenarios.find(item => item.tags.includes('attachment'));
  assertScenarioContract(scenario);
  assert.match(userCorpus(scenario), /loom-yield token of 18420/u);
  for (const change of [
    { filename: '../note.md' },
    { filename: 'folder\\note.md' },
    { media_type: 'image/png' },
    { text: 18420 },
  ]) {
    const invalid = structuredClone(scenario);
    Object.assign(invalid.sessions[0].turns[0].attachments[0], change);
    assert.throws(() => assertScenarioContract(invalid));
  }
});
