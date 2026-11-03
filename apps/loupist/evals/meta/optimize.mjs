#!/usr/bin/env node
/** Offline proposal validation + paid, opt-in production-path scoring. Never adopts. */
import { spawn, execFileSync } from 'node:child_process';
import { mkdtemp, mkdir, readFile, writeFile, rm, realpath } from 'node:fs/promises';
import { tmpdir, homedir } from 'node:os';
import { dirname, resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { validateSuite } from '../agent/eval-contract.mjs';
import { isHeldoutSuitePath } from '../agent/suite-contract.mjs';
import { buildReport, tokenTotals } from '../agent/report.mjs';
import { judgeWithRecovery } from './judging.mjs';
import { optionalJson, suiteHash, assertReusableTraces, assertStageSignature, preserveAttempt, writeJson, acquireRunLock } from './resume.mjs';
import { DEFAULT_CONTEXT, editable, lockedRegions, applyPatch, diffSurface, surfaceHash, sha256, readSurface, writeSurface } from './surface.mjs';
import { partition, adaptiveSelection, score, beatsSubset, pairedComparison, acceptance, archiveCandidate, failingExcerpts, redact } from './core.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../../../..');
const app = resolve(here, '../..');
const json = value => JSON.stringify(value, null, 2) + '\n';
const readJson = async path => JSON.parse(await readFile(path, 'utf8'));
const readJsonl = async path => (await readFile(path, 'utf8')).split('\n').filter(Boolean).map(line => JSON.parse(line));

export function parseArgs(argv) {
  const result = { iterations: 2, proposer: 'codex', model: 'openai-codex/gpt-6.1-sol', thinking: 'medium' };
  const allowed = new Set(['suite', 'iterations', 'proposer', 'out', 'resume', 'runner', 'pi-agent-dir', 'taxonomy', 'model', 'thinking']);
  const provided = [];
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--help') return { help: true };
    const key = argv[i].replace(/^--/u, '');
    if (!argv[i].startsWith('--') || !allowed.has(key) || !argv[i + 1] || argv[i + 1].startsWith('--')) throw new Error(`Unknown option or missing value: ${argv[i]}`);
    result[key] = argv[++i]; provided.push(key);
  }
  result.provided = provided;
  if (result.resume && result.out && resolve(result.resume) !== resolve(result.out)) throw new Error('--resume and --out must identify the same run');
  result.out ??= result.resume;
  result.iterations = Number(result.iterations);
  if ((!result.suite && !result.resume) || !result.out || !Number.isSafeInteger(result.iterations) || result.iterations < 1 || !['codex', 'grok'].includes(result.proposer)) throw new Error('Require --suite, --out, positive --iterations and --proposer codex|grok');
  return result;
}

export async function loadSuite(path) {
  if (isHeldoutSuitePath(path) || /held[-_]?out/iu.test(path)) throw new Error('Held-out files are forbidden');
  const canonical = await realpath(path);
  if (isHeldoutSuitePath(canonical) || /held[-_]?out/iu.test(canonical)) throw new Error('Held-out files are forbidden');
  const suite = validateSuite(await readJson(canonical));
  if (suite.scenarios.some(s => s.split === 'heldout')) throw new Error('Held-out scenarios are forbidden');
  return suite;
}

function processRun(command, args, { cwd = repo, input, capture = false } = {}) {
  return new Promise((resolvePromise, reject) => {
    const child = spawn(command, args, { cwd, stdio: [input ? 'pipe' : 'ignore', capture ? 'pipe' : 'inherit', capture ? 'pipe' : 'inherit'] });
    let stdout = '', stderr = '';
    if (capture) {
      child.stdout.on('data', chunk => { stdout += chunk; });
      child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-20000); });
    }
    if (input) child.stdin.end(input);
    child.on('error', reject);
    child.on('close', code => resolvePromise({ code: code ?? 1, stdout, stderr }));
  });
}
function git(args) { return execFileSync('git', args, { cwd: repo, encoding: 'utf8', maxBuffer: 8 * 1024 * 1024 }); }
export function committedSurface(commit) {
  const prefix = 'apps/loupist/agent/';
  const paths = git(['ls-tree', '-r', '--name-only', commit, '--', prefix]).trim().split('\n').filter(path => editable(path.slice(prefix.length)));
  const assets = Object.fromEntries(paths.map(path => [path.slice(prefix.length), git(['show', `${commit}:${path}`])]));
  // Lock markers are metadata only; preserve committed prose when local lock insertion is newer.
  assets['context-policy.json'] = json(DEFAULT_CONTEXT);
  return assets;
}
export function addCommittedLocks(committed, marked) {
  if (lockedRegions(committed).length) return committed;
  const locks = lockedRegions(marked);
  if (!locks.length) throw new Error('SYSTEM.md needs safety lock markers before optimization');
  let result = committed;
  for (const lock of locks) {
    const text = lock.replace(/<!-- \/?safety:locked -->/gu, '');
    if (!text || result.split(text).length !== 2) throw new Error('Committed safety sentence differs from locked baseline');
    result = result.replace(text, lock);
  }
  return result;
}

export function proposerArgs(kind, cwd, output) {
  if (kind === 'codex') return ['exec', '--ignore-user-config', '--ignore-rules', '--skip-git-repo-check', '--ephemeral', '-m', 'gpt-6-astra', '-c', 'model_reasoning_effort="high"', '-c', 'features.shell_tool=false', '-c', 'web_search="disabled"', '-c', 'features.multi_agent=false', '--sandbox', 'read-only', '--cd', cwd, '--json', '--output-last-message', output, '-'];
  return ['-p', '--model', 'grok-4.7-build-fast', '--reasoning-effort', 'high', '--disable-web-search', '--no-subagents', '--tools', '', '--max-turns', '1', '--verbatim', '--output-format', 'json', '--cwd', cwd];
}
async function propose(kind, assets, context, parent) {
  const scratch = await mkdtemp(join(tmpdir(), 'loupist-meta-proposer-'));
  const started = Date.now();
  try {
    await writeSurface(join(scratch, 'agent'), assets);
    await writeFile(join(scratch, 'context.json'), json(context), { mode: 0o600 });
    const prompt = [
      'Propose one small general improvement to this financial research agent harness.',
      'Return ONLY a unified diff with --- a/SYSTEM.md and +++ b/SYSTEM.md (or other asset-relative paths).',
      'You may edit SYSTEM.md outside locked regions, existing Skill bodies with identical frontmatter, catalog description strings, or context-policy.json numeric values within ±50% of defaults.',
      'Preserve every authority, permission and safety rule. Explicit safety lock markers and their contents are immutable.',
      'No graders, judges, suites, held-out data, optimizer changes, benchmark-specific IDs/names/numbers, or memorized case answers.',
      'Use only the supplied context. Do not call tools, read outside the scratch directory, or modify files. The full editable surface follows so no tool access is needed.',
      'Generalize failing behavior into reusable methods. Keep the change concise; explain nothing outside the diff.',
      JSON.stringify({ assets, context }),
    ].join('\n');
    const output = join(scratch, 'proposal.patch');
    const args = proposerArgs(kind, scratch, output);
    // Grok's file input is the same single-turn mode without macOS ARG_MAX limits.
    if (kind === 'grok') {
      const promptPath = join(scratch, 'prompt.txt');
      await writeFile(promptPath, prompt, { mode: 0o600 });
      args.splice(0, 1, '--prompt-file', promptPath);
    }
    const result = await processRun(kind, args, { cwd: scratch, input: kind === 'codex' ? prompt : undefined, capture: true });
    const events = result.stdout.split('\n').flatMap(line => { try { return [JSON.parse(line)]; } catch { return []; } });
    const usage = events.filter(e => e.usage).map(e => e.usage);
    const metadata = { proposer: kind, model: kind === 'codex' ? 'gpt-6-astra' : 'grok-4.7-build-fast', effort: 'high', duration_ms: Date.now() - started, usage: usage.length ? usage : null, exit_code: result.code };
    await writeFile(join(parent, 'proposer.json'), json(metadata), { mode: 0o600 });
    if (result.code !== 0) {
      const messages = events.flatMap(e => [e.message, e.error?.message]).filter(v => typeof v === 'string');
      const reason = redact([...messages, result.stderr].join('\n')).slice(-2000);
      throw Object.assign(new Error(`Proposer ${kind} failed: ${reason}`), { quota: /usage limit|weekly|quota|limit reached/iu.test(reason), metadata });
    }
    let patch;
    if (kind === 'codex') patch = await readFile(output, 'utf8');
    else {
      const value = events.at(-1);
      patch = value?.text ?? value?.result;
      if (typeof patch !== 'string') throw new Error('Grok did not return a text diff');
    }
    return { patch, metadata };
  } finally { await rm(scratch, { recursive: true, force: true }); }
}

async function runScore({ assets, suite, suitePath, ids, trials, directory, options, deterministicOnly = false }) {
  await mkdir(directory, { recursive: true });
  const selected = { ...suite, scenarios: suite.scenarios.filter(s => ids.includes(s.id)) };
  if (!selected.scenarios.length) throw new Error('Refusing empty eval selection');
  const surface = await readSurface(assets);
  const signature = { assets: surfaceHash(surface), suite: await suiteHash(suite, suitePath), ids, trials,
    model: options.model, thinking: options.thinking, runner: options.runnerHash };
  const saved = await optionalJson(join(directory, 'stage.json'));
  let traces, reused = false;
  try { traces = await readJsonl(join(directory, 'traces.jsonl')); }
  catch (error) { if (error.code !== 'ENOENT') throw error; }
  if (traces) {
    if (!options.resume) throw new Error('Stage traces already exist; use an explicit --resume');
    if (!saved) throw new Error('Saved stage has no input provenance');
    assertStageSignature(saved, signature);
    assertReusableTraces({ suite: selected, traces, trials, assets: surface, model: options.model, thinking: options.thinking });
    reused = true;
    console.error(`Reusing ${traces.length} verified agent turns: ${directory}`);
  } else {
    if (sha256(await readFile(options.runner)) !== options.runnerHash) throw new Error('Compiled runner changed during optimization');
    await writeJson(join(directory, 'stage.json'), signature);
    const result = await processRun(options.runner, ['--suite', suitePath, '--scenarios', ids.join(','), '--out', join(directory, 'traces.jsonl'), '--trials', String(trials), '--model', options.model, '--thinking', options.thinking, '--pi-agent-dir', options['pi-agent-dir'], '--agent-assets', assets]);
    traces = await readJsonl(join(directory, 'traces.jsonl')).catch(error => { if (result.code !== 0 && error.code === 'ENOENT') return []; throw error; });
    if (result.code !== 0) throw Object.assign(new Error(`Eval runner exited ${result.code}; partial traces retained at ${directory}`), { traces });
  }
  await writeJson(join(directory, 'stage.json'), signature);
  let judgments = [], judgeUsage = { calls: 0, new_calls: 0, cached: 0, tokens: null };
  if (!deterministicOnly) {
    const result = await judgeWithRecovery({ suite: selected, traces, cacheDir: join(options.out, 'judge-cache'), directory, concurrency: 3 });
    judgments = result.judgments; judgeUsage = result.usage;
    await writeFile(join(directory, 'judgments.jsonl'), judgments.map(row => JSON.stringify(row)).join('\n') + '\n', { mode: 0o600 });
  }
  const scores = score(selected, traces, judgments, trials, deterministicOnly);
  await writeFile(join(directory, 'report.json'), json(buildReport({ suite: selected, traces, judgments, trials })), { mode: 0o600 });
  scores.judge_usage = judgeUsage; scores.agent_reused = reused;
  await writeJson(join(directory, 'scores.json'), scores);
  return { scores, traces };
}

export function report({ commit, split, baseline, best, iterations, validation, original, assets, usage, error }) {
  const fraction = value => value ? `${value.passed}/${value.eligible_total ?? value.total}` : 'not scored';
  const rows = [['baseline', '—', fraction(baseline), 'baseline'], ...iterations.map(i => [i.n, fraction(i.subset), fraction(i.train), i.reason])];
  const bestTrain = best === 0 ? baseline : iterations.find(i => i.n === best)?.train;
  const meanInput = value => {
    const fields = ['input', 'cache_read', 'cache_write'].map(key => value?.usage?.[key]);
    return fields.every(Number.isFinite) && value?.coverage?.expected_turns > 0
      ? fields.reduce((a, b) => a + b, 0) / value.coverage.expected_turns : null;
  };
  const beforeInput = meanInput(baseline), afterInput = meanInput(bestTrain);
  const inputDelta = beforeInput === null || afterInput === null ? null : afterInput - beforeInput;
  const errors = [{ stage: 'baseline', scores: baseline }, ...iterations.map(i => ({ stage: `candidate-${i.n}`, scores: i.train ?? i.subset })), { stage: 'validation-baseline', scores: validation?.baseline }, { stage: 'validation-best', scores: validation?.best }].flatMap(({ stage, scores }) => (scores?.judge_errors ?? []).map(row => ({ stage, scenario_id: row.scenario_id, item_id: row.item_id, reason: row.reason, attempts: row.attempts })));
  const size = value => Object.values(value).reduce((sum, text) => sum + Buffer.byteLength(text), 0);
  return `# Meta-harness adoption report\n\nStatus: ${error ? 'incomplete — ' + error : 'complete; human adoption pending'}. No held-out suite was loaded or run, and no candidate was adopted.\n\nBaseline commit: \`${commit}\`. Partition: ${split.train.length} train / ${split.validation.length} validation scenarios; SHA-256 deterministic 70/30 allocation. Capability scores require every non-diagnostic deterministic assertion and independent Grok rubric to pass in both trials. Missing evidence cannot pass.\n\n| Candidate | Adaptive subset passed | Full train passed | Decision |\n| --- | ---: | ---: | --- |\n${rows.map(row => '| ' + row.map(value => String(value ?? '').replaceAll('|', '\\|').replaceAll('\n', ' ')).join(' | ') + ' |').join('\n')}\n\nBest candidate: ${best}.\n\n| Validation (2 trials) | Passed | Eligible total |\n| --- | ---: | ---: |\n| Baseline | ${validation?.baseline?.passed ?? 'unmeasured'} | ${validation?.baseline?.eligible_total ?? 'unmeasured'} |\n| Best | ${validation?.best?.passed ?? 'unmeasured'} | ${validation?.best?.eligible_total ?? 'unmeasured'} |\n\nPaired validation comparison (identical eligible scenarios): ${validation ? JSON.stringify(pairedComparison(validation.baseline, validation.best)) : 'unmeasured'}.\n\n## Usage and context size\n\nMean provider input tokens per train turn (including cache): ${beforeInput ?? 'unknown'} → ${afterInput ?? 'unknown'} (delta ${inputDelta ?? 'unknown'}).\n\nEditable text bytes: ${size(original)} → ${size(assets)} (delta ${size(assets) - size(original)}). This is asset size, not delivered prompt tokens. Provider input/cache/output counters and proposer usage are recorded below; judge tokens remain unknown.\n\n\`\`\`json\n${JSON.stringify({ context_policy: { baseline: JSON.parse(original['context-policy.json']), best: JSON.parse(assets['context-policy.json']) }, runs: usage }, null, 2)}\n\`\`\`\n\n## Judge errors\n\n${errors.length ? JSON.stringify(errors, null, 2) : 'None.'}\n\nA scenario with a judge_error is excluded from both sides of the acceptance comparison; it is neither a pass nor a fail.\n\n## Best candidate diff\n\n\`\`\`diff\n${diffSurface(original, assets) || '(no accepted change)\n'}\`\`\`\n\n## Adoption\n\nThe coordinator reviews this diff, train/validation evidence and regression smokes, then performs one independent held-out run. Use the agent CLI adopt check; deterministic security failures veto judge scores. Validation cannot select another candidate or feed a proposer. No source file is updated by this optimizer.\n`;
}

export async function main(argv = process.argv.slice(2)) {
  const options = parseArgs(argv);
  if (options.help) { console.log('optimize.mjs --suite learning-v1 --iterations 2 --proposer codex|grok --out <new-dir> [--runner <compiled-agent_eval>] [--pi-agent-dir <dir>] --taxonomy <development-taxonomy.md>'); return; }
  options.out = resolve(options.out);
  const release = await acquireRunLock(options.out);
  try { await optimize(options); } finally { await release(); }
}

async function optimize(options) {
  const savedManifest = options.resume ? await optionalJson(join(options.out, 'manifest.json')) : null;
  if (savedManifest) for (const [key, value] of Object.entries(savedManifest.options)) {
    if (options.provided.includes(key) && String(options[key]) !== String(value)) throw new Error(`--${key} differs from saved run`);
    options[key] = value;
  }
  options['pi-agent-dir'] = resolve(options['pi-agent-dir'] ?? join(homedir(), '.loupist/development/pi'));
  options.runner = resolve(options.runner ?? join(app, 'src-tauri/target/debug/examples/agent_eval'));
  options.runnerHash = sha256(await readFile(options.runner));
  if (!options.suite) throw new Error('--suite is required');
  const suitePath = options.suite.endsWith('.json') ? resolve(options.suite) : join(here, '../agent/suites', options.suite + '.json');
  const suite = await loadSuite(suitePath), split = partition(suite), fingerprint = await suiteHash(suite, suitePath);
  if (!options.taxonomy) throw new Error('--taxonomy must identify a development failure taxonomy (never held-out output)');
  if (isHeldoutSuitePath(await realpath(options.taxonomy))) throw new Error('Held-out taxonomy path is forbidden');
  const taxonomy = savedManifest?.taxonomy ?? redact((await readFile(options.taxonomy, 'utf8')).split('\n## ')[0]);
  const baselineDir = join(options.out, 'baseline-assets'), bestDir = join(options.out, 'best-assets');
  let commit, original;
  if (options.resume) {
    if (!savedManifest) throw new Error('Resume requires the saved manifest.json');
    const previous = await readJson(join(options.out, 'run.json'));
    const partitionRecord = await readJson(join(options.out, 'partition.json'));
    if (partitionRecord.suite_sha256 !== fingerprint || JSON.stringify(partitionRecord.train) !== JSON.stringify(split.train)
      || JSON.stringify(partitionRecord.validation) !== JSON.stringify(split.validation)) throw new Error('Suite or partition changed since the saved run');
    commit = previous.commit;
    original = await readSurface(baselineDir);
    const committed = committedSurface(commit);
    committed['SYSTEM.md'] = addCommittedLocks(committed['SYSTEM.md'], original['SYSTEM.md']);
    if (surfaceHash(original) !== surfaceHash(committed)) throw new Error('Saved baseline differs from its committed assets and lock comments');
    if (options.runnerHash !== savedManifest.runnerHash) throw new Error('Compiled runner differs from the saved run');
    if (savedManifest.surfaceHash !== surfaceHash(original) || savedManifest.suiteHash !== fingerprint) throw new Error('Saved run assets or suite were modified');
    await preserveAttempt(options.out);
  } else {
    commit = git(['rev-parse', 'HEAD']).trim();
    original = committedSurface(commit);
    original['SYSTEM.md'] = addCommittedLocks(original['SYSTEM.md'], await readFile(join(app, 'agent/SYSTEM.md'), 'utf8'));
    await mkdir(options.out, { recursive: false });
    await writeJson(join(options.out, 'partition.json'), { ...split, suite_sha256: fingerprint, commit });
    await writeSurface(baselineDir, original);
  }
  await writeJson(join(options.out, 'manifest.json'), { schema: 'loupist-meta-run/1', commit, runnerHash: options.runnerHash,
    surfaceHash: surfaceHash(original), suiteHash: fingerprint, taxonomy,
    options: Object.fromEntries(['suite', 'iterations', 'proposer', 'model', 'thinking', 'runner', 'pi-agent-dir', 'taxonomy'].map(key => [key, options[key]])) });
  let assets = original, best = 0, baseline, incumbent, validation, smoke, incumbentTraces = [];
  const iterations = [], usage = [];
  const doScore = async args => {
    try {
      const result = await runScore({ options, ...args });
      usage.push({ stage: args.directory.slice(options.out.length + 1), agent: result.scores.usage, agent_reused: result.scores.agent_reused, judge: result.scores.judge_usage });
      return result;
    } catch (error) {
      const partial = error.traces ?? await readJsonl(join(args.directory, 'traces.jsonl')).catch(() => []);
      usage.push({ stage: args.directory.slice(options.out.length + 1), incomplete: true, agent: tokenTotals(partial), judge: { tokens: null, calls: null } });
      throw error;
    }
  };
  const smokeRun = async (path, directory) => {
    const scores = {}, traces = [];
    for (const name of ['principles-v1', 'security-v1']) {
      const pathToSuite = join(here, '../agent/suites', name + '.json'), smokeSuite = await loadSuite(pathToSuite);
      const result = await doScore({ assets: path, suite: smokeSuite, suitePath: pathToSuite, ids: smokeSuite.scenarios.filter(s => s.split === 'regression').map(s => s.id), trials: 1, directory: join(directory, name), deterministicOnly: true });
      scores[name] = result.scores; traces.push(...result.traces);
    }
    return { scores, traces };
  };
  let failure;
  try {
    console.error('Scoring baseline train (2 trials)');
    const first = await doScore({ assets: baselineDir, suite, suitePath, ids: split.train, trials: 2, directory: join(options.out, 'baseline-train') });
    baseline = incumbent = first.scores; incumbentTraces = first.traces;
    if (!baseline.complete || baseline.eligible_total === 0) throw new Error('Baseline train evidence is incomplete; refusing optimization');
    smoke = (await smokeRun(baselineDir, join(options.out, 'baseline-smoke'))).scores;
    await archiveCandidate(options.out, 0, { diff: '', assets, scores: { train: baseline, smoke }, traces: first.traces });
    for (let n = 1; n <= options.iterations; n++) {
      console.error(`Proposing candidate ${n}/${options.iterations}`);
      const directory = join(options.out, 'candidates', String(n)); await mkdir(directory, { recursive: true });
      const selected = adaptiveSelection(incumbent, n, commit), context = { taxonomy, failing_traces: failingExcerpts(incumbent, incumbentTraces), archive: iterations.map(i => ({ n: i.n, diff: i.diff, train: i.train, subset: i.subset, reason: i.reason })) };
      let proposal;
      try {
        let savedPatch;
        if (options.resume) { try { savedPatch = await readFile(join(directory, 'diff.patch'), 'utf8'); } catch (error) { if (error.code !== 'ENOENT') throw error; } }
        if (savedPatch) proposal = { patch: savedPatch, metadata: { ...(await readJson(join(directory, 'proposer.json'))), reused: true } };
        else try { proposal = await propose(options.proposer, assets, context, directory); }
        catch (error) {
          if (options.proposer !== 'codex' || !error.quota) throw error;
          console.error('Codex quota blocked proposer; falling back to Grok');
          await writeFile(join(directory, 'codex-quota.json'), json(error.metadata));
          usage.push({ stage: `proposer-${n}-codex-quota`, ...error.metadata });
          proposal = await propose('grok', assets, context, directory);
        }
      } catch (error) {
        const failed = { n, accepted: false, reason: error.message, proposer: error.metadata };
        iterations.push(failed);
        await archiveCandidate(options.out, n, { diff: '', assets: null, scores: failed, traces: [] });
        throw error;
      }
      await writeFile(join(directory, 'diff.patch'), proposal.patch, { mode: 0o600 });
      usage.push({ stage: `proposer-${n}`, ...proposal.metadata });
      let candidate;
      try { candidate = applyPatch(assets, proposal.patch); }
      catch (error) {
        const result = { n, diff: proposal.patch, accepted: false, reason: error.message };
        iterations.push(result);
        await archiveCandidate(options.out, n, { diff: proposal.patch, assets: null, scores: result, traces: [] });
        continue;
      }
      const temporary = await mkdtemp(join(tmpdir(), 'loupist-meta-candidate-'));
      const traces = [], result = { n, parent: best, diff: proposal.patch, selected, accepted: false };
      try {
        await writeSurface(temporary, candidate);
        const sampled = await doScore({ assets: temporary, suite, suitePath, ids: selected, trials: 2, directory: join(directory, 'adaptive') });
        result.subset = sampled.scores; traces.push(...sampled.traces);
        if (!beatsSubset(incumbent, sampled.scores, selected)) result.reason = 'Adaptive train subset did not improve';
        else {
          // Reuse measured subset turns; run only the unmeasured train complement.
          const rest = split.train.filter(id => !selected.includes(id));
          let allTraces = sampled.traces, allJudgments = await readJsonl(join(directory, 'adaptive/judgments.jsonl'));
          if (rest.length) {
            const remaining = await doScore({ assets: temporary, suite, suitePath, ids: rest, trials: 2, directory: join(directory, 'train-complement') });
            traces.push(...remaining.traces); allTraces = [...allTraces, ...remaining.traces];
            allJudgments.push(...await readJsonl(join(directory, 'train-complement/judgments.jsonl')));
          }
          result.train = score({ ...suite, scenarios: suite.scenarios.filter(s => split.train.includes(s.id)) }, allTraces, allJudgments, 2);
          if (pairedComparison(incumbent, result.train).delta < 1 || !result.train.complete) result.reason = 'Full train did not improve';
          else {
            const check = await smokeRun(temporary, join(directory, 'smoke')); result.smoke = check.scores; traces.push(...check.traces);
            Object.assign(result, acceptance(incumbent, result.train, smoke, check.scores));
            if (result.accepted) { assets = candidate; best = n; incumbent = result.train; incumbentTraces = allTraces; smoke = check.scores; }
          }
        }
      } catch (error) {
        result.reason = error.message; traces.push(...(error.traces ?? [])); iterations.push(result); throw error;
      } finally {
        await archiveCandidate(options.out, n, { diff: proposal.patch, assets: candidate, scores: result, traces });
        await rm(temporary, { recursive: true, force: true });
      }
      iterations.push(result);
    }
    await writeSurface(bestDir, assets);
    console.error('Confirming fixed best candidate on validation (2 trials)');
    const base = await doScore({ assets: baselineDir, suite, suitePath, ids: split.validation, trials: 2, directory: join(options.out, 'validation-baseline') });
    // Even when no proposal improved, confirmation independently measures the selected incumbent.
    const confirmed = await doScore({ assets: bestDir, suite, suitePath, ids: split.validation, trials: 2, directory: join(options.out, 'validation-best') });
    validation = { baseline: base.scores, best: confirmed.scores };
    await writeFile(join(options.out, 'validation.json'), json(validation));
  } catch (error) { failure = redact(error.message); }
  finally {
    await writeSurface(bestDir, assets);
    await writeFile(join(options.out, 'report.md'), report({ commit, split, baseline, best, iterations, validation, original, assets, usage, error: failure }), { mode: 0o600 });
    await writeFile(join(options.out, 'run.json'), json({ status: failure ? 'failed' : 'completed', error: failure, commit, best, baseline, train: incumbent, validation, usage, iterations }), { mode: 0o600 });
  }
  if (failure) throw new Error(failure);
  console.log(`Report: ${join(options.out, 'report.md')}`);
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main().catch(error => { console.error(error.message); process.exitCode = 1; });
