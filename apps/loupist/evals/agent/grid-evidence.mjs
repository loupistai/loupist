/** Final grid publications, as observed in the turn's Pi trace. */
import { gridCellsAssertion, isRecord } from './eval-contract.mjs';
import { comparableArgs, hostResult } from './tool-evidence.mjs';

const LINKED_CELL = new Set(['matched', 'rounded']);
const LAYOUTS = new Set(['assets', 'conflict']);
const knownId = (value) => typeof value === 'string' && value && comparableArgs(value) !== null;

export function finalGrids(calls) {
  const grids = new Map();
  for (const call of calls) {
    if (call.name !== 'grid_publish' || call.outcome === 'error') continue;
    const result = hostResult(call.result);
    const { edit_token: _redacted, ...args } = isRecord(call.args) ? call.args : {};
    const id = result?.artifact?.artifact_id;
    const requestedId = args.mode === 'revise' ? args.artifact_id : undefined;
    const key = knownId(id) ? id : knownId(requestedId) ? requestedId : Symbol();
    const requestedLayout = args.layout ?? (isRecord(call.args) && comparableArgs(args) !== null ? 'assets' : null);
    const layout = LAYOUTS.has(requestedLayout) && (result?.layout === undefined || result.layout === requestedLayout)
      ? requestedLayout : null;
    const staged = call.outcome === 'ok' && result?.status === 'staged' && result.layout === layout
      && result.truncated !== true;
    grids.set(key, { args, layout, staged, result });
  }
  return [...grids.values()];
}

export function gridCellsEvidence(assertion, calls) {
  const { layout, rows, columns, empty, when } = gridCellsAssertion(assertion);
  const publications = finalGrids(calls);
  const relevant = publications.filter(grid => grid.layout === layout);
  let unknown = publications.some(grid => grid.layout === null);
  const problems = [];
  let cellCount = 0;
  for (const { args, staged, result } of relevant) {
    const checks = result?.cell_checks;
    if (!staged || !Array.isArray(checks) || comparableArgs(checks) === null || comparableArgs(args) === null) {
      unknown = true;
      continue;
    }
    const issues = [];
    if (!checks.length) issues.push('no checked cells');
    const flagged = checks.filter((check) => !LINKED_CELL.has(check?.status));
    if (flagged.length) issues.push(`cells ${flagged.slice(0, 5).map((check) => `${check?.cell ?? '?'} ${check?.status ?? 'unknown'}`).join(', ')}`);
    if (layout === 'assets') {
      const rowCount = Array.isArray(args?.rows) ? args.rows.length : null;
      const columnCount = Array.isArray(args?.columns) ? args.columns.length : null;
      if (rows !== null && rowCount !== rows) issues.push(`${rowCount ?? 'unknown'} rows, expected ${rows}`);
      if (columns !== null && columnCount !== columns) issues.push(`${columnCount ?? 'unknown'} columns, expected ${columns}`);
      const reasons = new Set((Array.isArray(args?.cells) ? args.cells : []).flat()
        .flatMap((cell) => typeof cell?.empty === 'string' ? [cell.empty] : []));
      const missing = empty.filter((reason) => !reasons.has(reason));
      if (missing.length) issues.push(`no empty cell marked ${missing.join(', ')}`);
    }
    cellCount += checks.length;
    problems.push(...issues);
  }
  if (problems.length) return { pass: false, detail: problems.join('; ') };
  if (unknown) return { pass: null, detail: 'grid_publish checks unavailable' };
  if (relevant.length) return { pass: true, detail: `${cellCount} ${layout} cells linked` };
  return when === 'published'
    ? { pass: true, detail: `no ${layout} grid publication` }
    : { pass: false, detail: `no ${layout} grid_publish call` };
}
