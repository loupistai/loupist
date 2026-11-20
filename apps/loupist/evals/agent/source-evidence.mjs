/** Grade the search → exact Source read → cited figure within one trace row. */
import { createHash } from 'node:crypto';
import { hostResult } from './tool-evidence.mjs';
import { citesFigure, isRecord, memoryReads, sourceSpanAssertion, toolCalls } from './eval-contract.mjs';

function spanKey(ref) {
  if (ref?.scope !== 'source') return null;
  const ids = [ref.recordId, ref.revisionId, ref.spanId];
  return ids.every(id => typeof id === 'string' && id.length > 0) ? JSON.stringify(ids) : null;
}

export function sourceSpanEvidence(assertion, trace, answer, scenario) {
  const { source, figure, allowPlainCount } = sourceSpanAssertion(assertion, scenario);
  const reads = memoryReads(trace);
  const calls = toolCalls(trace);
  if (!reads || !calls) return { pass: null, detail: 'Source read or tool call evidence unavailable' };
  const records = new Set(reads.filter(read => read.access === 'exact_read' && read.kind === 'source'
    && read.title === source.title).map(read => read.record_id));
  const digest = createHash('sha256').update(source.text).digest('hex');
  const discovered = new Set();
  let unavailable = false;
  for (const call of calls) {
    if (!['world_search', 'world_read'].includes(call.name) || call.outcome === 'error') continue;
    const result = hostResult(call.result);
    if (call.outcome !== 'ok' || call.result_redacted || !result) {
      unavailable = true;
      continue;
    }
    if (call.name === 'world_search') {
      if (!Array.isArray(result.hits)) {
        unavailable = true;
        continue;
      }
      for (const hit of result.hits) {
        const key = spanKey(hit?.readArgs);
        if (key && spanKey(hit) === key) discovered.add(key);
      }
      continue;
    }
    const key = spanKey(call.args);
    if (!discovered.has(key) || !records.has(result.recordId)
      || key !== spanKey({ ...result, spanId: result.source?.spanId })
      || result.source.selectedDigest !== digest
      || result.source.attribution !== `${source.title} — ${source.origin} (eval-fixture)`) continue;
    if (/^#evidence-ev_[0-9a-f]{32}$/u.test(result.citation_url)
      && citesFigure(answer, result.citation_url, figure, allowPlainCount)) {
      return { pass: true, detail: `cited figure from ${source.title} (${key})` };
    }
  }
  return unavailable ? { pass: null, detail: 'Source search or read delivery unavailable' }
    : { pass: false, detail: `no figure cited to a discovered and exact-read span of ${source.title}` };
}
