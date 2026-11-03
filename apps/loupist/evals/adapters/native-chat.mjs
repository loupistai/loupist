/** Opt-in real Tauri Chat adapter. Uses shipped UI, never bypasses the broker. */
import { readFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { pathToFileURL } from 'node:url';
import { createWebdriverHelpers } from '../../e2e/webdriver-helpers.mjs';
import { createWorkspaceNavigation } from '../../e2e/workspace-navigation.mjs';
import { collectWorkProgress } from '../../e2e/work-progress.mjs';

export async function open(config) {
  if (!config.session_path || !config.model_label || !config.effort || !config.conditions) throw new Error('Native config needs session_path, model_label, effort, conditions');
  if (!Number.isInteger(config.budget?.timeout_ms) || config.budget.timeout_ms < 1 || config.budget.max_output_tokens != null) throw new Error('Native budget needs timeout_ms and max_output_tokens:null; UI does not enforce a token cap');
  if (config.timeout_ms != null && config.timeout_ms !== config.budget.timeout_ms) throw new Error('timeout_ms must match the comparison budget');
  const receipt = JSON.parse(await readFile(config.session_path, 'utf8'));
  if (receipt.webdriverConfig?.hostname !== '127.0.0.1' || receipt.webdriverConfig?.protocol !== 'http' || receipt.capabilities?.browserName !== 'tauri' || receipt.profile === 'manual') throw new Error('Expected the launcher-owned local Tauri WebDriver session');
  const require = createRequire(new URL('../../e2e/package.json', import.meta.url));
  const { remote } = await import(pathToFileURL(require.resolve('webdriverio')).href);
  const browser = await remote({ ...receipt.webdriverConfig, capabilities: receipt.capabilities, logLevel: 'error' });
  const helpers = createWebdriverHelpers(browser, { defaultTimeout: 20_000 });
  const nav = createWorkspaceNavigation(browser, helpers);
  return {
    async run(request) {
      if (request.arm !== 'app') throw new Error('Native adapter only runs app arm');
      const condition = config.conditions[request.condition];
      if (!condition?.loupist_name?.startsWith('Finance Eval ')) throw new Error('Use a dedicated Finance Eval Loupist for each condition');
      if (request.condition !== 'memory_off' && !condition.evidence?.verified_at) throw new Error('Memory arm needs an independently verified condition manifest');
      if (request.condition === 'learned_lens' && !condition.evidence?.learned_lens?.authoring_run_id) throw new Error('Learned Lens must come from a recorded disjoint training run');
      await nav.chooseLoupist(condition.loupist_name);
      await nav.openMainView('chat');
      await helpers.clickButton('New chat');
      await browser.waitUntil(async () => (await browser.$$('article.message')).length === 0, { timeout: 15_000 });
      await nav.selectMemoryMode(request.condition === 'memory_off' ? 'off' : 'read');
      const selected = await nav.readMemoryMode();
      if (selected !== (request.condition === 'memory_off' ? 'Memory off' : 'Use saved context only')) throw new Error('Memory mode did not match evaluation condition');
      const modelButton = await helpers.displayed('[aria-label="Model settings for this message"]');
      const selection = await modelButton.getAttribute('aria-description');
      const parts = String(selection).split(' · ').map((part) => part.trim());
      if (!parts.includes(config.model_label) || !parts.includes(config.effort)) throw new Error('Select and verify the configured model/effort before a paid evaluation');
      await (await helpers.displayed('textarea[aria-label="Message Loupist"]')).setValue(request.prompt);
      const turnStarted = performance.now();
      await (await helpers.displayed('button[aria-label="Send"]')).click();
      let article;
      await browser.waitUntil(async () => {
        const candidates = await browser.$$('article.message.assistant.complete, article.message.assistant.error, article.message.assistant.aborted');
        article = candidates.at(-1); return article != null;
      }, { timeout: config.budget.timeout_ms, interval: 500 });
      const classes = await article.getAttribute('class');
      const content = await article.$('.message-content');
      let answer = await content.isExisting() ? await content.getText() : await article.getText();
      for (const code of await article.$$('pre code')) answer += `\n\`\`\`json\n${await code.getText()}\n\`\`\``;
      const details = await article.$('[aria-label="Response details"]');
      const description = await details.isExisting() ? await details.getAttribute('aria-description') : '';
      const input = String(description).match(/(\d+) input tokens/u);
      const output = String(description).match(/(\d+) output tokens/u);
      const progress = await collectWorkProgress(browser, { latestOnly: true });
      const observedModel = String(description).split(' · ')[1]?.trim() || null;
      return { status: classes.includes('complete') && observedModel ? 'completed' : classes.includes('aborted') ? 'cancelled' : 'failed', answer,
        ...(classes.includes('complete') && !observedModel ? { error: 'Native response did not expose the actual execution model identity.' } : {}),
        execution: progress.flatMap((section) => section.rows), condition_evidence: condition.evidence ?? { verified_at: new Date().toISOString(), memory: 'off' },
        observed_model: observedModel, observed_model_label: selection, performance_description: description,
        duration_ms: Math.round(performance.now() - turnStarted),
        usage: { input_tokens: input ? Number(input[1]) : null, output_tokens: output ? Number(output[1]) : null, input_includes_cache: true },
        cost_usd: null, cost_basis: 'not_exposed_by_ui' };
    },
    async close() { await browser.deleteSession(); },
  };
}
