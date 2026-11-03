import i18n from "../../i18n";
import { relationName } from "./relations";
it("translates curated relations and humanizes unknown vocabulary in every locale", () => {
  for (const locale of ["en", "ko"]) {
    const t = i18n.getFixedT(locale, "knowledge");
    expect(relationName("SUPPLIES", t)).toBe(locale === "ko" ? "공급" : "supplies");
    expect(relationName("depends_on", t)).toBe(locale === "ko" ? "의존" : "depends on");
    expect(relationName("RAISED_PRICES_FOR", t)).toBe("raised prices for");
  }
});
