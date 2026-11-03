/** Explicit opt-in same-model baseline. No credentials are written to traces. */
export async function open(config) {
  if (!config.endpoint || !config.api_key_env || !config.model) throw new Error('Provider config needs endpoint, api_key_env, and model');
  if (!config.effort || !Number.isInteger(config.budget?.timeout_ms) || config.budget.timeout_ms < 1) throw new Error('Provider config needs effort and a measured timeout budget');
  if (config.timeout_ms != null && config.timeout_ms !== config.budget.timeout_ms) throw new Error('timeout_ms must match the comparison budget');
  if (config.data_access?.fixture !== 'supplied' || config.data_access?.external !== 'disabled') throw new Error('Bare provider data access is supplied fixtures with external access disabled');
  const options = config.request_options ?? {};
  if (options.reasoning_effort != null && options.reasoning_effort !== config.effort) throw new Error('Request effort must match the comparison effort');
  const outputBudget = config.budget.max_output_tokens;
  if (outputBudget != null && (!Number.isInteger(outputBudget) || outputBudget < 1)) throw new Error('Invalid output token budget');
  if (options.max_tokens != null || (options.max_completion_tokens != null && options.max_completion_tokens !== outputBudget)) throw new Error('Use budget.max_output_tokens for the compared output limit');
  const url = new URL(config.endpoint);
  if (url.protocol !== 'https:' && !['127.0.0.1', 'localhost'].includes(url.hostname)) throw new Error('Provider endpoint must use HTTPS');
  const apiKey = process.env[config.api_key_env];
  if (!apiKey) throw new Error('Configured API credential is unavailable');
  return {
    async run(request) {
      if (request.arm !== 'same_model' || request.condition !== 'memory_off') throw new Error('Bare-model adapter only runs same_model/memory_off');
      const response = await fetch(url, { method: 'POST', signal: AbortSignal.timeout(config.budget.timeout_ms),
        headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${apiKey}` },
        body: JSON.stringify({ ...options, model: config.model, reasoning_effort: config.effort,
          ...(outputBudget != null ? { max_completion_tokens: outputBudget } : {}), messages: [{ role: 'user', content: request.prompt }] }),
      });
      if (!response.ok) { const error = new Error('Provider request rejected'); error.safeMessage = `Provider HTTP ${response.status}`; throw error; }
      const result = await response.json();
      const answer = result.choices?.[0]?.message?.content;
      if (typeof answer !== 'string') throw new Error('Provider did not return text');
      const input = result.usage?.prompt_tokens;
      const output = result.usage?.completion_tokens;
      const cached = result.usage?.prompt_tokens_details?.cached_tokens;
      let cost = null;
      if (config.pricing?.as_of && Number.isFinite(input) && Number.isFinite(output)) {
        const rates = config.pricing;
        if ([rates.input_per_million, rates.output_per_million].every((rate) => Number.isFinite(rate) && rate >= 0)) {
          if (cached === 0 || rates.cached_input_per_million === rates.input_per_million) cost = (input * rates.input_per_million + output * rates.output_per_million) / 1_000_000;
          else if (Number.isFinite(cached) && cached >= 0 && cached <= input && Number.isFinite(rates.cached_input_per_million) && rates.cached_input_per_million >= 0) cost = ((input - cached) * rates.input_per_million + cached * rates.cached_input_per_million + output * rates.output_per_million) / 1_000_000;
        }
      }
      const observedModel = typeof result.model === 'string' && result.model.trim() ? result.model : null;
      return { status: observedModel ? 'completed' : 'failed', answer, execution: [], provider_response_id: result.id ?? null,
        ...(observedModel ? {} : { error: 'Provider did not expose the actual execution model identity.' }),
        observed_model: observedModel, usage: { input_tokens: input ?? null, output_tokens: output ?? null, cached_input_tokens: cached ?? null },
        cost_usd: cost, cost_basis: cost == null ? 'unreported' : 'configured_rates', pricing_as_of: config.pricing?.as_of ?? null };
    },
  };
}
