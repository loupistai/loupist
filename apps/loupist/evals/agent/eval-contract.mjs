/** Shared identities and the current agent evaluation contract. No I/O or scoring. */
import { fromMarkdown } from 'mdast-util-from-markdown';

export const TRACE_SCHEMA = 'loupist-agent-trace/1';
export const SUITE_SCHEMA = 'loupist-agent-suite/1';
export const TRACE_ASSERTS = new Set(['tool_called', 'tool_not_called', 'tool_called_before', 'max_tool_calls', 'max_tool_errors', 'no_repeated_failed_call', 'capability_loaded', 'scheduled_tasks', 'tool_args_not_matching', 'calculate_inputs_cited', 'as_of_left_out', 'as_of_refused', 'grid_cells_linked', 'scheduled_delivery', 'scheduled_quiet', 'followed_names', 'replayed_price', 'skill_read',
  'scheduled_run', 'check_dry_run', 'progress_language', 'file_written_before']);
export const LENS_ASSERTS = new Set(['lens_pages', 'lens_unchanged', 'lens_read', 'lens_not_read']);
export const MEMORY_ASSERTS = new Set(['committed', 'not_committed', ...LENS_ASSERTS]);
const LENS_KINDS = new Set(['principle', 'perspective', 'method', 'subject', 'idea']);
const BASIS_KINDS = new Set(['span', 'claim', 'said', 'page']);
export const MEMORY_KINDS = new Set(['source', 'note', 'lens', 'principle', 'journal', 'priceread']);
export const ANSWER_ASSERTS = new Set(['metrics', 'contains_any', 'contains_number_any', 'figures_present', 'not_contains', 'answer_matching', 'answer_not_matching', 'prediction', 'cites_result', 'cites_source_span', 'numbers_linked', 'calculation_chip', 'no_false_save_claim', 'answer_language', 'visible_labels']);
/** Saved Memory an as-of answer left out, as Rust counts it in `as_of_report.left_out`. */
export const AS_OF_LEFT_OUT_KINDS = new Set(['facts', 'sources', 'notes', 'methods']);
const NUMBER_CHECK_STATUSES = new Set(['matched', 'rounded', 'not_found', 'differs']);
const CITATION_URL = /#evidence-ev_[0-9a-f]{32}/gu;

const SCRIPT_COMPONENTS = {
  Jpan: ['Hani', 'Hira', 'Kana'], Hrkt: ['Hira', 'Kana'], Kore: ['Hang'],
  Hans: ['Hani'], Hant: ['Hani'], Hanb: ['Hani', 'Bopo'],
};

export function answerLanguageScript(script) {
  if (!script) return null;
  const names = SCRIPT_COMPONENTS[script] ?? [script];
  try {
    return new RegExp(names.map(name => `\\p{Script_Extensions=${name}}`).join('|'), 'u');
  } catch {
    return null; // ICU also permits script codes that are not Unicode script properties.
  }
}

/** BCP 47 answer language, independent of the scenario's appearance. */
export function answerLanguageAssertion(assertion) {
  let locale;
  try {
    if (typeof assertion.language === 'string') locale = new Intl.Locale(assertion.language);
  } catch { /* Invalid BCP 47 language identifier. */ }
  if (!locale || locale.language === 'und' || !answerLanguageScript(locale.maximize().script)) {
    throw new Error('answer_language requires a BCP 47 language with a known script');
  }
  if (assertion.per_paragraph !== undefined && typeof assertion.per_paragraph !== 'boolean') throw new Error('answer_language per_paragraph must be a boolean');
  return locale.baseName;
}

export const VISIBLE_LABEL_KEYS = new Set(['bot', 'map', 'schedule', 'research_sessions_heading', 'market_data_group']);

export function visibleLabelsAssertion(assertion, scenario) {
  const { keys } = assertion;
  if (!Array.isArray(keys) || !keys.length || new Set(keys).size !== keys.length
    || keys.some(key => !VISIBLE_LABEL_KEYS.has(key))) throw new Error('visible_labels requires distinct supported keys');
  if (!['en', 'ko'].includes(scenario.language)) throw new Error('visible_labels requires a fixed scenario appearance');
  return keys;
}

const SCHEDULED_KEYS = {
  briefing: ['kind'], custom: ['kind', 'run_existing', 'clock'],
  learning: ['kind', 'program', 'clock'], check: ['kind', 'task', 'clock', 'dry_run'],
};

export function scheduledFixture(scenario, session, turn) {
  if (!('scheduled' in turn)) return;
  const fixture = turn.scheduled;
  if (!isRecord(fixture) || !Object.hasOwn(SCHEDULED_KEYS, fixture.kind)) throw new Error('scheduled requires kind briefing, custom, learning or check');
  if (!Object.keys(fixture).every(key => SCHEDULED_KEYS[fixture.kind].includes(key))) throw new Error(`scheduled ${fixture.kind} has an unknown key`);
  const seeded = fixture.kind === 'learning' || fixture.kind === 'check';
  if (seeded) {
    // The occurrence of a seeded program or check: its own prompt runs, at the stated clock.
    const [key, seeds] = fixture.kind === 'learning' ? ['program', scenario.seed?.programs] : ['task', scenario.seed?.checks];
    if (typeof fixture.clock !== 'string' || !Number.isFinite(Date.parse(fixture.clock))) throw new Error(`scheduled ${fixture.kind} requires a clock`);
    // Without a name, the occurrence is the one a previous turn saved and accepted.
    if (fixture[key] === undefined) {
      if (session.turns.indexOf(turn) < 1 || session.turns.slice(0, session.turns.indexOf(turn)).every(earlier => earlier.approvals !== 'accept')) {
        throw new Error(`scheduled ${fixture.kind} without a ${key} needs an earlier turn that accepts its card`);
      }
    } else if (!(seeds ?? []).some(seed => seed?.id === fixture[key])) throw new Error(`scheduled ${fixture.kind} names a ${key} the scenario does not seed`);
    if (fixture.dry_run !== undefined && typeof fixture.dry_run !== 'boolean') throw new Error('scheduled check dry_run must be a boolean');
  } else if (fixture.run_existing === true) {
    if (fixture.kind !== 'custom'
      || typeof fixture.clock !== 'string' || !Number.isFinite(Date.parse(fixture.clock))
      || session.turns.indexOf(turn) < 1 || session.turns[0].scheduled) {
      throw new Error('run_existing requires a previous creation turn and a clock for the custom run');
    }
  } else if (Object.keys(fixture).length !== 1 || session.turns.length !== 1) {
    throw new Error('scheduled fixture requires one isolated turn');
  }
  if (turn.briefing || turn.as_of !== undefined || turn.attachments?.length
    || scenario.sessions.length !== 1
    || session.id !== 'bot' || session.role !== 'probe' || session.memory !== 'remember'
    || !['en', 'ko'].includes(scenario.language)) {
    throw new Error('scheduled requires an isolated bot/probe/remember turn with fixed appearance and no conflicting fixture flags');
  }
  const populatedSeed = Object.values(scenario.seed ?? {}).some(value => Array.isArray(value) ? value.length > 0 : Boolean(value));
  if (turn.scheduled.kind === 'briefing' && ((scenario.connections?.length ?? 0) > 0
    || Object.keys(scenario.workspace_files ?? {}).length || populatedSeed)) throw new Error('scheduled briefing requires an empty library and no connections');
  if (turn.scheduled.kind === 'custom' && turn.user.trim() !== turn.user) {
    throw new Error('the saved custom request must have no surrounding whitespace');
  }
}

const SCENARIO_KEYS = ['id', 'title', 'split', 'tags', 'connections', 'sessions', 'seed', 'replay', 'replay_needs', 'language', 'clock', 'workspace_files', 'hidden', 'world'];
const SEED_KEYS = ['instructions', 'lens', 'sources', 'principles', 'watchlist', 'journal', 'shared_packs', 'asset_links', 'lens_pages', 'programs', 'checks', 'learning_allowance', 'corrections'];
const SOURCE_KEYS = ['id', 'title', 'text', 'origin', 'company', 'form', 'publication_time', 'language'];
const SESSION_KEYS = ['id', 'role', 'memory', 'turns'];
const TURN_KEYS = ['user', 'expect', 'scheduled', 'as_of', 'attachments', 'discriminator', 'briefing', 'replay_price', 'approvals'];
// A program and a check are seeded with the arguments of the tool that saves them.
const PROGRAM_KEYS = ['id', 'title', 'prompt', 'schedule', 'subjects', 'depth', 'scope', 'keeps', 'report', 'delivery', 'preset', 'previous_run'];
const CHECK_KEYS = ['id', 'title', 'prompt', 'terms', 'when_met_summary', 'template', 'template_params'];
const FACT_KEYS = ['id', 'subject', 'object', 'relation', 'text', 'valid_at', 'invalid_at'];
const RESOLUTION_KEYS = ['duplicates', 'contradicts', 'same_assertion'];
const FIXTURE_ID = /^[a-z0-9]+(?:-[a-z0-9]+)*$/u;

/** A calendar value at year, month or day precision, or an RFC 3339 instant. */
function fixtureTime(value) {
  return typeof value === 'string' && (/^\d{4}(?:-\d{2})?$/u.test(value) && Number.isFinite(Date.parse(value))
    || validDate(value) || (/^\d{4}-\d{2}-\d{2}T/u.test(value) && Number.isFinite(Date.parse(value))));
}

/**
 * The scenario input the runner reads: its keys, the seeds, scripted World and approvals.
 * A key the runner does not read is an error here, not a silently ignored fixture.
 */
export function runnerInputContract(scenario) {
  const where = `Scenario ${scenario.id}`;
  const text = (value) => typeof value === 'string' && value.trim().length > 0;
  const only = (value, keys, label) => {
    if (!isRecord(value)) throw new Error(`${where} ${label} must be an object`);
    const unknown = Object.keys(value).filter(key => !keys.includes(key));
    if (unknown.length) throw new Error(`${where} ${label} has unknown key ${unknown.join(', ')}`);
  };
  const list = (value, label) => {
    if (value !== undefined && !Array.isArray(value)) throw new Error(`${where} ${label} must be a list`);
    return value ?? [];
  };
  const unique = (values, label) => {
    if (new Set(values).size !== values.length) throw new Error(`${where} ${label} must be distinct`);
  };
  only(scenario, SCENARIO_KEYS, 'scenario');
  const seed = scenario.seed ?? {};
  only(seed, SEED_KEYS, 'seed');

  const sources = new Map();
  for (const source of list(seed.sources, 'seed.sources')) {
    only(source, SOURCE_KEYS, 'seeded Source');
    if (![source.title, source.text, source.origin].every(text)) throw new Error(`${where} seeded Source needs title, text and origin`);
    for (const key of ['company', 'form']) if (source[key] !== undefined && !text(source[key])) throw new Error(`${where} seeded Source ${key} must be text`);
    if (source.publication_time !== undefined && !fixtureTime(source.publication_time)) throw new Error(`${where} seeded Source publication_time must be YYYY-MM-DD or RFC 3339`);
    if (source.language !== undefined) {
      try { new Intl.Locale(source.language); } catch { throw new Error(`${where} seeded Source language must be BCP 47`); }
    }
    if (source.id === undefined) continue;
    if (!FIXTURE_ID.test(source.id) || sources.has(source.id)) throw new Error(`${where} seeded Source id ${source.id} must be a distinct lowercase id`);
    sources.set(source.id, source);
  }

  // The shape of sessions and turns is the caller's check; this reads only what is well formed.
  const sessions = Array.isArray(scenario.sessions) ? scenario.sessions.filter(session => isRecord(session) && Array.isArray(session.turns)) : [];
  const turnClocks = new Map(sessions.flatMap(session => session.turns.map((turn, index) =>
    [`${session.id}\u0000${index}`, turn?.scheduled?.clock ?? scenario.clock ?? null])));
  const entities = new Set();
  const facts = new Map();
  if (scenario.world !== undefined) {
    only(scenario.world, ['mode', 'publications'], 'world');
    const { mode, publications } = scenario.world;
    if (!['scripted', 'live'].includes(mode)) throw new Error(`${where} world mode must be scripted or live`);
    if (!Array.isArray(publications) || !publications.length) throw new Error(`${where} world needs publications`);
    unique(publications.map(item => item?.source), 'published Sources');
    // Publications run in recorded order, so a resolution may name only a fact recorded earlier.
    const recorded = (publication) => Date.parse(publication.recorded_time ?? sources.get(publication.source).publication_time);
    for (const publication of publications) {
      only(publication, ['source', 'after', 'script', 'recorded_time'], 'publication');
      const source = sources.get(publication.source);
      if (!source) throw new Error(`${where} publication names a Source the scenario does not seed by id`);
      if (!source.publication_time) throw new Error(`${where} published Source ${publication.source} needs a publication_time`);
      if (publication.recorded_time !== undefined && !fixtureTime(publication.recorded_time)) throw new Error(`${where} publication recorded_time must be YYYY-MM-DD or RFC 3339`);
      if (publication.after !== 'seed') {
        only(publication.after, ['session', 'turn'], 'publication after');
        const clock = turnClocks.get(`${publication.after.session}\u0000${publication.after.turn}`);
        if (clock === undefined) throw new Error(`${where} publication is placed after a turn the scenario does not have`);
        if (clock === null || !(recorded(publication) > Date.parse(clock))) throw new Error(`${where} a publication after a turn must be recorded later than that turn's clock`);
      }
      if ((mode === 'scripted') !== (publication.script !== undefined)) throw new Error(`${where} a scripted World scripts every publication and a live one scripts none`);
    }
    for (const publication of [...publications].sort((left, right) => recorded(left) - recorded(right))) {
      if (!publication.script) continue;
      const { script } = publication;
      only(script, ['entities', 'facts', 'resolutions'], 'script');
      for (const entity of list(script.entities, 'script entities')) {
        only(entity, ['name', 'type', 'codes'], 'script entity');
        if (!text(entity.name) || !text(entity.type) || list(entity.codes, 'entity codes').some(code => !text(code))) throw new Error(`${where} script entity needs a name, a type and text codes`);
        entities.add(entity.name);
      }
      const own = new Set();
      for (const fact of list(script.facts, 'script facts')) {
        only(fact, FACT_KEYS, 'script fact');
        if (!text(fact.id) || !FIXTURE_ID.test(fact.id) || facts.has(fact.id)) throw new Error(`${where} script fact id ${fact.id} must be a lowercase id, distinct in the scenario`);
        if (!entities.has(fact.subject) || !entities.has(fact.object)) throw new Error(`${where} script fact ${fact.id} names an entity no script has introduced`);
        if (!text(fact.relation) || !text(fact.text)) throw new Error(`${where} script fact ${fact.id} needs a relation and a text`);
        for (const key of ['valid_at', 'invalid_at']) {
          if (fact[key] !== undefined && fact[key] !== null && !fixtureTime(fact[key])) throw new Error(`${where} script fact ${fact.id} ${key} must be a date, a month, a year or an instant`);
        }
        own.add(fact.id);
      }
      const earlier = new Set(facts.keys());
      for (const id of own) facts.set(id, publication.source);
      if (script.resolutions !== undefined) {
        only(script.resolutions, [...own], 'script resolutions');
        for (const [id, resolution] of Object.entries(script.resolutions)) {
          only(resolution, RESOLUTION_KEYS, `resolution of ${id}`);
          for (const [kind, ids] of Object.entries(resolution)) {
            if (!Array.isArray(ids) || !ids.length || ids.some(other => !earlier.has(other))) throw new Error(`${where} resolution ${id}.${kind} must name facts recorded earlier`);
          }
        }
      }
    }
  }

  const followed = new Set(list(seed.watchlist, 'seed.watchlist').map(asset => asset?.name));
  for (const link of list(seed.asset_links, 'seed.asset_links')) {
    only(link, ['asset', 'company'], 'asset link');
    if (!followed.has(link.asset)) throw new Error(`${where} asset link names an asset the scenario does not follow`);
    if (!entities.has(link.company)) throw new Error(`${where} asset link names a company no script has introduced`);
  }
  for (const page of list(seed.lens_pages, 'seed.lens_pages')) {
    only(page, ['title', 'summary', 'kind', 'sections'], 'Lens page');
    if (!text(page.title) || !text(page.kind) || !Array.isArray(page.sections) || !page.sections.length) throw new Error(`${where} Lens page needs a title, a kind and sections`);
    for (const section of page.sections) {
      only(section, ['key', 'heading', 'markdown', 'refs'], 'Lens page section');
      if (![section.key, section.heading, section.markdown].every(text)) throw new Error(`${where} Lens page section needs key, heading and markdown`);
      for (const ref of list(section.refs, 'section refs')) {
        only(ref, ['source', 'fact'], 'section ref');
        if (facts.get(ref.fact) !== ref.source) throw new Error(`${where} section ref names a fact its Source does not script`);
      }
    }
  }
  for (const [key, keys, required] of [['programs', PROGRAM_KEYS, ['title', 'prompt']], ['checks', CHECK_KEYS, ['prompt', 'terms', 'template']]]) {
    const seeds = list(seed[key], `seed.${key}`);
    unique(seeds.map(item => item?.id), `seed.${key} ids`);
    for (const item of seeds) {
      only(item, keys, `seeded ${key.slice(0, -1)}`);
      if (!text(item.id) || !FIXTURE_ID.test(item.id) || !required.every(field => text(item[field]))) throw new Error(`${where} seeded ${key.slice(0, -1)} needs a lowercase id and ${required.join(', ')}`);
      if (item.previous_run !== undefined) {
        only(item.previous_run, ['clock'], 'previous_run');
        if (!fixtureTime(item.previous_run.clock)) throw new Error(`${where} previous_run needs a clock`);
      }
    }
  }
  // The user's own corrections, applied through the bridge commands once World is staged.
  for (const correction of list(seed.corrections, 'seed.corrections')) {
    if (isRecord(correction) && Object.hasOwn(correction, 'claim')) {
      only(correction, ['claim', 'action'], 'claim correction');
      only(correction.claim, ['source', 'fact'], 'corrected claim');
      if (facts.get(correction.claim.fact) !== correction.claim.source) throw new Error(`${where} correction names a fact its Source does not script`);
      if (!['dispute', 'keep_current'].includes(correction.action)) throw new Error(`${where} claim correction action must be dispute or keep_current`);
    } else {
      only(correction, ['entity', 'action', 'alias'], 'entity correction');
      if (!entities.has(correction.entity)) throw new Error(`${where} correction names an entity no script has introduced`);
      if (correction.action !== 'add_alias' || !text(correction.alias)) throw new Error(`${where} entity correction must be add_alias with an alias`);
    }
  }
  // Requests left for learning today when the first occurrence starts.
  if (seed.learning_allowance !== undefined && (!Number.isSafeInteger(seed.learning_allowance) || seed.learning_allowance < 0)) {
    throw new Error(`${where} learning_allowance must be a non-negative integer`);
  }
  for (const session of sessions) {
    only(session, SESSION_KEYS, `session ${session.id}`);
    for (const turn of session.turns.filter(isRecord)) {
      only(turn, TURN_KEYS, `session ${session.id} turn`);
      if (turn.approvals !== undefined && !['accept', 'decline'].includes(turn.approvals)) throw new Error(`${where} approvals must be accept or decline`);
    }
  }
}

/** A checked answer link to a formula computed from pinned dataset cells. */
export function calculationChipAssertion(assertion) {
  for (const field of ['operation', 'formula', 'text']) {
    if (typeof assertion[field] !== 'string' || !assertion[field].trim()) throw new Error(`calculation_chip requires ${field}`);
  }
  if (!isRecord(assertion.inputs) || !Object.keys(assertion.inputs).length
    || Object.values(assertion.inputs).some(value => !Number.isFinite(value))) throw new Error('calculation_chip inputs must contain finite expected values');
  return assertion;
}

/** The benchmark identifies a seeded Source by origin. Its runtime IDs vary per trial. */
export function sourceSpanAssertion(assertion, scenario) {
  const sources = (scenario.seed?.sources ?? []).filter(source => source.origin === assertion.source_origin);
  if (!assertion.source_origin || sources.length !== 1 || !sources[0].title || !sources[0].text) {
    throw new Error('cites_source_span requires one seeded Source with the specified origin, title and text');
  }
  if (typeof assertion.figure_pattern !== 'string' || !assertion.figure_pattern || assertion.figure_pattern.length > 200) {
    throw new Error('cites_source_span requires a figure_pattern of at most 200 characters');
  }
  const figure = new RegExp(assertion.figure_pattern, 'iu');
  if (figure.test('') || !figure.test(sources[0].text)) throw new Error('figure_pattern must match a figure in the seeded text');
  const { allow_plain_count = false } = assertion;
  if (typeof allow_plain_count !== 'boolean') throw new Error('cites_source_span allow_plain_count must be a boolean');
  return { source: sources[0], figure, allowPlainCount: allow_plain_count };
}

/** `cites_result` names one tool or a list and optional nearby strings (the material value as written). */
export function citationAssertion(assertion) {
  const tools = Array.isArray(assertion.tool) ? assertion.tool : [assertion.tool];
  if (!tools.length || tools.some((tool) => typeof tool !== 'string' || !tool)) throw new Error('cites_result requires a tool or a list of tools');
  const near = assertion.near ?? [];
  if (!Array.isArray(near) || near.some((value) => typeof value !== 'string' || !value || value.length > 80)) {
    throw new Error('cites_result near must be nonempty strings of at most 80 characters');
  }
  return { tools, near };
}

/** `numbers_linked` may name printed numbers that must have a matched or rounded check. */
export function numbersLinkedAssertion(assertion) {
  const checked = assertion.checked ?? [];
  if (!Array.isArray(checked) || checked.some((value) => typeof value !== 'string' || !value.trim() || value.length > 40)) {
    throw new Error('numbers_linked checked must be nonempty strings of at most 40 characters');
  }
  return { checked };
}

/** `as_of_left_out` reads one kind of the as-of report's left-out counts. */
export function asOfLeftOutAssertion(assertion) {
  if (!AS_OF_LEFT_OUT_KINDS.has(assertion.kind)) throw new Error('as_of_left_out kind must be facts, sources, notes, or methods');
  const min = assertion.min ?? 1;
  if (!Number.isSafeInteger(min) || min < 1) throw new Error('as_of_left_out min must be a positive integer');
  return { kind: assertion.kind, min };
}

/** `as_of_refused` names the tool the as-of report must list as refused. */
export function asOfRefusedAssertion(assertion) {
  if (typeof assertion.tool !== 'string' || !assertion.tool) throw new Error('as_of_refused requires a tool');
  return { tool: assertion.tool };
}

/**
 * `tool_called` names one tool and, optionally, `args`: fields a call that did
 * not fail must carry (a subset match, so `{"operation": "goal_grid"}` matches
 * any goal_grid calculation).
 */
export function toolCalledAssertion(assertion) {
  if (typeof assertion.tool !== 'string' || !assertion.tool) throw new Error('tool_called requires a tool');
  const args = assertion.args;
  if (args !== undefined && (!isRecord(args) || Object.keys(args).length === 0)) {
    throw new Error('tool_called args must be a nonempty object of expected argument fields');
  }
  if (assertion.successful !== undefined && typeof assertion.successful !== 'boolean') throw new Error('tool_called successful must be a boolean');
  return { tool: assertion.tool, args: args ?? null, successful: assertion.successful === true };
}

/** `file_written_before` names a file by a path pattern and the tools its first successful write must precede. */
export function fileWrittenBeforeAssertion(assertion) {
  const { path_matching: pattern, before } = assertion;
  if (typeof pattern !== 'string' || !pattern || pattern.length > 200) throw new Error('file_written_before requires a path_matching pattern of at most 200 characters');
  if (!Array.isArray(before) || !before.length || before.some(tool => typeof tool !== 'string' || !tool)) throw new Error('file_written_before requires the tools the write must precede');
  return { path: new RegExp(pattern, 'u'), before };
}

/** A successful native read of the named Skill's entrypoint. */
export function skillReadAssertion(assertion) {
  if (typeof assertion.id !== 'string' || !/^[a-z0-9][a-z0-9-]{0,63}$/u.test(assertion.id)) throw new Error('skill_read requires a bounded skill id');
  return { id: assertion.id };
}

export function toolOrderAssertion(assertion) {
  const { tools, id } = assertion;
  if (!Array.isArray(tools) || tools.length < 2 || tools.some(tool => typeof tool !== 'string' || !tool)) throw new Error('tool_called_before requires at least two tools');
  if (tools[0] === 'capability_load') {
    if (typeof id !== 'string' || !/^[a-z0-9][a-z0-9-]{0,63}$/u.test(id)) throw new Error('tool_called_before capability_load requires a pack id');
  } else if (id !== undefined) throw new Error('tool_called_before id requires capability_load first');
  return { tools, id };
}

const NEGATIVE_ASSERTS = new Set(['tool_not_called', 'tool_args_not_matching', 'max_tool_calls', 'max_tool_errors',
  'no_repeated_failed_call', 'not_committed', 'lens_unchanged', 'lens_not_read', 'not_contains',
  'answer_not_matching', 'no_false_save_claim']);

/** Negative evidence cannot turn a silent completed turn into a passing answer. */
export function isNegativeAssertion(assertion) {
  const pageMinimum = assertion.count ?? assertion.min ?? (assertion.max === undefined ? 1 : 0);
  return NEGATIVE_ASSERTS.has(assertion.assert)
    || assertion.assert === 'lens_pages' && (pageMinimum === 0 || assertion.unchanged === true)
    || assertion.assert === 'scheduled_tasks' && assertion.count === 0;
}

/** Whether `actual` carries every field of `expected`; arrays match item by item. */
export function argumentsInclude(actual, expected) {
  if (Array.isArray(expected)) {
    return Array.isArray(actual) && actual.length === expected.length
      && expected.every((item, index) => argumentsInclude(actual[index], item));
  }
  if (isRecord(expected)) {
    return isRecord(actual) && Object.entries(expected).every(([key, value]) => argumentsInclude(actual[key], value));
  }
  return actual === expected;
}

/** `calculate_inputs_cited` names inputs a calculate call must link to evidence with input_sources. */
export function calculateInputsAssertion(assertion) {
  const inputs = assertion.inputs;
  if (!Array.isArray(inputs) || inputs.length === 0 || inputs.some((name) => typeof name !== 'string' || !name)) {
    throw new Error('calculate_inputs_cited requires input names');
  }
  if (assertion.operation !== undefined && (typeof assertion.operation !== 'string' || !assertion.operation)) {
    throw new Error('calculate_inputs_cited operation must be a nonempty string');
  }
  return { inputs, operation: assertion.operation ?? null };
}

/** Reasons a grid cell may stay empty (docs/analysis/grids.md). */
export const GRID_EMPTY_REASONS = new Set(['not_reported', 'not_applicable', 'unavailable', 'not_checked']);
const GRID_LAYOUTS = new Set(['assets', 'conflict']);

/**
 * `grid_cells_linked` asks for a `grid_publish` call of one layout whose every
 * checked cell matched or rounded against its evidence; optional exact row and
 * column counts and empty-cell reasons are read from the call's arguments.
 */
export function gridCellsAssertion(assertion) {
  const layout = assertion.layout ?? 'assets';
  const { when } = assertion;
  if (when !== undefined && when !== 'published') throw new Error('grid_cells_linked when must be published');
  if (!GRID_LAYOUTS.has(layout)) throw new Error('grid_cells_linked layout must be assets or conflict');
  for (const key of ['rows', 'columns']) {
    const value = assertion[key];
    if (value !== undefined && (!Number.isSafeInteger(value) || value < 1)) throw new Error(`grid_cells_linked ${key} must be a positive integer`);
  }
  const empty = assertion.empty ?? [];
  if (!Array.isArray(empty) || empty.some((reason) => !GRID_EMPTY_REASONS.has(reason))) {
    throw new Error('grid_cells_linked empty must list not_reported, not_applicable, unavailable or not_checked');
  }
  if (layout === 'conflict' && (assertion.rows !== undefined || assertion.columns !== undefined || empty.length)) {
    throw new Error('grid_cells_linked rows, columns and empty apply to the assets layout');
  }
  return { layout, rows: assertion.rows ?? null, columns: assertion.columns ?? null, empty, when };
}

/** An absolute metric tolerance, when the scenario accepts rounding. */
export function metricTolerance(assertion) {
  if (assertion.tolerance === undefined) return null;
  if (typeof assertion.tolerance !== 'number' || !Number.isFinite(assertion.tolerance) || assertion.tolerance <= 0) {
    throw new Error('metrics tolerance must be a positive finite number');
  }
  return assertion.tolerance;
}

/** Rust's checks of the answer's number links, or null when the record does not carry them. */
export function numberChecks(trace) {
  const checks = trace.number_checks;
  return Array.isArray(checks) && checks.every((check) => isRecord(check) && typeof check.text === 'string'
    && NUMBER_CHECK_STATUSES.has(check.status)) ? checks : null;
}

/** The as-of report: undefined when the record predates it, null for an answer without a cutoff. */
export function asOfReport(trace) {
  if (!('as_of_report' in trace)) return undefined;
  return isRecord(trace.as_of_report) ? trace.as_of_report : null;
}

/** citation_url values a tool returned, or null when its delivered results are unavailable. */
export function returnedCitationUrls(calls, tool) {
  const urls = new Set();
  let known = false;
  for (const call of calls.filter((item) => item.name === tool && item.outcome !== 'error')) {
    // Redacted or size-truncated deliveries cannot show which citation they carried.
    if (!isRecord(call.result) || call.result_redacted === true || call.result.truncated === true) continue;
    known = true;
    for (const match of JSON.stringify(call.result).match(CITATION_URL) ?? []) urls.add(match);
  }
  return known ? urls : null;
}

function linkDestination(tree) {
  const definitions = new Map();
  const collectDefinitions = (node) => {
    // CommonMark resolves duplicate reference definitions to the first one.
    if (node.type === 'definition' && !definitions.has(node.identifier)) definitions.set(node.identifier, node.url);
    node.children?.forEach(collectDefinitions);
  };
  collectDefinitions(tree);
  return node => node.type === 'link' ? node.url
    : node.type === 'linkReference' ? definitions.get(node.identifier) : undefined;
}

function citationMatcher(tree, urls, labelPattern) {
  const destination = linkDestination(tree);
  const hasCitation = (node) => {
    const href = destination(node);
    // Match the full href, as the answer renderer does. Bare handles, code and
    // image destinations are not link nodes and cannot stand in for a citation.
    return (urls.has(href) && (!labelPattern || labelPattern.test(visibleText(node)))) || (node.children?.some(hasCitation) ?? false);
  };
  return hasCitation;
}

export function citesFigure(answer, url, figure, allowPlainCount) {
  const tree = fromMarkdown(answer);
  const urls = new Set([url]);
  if (citationMatcher(tree, urls, figure)(tree)) return true;
  if (!allowPlainCount) return false;
  const citesSource = citationMatcher(tree, urls);
  const visit = node => (node.type === 'paragraph' && citesSource(node)
    && figure.test(visibleText(node, undefined, true).replace(/https?:\/\/\S+/gu, ' ')))
    || (node.children?.some(visit) ?? false);
  return visit(tree);
}

/** Answer paragraphs that cite one of `urls`. */
export function citingParagraphs(answer, urls) {
  const markdown = String(answer ?? '');
  const tree = fromMarkdown(markdown);
  const hasCitation = citationMatcher(tree, urls);
  const paragraphs = [];
  const visit = (node) => {
    if (node.type === 'paragraph' && hasCitation(node)) {
      paragraphs.push(markdown.slice(node.position.start.offset, node.position.end.offset));
    }
    node.children?.forEach(visit);
  };
  visit(tree);
  return paragraphs;
}

// Link destinations, literal URLs, code blocks and HTML attributes are absent from screen-label evidence.
function visibleText(node, markdown, omitLinks = false) {
  if (['code', 'html', 'definition', 'image', 'imageReference'].includes(node.type)) return '';
  if (omitLinks && ['link', 'linkReference'].includes(node.type)) return ' ';
  if (node.type === 'list' && node.ordered && markdown !== undefined) {
    const delimiter = /^\d+([.)])/u.exec(markdown.slice(node.position.start.offset))?.[1] ?? '.';
    return node.children.map((item, index) => `${node.start + index}${delimiter} ${visibleText(item, markdown)}`).join('\n');
  }
  return node.value ?? (node.children ?? []).map(child => visibleText(child, markdown, omitLinks)).join(
    ['root', 'list', 'listItem', 'blockquote'].includes(node.type) ? '\n' : '',
  );
}

export function visibleAnswerText(answer) {
  const markdown = String(answer ?? '');
  return visibleText(fromMarkdown(markdown), markdown).replace(/https?:\/\/\S+/gu, ' ');
}

/**
 * The Markdown with only its bold and italic delimiters removed, so a phrase matches
 * however much of it is emphasized. Links, code, intraword underscores and every
 * other character stay as written.
 */
export function withoutEmphasis(text) {
  const markdown = String(text ?? '');
  const cuts = [];
  const visit = (node) => {
    if ((node.type === 'emphasis' || node.type === 'strong') && node.children?.length) {
      cuts.push([node.position.start.offset, node.children[0].position.start.offset],
        [node.children.at(-1).position.end.offset, node.position.end.offset]);
    }
    node.children?.forEach(visit);
  };
  visit(fromMarkdown(markdown));
  cuts.sort((left, right) => left[0] - right[0]);
  let kept = '';
  let from = 0;
  for (const [start, end] of cuts) {
    kept += markdown.slice(from, start);
    from = end;
  }
  return kept + markdown.slice(from);
}

export function evidenceLinkLabels(answer) {
  const tree = fromMarkdown(answer);
  const destination = linkDestination(tree);
  const labels = [];
  const visit = node => {
    if (/^#evidence-ev_[0-9a-f]{32}$/u.test(destination(node) ?? '')) labels.push(visibleText(node));
    else node.children?.forEach(visit);
  };
  visit(tree);
  return labels;
}

export function numberSpellings(values, name = 'contains_number_any') {
  if (!Array.isArray(values) || !values.length
    || values.some(value => typeof value !== 'string' || !value.trim())) {
    throw new Error(`${name} requires nonempty numeric spellings`);
  }
  return values;
}

export function figureSpellings(assertion) {
  if (!Array.isArray(assertion.figures) || !assertion.figures.length) {
    throw new Error('figures_present requires nonempty groups of figure spellings');
  }
  return assertion.figures.map(values => numberSpellings(values, 'figures_present'));
}

/** Compile at load time, including when no traces are available to grade. */
export function answerPatterns(assertion) {
  const name = assertion.assert ?? 'answer_not_matching';
  if (!Array.isArray(assertion.patterns) || assertion.patterns.length === 0
    || assertion.patterns.some((pattern) => typeof pattern !== 'string' || pattern.length === 0 || pattern.length > 200)) {
    throw new Error(`${name} requires nonempty patterns of at most 200 characters`);
  }
  const flags = assertion.flags ?? 'iu';
  if (typeof flags !== 'string') throw new Error(`${name} flags must be a string`);
  try {
    return assertion.patterns.map((pattern) => new RegExp(pattern, flags));
  } catch (error) {
    throw new Error(`${name} has an invalid regular expression or flags: ${error.message}`);
  }
}
const canonical = (value) => Array.isArray(value) ? value.map(canonical) : value && typeof value === 'object'
  ? Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical(value[key])])) : value;

export function canonicalJson(value) {
  return JSON.stringify(canonical(value ?? null));
}

/** One runner writes one run_id for every turn, so a turn needs the rest of this key. */
export function turnIdentity(row) {
  return canonicalJson([row.run_id, ...turnCoordinates(row)]);
}

export function hasTurnIdentity(row) {
  return row != null && ['run_id', 'suite', 'scenario_id', 'session_id'].every((field) => typeof row[field] === 'string' && row[field])
    && Number.isInteger(row.turn_index) && row.turn_index >= 0 && Number.isInteger(row.trial) && row.trial >= 0;
}

export function isRecord(value) {
  return value !== null && typeof value === 'object' && !Array.isArray(value);
}

export function toolCalls(trace) {
  const calls = trace.tool_calls;
  return Array.isArray(calls) && calls.every((call) => isRecord(call) && typeof call.name === 'string' && call.name)
    ? calls : null;
}

export function memoryCommits(trace) {
  const commits = trace.memory_commits;
  return Array.isArray(commits) && commits.every((commit) => isRecord(commit) && MEMORY_KINDS.has(commit.kind))
    ? commits : null;
}

/** Canonical Lens pages the runner read from the store after the turn. */
export function lensPages(trace) {
  const pages = trace.lens?.pages;
  return Array.isArray(pages) && pages.every((page) => isRecord(page) && typeof page.title === 'string'
    && Array.isArray(page.sections) && Array.isArray(page.ops_this_turn)) ? pages : null;
}

export function memoryReads(trace) {
  const reads = trace.memory_reads;
  return Array.isArray(reads) && reads.every(isRecord) ? reads : null;
}

function lensPattern(value, field) {
  if (value === undefined) return null;
  if (typeof value !== 'string' || !value) throw new Error(`Lens ${field} must be a non-empty pattern`);
  return new RegExp(value, 'iu');
}

export function lensAssertion(assertion) {
  const { kind, basis } = assertion;
  if (kind !== undefined && !LENS_KINDS.has(kind)) throw new Error(`Unknown Lens kind ${kind}`);
  if (basis !== undefined && !BASIS_KINDS.has(basis)) throw new Error(`Unknown basis kind ${basis}`);
  for (const field of ['count', 'min', 'max']) {
    const value = assertion[field];
    if (value !== undefined && (!Number.isSafeInteger(value) || value < 0)) throw new Error(`Lens ${field} must be a non-negative integer`);
  }
  if (assertion.assert !== 'lens_unchanged' && assertion.title_matches === undefined) throw new Error(`${assertion.assert} needs title_matches`);
  if (assertion.unchanged !== undefined && assertion.unchanged !== true) throw new Error('Lens unchanged must be true when present');
  return {
    kind, basis, count: assertion.count, min: assertion.min, max: assertion.max, unchanged: assertion.unchanged === true,
    title: lensPattern(assertion.title_matches, 'title_matches'),
    sectionMatches: lensPattern(assertion.section_matches, 'section_matches'),
    sectionNotMatching: lensPattern(assertion.section_not_matching, 'section_not_matching'),
  };
}

/** An observed empty answer is data; an absent or non-text answer is not. */
export function answerEvidence(trace) {
  return typeof trace.answer === 'string' ? trace.answer : null;
}

export function traceEvidenceIssues(trace) {
  return [toolCalls(trace) === null && 'tool_calls', memoryCommits(trace) === null && 'memory_commits',
    answerEvidence(trace) === null && 'answer'].filter(Boolean);
}

export function validDate(value) {
  if (typeof value !== 'string' || !/^\d{4}-\d{2}-\d{2}$/u.test(value)) return false;
  const [year, month, day] = value.split('-').map(Number);
  const date = new Date(`${value}T00:00:00.000Z`);
  return date.getUTCFullYear() === year && date.getUTCMonth() === month - 1 && date.getUTCDate() === day;
}

export function executionMismatches(trace, session, turn) {
  const expected = { user: turn.user, memory_mode: session.memory, session_role: session.role };
  return Object.keys(expected).filter((field) => trace[field] !== expected[field]);
}

export function validateSuite(suite) {
  if (suite?.schema !== SUITE_SCHEMA || typeof suite.suite !== 'string' || !suite.suite || !Array.isArray(suite.scenarios)) throw new Error('Unsupported agent suite');
  if (suite.budget !== undefined) {
    for (const key of ['maxInputTokensPerTurn', 'maxTurns']) {
      if (!Number.isSafeInteger(suite.budget?.[key]) || suite.budget[key] < 1) throw new Error(`budget.${key} must be a positive integer`);
    }
  }
  const ids = new Set();
  for (const scenario of suite.scenarios) {
    if (typeof scenario.id !== 'string' || !scenario.id || ids.has(scenario.id) || !Array.isArray(scenario.sessions) || !scenario.sessions.length) throw new Error('Invalid or duplicate scenario');
    ids.add(scenario.id);
    runnerInputContract(scenario);
    const sessions = new Set();
    for (const session of scenario.sessions) {
      if (typeof session.id !== 'string' || !session.id || sessions.has(session.id) || !Array.isArray(session.turns) || !session.turns.length) throw new Error(`Scenario ${scenario.id} has an invalid session`);
      if (!['train', 'probe'].includes(session.role) || !['off', 'read_only', 'remember'].includes(session.memory)) throw new Error(`Session ${session.id} must declare role and memory`);
      sessions.add(session.id);
      for (const turn of session.turns) {
        if (typeof turn.user !== 'string' || !turn.user.trim()) throw new Error(`Session ${session.id} has an invalid user prompt`);
        scheduledFixture(scenario, session, turn);
        if (turn.as_of !== undefined && !validDate(turn.as_of)) throw new Error(`Session ${session.id} has an invalid as_of date`);
        for (const assertion of turn.expect?.answer ?? []) {
          if (assertion.assert === 'figures_present') figureSpellings(assertion);
          if (assertion.assert === 'contains_number_any') numberSpellings(assertion.values);
          if (['answer_matching', 'answer_not_matching'].includes(assertion.assert)) answerPatterns(assertion);
          if (assertion.assert === 'cites_source_span') sourceSpanAssertion(assertion, scenario);
          if (assertion.assert === 'cites_result') citationAssertion(assertion);
          if (assertion.assert === 'numbers_linked') numbersLinkedAssertion(assertion);
          if (assertion.assert === 'calculation_chip') calculationChipAssertion(assertion);
          if (assertion.assert === 'answer_language') answerLanguageAssertion(assertion);
          if (assertion.assert === 'visible_labels') visibleLabelsAssertion(assertion, scenario);
          if (assertion.assert === 'metrics') metricTolerance(assertion);
        }
        for (const assertion of turn.expect?.trace ?? []) {
          if (assertion.assert === 'tool_args_not_matching') answerPatterns(assertion);
          if (assertion.assert === 'as_of_left_out') asOfLeftOutAssertion(assertion);
          if (assertion.assert === 'as_of_refused') asOfRefusedAssertion(assertion);
          if (assertion.assert === 'calculate_inputs_cited') calculateInputsAssertion(assertion);
          if (assertion.assert === 'tool_called') toolCalledAssertion(assertion);
          if (assertion.assert === 'grid_cells_linked') gridCellsAssertion(assertion);
          if (assertion.assert === 'followed_names' && (!Array.isArray(assertion.names) || !assertion.names.length
            || assertion.names.some(name => typeof name !== 'string' || !name.trim())
            || new Set(assertion.names).size !== assertion.names.length)) throw new Error('followed_names requires distinct names');
          if (assertion.assert === 'replayed_price' && (typeof turn.replay_price !== 'string' || !turn.replay_price.trim()
            || turn.replay_price !== assertion.asset || !Number.isFinite(assertion.value) || !validDate(assertion.date))) {
            throw new Error('replayed_price requires the replay target, expected value and date');
          }
          if (assertion.assert === 'scheduled_quiet' && turn.scheduled?.kind !== 'custom') throw new Error('scheduled_quiet requires a custom run');
          if (assertion.assert === 'scheduled_delivery' && !['briefing', 'custom'].includes(turn.scheduled?.kind)) throw new Error('scheduled_delivery requires a briefing or custom run');
          if (assertion.assert === 'scheduled_run' && !['learning', 'check'].includes(turn.scheduled?.kind)) throw new Error('scheduled_run requires a learning or check occurrence');
          if (assertion.assert === 'progress_language') answerLanguageAssertion(assertion);
          if (assertion.assert === 'file_written_before') fileWrittenBeforeAssertion(assertion);
          if (assertion.assert === 'skill_read') skillReadAssertion(assertion);
          if (assertion.assert === 'tool_called_before') toolOrderAssertion(assertion);
        }
        for (const assertion of turn.expect?.memory ?? []) {
          if (assertion.count !== undefined && (!Number.isSafeInteger(assertion.count) || assertion.count < 0)) throw new Error('Memory count must be a non-negative integer');
          if (LENS_ASSERTS.has(assertion.assert)) lensAssertion(assertion);
          else {
            const kinds = Array.isArray(assertion.kind) ? assertion.kind : [assertion.kind];
            if (!MEMORY_ASSERTS.has(assertion.assert)) throw new Error(`Unknown memory assertion ${assertion.assert}`);
            if (kinds.length === 0 || kinds.some((kind) => !MEMORY_KINDS.has(kind))) {
              throw new Error(`Unknown memory kind ${assertion.kind}`);
            }
          }
        }
        const criteria = new Set();
        for (const rubric of turn.expect?.rubric ?? []) {
          if (typeof rubric.id !== 'string' || !rubric.id || criteria.has(rubric.id)) throw new Error(`Session ${session.id} has an invalid or duplicate rubric`);
          criteria.add(rubric.id);
        }
      }
    }
  }
  return suite;
}

export function validateTraceRecord(trace) {
  if (trace?.schema !== TRACE_SCHEMA) throw new Error('Unsupported agent trace schema');
  for (const field of ['run_id', 'suite', 'scenario_id', 'session_id']) {
    if (typeof trace[field] !== 'string' || !trace[field]) throw new Error(`Trace missing ${field}`);
  }
  if (!Number.isInteger(trace.turn_index) || trace.turn_index < 0) throw new Error('Trace turn_index must be a non-negative integer');
  if (!Number.isInteger(trace.trial) || trace.trial < 0) throw new Error('Trace trial must be a non-negative integer');
  return trace;
}

export function locateTurn(suite, trace) {
  if (trace.suite !== suite.suite) return { scenario: null, session: null, turn: null };
  const scenario = suite.scenarios.find((item) => item.id === trace.scenario_id) ?? null;
  const session = scenario?.sessions.find((item) => item.id === trace.session_id) ?? null;
  const turn = session && Number.isInteger(trace.turn_index) ? session.turns[trace.turn_index] ?? null : null;
  return { scenario, session, turn };
}

/** A logical cell has no run ID; an attempt identity does. Use both deliberately. */
function turnCoordinates(row) {
  return [row.suite ?? null, row.scenario_id, row.session_id, row.turn_index, row.trial];
}

export function turnKey(row) {
  return canonicalJson(turnCoordinates(row));
}

export function turnFields(row) {
  return { suite: row.suite ?? null, scenario_id: row.scenario_id, session_id: row.session_id,
    turn_index: row.turn_index, trial: row.trial };
}

export function trialKey(row) {
  return canonicalJson([row.suite ?? null, row.scenario_id, row.trial]);
}

export function groupBy(rows, keyOf) {
  const grouped = new Map();
  for (const row of rows) {
    const key = keyOf(row);
    const group = grouped.get(key) ?? [];
    group.push(row);
    grouped.set(key, group);
  }
  return grouped;
}

export function positiveTrialCount(value = 3) {
  if (!Number.isInteger(value) || value < 1) throw new Error('trials must be a positive integer');
  return value;
}

export const FAILED_TURN_STATUSES = new Set(['failed', 'cancelled', 'interrupted', 'aborted']);

/** Missing execution evidence is unknown; only a recorded failure is false. */
export function turnCompleted(trace) {
  return trace.status === 'completed' ? true : FAILED_TURN_STATUSES.has(trace.status) ? false : null;
}
