import test from "node:test";
import assert from "node:assert/strict";
import { inspectCatalog, koreanNounsEndingInYo, validateAllowlist } from "./check-ui-copy.mjs";

const inspect = (value, locale = "en", entries = []) => inspectCatalog("catalog.json", JSON.stringify(value, null, 2), locale, "app", entries);

test("every English and Korean advice phrase is an error, case-insensitively for English", () => {
  const english = ["you should buy", "you should sell", "you should hold", "we recommend", "our recommendation", "buy now", "sell now", "guaranteed return", "guaranteed returns"];
  const korean = ["매수하세요", "매도하세요", "추천 종목", "수익 보장", "꼭 사세요", "꼭 파세요"];
  for (const [locale, values] of [["en", english], ["ko", korean]]) {
    for (const phrase of values) {
      const [hit] = inspect({ nested: { text: phrase.toUpperCase() } }, locale);
      assert.equal(hit.rule, "ui-copy/advice");
      assert.equal(hit.key, `${locale}.app.nested.text`);
      assert.equal(hit.allowed, false);
      assert.equal(hit.line, 3);
    }
  }
});

test("jargon is matched in values, including arrays, but not key names or larger words", () => {
  const terms = ["revision id", "digest", "receipt", "envelope", "byte offset", "span selector", "worker", "sidecar", "stdio", "JSON-RPC"];
  assert.equal(inspect({ values: terms.map((term) => term.toUpperCase()) }).length, terms.length);
  assert.deepEqual(inspect({ worker: "workership and receipts are outside the exact pattern", digest: "Research overview" }), []);
  assert.equal(inspect({ text: "worker worker" }).length, 2);
});

test("justified allowlists suppress only an exact locale, namespace, key and jargon term", () => {
  const entries = [{ key: "en.app.label", term: "WORKER", reason: "Fixture technical label" }];
  assert.equal(inspect({ label: "Worker" }, "en", entries)[0].allowed, true);
  assert.equal(inspect({ other: "Worker" }, "en", entries)[0].allowed, false);
  assert.equal(inspect({ label: "Worker" }, "ko", entries)[0].allowed, false);
  const hits = inspect({ label: "Worker: we recommend" }, "en", entries);
  assert.equal(hits.find((hit) => hit.rule === "ui-copy/advice").allowed, false);
  assert.throws(() => validateAllowlist([{ key: "en.app.label", term: "we recommend", reason: "No bypass" }]));
  assert.throws(() => validateAllowlist([{ key: "en.app.label", term: "worker", reason: " " }]));
  assert.throws(() => inspectCatalog("broken.json", "{broken}", "en", "app"));
});

test("Korean copy puts no single bare particle after a placeholder and the right one after the app name", () => {
  const wrong = ["루피스트이 최신 버전입니다", "루피스트을 열지 못했습니다", "{{name}}을 삭제하시겠습니까?", "{{loupist}}가 확인합니다", "{{provider}}로 로그인", "“{{title}}”을 바꿔 줘", "{{date}}이 끝날 때까지"];
  for (const value of wrong) {
    assert.ok(inspect({ text: value }, "ko").some((hit) => hit.rule === "ui-copy/particle" && !hit.allowed), value);
  }
  const right = ["루피스트가 최신 버전입니다", "{{name}} 스킬을 삭제하시겠습니까?", "{{label}}을(를) 삭제하시겠습니까?", "버전 {{to}}(으)로 추가", "“{{title}}”을(를) 바꿔 줘", "{{loupist}}에서 확인합니다", "{{date}}까지", "{{nearest}}인데", "‘깨짐’으로 표시"];
  for (const value of right) assert.deepEqual(inspect({ text: value }, "ko"), [], value);
  assert.deepEqual(inspect({ text: "{{name}}을" }, "en"), []);
});

test("Korean copy ends no sentence in a 해요체 요 form, but a noun ending in 요 is allowed", () => {
  const casual = ["아직 알림이 없어요", "다시 시도해 주세요.", "연결을 확인했어요", "삭제할까요?", "최신 버전이에요",
    "{{count}}개예요", "준비됐네요", "그렇죠", "새로 시작하세요", "확인 중이에요 · {{date}}", "<0>저장했어요</0> 다음", "“완료”라고 해요”"];
  for (const value of casual) {
    const hits = inspect({ text: value }, "ko").filter((hit) => hit.rule === "ui-copy/register");
    assert.equal(hits.length, 1, value);
    assert.equal(hits[0].allowed, false, value);
  }
  assert.equal(inspect({ text: "저장했어요. 다시 시도해 주세요." }, "ko").length, 2);
  const formal = ["아직 알림이 없습니다", "다시 시도하십시오.", "삭제하시겠습니까?", "확인 중", "요약", "월요일 오전 8시", "요청 보내기",
    "확인 필요", "불필요한 항목", "중요", "주요 일정", "개요", "수요와 공급", "소요 시간", "“NVDA 관심 자산에 추가해 줘”"];
  for (const value of formal) assert.deepEqual(inspect({ text: value }, "ko"), [], value);
  for (const noun of koreanNounsEndingInYo) assert.deepEqual(inspect({ text: `${noun}.` }, "ko"), [], noun);
  assert.deepEqual(inspect({ text: "없어요" }, "en"), []);
});
