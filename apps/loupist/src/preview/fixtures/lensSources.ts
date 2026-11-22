import { prepareFixture, previewKo, revision, type PreviewBridge } from "../fixtures";

/** Four sections at the minimum window, including an untitled one and long, similarly named sources. */
export async function prepareLensSources(bridge: PreviewBridge) {
  await prepareFixture(bridge);
  const ko = previewKo();
  const publisher = ko ? "한울정밀 투자자 관계" : "Hanul Precision Investor Relations";
  const comparison = ko
    ? ["해외 수주와 생산 설비 가동률이 함께 올랐습니다.", "| 보고 분기 | 해외 사업 부문 신규 수주액 (백만 원) | 전체 생산 설비 가동률 (%) | 전년 동기 대비 수주액 증가율 (%) |"]
    : ["Overseas orders and capacity utilization both rose.", "| Reporting quarter | New overseas manufacturing orders (millions) | Production capacity utilization (%) | Year-over-year order growth (%) |"];
  const markdown = [comparison[0], "", comparison[1], "| --- | ---: | ---: | ---: |",
    "| 2025 Q1 | 1,120 | 71 | 4 |", "| 2025 Q2 | 1,180 | 74 | 5 |", "| 2025 Q3 | 1,230 | 76 | 7 |",
    "| 2025 Q4 | 1,310 | 78 | 8 |", "| 2026 Q1 | 1,360 | 81 | 21 |", "",
    ...(ko ? ["- 신규 수주가 늘었습니다.", "- 설비 가동률이 상승했습니다.", "- 수주 증가율은 전년 동기와 비교합니다."]
      : ["- New orders increased.", "- Capacity utilization rose.", "- Order growth compares the same quarter a year earlier."])].join("\n");
  const sources = Array.from({ length: 30 }, (_, index) => {
    const quarter = `${2019 + Math.floor(index / 4)} Q${index % 4 + 1}`;
    const title = ko ? `한울정밀 분기 실적 발표 자료 — ${quarter} 해외 사업 부문 신규 수주 및 생산 설비 가동률 보고서`
      : `Hanul Precision quarterly results — ${quarter} overseas orders and manufacturing capacity utilization report`;
    const recordId = `quarterly-results-${index}`;
    return { ...revision, kind: "source" as const, recordId, title, objects: [{ id: recordId, title, originLabel: publisher }] };
  });
  const catalog = bridge.knowledgeCatalog.bind(bridge);
  bridge.knowledgeCatalog = async request => {
    const result = await catalog(request);
    return { ...result, sources: [...result.sources, ...sources] };
  };
  const read = bridge.knowledgeLensRead.bind(bridge);
  bridge.knowledgeLensRead = async request => {
    const page = await read(request);
    const body = page.body;
    if (body.kind === "subject") {
      body.sections = [8, 1, 0, 30].map((count, index) => ({
        ...body.sections[index]!, heading: index === 1 ? "" : body.sections[index]!.heading,
        markdown: index === 0 ? markdown : body.sections[index]!.markdown,
        basis: { refs: sources.slice(0, count).map(source => ({ kind: "span", recordId: source.recordId, revisionDigest: source.revisionDigest, spanId: "orders" })) },
      }));
    }
    return page;
  };
  const readSource = bridge.knowledgeSourceRead.bind(bridge);
  bridge.knowledgeSourceRead = async request => {
    const source = sources.find(item => item.recordId === request.recordId);
    if (!source) return readSource(request);
    return { recordId: source.recordId, revisionId: source.revisionId, sourceId: source.objects[0]!.id, title: source.title, originLabel: publisher,
      sourceClass: "publisher_document", coverage: "included_excerpt", material: "extracted_text", verification: "verified_bytes",
      spans: [{ spanId: "orders", selector: "line:1", mediaType: "text/plain", text: ko ? "해외 수주가 늘었습니다." : "Overseas orders increased.", contextNote: "" }],
      selectedSpanId: "orders", publicationTime: null, acquiredAt: null, relatedClaims: [], processing: null, derivation: null };
  };
}
