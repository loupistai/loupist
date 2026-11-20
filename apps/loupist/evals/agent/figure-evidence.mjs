/** Match literal spellings at number boundaries, including Korean units and suffixes. */
import { evidenceLinkLabels, figureSpellings } from './eval-contract.mjs';
import { finalGrids } from './grid-evidence.mjs';
import { comparableArgs } from './tool-evidence.mjs';
const normalized = text => text.normalize('NFKC').replaceAll('−', '-');

function numberPattern(value) {
  const spelling = normalized(value).trim();
  const literal = spelling.split(/\s+/u).map(RegExp.escape).join('[ \\t]+');
  if (!/\p{Nd}/u.test(spelling)) {
    const word = '[\\p{L}\\p{M}\\p{N}_]';
    return new RegExp('(?<!' + word + ')' + literal + '(?!' + word + ')', 'iu');
  }
  // A digit inside a decimal or grouped value does not start a new number.
  const before = '(?<![\\p{Script=Latin}\\p{Nd}_+\\-])(?<!\\p{Nd}[.,])';
  const after = /[\p{Script=Latin}%]$/u.test(spelling)
    ? '(?![\\p{Script=Latin}\\p{Nd}_])'
    : '(?!\\p{Nd}|[.,]\\p{Nd})';
  return new RegExp(before + literal + after, 'iu');
}

export function matchingNumber(values, texts) {
  const normalizedTexts = texts.map(normalized);
  return values.find(value => {
    const pattern = numberPattern(value);
    return normalizedTexts.some(text => pattern.test(text));
  });
}

export function figuresPresentEvidence(assertion, answer, calls) {
  const figures = figureSpellings(assertion);
  const labels = evidenceLinkLabels(answer);
  const needsGrid = figures.filter(values => !matchingNumber(values, labels));
  if (!needsGrid.length) return { pass: true, detail: `all ${figures.length} figures present` };

  let unknown = calls === null;
  const values = [];
  for (const grid of finalGrids(calls ?? [])) {
    if (grid.layout === 'conflict') continue;
    const { cells, columns } = grid.args;
    if (!grid.staged || !Array.isArray(cells) || !Array.isArray(columns)
      || comparableArgs({ cells, columns }) === null) {
      unknown = true;
      continue;
    }
    for (const row of cells) {
      for (const [index, cell] of (Array.isArray(row) ? row : []).entries()) {
        if (typeof cell?.value !== 'string') continue;
        const value = normalized(cell.value).trim();
        const barePercent = columns[index]?.unit === 'percent' && /^[+-]?\d+(?:,\d{3})*(?:\.\d+)?$/u.test(value);
        values.push(barePercent ? `${value}%` : value);
      }
    }
  }
  const missing = needsGrid.filter(spellings => !matchingNumber(spellings, values));
  if (!missing.length) return { pass: true, detail: `all ${figures.length} figures present` };
  return { pass: unknown ? null : false,
    detail: `missing ${missing.map(spellings => spellings.join(' or ')).join(', ')}${unknown ? ' (grid evidence unavailable)' : ''}` };
}
