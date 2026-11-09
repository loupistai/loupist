/** Finance app arm through production Rust Chat; every attempt owns a fresh Loupist. */
import { mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { spawn } from 'node:child_process';
import { isAbsolute, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const tauri = fileURLToPath(new URL('../../src-tauri/', import.meta.url));
const output = fileURLToPath(new URL('../output/', import.meta.url));
export function validateConfig(config) {
  if (!/^[^/\s]+\/[^\s]+$/u.test(config.model ?? '') || typeof config.effort !== 'string' || !config.effort.trim()) throw new Error('Headless config requires provider/model and effort (Pi thinking level)');
  if (!isAbsolute(config.pi_agent_dir ?? '')) throw new Error('Headless config requires absolute pi_agent_dir; credentials are used in place');
  if (!Number.isSafeInteger(config.budget?.timeout_ms) || config.budget.timeout_ms < 1 || config.budget.max_output_tokens !== null || Object.keys(config.budget).some((key) => !['timeout_ms', 'max_output_tokens'].includes(key))) throw new Error('Headless budget requires timeout_ms and max_output_tokens:null; other budgets cannot be enforced');
  if (config.data_access?.fixture !== 'supplied' || config.data_access?.external !== 'runtime_tools' || Object.keys(config.data_access).some((key) => !['fixture', 'external'].includes(key))) throw new Error('Headless data_access must be {fixture:supplied,external:runtime_tools}; native workspace/run_python/web tools retain production permissions');
  if (config.timeout_ms != null && config.timeout_ms !== config.budget.timeout_ms) throw new Error('timeout_ms must match the comparison budget');
  if (config.thinking != null && config.thinking !== config.effort) throw new Error('thinking must match effort');
  if (config.request_options != null || config.connections != null) throw new Error('Headless does not support request_options or connection overrides');
}
export function translateRequest(request) {
  if (request.arm !== 'app' || request.condition !== 'memory_off') throw new Error('Headless adapter only runs app/memory_off');
  if (!request.run_id || !request.task_id || typeof request.prompt !== 'string' || !request.prompt.trim()) throw new Error('Missing finance request identity or prompt');
  // taskRequest already includes the supplied facts and revisit evidence, without gold answers.
  return { schema: 'loupist-agent-suite/1', suite: 'finance', version: 1, scenarios: [{
    id: request.run_id, title: `Finance Eval ${request.task_id}`, tags: ['finance', 'held-out'], connections: [],
    sessions: [{ id: 'attempt', role: 'probe', memory: 'off', turns: [{ user: request.prompt }] }],
  }] };
}
export function mapResult(record, traceRef) {
  const observed = typeof record.observed_model === 'string' && record.observed_model.trim() ? record.observed_model : null;
  return {
    status: record.status === 'completed' && observed ? 'completed' : record.status === 'aborted' ? 'cancelled' : 'failed',
    answer: typeof record.answer === 'string' ? record.answer : '', observed_model: observed,
    error: record.error ?? (record.status === 'completed' && !observed ? 'Headless trace lacks observed execution model identity.' : null),
    execution: record.tool_calls ?? [], trace_ref: traceRef, harness: record.harness ?? null,
    usage: { input_tokens: record.usage?.input ?? null, output_tokens: record.usage?.output ?? null,
      cached_input_tokens: record.usage?.cache_read ?? null, cache_write_tokens: record.usage?.cache_write ?? null, input_includes_cache: false },
    duration_ms: record.duration_ms ?? null, performance: record.performance ?? null,
    cost_usd: null, cost_basis: 'unreported',
    condition_evidence: { verified_at: new Date().toISOString(), memory: 'off', isolation: 'fresh_temporary_loupist_per_attempt' },
  };
}
function execute(args) {
  return new Promise((resolve, reject) => {
    const child = spawn('cargo', ['run', '--locked', '--offline', '--features', 'eval-harness', '--example', 'agent_eval', '--', ...args], {
      cwd: tauri, env: process.env, stdio: ['ignore', 'ignore', 'inherit'],
    });
    child.once('error', reject);
    child.once('exit', (code, signal) => resolve({ code, signal }));
  });
}
export async function open(config) {
  validateConfig(config);
  if (!process.env.CARGO_TARGET_DIR || !isAbsolute(process.env.CARGO_TARGET_DIR)) throw new Error('Set an absolute CARGO_TARGET_DIR to the shared existing Rust target before running');
  return {
    async run(request) {
      const suite = translateRequest(request);
      await mkdir(output, { recursive: true });
      // Retain the exact inputs and runner trace for audit; only the Loupist itself is ephemeral.
      const directory = await mkdtemp(join(output, 'finance-headless-'));
      const suitePath = join(directory, 'suite.json');
      const tracePath = join(directory, 'trace.jsonl');
      await writeFile(suitePath, `${JSON.stringify(suite, null, 2)}\n`, { mode: 0o600 });
      const outcome = await execute(['--suite', suitePath, '--out', tracePath, '--model', config.model,
        '--thinking', config.effort, '--turn-timeout-ms', String(config.budget.timeout_ms), '--pi-agent-dir', config.pi_agent_dir]);
      let records;
      try { records = (await readFile(tracePath, 'utf8')).split('\n').filter((line) => line.trim()).map((line) => JSON.parse(line)); }
      catch { return { status: 'failed', answer: '', error: 'Headless runner produced no readable trace; inspect stderr.', observed_model: null, usage: null, cost_usd: null, trace_ref: tracePath }; }
      if (records.length !== 1 || records[0].schema !== 'loupist-agent-trace/1' || records[0].scenario_id !== request.run_id || records[0].memory_mode !== 'off') throw new Error('Unexpected headless attempt trace');
      const response = mapResult(records[0], tracePath);
      if (outcome.code !== 0 && response.status === 'completed') return { ...response, status: 'failed', error: 'Headless process did not exit successfully; trace retained.' };
      return response;
    },
  };
}
