import i18n from "@/i18n";
import { displayPageTitle, displaySectionHeading } from "./hostDefaults";

afterEach(async () => { await i18n.changeLanguage("en"); });

it("shows the host-default perspective title and English default headings in the interface language", async () => {
  await i18n.changeLanguage("ko");
  expect(displayPageTitle("perspective", "Outlook")).toBe("관점");
  expect(displayPageTitle("perspective", "My edge")).toBe("My edge");
  expect(displayPageTitle("method", "Outlook")).toBe("Outlook");
  expect(displaySectionHeading("beliefs", "Beliefs")).toBe("신념");
  expect(displaySectionHeading("focus", "Focus")).toBe("관심 분야");
  expect(displaySectionHeading("open-questions", "Open questions")).toBe("열린 질문");
  expect(displaySectionHeading("beliefs", "What I think")).toBe("What I think");
  expect(displaySectionHeading("custom", "Custom")).toBe("Custom");
});

it("keeps the English words in English", () => {
  expect(displayPageTitle("perspective", "Outlook")).toBe("Outlook");
  expect(displaySectionHeading("value-chain", "Value chain")).toBe("Value chain");
});
