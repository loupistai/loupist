import { readFile, writeFile } from 'node:fs/promises';
import { buildFeedbackEvalCase, portableFeedbackText } from '../../src/chat/answerFeedbackCore.ts';
import { assertScenarioContract } from './suite-contract.mjs';

// Explicit developer input only. Never scan a Loupist folder or export answer/Memory bodies.
const [inputPath, outputPath, ...extra] = process.argv.slice(2);
if (!inputPath || !outputPath || extra.length) {
  console.error('Usage: node evals/agent/export-feedback.mjs input.json output.json');
  process.exitCode = 1;
} else {
  try {
    const input = JSON.parse(await readFile(inputPath, 'utf8'));
    if (!['messageId', 'request', 'correction'].every(key => typeof input?.[key] === 'string' && input[key].trim())) {
      throw new Error('invalid input');
    }
    const scenario = buildFeedbackEvalCase(input.messageId, input.request, input.correction);
    if (input.principle !== undefined) {
      if (typeof input.principle !== 'string' || !input.principle.trim()) throw new Error('invalid principle');
      const principle = portableFeedbackText(input.principle);
      if (!principle || principle.length > 280) throw new Error('invalid principle');
      scenario.title = 'User principle carry-forward candidate';
      scenario.tags.push('principle', 'carry-forward');
      scenario.sessions.unshift({ id: 'teach-principle', role: 'train', memory: 'remember', turns: [{
        user: `Remember this research principle: ${principle}`,
        expect: { memory: [{ assert: 'committed', kind: 'principle', count: 1, metric: 'save_recall' }] },
      }] });
      scenario.sessions[1].memory = 'read_only';
      scenario.sessions[1].turns[0].expect.rubric[0].metric = 'carry_forward';
      scenario.sessions[1].turns[0].expect.rubric[0].criterion += ` Apply the user-stated principle where relevant: ${principle}`;
    }
    assertScenarioContract(scenario, { suite: 'feedback', label: 'feedback export' });
    await writeFile(outputPath, JSON.stringify(scenario, null, 2) + '\n', { flag: 'wx', mode: 0o600 });
    console.log('Exported feedback scenario. Review it before sharing.');
  } catch {
    console.error('Could not export feedback. Check the input fields and use a new output path.');
    process.exitCode = 1;
  }
}
