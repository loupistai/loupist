import i18n from "@/i18n";
import { displaySectionHeading } from "./hostDefaults";

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
