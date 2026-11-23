import i18n from "@/i18n";
import { displaySectionHeading, historySectionHeading } from "./hostDefaults";

afterEach(async () => { await i18n.changeLanguage("en"); });

it("shows English default headings in the interface language", async () => {
  await i18n.changeLanguage("ko");
  expect(displaySectionHeading("beliefs", "Beliefs")).toBe("신념");
  expect(displaySectionHeading("focus", "Focus")).toBe("관심 분야");
  expect(displaySectionHeading("open-questions", "Open questions")).toBe("열린 질문");
  expect(displaySectionHeading("beliefs", "What I think")).toBe("What I think");
  expect(displaySectionHeading("custom", "Custom")).toBe("Custom");
});

it("keeps English default headings in English", () => {
  expect(displaySectionHeading("value-chain", "Value chain")).toBe("Value chain");
});

it("names the sections the app generates in the interface language, and keeps written ones", async () => {
  await i18n.changeLanguage("ko");
  // A principle save records its statement; method learning writes four sections.
  expect(historySectionHeading({ key: "statement", sectionHeading: "Statement" })).toBe("원칙");
  expect(historySectionHeading({ key: "statement" })).toBe("원칙");
  expect([["use-when", "Use when"], ["steps", "Steps"], ["checks", "Checks"], ["limits", "Limits"]]
    .map(([key, heading]) => displaySectionHeading(key!, heading!))).toEqual(["적용할 때", "단계", "점검", "한계"]);
  expect(historySectionHeading({ key: "statement", sectionHeading: "P/E 비교 원칙" })).toBe("P/E 비교 원칙");
  expect(historySectionHeading({ key: "supply" })).toBe(i18n.t("knowledge:pageReader.unnamedSection"));
});
