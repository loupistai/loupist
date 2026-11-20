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
