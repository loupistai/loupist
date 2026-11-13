import i18n from "@/i18n";
import { exportTitle, titledExport } from "./exportTitle";

afterEach(async () => { await i18n.changeLanguage("en"); });

it("titles the Save dialog for every export kind in the interface language", async () => {
  expect(["png", "mp4", "webm", "md", "csv"].map((kind) => exportTitle(kind as Parameters<typeof exportTitle>[0])))
    .toEqual(["Save image", "Save video", "Save video", "Save Markdown", "Save CSV"]);
  await i18n.changeLanguage("ko");
  expect(["png", "mp4", "webm", "md", "csv"].map((kind) => exportTitle(kind as Parameters<typeof exportTitle>[0])))
    .toEqual(["이미지 저장", "동영상 저장", "동영상 저장", "마크다운 저장", "CSV 저장"]);
  expect(titledExport({ suggestedName: "표.csv", kind: "csv" as const, bytes: [1] })).toEqual({ suggestedName: "표.csv", kind: "csv", bytes: [1], title: "CSV 저장" });
});
