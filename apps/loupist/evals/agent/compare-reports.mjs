/** Summary comparisons are descriptive; they do not assert trace-level pairing. */
export function compareReports(left, right) {
  const baseline = left.summary ?? left;
  const candidate = right.summary ?? right;
  for (const report of [baseline, candidate]) {
    if (report?.schema !== 'loupist-agent-report/1' || !report.metrics || !report.by_split) throw new Error('compare requires reports with metrics and by_split');
  }
  if (baseline.suite !== candidate.suite || baseline.version !== candidate.version) throw new Error('Cannot compare different suites or versions');
  const delta = (a, b) => Number.isFinite(a) && Number.isFinite(b) ? b - a : null;
  const rows = [];
  const usage = [];
  for (const split of ['all', ...new Set([...Object.keys(baseline.by_split), ...Object.keys(candidate.by_split)])]) {
    const a = split === 'all' ? baseline : baseline.by_split[split];
    const b = split === 'all' ? candidate : candidate.by_split[split];
    for (const metric of new Set([...Object.keys(a?.metrics ?? {}), ...Object.keys(b?.metrics ?? {})])) {
      const x = a?.metrics[metric], y = b?.metrics[metric];
      const change = delta(x?.headline?.rate, y?.headline?.rate);
      const checksDelta = delta(x?.checks?.rate, y?.checks?.rate);
      const trialsDelta = delta(x?.trials?.rate, y?.trials?.rate);
      rows.push({ split, metric, baseline: x?.headline ?? null, candidate: y?.headline ?? null,
        baseline_trials: x?.trials ?? null, candidate_trials: y?.trials ?? null,
        delta: change, checks_delta: checksDelta, trials_delta: trialsDelta,
        regression: split === 'regression' && [change, checksDelta, trialsDelta].some(d => d !== null && d < 0) });
    }
    for (const field of ['input', 'cache_read', 'cache_write', 'output', 'total']) {
      usage.push({ split, field, baseline: a?.tokens?.[field] ?? null, candidate: b?.tokens?.[field] ?? null,
        delta: delta(a?.tokens?.[field], b?.tokens?.[field]) });
    }
  }
  return { schema: 'loupist-agent-report-comparison/1', suite: baseline.suite,
    comparison: 'report_summary_unpaired', baseline_trials: baseline.trials ?? null, candidate_trials: candidate.trials ?? null,
    baseline_coverage: baseline.coverage, candidate_coverage: candidate.coverage,
    regression_drop: rows.some(r => r.regression), rows, usage };
}
export function comparisonMarkdown(report) {
  const show = value => value == null ? 'unknown' : String(value);
  return [`# ${report.suite}: report comparison`, '',
    `Regression split drop: **${report.regression_drop ? 'YES' : 'none observed'}**. Summary deltas are unpaired.`,
    `Trials: baseline ${show(report.baseline_trials)}, candidate ${show(report.candidate_trials)}.`, '',
    '| Split | Metric | Baseline | Candidate | Headline delta | Check delta | Trial delta | Trial counts (B/C) | Regression |',
    '| --- | --- | ---: | ---: | ---: | ---: | ---: | --- | --- |',
    ...report.rows.map(r => `| ${r.split} | ${r.metric} | ${show(r.baseline?.rate)} | ${show(r.candidate?.rate)} | ${show(r.delta)} | ${show(r.checks_delta)} | ${show(r.trials_delta)} | ${show(r.baseline_trials?.total)}/${show(r.candidate_trials?.total)} | ${r.regression ? 'DROP' : ''} |`), '',
    '| Split | Tokens | Baseline | Candidate | Delta |', '| --- | --- | ---: | ---: | ---: |',
    ...report.usage.map(r => `| ${r.split} | ${r.field} | ${show(r.baseline)} | ${show(r.candidate)} | ${show(r.delta)} |`), ''].join('\n');
}
