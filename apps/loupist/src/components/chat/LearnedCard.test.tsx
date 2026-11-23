import { render, screen } from "@testing-library/react";
import i18n from "../../i18n";
import { LearnedCard } from "./LearnedCard";

it("keeps the separator before “new page” with the words after it, so a line never ends on it", () => {
  render(<LearnedCard id="learned" loupistId="loupist-a" digest={{ messageId: "answer-1", pages: [
    { recordId: "rec_a", title: "HBM", kind: "subject", created: true, outcomes: [{ opId: "op-1", outcome: "added", key: "supply" }] },
  ] }} />);
  const kind = screen.getByRole("group", { name: "Filed in this answer" }).querySelector(".learned-page-kind")!;
  expect(kind.textContent).toBe(`${i18n.t("knowledge:pageReader.kind.subject")} · new page`);
});

it("names a principle's statement in the interface language when the page has no section to read", async () => {
  await i18n.changeLanguage("ko");
  try {
    render(<LearnedCard id="learned" loupistId="loupist-a" digest={{ messageId: "answer-1", pages: [
      { recordId: "rec_p", title: "P/E를 비교하기 전에 기준일을 확인한다", kind: "principle", created: true, outcomes: [{ opId: "op-1", outcome: "added", key: "statement" }] },
      { recordId: "rec_m", title: "Capacity costs", kind: "method", created: false, outcomes: [{ opId: "op-2", outcome: "replaced", key: "use-when" }, { opId: "op-3", outcome: "added", key: "value-chain-2" }] },
    ] }} />);
    expect([...document.querySelectorAll(".learned-section strong")].map(item => item.textContent)).toEqual(["원칙", "적용할 때", "value-chain-2"]);
  } finally { await i18n.changeLanguage("en"); }
});
