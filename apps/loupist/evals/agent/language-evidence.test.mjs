import test from 'node:test';
import assert from 'node:assert/strict';
import { ownWords, answerLanguageEvidence } from './language-evidence.mjs';
import { answerLanguageAssertion, validateSuite } from './eval-contract.mjs';

test('the share counts only the answer\'s own words', () => {
  const words = ownWords([
    '[매출](#evidence-ev_1) rose, see `코드` and https://fictional.example/가나다.',
    '```\n한국어 코드\n```',
    'It said "현금흐름" and 「한국어」 (삼성전자 (우선주)) for NVDA, FY2026 and $35.1B.',
    '> 인용한 한국어 문장',
  ].join('\n\n'));
  assert.match(words, /매출/u);
  for (const left of ['evidence', '코드', 'fictional', '현금흐름', '한국어', '삼성전자', '우선주', 'NVDA', 'FY2026', '35.1B', '인용']) {
    assert.equal(words.includes(left), false, left);
  }
});

test('English must stay below the low limit and Korean above the high one', () => {
  // Three Hangul syllables and seven Latin letters: a share of exactly 0.3.
  const atLow = '가나다 abcdefg';
  assert.equal(answerLanguageEvidence({ language: 'en' }, atLow).pass, false);
  assert.equal(answerLanguageEvidence({ language: 'ko' }, atLow).pass, false);
  // Seven Hangul syllables and thirteen Latin letters: a share of exactly 0.35.
  const atHigh = '가나다라마바사 abcdefghijklm';
  assert.equal(answerLanguageEvidence({ language: 'ko' }, atHigh).pass, false);
  assert.equal(answerLanguageEvidence({ language: 'ko' }, '가나다라마바사아 abcdefghijklm').pass, true);
  for (const answer of ['', '  \n', '12.5% · 2026-11-17']) {
    for (const language of ['en', 'ko']) assert.equal(answerLanguageEvidence({ language }, answer).pass, false, `${language} ${JSON.stringify(answer)}`);
  }
});

test('one-word answers, single quotes and apostrophes inside quotations are read as a reader would', () => {
  const korean = '한결해운은 2026년 3분기에 화물 1,800만 톤을 운송해 1년 전 같은 분기의 1,600만 톤보다 늘었고 영업이익률은 12%였다';
  const judged = (answer) => [answerLanguageEvidence({ language: 'en' }, answer).pass, answerLanguageEvidence({ language: 'ko' }, answer).pass];
  const english = [
    // A one-word answer that is all capitals.
    'OK', 'AI', 'US', 'Yes', 'No', 'Not sure',
    // Korean in ASCII single quotes, and a curly quotation holding an apostrophe.
    "The note says '한결해운은 화물 1,800만 톤을 운송했다.' That is 18 million tonnes.",
    `The Source reads '${korean}'. In short, volumes rose.`,
    `He said ‘the company’s ${korean}’ and the rest of the call covered costs.`,
    // Controls the recheck confirmed.
    '| Ticker | Close |\n| --- | --- |\n| NVDA | 156.14 |', `The filing says 「${korean}」 and margins held.`,
    `The note is below.\n\n> ${korean}\n\nVolumes rose.`, "The company's margin held, and the company’s guidance did too.",
  ];
  const koreanAnswers = [
    '네', '모름', '잘 모르겠어요', '| 종목 | 종가 |\n| --- | --- |\n| NVDA | 156.14 |',
    '보고서는 "Revenue grew strongly across every segment, led by data center demand and new products" 라고 적고 있습니다.',
  ];
  for (const answer of english) assert.deepEqual(judged(answer), [true, false], answer);
  for (const answer of koreanAnswers) assert.deepEqual(judged(answer), [false, true], answer);
  // No prose, or prose written in a third language.
  for (const answer of ['| NVDA | 156.14 |\n| AAPL | 352.78 |', '売上高は前年比で増加しました。']) assert.deepEqual(judged(answer), [false, false], answer);
});

test('answer_language accepts BCP 47 answer languages independently of appearance', () => {
  assert.equal(answerLanguageAssertion({ language: 'ko' }), 'ko');
  assert.equal(answerLanguageAssertion({ language: 'es' }), 'es');
  assert.equal(answerLanguageAssertion({ language: 'ja-jp' }), 'ja-JP');
  assert.throws(() => answerLanguageAssertion({ language: 'xx' }), /known script/u);
  assert.throws(() => answerLanguageAssertion({ language: 'en-Abcd' }), /known script/u);
  assert.throws(() => answerLanguageAssertion({}), /known script/u);
  const suite = { schema: 'loupist-agent-suite/1', suite: 'language', scenarios: [{ id: 'X', sessions: [{ id: 'bot', role: 'probe', memory: 'off',
    turns: [{ user: 'What is a margin?', expect: { answer: [{ assert: 'answer_language', language: 'fr', metric: 'answer_language' }] } }] }] }] };
  assert.doesNotThrow(() => validateSuite(suite));
});

test('screen label letters do not count toward the language of an explanation', () => {
  for (const answer of ['- 사이드바 제목: **Research sessions**', '- 제목: Research sessions입니다', '- 예약: **Schedule**']) {
    assert.equal(answerLanguageEvidence({ language: 'ko', per_paragraph: true }, answer, ['Research sessions', 'Schedule']).pass, true, answer);
  }
  for (const answer of ['- Heading: **리서치 세션**', '- Map: 지도']) {
    assert.equal(answerLanguageEvidence({ language: 'en', per_paragraph: true }, answer, ['리서치 세션', '지도']).pass, true, answer);
  }
  assert.equal(answerLanguageEvidence({ language: 'ko', per_paragraph: true }, '**Research sessions**', ['Research sessions']).pass, false);
  assert.equal(answerLanguageEvidence({ language: 'en', per_paragraph: true }, 'The Maple office is open.', ['Map']).pass, true);
});

test('a heading or list item in the wrong language fails the per-paragraph check', () => {
  const check = { language: 'ko', per_paragraph: true };
  const prose = '영업이익이 늘었습니다.';
  for (const heading of ['## The quarterly results', 'The quarterly results\n---']) {
    assert.equal(answerLanguageEvidence(check, `${heading}\n\n${prose}`).pass, false, heading);
  }
  for (const list of ['- The quarterly results', '1. The quarterly results', '- 주요 내용\n  - The quarterly results']) {
    assert.equal(answerLanguageEvidence(check, `${prose}\n\n${list}`).pass, false, list);
  }
  assert.equal(answerLanguageEvidence(check, `## **Research sessions**\n\n- ${prose}`, ['Research sessions']).pass, true);
  assert.equal(answerLanguageEvidence(check, '## 분기 실적', []).pass, true);
});

test('answer_language distinguishes shared and different scripts independent of appearance', () => {
  const samples = {
    es: 'Los ingresos crecieron un 12% en el trimestre.',
    en: 'The company reported higher revenue for the quarter.',
    ja: 'Alder Railの売上高は前年同期より増加しました。',
    zh: '本季度营业收入同比上升。',
    ru: 'Доходы выросли за последний квартал.',
    uk: 'У цьому кварталі виручка зросла порівняно з минулим роком.',
    ar: 'ارتفعت الإيرادات خلال الربع الأخير مقارنة بالفترة السابقة.',
  };
  assert.equal(answerLanguageEvidence({ language: 'en' }, samples.es).pass, false);
  for (const [expected, text] of Object.entries(samples)) {
    for (const language of Object.keys(samples)) {
      assert.equal(answerLanguageEvidence({ language }, text).pass, language === expected, `${expected} checked as ${language}`);
    }
  }
  assert.equal(answerLanguageEvidence({ language: 'en', per_paragraph: true }, `## ${samples.es}\n\n${samples.en}`).pass, false);
  assert.equal(answerLanguageEvidence({ language: 'km' }, 'ប្រាក់ចំណូលបានកើនឡើងនៅត្រីមាសនេះ។').pass, null);
  assert.equal(answerLanguageEvidence({ language: 'km' }, samples.en).pass, false);
  assert.equal(answerLanguageEvidence({ language: 'ja-JP', per_paragraph: true }, `- **Schedule**: ${samples.ja}`, ['Schedule']).pass, true);
});
