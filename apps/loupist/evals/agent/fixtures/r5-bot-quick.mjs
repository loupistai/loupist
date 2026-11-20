// Matched R5 fixture. Preserve the existing five Bot prompts, budgets and checks.
// Run with Node to write the augmented suite to stdout for the normal eval CLI.
import { readFile } from 'node:fs/promises';
import { pathToFileURL } from 'node:url';

export async function botQuickLanguageSuite() {
  const suite = JSON.parse(await readFile(new URL('../suites/bot-quick-probe-v1.json', import.meta.url), 'utf8'));
  const turns = suite.scenarios.find(scenario => scenario.id === 'BQ-PROBE-001').sessions[0].turns;
  const languages = ['ko', 'ko', 'ko', 'en', 'ko'];
  if (turns.length !== languages.length) throw new Error('The R5 fixture requires the five-turn Bot probe');
  for (const [index, turn] of turns.entries()) {
    turn.expect.trace.push({ assert: 'tool_not_called', tool: 'interface_labels', metric: 'efficiency' });
    turn.expect.answer.push({ assert: 'answer_language', language: languages[index], metric: 'answer_language' });
  }
  return suite;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  console.log(JSON.stringify(await botQuickLanguageSuite(), null, 2));
}
