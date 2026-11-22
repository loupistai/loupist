import { useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import type { BasisRefView, KnowledgeCatalog, KnowledgeLocation } from "../../types";
import { compactOrigin, sourceOrigin, sourceTitle } from "../../knowledge/hostSource";
import { Disclosure } from "../ui/disclosure";

type SourceRef = Extract<BasisRefView, { kind: "span" }>;

export function SectionSources({ refs, catalog, loupistId, onOpen }: {
  refs: SourceRef[];
  catalog?: KnowledgeCatalog;
  loupistId: string;
  onOpen: (location: KnowledgeLocation) => void;
}) {
  const { t } = useTranslation("knowledge");
  if (!refs.length) return null;
  return <Disclosure className="message-sources" title={t("pageReader.sourcesCount", { formattedCount: formatNumber(refs.length) })}>
    <ol>{refs.map((ref, index) => {
      const record = catalog?.sources.find(item => item.recordId === ref.recordId);
      const source = record?.objects[0];
      const title = (source && sourceTitle(source)) || (record && sourceTitle(record)) || t("pageReader.chip.source");
      const origin = source && (source.origin?.name || compactOrigin(sourceOrigin(source)) || (source.userProvided && t("sourceWorkspace.userProvided")));
      const row = <><span className="source-number" aria-hidden="true">{formatNumber(index + 1)}</span>
        <span><strong>{title}</strong>{origin && <small>{origin}</small>}</span></>;
      return <li key={index}>{record ? <button type="button" role="link" aria-label={title} title={title}
        onClick={() => onOpen({ loupistId, surface: "source", recordId: record.recordId, revisionId: record.revisionId, objectId: source?.id,
          selection: { kind: "source", sourceId: source?.id, spanId: ref.spanId } })}>{row}</button>
        : <div className="source-row-static">{row}</div>}</li>;
    })}</ol>
  </Disclosure>;
}
