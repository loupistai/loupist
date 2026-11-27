import assert from 'node:assert/strict';
import { readFile, readdir } from 'node:fs/promises';
import test from 'node:test';

const directory = new URL('./lens/', import.meta.url);
const fixtures = [];
for (const name of (await readdir(directory)).filter((file) => /^research-.*\.json$/u.test(file)).sort()) {
  fixtures.push({ name, json: JSON.parse(await readFile(new URL(name, directory), 'utf8')) });
}
const texts = (fixture) => [...fixture.training, fixture.probes.applicable].map((step) => step.attachments[0].text);

test('every method-learning fixture has the shape the research runner reads, and nothing else', () => {
  assert.deepEqual(fixtures.map(({ name }) => name), ['research-macro-v1.json', 'research-theme-v1.json', 'research-v1.json']);
  for (const { name, json } of fixtures) {
    assert.deepEqual(Object.keys(json), ['schema', 'loupist', 'training', 'probes'], name);
    assert.equal(json.schema, 'loupist-lens-research/1', name);
    assert.deepEqual(Object.keys(json.loupist), ['name', 'focus', 'instructions'], name);
    assert.deepEqual(Object.keys(json.probes), ['applicable', 'unrelated'], name);
    // Two originals taught in two Chats, a third subject that the method fits, and a question it does not.
    assert.equal(json.training.length, 2, name);
    const steps = [...json.training, json.probes.applicable, json.probes.unrelated];
    assert.equal(new Set(steps.map((step) => step.id)).size, 4, name);
    for (const step of steps) {
      assert.deepEqual(Object.keys(step), step === json.probes.unrelated ? ['id', 'user'] : ['id', 'user', 'attachments'], `${name} ${step.id}`);
      assert.ok(step.user.trim().length > 40, `${name} ${step.id}`);
      if (step === json.probes.unrelated) continue;
      assert.equal(step.attachments.length, 1, `${name} ${step.id}`);
      assert.deepEqual(Object.keys(step.attachments[0]), ['filename', 'media_type', 'text'], `${name} ${step.id}`);
      assert.equal(step.attachments[0].media_type, 'text/markdown');
      assert.match(step.attachments[0].text, /fictional[\s\S]*created for a Loupist evaluation/u, `${name} ${step.id}`);
      // The question never states a figure the report holds: the answer has to come from the attachment.
      assert.doesNotMatch(step.user, /https?:/u);
    }
    assert.equal(new Set(steps.filter((step) => step.attachments).map((step) => step.attachments[0].filename)).size, 3, name);
  }
});

test('the macro and theme fixtures teach from subjects that are not companies', () => {
  const macro = fixtures.find(({ name }) => name === 'research-macro-v1.json').json;
  const theme = fixtures.find(({ name }) => name === 'research-theme-v1.json').json;
  for (const fixture of [macro, theme]) {
    for (const text of texts(fixture)) {
      // No issuer: a statistics office, an association or an agency publishes each report, about a place or a build-out.
      assert.doesNotMatch(text, /\b(?:plc|Inc\.|Ltd|Limited|S\.A\.|ASA|GmbH|AG|Corp\.)\b/u);
    }
  }
  // A macro series: three countries' consumer price releases, each with a one-off that flatters the headline rate.
  for (const text of texts(macro)) assert.match(text, /from a year earlier[\s\S]*(?:excluding|Underlying)/u);
  assert.deepEqual(texts(macro).map((text) => /^# (\w+)/u.exec(text)[1]), ['Veldmark', 'Tarsis', 'Corran']);
  // A theme: three build-outs, each with an announced pipeline far above what operates.
  for (const text of texts(theme)) assert.match(text, /announced[\s\S]*(?:operating|in commercial operation|built)/iu);
  assert.equal(new Set(texts(theme).map((text) => /^# ([^:]+):/u.exec(text)[1])).size, 3);
  // The unrelated probe is from another field, so applying the learned method to it is a failure the run can see.
  assert.match(macro.probes.unrelated.user, /modified duration/u);
  assert.match(theme.probes.unrelated.user, /like-for-like/u);
});
