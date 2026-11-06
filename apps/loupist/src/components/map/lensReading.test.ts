import i18n from "@/i18n";
import type { KnowledgeMapNode, KnowledgeRecordSummary } from "../../contracts/knowledge";
import { lensDisplayTitle } from "./lensReading";

afterEach(async () => { await i18n.changeLanguage("en"); });

const node = (label: string) => ({ id: "n1", kind: "lens_perspective", label, recordId: "r1" }) as KnowledgeMapNode;
const lens = (title: string) => [{ recordId: "r1", title, lensKind: "perspective", isStanding: true }] as unknown as KnowledgeRecordSummary[];

it("labels the host-default perspective in the interface language and keeps a chosen title", async () => {
  await i18n.changeLanguage("ko");
  expect(lensDisplayTitle(node("Outlook"), lens("Outlook"))).toBe("관점");
  expect(lensDisplayTitle(node("My edge"), lens("My edge"))).toBe("My edge");
});
