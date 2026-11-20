/** Evidence for dataset-backed calculation popovers. */
import { calculationChipAssertion, numberChecks, toolCalls, returnedCitationUrls, isRecord } from './eval-contract.mjs';
import { comparableArgs } from './tool-evidence.mjs';

const result = (pass, detail) => ({ pass, detail });

// Pi delivers JSON either directly, in details, or in a text content block.
function objects(value, out = []) {
  if (isRecord(value)) {
    out.push(value);
    for (const [key, child] of Object.entries(value)) {
      if (key === 'text' && typeof child === 'string') {
        try { objects(JSON.parse(child), out); } catch { /* Ordinary prose. */ }
      } else objects(child, out);
    }
  } else if (Array.isArray(value)) value.forEach(child => objects(child, out));
  return out;
}
const normalized = text => String(text).replace(/[\s,]/gu, '');
const hasFormula = (formulas, expected) => Array.isArray(formulas) && formulas.some(line => normalized(line).includes(normalized(expected)));

export function calculationChipEvidence(assertion, trace, answer) {
  const { operation, inputs, formula, text } = calculationChipAssertion(assertion);
  const calls = toolCalls(trace);
  const checks = numberChecks(trace);
  if (calls === null || checks === null) return result(null, 'calculation or number-check evidence unavailable');
  const links = [...answer.matchAll(/\[([^\]\n]*)\]\(#evidence-(ev_[0-9a-f]{32})\)/gu)];
  let unknown = false;
  for (const [index, call] of calls.entries()) {
    if (call.name !== 'calculate' || call.outcome === 'error') continue;
    if (call.outcome !== 'ok' || comparableArgs(call.args) === null || call.result_redacted || comparableArgs(call.result) === null) { unknown = true; continue; }
    if (call.args.operation !== operation) continue;
    const urls = returnedCitationUrls([call], 'calculate');
    const calculation = objects(call.result).find(value => value.operation === operation && Array.isArray(value.inputs) && Array.isArray(value.formulas));
    if (!calculation || !urls) { unknown = true; continue; }
    if (!hasFormula(calculation.formulas, formula)) continue;
    const inputSources = new Map();
    const pinned = Object.entries(inputs).every(([name, expected]) => {
      const ref = call.args.inputs?.[name];
      const input = calculation.inputs.find(item => item.name === name);
      const origin = input?.provenance;
      const sources = calls.slice(0, index).filter(read => read.outcome === 'ok' && !read.result_redacted
        && objects(read.result).some(receipt => receipt.dataset_id === ref?.dataset_id && receipt.revision === ref?.revision
          && receipt.digest === origin?.digest));
      inputSources.set(name, new Set(sources.flatMap(read => [...(returnedCitationUrls([read], read.name) ?? [])])));
      const resolved = isRecord(ref) && typeof ref.dataset_id === 'string' && ref.dataset_id.length > 0
        && Number.isInteger(ref.revision) && ref.revision > 0 && typeof ref.pointer === 'string' && ref.pointer.startsWith('/')
        && input?.value === expected && origin?.kind === 'dataset' && typeof origin.digest === 'string' && origin.digest.length > 0
        && ['dataset_id', 'revision', 'pointer'].every(key => origin[key] === ref[key]);
      if (resolved && !sources.length && calls.slice(0, index).some(read => read.outcome === 'ok'
        && (read.result_redacted || comparableArgs(read.result) === null))) unknown = true;
      return resolved && sources.length > 0;
    });
    if (!pinned) continue;
    const linked = checks.some(check => check.kind === 'calculated' && ['matched', 'rounded'].includes(check.status)
      && normalized(check.text) === normalized(text) && urls.has(`#evidence-${check.evidence_id}`)
      && links[check.link_index]?.[2] === check.evidence_id && normalized(links[check.link_index]?.[1]) === normalized(text)
      && hasFormula(check.calc?.formulas, formula)
      && Object.entries(inputs).every(([name, expected]) => check.calc?.inputs?.some(input => input.name === name
        && Number(normalized(input.display)) === expected && inputSources.get(name).has(`#evidence-${input.evidence_id}`)
        && !['not_found', 'differs'].includes(input.status))));
    if (linked) return result(true, `${text} links to ${operation} with the expected formula and pinned inputs`);
  }
  return result(unknown ? null : false, unknown ? 'calculation delivery unavailable' : 'no checked calculation chip with the expected formula and dataset inputs');
}
