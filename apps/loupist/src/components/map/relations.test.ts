import i18n, { resources } from "../../i18n";
import { relationKey, relationName } from "./relations";

const ko = i18n.getFixedT("ko", "knowledge");
const en = i18n.getFixedT("en", "knowledge");

it("labels reviewed types, then the bundled table, in each interface language", () => {
  expect(relationName("SUPPLIES", ko)).toBe("공급");
  expect(relationName("SUPPLIES", en)).toBe("supplies");
  expect(relationName("depends_on", ko)).toBe("의존");
  // Open relations the extraction model named itself, as a real World stores them.
  for (const [relation, korean, english] of [
    ["OPERATES", "운영", "operates"],
    ["MANUFACTURES_IN", "생산지", "manufactures in"],
    ["FILES_WITH", "공시 제출처", "files with"],
    ["LISTS_ON", "상장 거래소", "lists on"],
    ["PROVIDES_DATA_FOR", "데이터 제공", "provides data for"],
    ["PAYS_DIVIDENDS_TO", "배당 지급", "pays dividends to"],
    ["PROVIDES_FUNDING_TO", "자금 제공", "provides funding to"],
  ]) {
    expect(relationName(relation!, ko)).toBe(korean);
    expect(relationName(relation!, en)).toBe(english);
  }
});

it("shows a relation with no entry as lowercase English in every language", () => {
  for (const t of [en, ko]) {
    expect(relationName("RAISED_PRICES_FOR", t)).toBe("raised prices for");
    expect(relationName("raised_prices_for", t)).toBe("raised prices for");
    expect(relationName("raisedPricesFor", t)).toBe("raised prices for");
    expect(relationName("  Raised  Prices-For ", t)).toBe("raised prices for");
  }
});

it("reads every spelling of a name as its stored key, as the host normalizes it", () => {
  expect(relationKey("FILES_WITH")).toBe("FILES_WITH");
  expect(relationKey("files with")).toBe("FILES_WITH");
  expect(relationKey("filesWith")).toBe("FILES_WITH");
  expect(relationKey("__files--with__")).toBe("FILES_WITH");
  expect(relationKey("공급")).toBe("RELATES_TO");
  expect(relationName("manufactures in", ko)).toBe("생산지");
});

it("leaves reviewed types out of the table, whose keys are stored keys in every locale", () => {
  const reviewed = Object.keys(resources.en.knowledge.relations);
  const table = Object.keys(resources.en.knowledge.customRelations);
  expect(table.filter((key) => reviewed.includes(key))).toEqual([]);
  expect(table.filter((key) => relationKey(key) !== key)).toEqual([]);
  expect(Object.keys(resources.ko.knowledge.customRelations)).toEqual(table);
});
