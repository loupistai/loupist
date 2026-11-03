import i18n from "./index";
import { finalSound, resolveKoreanParticles } from "./koreanParticles";

afterEach(async () => { await i18n.changeLanguage("en"); });

it("picks the particle by the final consonant of a Hangul word", () => {
  expect(resolveKoreanParticles("가격을(를) 확인")).toBe("가격을 확인");
  expect(resolveKoreanParticles("공시을(를) 확인")).toBe("공시를 확인");
  expect(resolveKoreanParticles("한울정밀이(가) 넘었어요")).toBe("한울정밀이 넘었어요");
  expect(resolveKoreanParticles("루피스트이(가) 알아요")).toBe("루피스트가 알아요");
  expect(resolveKoreanParticles("약해짐은(는) · 깨짐과(와)")).toBe("약해짐은 · 깨짐과");
});

it("writes 로 after ㄹ and after a vowel, 으로 after any other final consonant", () => {
  expect(resolveKoreanParticles("서울(으)로")).toBe("서울로");
  expect(resolveKoreanParticles("아직 유효(으)로 되돌림")).toBe("아직 유효로 되돌림");
  // After a closing quote or bracket the particle is joined, so no line breaks before it.
  expect(resolveKoreanParticles("‘깨짐’(으)로 표시")).toBe("‘깨짐’\u2060으로 표시");
  expect(resolveKoreanParticles("분기보고서 (2026.09)을(를) 제출")).toBe("분기보고서 (2026.09)\u2060를 제출");
  expect(resolveKoreanParticles("약해짐으로(로)")).toBe("약해짐으로");
});

it("reads digits as Korean numbers", () => {
  expect(resolveKoreanParticles("1을(를)")).toBe("1을");
  expect(resolveKoreanParticles("2을(를)")).toBe("2를");
  expect(resolveKoreanParticles("3(으)로")).toBe("3으로");
  expect(resolveKoreanParticles("7(으)로")).toBe("7로");
  expect(resolveKoreanParticles("10이(가)")).toBe("10이");
  expect(resolveKoreanParticles("2,000이(가)")).toBe("2,000이");
  expect(resolveKoreanParticles("27.4%이(가)")).toBe("27.4%가");
});

it("follows how Korean readers say Latin names and acronyms", () => {
  expect(resolveKoreanParticles("Hanul Precision이(가)")).toBe("Hanul Precision이");
  expect(resolveKoreanParticles("Morrow Components을(를)")).toBe("Morrow Components를");
  expect(resolveKoreanParticles("Apple은(는)")).toBe("Apple은");
  expect(resolveKoreanParticles("Quality Compounder이(가)")).toBe("Quality Compounder가");
  expect(resolveKoreanParticles("KRX을(를) 켜세요")).toBe("KRX를 켜세요");
  expect(resolveKoreanParticles("OpenDART을(를)")).toBe("OpenDART를");
  expect(resolveKoreanParticles("IBM은(는)")).toBe("IBM은");
  expect(resolveKoreanParticles("SEC EDGAR을(를)")).toBe("SEC EDGAR를");
  expect(finalSound("Hanul")).toBe("rieul");
});

it("keeps the pair when the sound is unknown and leaves other parentheses alone", () => {
  expect(resolveKoreanParticles("★이(가)")).toBe("★이(가)");
  expect(resolveKoreanParticles("넘지 않음 · 최근 21% (10월 26일)")).toBe("넘지 않음 · 최근 21% (10월 26일)");
});

it("resolves catalog strings after interpolation in Korean", async () => {
  await i18n.changeLanguage("ko");
  expect(i18n.t("assets:noThesis.body", { name: "Morrow Components" })).toMatch(/^Morrow Components를 왜 지켜보는지/u);
  expect(i18n.t("chat:redLine.message", { name: "한울정밀", date: "9월 26일", claim: "고객 한 곳 매출 비중 25% 초과", value: "27.4%", factDate: "10월 26일" })).toMatch(/^한울정밀이 9월 26일에/u);
});
