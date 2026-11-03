/** Synthetic fictional suite and traces for agent-eval tests. */
export const HIDDEN_SENTINEL = 'HIDDEN_SENTINEL_z9';

const model = { provider: 'openai-codex', model: 'fixture-model', thinking: 'low' };
const harness = { git_commit: 'abc', system_prompt_sha256: 'prompt', tool_catalog_sha256: 'catalog', scenario_input_sha256: 'a'.repeat(64) };

export const suite = {
  schema: 'loupist-agent-suite/1',
  suite: 'learning',
  version: 1,
  description: 'Synthetic grader fixture. All figures are fictional.',
  scenarios: [{
    id: 'L-001',
    title: 'Diluted shares',
    tags: ['lens', 'correction'],
    split: 'capability',
    connections: ['opendart'],
    hidden: { outcome: { direction: 'up', sentinel: HIDDEN_SENTINEL } },
    sessions: [
      {
        id: 'teach-1', role: 'train', memory: 'remember',
        turns: [{
          user: 'Hanbit Robotics is fictional. Its diluted shares are 120 million. Remember that for later valuation.',
          expect: {
            trace: [
              { assert: 'tool_called', tool: 'lens_update', metric: 'save_recall' },
              { assert: 'no_repeated_failed_call', metric: 'tool_validity' },
            ],
            memory: [{ assert: 'committed', kind: 'lens', min: 1, title_contains: 'Diluted', metric: 'save_recall' }],
            answer: [{ assert: 'contains_any', values: ['120 million'], metric: 'task_success' }],
            rubric: [{ id: 'states-diluted-shares', criterion: 'The answer states that diluted shares are 120 million.', metric: 'save_recall' }],
          },
        }],
      },
      {
        id: 'probe-1', role: 'probe', memory: 'read_only',
        turns: [{
          user: 'What is a fair value range for fictional Hanbit Robotics?',
          expect: {
            trace: [
              { assert: 'tool_called_before', tools: ['lens_search', 'run_valuation'], metric: 'carry_forward' },
              { assert: 'capability_loaded', id: 'valuation', metric: 'tool_validity' },
              { assert: 'tool_not_called', tool: 'web_search', metric: 'connection_honesty' },
              { assert: 'max_tool_errors', n: 1, metric: 'tool_validity' },
              { assert: 'no_repeated_failed_call', metric: 'tool_validity' },
            ],
            memory: [{ assert: 'not_committed', kind: 'lens', metric: 'negative_transfer' }],
            answer: [
              { assert: 'metrics', values: { fair_value_low: 41.2 }, metric: 'task_success' },
              { assert: 'not_contains', values: ['guaranteed return'], metric: 'task_success' },
            ],
            rubric: [{ id: 'applies-diluted-shares', criterion: 'The valuation uses 120 million diluted shares from the earlier session.', metric: 'carry_forward' }],
          },
        }],
      },
    ],
  }],
};

function trace(overrides) {
  return {
    schema: 'loupist-agent-trace/1', run_id: 'run', suite: 'learning', scenario_id: 'L-001',
    session_id: 'probe-1', session_role: 'probe', turn_index: 0, trial: 0,
    started_at: '2026-10-18T00:00:00.000Z', duration_ms: 1000, model, harness, memory_mode: 'read_only',
    user: suite.scenarios[0].sessions[1].turns[0].user, answer: '', status: 'completed', error: null,
    tool_calls: [], memory_reads: [], memory_commits: [],
    usage: { input: 100, output: 40, cache_read: 0, cache_write: 0 },
    performance: { setup_ms: 1, first_text_ms: 2, total_ms: 1000 },
    ...overrides,
  };
}

const prediction = (direction, probability, metrics) => `Hanbit Robotics is fictional.\n${JSON.stringify({ metrics, prediction: { direction, probability } })}`;

export const traces = [
  trace({
    run_id: 'train-0', session_id: 'teach-1', session_role: 'train', trial: 0, memory_mode: 'remember', duration_ms: 500,
    user: suite.scenarios[0].sessions[0].turns[0].user,
    answer: 'Recorded diluted shares of 120 million for fictional Hanbit Robotics.',
    tool_calls: [
      { call_id: 'c1', name: 'lens_update', component: 'lens', args: { title: 'Diluted shares' }, duration_ms: 10, outcome: 'ok', error_code: null, error_message: null, result_bytes: 20 },
      { call_id: 'c2', name: 'missing_tool', component: null, args: {}, duration_ms: 5, outcome: 'error', error_code: 'tool_not_found', error_message: 'Tool "missing_tool" not found', result_bytes: 0 },
    ],
    memory_commits: [{ kind: 'lens', record_id: 'lens-1', title: 'Diluted share count', summary: 'Use 120 million diluted shares when valuing fictional Hanbit Robotics.' }],
    usage: { input: 20, output: 10, cache_read: 0, cache_write: 0 },
    performance: { setup_ms: 1, first_text_ms: 2, total_ms: 500 },
  }),
  trace({
    run_id: 'probe-0', trial: 0, duration_ms: 1000,
    answer: prediction('up', 0.8, { fair_value_low: 41.2 }),
    tool_calls: [
      { call_id: 'p0load', name: 'capability_load', component: null, args: { id: 'valuation' }, duration_ms: 2, outcome: 'ok', error_code: null, error_message: null, result_bytes: 10 },
      { call_id: 'p0a', name: 'lens_search', component: 'lens', args: { query: 'diluted' }, duration_ms: 8, outcome: 'ok', error_code: null, error_message: null, result_bytes: 40 },
      { call_id: 'p0b', name: 'run_valuation', component: 'valuation', args: { shares: 120000000 }, duration_ms: 20, outcome: 'ok', error_code: null, error_message: null, result_bytes: 30 },
    ],
    memory_reads: [{ kind: 'lens', record_id: 'lens-1', title: 'Diluted share count', access: 'exact_read' }],
    usage: { input: 100, output: 40, cache_read: 1, cache_write: 2 },
  }),
  trace({
    run_id: 'probe-1', trial: 1, duration_ms: 3000,
    answer: prediction('up', 0.9, { fair_value_low: 41.2002 }),
    tool_calls: [
      { call_id: 'p1load', name: 'capability_load', component: null, args: { id: 'valuation' }, duration_ms: 2, outcome: 'ok', error_code: null, error_message: null, result_bytes: 10 },
      { call_id: 'p1a', name: 'lens_search', component: 'lens', args: { query: 'diluted' }, duration_ms: 8, outcome: 'ok', error_code: null, error_message: null, result_bytes: 40 },
      { call_id: 'p1b', name: 'run_valuation', component: 'valuation', args: { shares: 120000000 }, duration_ms: 20, outcome: 'ok', error_code: null, error_message: null, result_bytes: 30 },
    ],
  }),
  trace({
    run_id: 'probe-2', trial: 2, duration_ms: null, usage: null,
    answer: `A guaranteed return.\n${JSON.stringify({ metrics: { fair_value_low: 10 }, prediction: { direction: 'down', probability: 0.25 } })}`,
    tool_calls: [
      { call_id: 'p2a', name: 'run_valuation', component: 'valuation', args: { ticker: 'HNRB' }, duration_ms: 4, outcome: 'error', error_code: 'malformed_request', error_message: 'Validation failed for tool "run_valuation": ticker', result_bytes: 0 },
      { call_id: 'p2b', name: 'run_valuation', component: 'valuation', args: { ticker: 'HNRB' }, duration_ms: 4, outcome: 'error', error_code: 'tool_validation_failed', error_message: 'Validation failed for tool "run_valuation": ticker', result_bytes: 0 },
      { call_id: 'p2c', name: 'lens_search', component: 'lens', args: { query: 'diluted' }, duration_ms: 8, outcome: 'ok', error_code: null, error_message: null, result_bytes: 10 },
      { call_id: 'p2d', name: 'price_lookup', component: 'prices', args: {}, duration_ms: 3, outcome: 'error', error_code: 'rate_limit', error_message: 'provider rate limit', result_bytes: 0 },
    ],
    performance: { setup_ms: null, first_text_ms: null, total_ms: null },
  }),
];

export function withModel(row, modelId, extra = {}) {
  return { ...row, run_id: `${row.run_id}-${modelId}`, model: { ...row.model, model: modelId }, ...extra };
}
