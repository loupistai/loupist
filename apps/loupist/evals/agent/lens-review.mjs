/** Renders the Lens pages each run produced, for a person to read before adopting a change. */
function turnKey(row) {
  return `${row.scenario_id} trial ${row.trial}`;
}

function sectionBlock(section) {
  const basis = section.basis?.length ? section.basis.join(', ') : 'none';
  const flags = [`basis ${basis}`, `origin ${section.origin ?? 'unknown'}`, section.contested ? 'contested' : null].filter(Boolean);
  const uncertainty = section.uncertainty ? `\n\n> Uncertainty: ${section.uncertainty}` : '';
  return `#### ${section.heading ?? section.key} (\`${section.key}\`; ${flags.join('; ')})\n\n${section.markdown ?? ''}${uncertainty}`;
}

export function renderLensPages(traces) {
  const runs = new Map();
  for (const row of traces) {
    if (!row.lens?.pages) continue;
    const key = turnKey(row);
    // Runner output is in execution order, so the last row holds the final pages.
    const run = runs.get(key) ?? { last: row, ops: [] };
    for (const page of row.lens.pages) {
      for (const op of page.ops_this_turn ?? []) run.ops.push(`${row.session_id}#${row.turn_index} ${op.op} ${op.key ?? ''} on "${page.title}": ${op.outcome}`);
    }
    run.last = row;
    runs.set(key, run);
  }
  const blocks = [...runs.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([key, run]) => {
    const pages = run.last.lens.pages.map((page) => [
      `### ${page.kind}: ${page.title}${page.archived ? ' (archived)' : ''}`,
      page.summary ? `> ${page.summary}` : null,
      ...page.sections.map(sectionBlock),
    ].filter(Boolean).join('\n\n'));
    const log = run.ops.length ? run.ops.map((line) => `- ${line}`).join('\n') : '- No Lens op was logged.';
    return [`## ${key}`, `Ops by turn:\n\n${log}`, ...(pages.length ? pages : ['No Lens pages.'])].join('\n\n');
  });
  return `# Lens pages by run\n\nRead each page as a user would. Note anything generalized from one case, stale, duplicated, or thinner than its Source.\n\n${blocks.join('\n\n')}\n`;
}
