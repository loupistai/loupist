import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const cases = JSON.parse(await readFile(new URL('./figure-corpus-v1.json', import.meta.url), 'utf8'));
const VERDICTS = ['matched', 'rounded', 'differs', 'not_found', 'unchecked'];
// What marks each market profile's own number format in its cases.
const PROFILES = {
  us: /\$[\d,.]+ (?:million|billion)/u,
  korea: /[\d,]+억 원|\d조/u,
  japan: /[\d,]+億|[\d,]+万円/u,
  germany: /\d,\d+ (?:Mio|Mrd)\. EUR/u,
  united_kingdom: /£[\d,.]+|[\d,.]+p\b|GBX/u,
  india: /₹[\d,.]+ (?:crore|lakh)/u,
  brazil: /R\$ [\d.]+,\d+ (?:milhões|bilhão)/u,
  hong_kong_china: /[\d.,]+(?:亿|万)/u,
};
const of = (profile, expect) => cases.filter((item) => item.profile === profile && (!expect || item.expect === expect));

test('the figure corpus is a table of five string fields a Rust test can read as it is', () => {
  assert.ok(Array.isArray(cases) && cases.length >= 64);
  for (const item of cases) {
    assert.deepEqual(Object.keys(item), ['id', 'profile', 'passage', 'proposition', 'expect'], item.id);
    for (const value of Object.values(item)) assert.ok(typeof value === 'string' && value.trim() === value && value, item.id);
    assert.ok(VERDICTS.includes(item.expect), `${item.id} ${item.expect}`);
    assert.match(item.id, /^F-[A-Z]{2}-\d{2}$/u);
  }
  assert.equal(new Set(cases.map((item) => item.id)).size, cases.length);
  assert.equal(new Set(cases.map((item) => `${item.passage}\u0000${item.proposition}`)).size, cases.length);
});

test('each of the eight market profiles has eight cases in its own format, and no profile is the default', () => {
  assert.deepEqual([...new Set(cases.map((item) => item.profile))].sort(), Object.keys(PROFILES).sort());
  for (const [profile, format] of Object.entries(PROFILES)) {
    assert.ok(of(profile).length >= 8, profile);
    assert.ok(of(profile).filter((item) => format.test(item.passage)).length >= 4, `${profile} passages in the profile's format`);
    // Every profile carries both planted errors, a rounding and a derived figure.
    assert.ok(of(profile, 'differs').length >= 2, `${profile} planted errors`);
    assert.ok(of(profile, 'rounded').length >= 1, `${profile} rounded`);
    assert.ok(of(profile, 'not_found').length >= 1, `${profile} derived figure`);
    assert.ok(of(profile, 'matched').length >= 2, `${profile} matched`);
  }
  assert.equal(Math.max(...Object.keys(PROFILES).map((profile) => of(profile).length)), Math.min(...Object.keys(PROFILES).map((profile) => of(profile).length)));
});

test('the planted errors and the controls the check is specified against are present', () => {
  const by = (id) => cases.find((item) => item.id === id);
  // A wrong power of ten, and a flipped sign stated by a direction word or an explicit sign.
  assert.deepEqual([by('F-KR-03').expect, by('F-KR-02').expect], ['differs', 'matched']);
  assert.match(by('F-KR-03').proposition, /KRW 12\.34 billion/u);
  assert.match(by('F-KR-02').proposition, /KRW 123\.4 billion/u);
  assert.match(by('F-BR-07').passage, /-18,3%/u);
  assert.match(by('F-BR-07').proposition, /\+18\.3%/u);
  // Pence against pounds, lakh and crore against million, a decimal comma, a parenthesized negative.
  assert.deepEqual([by('F-UK-02').expect, by('F-UK-04').expect], ['matched', 'differs']);
  assert.deepEqual([by('F-IN-02').expect, by('F-IN-03').expect, by('F-IN-04').expect], ['matched', 'matched', 'differs']);
  assert.deepEqual([by('F-DE-02').expect, by('F-DE-05').expect], ['matched', 'differs']);
  assert.match(by('F-US-07').passage, /\(45\.2\)/u);
  assert.equal(by('F-US-07').expect, 'matched');
  // A percentage is not a change in percentage points, and a derived figure is not in the passage.
  assert.match(by('F-US-08').passage, /2\.3 percentage points/u);
  assert.equal(by('F-US-08').expect, 'not_found');
  // A fact with no amount, and a scale word outside the recognized formats, have no verdict.
  assert.doesNotMatch(by('F-IN-07').proposition.replace(/\b1 October 2026\b/u, ''), /\d/u);
  assert.deepEqual(['F-IN-07', 'F-BR-08', 'F-JP-05', 'F-DE-08'].map((id) => by(id).expect), Array(4).fill('unchecked'));
});
