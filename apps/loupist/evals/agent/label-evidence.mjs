import enApp from '../../src/i18n/locales/en/app.json' with { type: 'json' };
import koApp from '../../src/i18n/locales/ko/app.json' with { type: 'json' };
import enConnections from '../../src/i18n/locales/en/connections.json' with { type: 'json' };
import koConnections from '../../src/i18n/locales/ko/connections.json' with { type: 'json' };
import { visibleAnswerText, visibleLabelsAssertion } from './eval-contract.mjs';

function labels({ shell }, { catalog }) {
  return {
    bot: shell.tabs.chat,
    map: shell.tabs.map,
    schedule: shell.tabs.schedule,
    research_sessions_heading: shell.sessions,
    market_data_group: catalog.MarketData,
  };
}

const byLanguage = { en: labels(enApp, enConnections), ko: labels(koApp, koConnections) };

export function visibleLabels(language) {
  return byLanguage[language] ?? {};
}

function labelPattern(label, { marketData = false, flags = 'u' } = {}) {
  // Hangul endings attach directly to Latin and Hangul labels.
  const latin = /^\p{Script=Latin}/u.test(label);
  const before = latin ? '(?<![\\p{Script=Latin}\\p{N}_])' : '(?<![\\p{Script=Hangul}])';
  const after = latin ? '(?![\\p{Script=Latin}\\p{N}_])' : '';
  const platform = marketData ? '(?!\\s+(?:platform|플랫폼))' : '';
  return new RegExp(before + RegExp.escape(label) + after + platform, flags);
}

export function stripVisibleLabels(text, labels) {
  return labels.reduce((words, label) => words.replace(labelPattern(label, { flags: 'gu' }), ' '), text);
}

export function visibleLabelsEvidence(assertion, answer, scenario, trace) {
  const keys = visibleLabelsAssertion(assertion, scenario);
  if (trace.resolved_appearance !== undefined && trace.resolved_appearance !== scenario.language) {
    return { pass: false, detail: 'resolved appearance differs from the scenario' };
  }
  const labels = visibleLabels(scenario.language);
  const text = visibleAnswerText(answer);
  const missing = keys.filter(key => !labelPattern(labels[key], {
    marketData: key === 'market_data_group',
  }).test(text));
  return { pass: missing.length === 0,
    detail: missing.length ? `missing visible labels: ${missing.map(key => labels[key]).join(', ')}` : `${keys.length} visible labels found` };
}
