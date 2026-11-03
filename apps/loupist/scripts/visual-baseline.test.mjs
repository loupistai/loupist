import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { acceptBaseline, main } from './visual-baseline.mjs';
const image = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aD1sAAAAASUVORK5CYII=', 'base64');
test('accept requires review reason, copies provenance and archives previous acceptance', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'visual-baseline-test-'));
  try {
    const from = join(dir, 'captures'), root = join(dir, 'baselines'); await mkdir(from);
    await writeFile(join(from, 'cell.png'), image);
    await writeFile(join(from, 'manifest.json'), JSON.stringify({ gitHead: 'capture-commit', dirty: true, cells: [{ file: 'cell.png', status: 'ok' }] }));
    await writeFile(join(from, 'index.html'), 'capture sheet');
    const options = { from, root, name: 'phase-cd', reason: 'Reviewed color adjustment' };
    await assert.rejects(acceptBaseline({ ...options, reason: '  ' }), /reason/);
    await assert.rejects(acceptBaseline({ ...options, name: '../escape' }), /name/);
    await assert.rejects(main(['accept', '--from', from, '--name', 'test']), /reason/);
    const accepted = await acceptBaseline(options);
    assert.equal(accepted.cellCount, 1); assert.equal(accepted.commit, 'capture-commit');
    assert.deepEqual(await readFile(join(accepted.path, 'cell.png')), image);
    const metadata = await readFile(join(accepted.path, 'BASELINE.md'), 'utf8');
    assert.match(metadata, /Reviewed color adjustment/); assert.match(metadata, /Cell count: 1/); assert.match(metadata, /dirty checkout/);
    await assert.rejects(acceptBaseline(options), /--replace/);
    await acceptBaseline({ ...options, replace: true, reason: 'Second reviewed change' });
    assert.equal(await readFile(join(root, 'phase-cd.prev', 'BASELINE.md'), 'utf8'), metadata);
    await acceptBaseline({ ...options, replace: true, reason: 'Third reviewed change' });
    assert.match(await readFile(join(root, 'phase-cd.prev', 'BASELINE.md'), 'utf8'), /Second reviewed change/);
    assert.deepEqual((await readdir(root)).sort(), ['phase-cd', 'phase-cd.prev']);
    await writeFile(join(from, 'manifest.json'), JSON.stringify({ cells: [{ file: 'missing.png', status: 'error' }] }));
    await assert.rejects(acceptBaseline({ ...options, replace: true }), /Incomplete/);
    assert.match(await readFile(join(root, 'phase-cd', 'BASELINE.md'), 'utf8'), /Third reviewed change/);
  } finally { await rm(dir, { recursive: true, force: true }); }
});
