#!/usr/bin/env node
import { execFileSync } from 'node:child_process';
import { copyFile, lstat, mkdir, mkdtemp, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { captureFiles } from './visual-diff.mjs';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const exists = async path => { try { await lstat(path); return true; } catch (error) { if (error.code === 'ENOENT') return false; throw error; } };
export async function acceptBaseline({ from, name, reason, replace = false, root = join(repoRoot, 'output/design/baselines') }) {
  if (!reason?.trim()) throw new Error('A non-empty --reason is required to accept a reviewed design change');
  if (!name || !/^[a-zA-Z0-9][a-zA-Z0-9_-]*$/.test(name)) throw new Error('Baseline name must contain only letters, numbers, underscores or hyphens');
  if (!from) throw new Error('--from is required');
  from = resolve(from); root = resolve(root);
  const target = join(root, name), previous = `${target}.prev`;
  if (from === target || from.startsWith(`${target}/`) || from === previous || from.startsWith(`${previous}/`)) throw new Error('Source must be separate from the baseline being replaced');
  if (await exists(target) && !replace) throw new Error(`Baseline ${name} exists; pass --replace to archive it as ${name}.prev`);
  const files = await captureFiles(from);
  let manifest;
  try { manifest = JSON.parse(await readFile(join(from, 'manifest.json'), 'utf8')); } catch (error) { if (error.code !== 'ENOENT') throw error; }
  const commit = manifest?.gitHead ?? execFileSync('git', ['rev-parse', 'HEAD'], { cwd: repoRoot, encoding: 'utf8' }).trim();
  await mkdir(root, { recursive: true });
  const staging = await mkdtemp(join(root, `.${name}-`));
  try {
    for (const file of files) await copyFile(join(from, file), join(staging, file));
    for (const file of ['manifest.json', 'index.html']) if (await exists(join(from, file))) await copyFile(join(from, file), join(staging, file));
    await writeFile(join(staging, 'BASELINE.md'), `# ${name}\n\n- Date: ${new Date().toISOString()}\n- Commit: ${commit}${manifest?.dirty ? ' (dirty checkout at capture)' : ''}\n- Cell count: ${files.length}\n- Source: ${from}\n\n## Reason\n\n${reason.trim()}\n`);
    if (await exists(target)) {
      // Only the immediately previous accepted baseline is retained.
      if (!replace) throw new Error(`Baseline ${name} was created concurrently; pass --replace`);
      await rm(previous, { recursive: true, force: true });
      await rename(target, previous);
    }
    try { await rename(staging, target); } catch (error) { if (!(await exists(target)) && await exists(previous)) await rename(previous, target); throw error; }
  } finally { await rm(staging, { recursive: true, force: true }); }
  return { path: target, cellCount: files.length, commit };
}
export async function main(args = process.argv.slice(2)) {
  if (args.shift() !== 'accept') throw new Error('Usage: visual-baseline.mjs accept --from <capture dir> --name <name> --reason "<text>" [--replace]');
  const options = {};
  for (let i = 0; i < args.length; i++) {
    if (args[i] === '--replace') options.replace = true;
    else if (['--from', '--name', '--reason'].includes(args[i]) && args[i + 1] && !args[i + 1].startsWith('--')) options[args[i].slice(2)] = args[++i];
    else throw new Error(`Unknown or incomplete option: ${args[i]}`);
  }
  const result = await acceptBaseline(options);
  console.log(`Accepted ${result.cellCount} cells: ${result.path}`);
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main().catch(error => { console.error(error.message); process.exitCode = 1; });
