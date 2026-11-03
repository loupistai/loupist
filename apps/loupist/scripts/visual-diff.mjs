#!/usr/bin/env node
import { spawn } from 'node:child_process';
import { copyFile, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, relative, resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
export const escapeHtml = value => String(value).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
// Shared by capture, comparison, and fixture tests. No browser automation dependency.
export async function openChrome() {
  if (typeof WebSocket === 'undefined') throw new Error('Node 22 or later is required');
  const profile = await mkdtemp(join(tmpdir(), 'loupist-visual-'));
  let chrome, socket;
  const pending = new Map(), listeners = new Set();
  const close = async () => {
    socket?.close();
    if (chrome?.pid && chrome.exitCode === null && chrome.signalCode === null) {
      const exited = new Promise(resolve => chrome.once('exit', resolve));
      chrome.kill('SIGTERM');
      await Promise.race([exited, sleep(2000)]);
      if (chrome.exitCode === null && chrome.signalCode === null) { chrome.kill('SIGKILL'); await exited; }
    }
    // Chrome's helper processes can still write to the profile just after the browser exits.
    await rm(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  };
  try {
    const endpoint = await new Promise((resolve, reject) => {
      const executable = process.env.CHROME_PATH || (process.platform === 'darwin'
        ? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' : 'google-chrome');
      chrome = spawn(executable, ['--headless=new', `--user-data-dir=${profile}`, '--remote-debugging-port=0', '--no-first-run', '--no-default-browser-check', '--disable-background-networking', '--disable-background-timer-throttling', '--disable-renderer-backgrounding', '--disable-backgrounding-occluded-windows', '--disable-features=BackForwardCache', '--hide-scrollbars', 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] });
      let stderr = '';
      const timer = setTimeout(() => reject(new Error('Chrome did not publish a DevTools endpoint')), 15000);
      chrome.once('error', error => { clearTimeout(timer); reject(error); });
      chrome.once('exit', code => { clearTimeout(timer); reject(new Error(`Chrome exited before DevTools was ready (${code})`)); });
      chrome.stderr.on('data', chunk => {
        stderr = (stderr + chunk).slice(-16000);
        const match = stderr.match(/DevTools listening on (ws:\/\/[^\s]+)/);
        if (match) { clearTimeout(timer); resolve(match[1]); }
      });
    });
    socket = new WebSocket(endpoint);
    await new Promise((resolve, reject) => { socket.addEventListener('open', resolve, { once: true }); socket.addEventListener('error', reject, { once: true }); });
    let sequence = 0;
    socket.addEventListener('message', ({ data }) => {
      const message = JSON.parse(data);
      if (!message.id) { for (const listener of listeners) listener(message); return; }
      const request = pending.get(message.id);
      if (!request) return;
      pending.delete(message.id); clearTimeout(request.timer);
      if (message.error) request.reject(new Error(message.error.message)); else request.resolve(message.result);
    });
    socket.addEventListener('close', () => { for (const request of pending.values()) { clearTimeout(request.timer); request.reject(new Error('Chrome DevTools connection closed')); } pending.clear(); });
    const command = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
      const id = ++sequence;
      const timer = setTimeout(() => { pending.delete(id); reject(new Error(`CDP timeout: ${method}`)); }, 30000);
      pending.set(id, { resolve, reject, timer });
      socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    });
    const { targetId } = await command('Target.createTarget', { url: 'about:blank' });
    const { sessionId } = await command('Target.attachToTarget', { targetId, flatten: true });
    const page = (method, params) => command(method, params, sessionId);
    await page('Page.enable'); await page('Runtime.enable');
    const evaluate = async expression => {
      const result = await page('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
      if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
      return result.result.value;
    };
    return { page, evaluate, close, onEvent: listener => { listeners.add(listener); return () => listeners.delete(listener); } };
  } catch (error) { await close(); throw error; }
}

// Runs in Chrome: compare decoded RGBA, including alpha, without PNG encoding noise.
export async function comparePixels(baselineUrl, candidateUrl) {
  const load = async url => { const image = new Image(); image.src = url; await image.decode(); return image; };
  const [baseline, candidate] = await Promise.all([load(baselineUrl), load(candidateUrl)]);
  const canvas = (width, height) => Object.assign(document.createElement('canvas'), { width, height });
  const width = Math.max(baseline.width, candidate.width), height = Math.max(baseline.height, candidate.height);
  const a = canvas(width, height), b = canvas(width, height), diff = canvas(width, height);
  a.getContext('2d').drawImage(baseline, 0, 0); b.getContext('2d').drawImage(candidate, 0, 0);
  const aa = a.getContext('2d').getImageData(0, 0, width, height).data;
  const bb = b.getContext('2d').getImageData(0, 0, width, height).data;
  const ctx = diff.getContext('2d'), pixels = ctx.createImageData(width, height);
  const sizeMismatch = baseline.width !== candidate.width || baseline.height !== candidate.height;
  let mismatchPixels = 0;
  for (let i = 0; i < aa.length; i += 4) {
    const different = sizeMismatch || [0, 1, 2, 3].some(channel => Math.abs(aa[i + channel] - bb[i + channel]) > 16);
    if (different) mismatchPixels++;
    pixels.data[i] = different ? 255 : Math.round(bb[i] * .3);
    pixels.data[i + 1] = different ? 45 : Math.round(bb[i + 1] * .3);
    pixels.data[i + 2] = different ? 85 : Math.round(bb[i + 2] * .3);
    pixels.data[i + 3] = different ? 255 : bb[i + 3];
  }
  ctx.putImageData(pixels, 0, 0);
  const side = canvas(baseline.width + candidate.width, height);
  side.getContext('2d').drawImage(baseline, 0, 0); side.getContext('2d').drawImage(candidate, baseline.width, 0);
  return { mismatchPixels, totalPixels: width * height, mismatchPercentage: mismatchPixels / (width * height) * 100, sizeMismatch, baselineSize: [baseline.width, baseline.height], candidateSize: [candidate.width, candidate.height], diff: diff.toDataURL('image/png'), sideBySide: side.toDataURL('image/png') };
}

export async function captureFiles(directory) {
  const files = (await readdir(directory, { withFileTypes: true })).filter(entry => entry.isFile() && entry.name.endsWith('.png')).map(entry => entry.name).sort();
  let manifest;
  try { manifest = JSON.parse(await readFile(join(directory, 'manifest.json'), 'utf8')); } catch (error) { if (error.code !== 'ENOENT') throw error; }
  if (manifest) {
    if (!Array.isArray(manifest.cells) || !manifest.cells.length) throw new Error(`Empty or invalid capture manifest: ${directory}`);
    const names = new Set();
    for (const cell of manifest.cells) {
      if (cell.status !== 'ok' || !files.includes(cell.file) || names.has(cell.file)) throw new Error(`Incomplete or duplicate capture cell: ${cell.file}`);
      names.add(cell.file);
    }
    return [...names].sort();
  }
  if (!files.length) throw new Error(`No PNG captures in ${directory}`);
  return files;
}
const inside = (parent, child) => { const path = relative(parent, child); return !path || (path !== '..' && !path.startsWith(`..${sep}`) && !path.startsWith(sep)); };
export async function visualDiff({ baseline, candidate, out, threshold = .2, report = false }) {
  baseline = resolve(baseline); candidate = resolve(candidate); out = resolve(out);
  if (!Number.isFinite(threshold) || threshold < 0 || threshold > 100) throw new Error('Threshold must be a percentage from 0 to 100');
  if ([baseline, candidate].some(input => inside(input, out) || inside(out, input))) throw new Error('Output must be separate from capture directories');
  const [a, b] = await Promise.all([captureFiles(baseline), captureFiles(candidate)]);
  const missing = a.filter(file => !b.includes(file)), added = b.filter(file => !a.includes(file));
  for (const dir of ['baseline', 'candidate', 'diff', 'side-by-side']) await mkdir(join(out, dir), { recursive: true });
  const browser = await openChrome(), cells = [];
  try {
    for (const file of a.filter(file => b.includes(file))) {
      const urls = await Promise.all([baseline, candidate].map(async dir => `data:image/png;base64,${(await readFile(join(dir, file))).toString('base64')}`));
      const result = await browser.evaluate(`(${comparePixels.toString()})(...${JSON.stringify(urls)})`);
      const { diff, sideBySide, ...stats } = result;
      await Promise.all([
        copyFile(join(baseline, file), join(out, 'baseline', file)), copyFile(join(candidate, file), join(out, 'candidate', file)),
        writeFile(join(out, 'diff', file), Buffer.from(diff.split(',')[1], 'base64')),
        writeFile(join(out, 'side-by-side', file), Buffer.from(sideBySide.split(',')[1], 'base64')),
      ]);
      cells.push({ file, ...stats, exceedsThreshold: stats.mismatchPercentage > threshold });
    }
  } finally { await browser.close(); }
  cells.sort((a, b) => b.mismatchPercentage - a.mismatchPercentage || a.file.localeCompare(b.file));
  const totals = { baseline: a.length, candidate: b.length, compared: cells.length, changed: cells.filter(cell => cell.mismatchPercentage > 0).length, exceeded: cells.filter(cell => cell.exceedsThreshold).length, missing: missing.length, added: added.length };
  const failed = totals.exceeded > 0 || missing.length > 0 || added.length > 0;
  const result = { baseline, candidate, threshold, channelTolerance: 16, totals, missing, added, cells, failed };
  await writeFile(join(out, 'diff.json'), JSON.stringify(result, null, 2) + '\n');
  const list = files => files.length ? `<ul>${files.map(file => `<li>${escapeHtml(file)}</li>`).join('')}</ul>` : '<p>None</p>';
  await writeFile(join(out, 'index.html'), `<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>Visual regression</title><style>body{font:14px system-ui;margin:24px;background:#f4f4f5;color:#18181b}article{margin:28px 0}section{display:flex;gap:12px}figure{margin:0;flex:1;min-width:0}img{width:100%;height:auto;border:1px solid #aaa}code{overflow-wrap:anywhere}.failed{color:#b91c1c}</style><h1>Visual regression</h1><p>${totals.compared} compared · ${totals.changed} changed · ${totals.exceeded} above ${threshold}% · ${missing.length} missing · ${added.length} new</p><p>Each channel tolerates differences up to 16. Pink pixels differ; the candidate is dimmed to 30%. Size changes count as 100%.</p><h2>Missing cells</h2>${list(missing)}<h2>New cells</h2>${list(added)}${cells.map(cell => `<article><h2 class="${cell.exceedsThreshold ? 'failed' : ''}"><code>${escapeHtml(cell.file)}</code> — ${cell.mismatchPercentage.toFixed(6)}%${cell.sizeMismatch ? ' (size mismatch)' : ''}</h2><section>${['baseline', 'candidate', 'diff'].map(kind => `<figure><figcaption>${kind}</figcaption><a href="${kind}/${encodeURIComponent(cell.file)}"><img loading="lazy" src="${kind}/${encodeURIComponent(cell.file)}" alt="${kind}: ${escapeHtml(cell.file)}"></a></figure>`).join('')}</section><a href="side-by-side/${encodeURIComponent(cell.file)}">Side-by-side</a></article>`).join('')}</html>`);
  return { ...result, exitCode: failed && !report ? 1 : 0 };
}

export async function main(args = process.argv.slice(2)) {
  const options = {};
  for (let i = 0; i < args.length; i++) {
    const key = args[i];
    if (key === '--report') options.report = true;
    else if (['--baseline', '--candidate', '--out', '--threshold'].includes(key) && args[i + 1] && !args[i + 1].startsWith('--')) options[key.slice(2)] = args[++i];
    else throw new Error(`Unknown or incomplete option: ${key}`);
  }
  if (!options.baseline || !options.candidate || !options.out) throw new Error('Usage: visual-diff.mjs --baseline <dir> --candidate <dir> --out <dir> [--threshold 0.2] [--report]');
  const result = await visualDiff({ ...options, threshold: options.threshold === undefined ? .2 : Number(options.threshold) });
  console.log(JSON.stringify(result.totals)); console.log(`Visual report: ${resolve(options.out)}/index.html`);
  return result.exitCode;
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main().then(code => { process.exitCode = code; }).catch(error => { console.error(error.message); process.exitCode = 1; });
