import i18n from "@/i18n";
import type { LoupistBridge } from "../contracts/bridge";
import type { ChatArtifactRef } from "../contracts/chat";
import type { ContextOption } from "./composerChips";
import { rankText } from "./composerChips";
import { sourceTitle } from "@/components/knowledge/hostSource";

const t = i18n.getFixedT(null, "chat");

const limit = 6;

export async function searchComposerContext(
  bridge: LoupistBridge,
  loupistId: string,
  threadId: string | null,
  query: string,
  connections: ReadonlyArray<{ id: string; name: string; description: string }>,
): Promise<ContextOption[]> {
  const options: ContextOption[] = [];
  const settled = await Promise.allSettled([
    bridge.workspaceList({ loupist_id: loupistId, path: "" }),
    bridge.knowledgeCatalog({ loupistId }),
    bridge.knowledgeLensSearch({ loupistId, query }),
    bridge.knowledgeResearchSearch({ loupistId, query: query.trim() || undefined }),
    threadId ? bridge.chatArtifactList(loupistId, threadId) : Promise.resolve([]),
  ]);
  const directory = settled[0].status === "fulfilled" ? settled[0].value : null;
  const catalog = settled[1].status === "fulfilled" ? settled[1].value : null;
  const lenses = settled[2].status === "fulfilled" ? settled[2].value : [];
  const research = settled[3].status === "fulfilled" ? settled[3].value.records : [];
  const artifacts = settled[4].status === "fulfilled" ? settled[4].value : [];

  for (const entry of directory?.entries ?? []) {
    if (entry.kind !== "file") continue;
    const score = rankText(query, [entry.name, entry.path]);
    if (!score) continue;
    options.push({ id: `file:${entry.path}`, group: "Files", label: entry.name, detail: entry.path, path: entry.path });
  }
  for (const lens of lenses) {
    const score = rankText(query, [lens.title, lens.snippet]);
    if (!score) continue;
    options.push({
      id: `lens:${lens.recordId}`,
      group: "Lens",
      label: lens.title,
      detail: lens.snippet,
      pin: { kind: "lens", loupistId, recordId: lens.recordId, revisionId: lens.revisionId, objectId: lens.recordId, label: lens.title },
    });
  }
  for (const source of catalog?.sources ?? []) {
    const title = sourceTitle(source);
    const score = rankText(query, [title, source.title, source.summary ?? ""]);
    if (!score) continue;
    options.push({
      id: `source:${source.recordId}`,
      group: "Sources",
      label: title,
      detail: source.summary ?? t("searchComposerContext.savedSource"),
      pin: {
        kind: "source",
        loupistId,
        recordId: source.recordId,
        revisionId: source.revisionId,
        objectId: source.objects[0]?.id ?? source.recordId,
        label: title,
      },
    });
  }
  const world = catalog?.worlds[0];
  if (world && query.trim()) {
    try {
      const graph = await bridge.knowledgeWorldQuery({
        loupistId,
        recordId: world.recordId,
        revisionId: world.revisionId,
        search: query,
        kinds: [],
        relations: [],
        bindingRoles: [],
        statuses: [],
        nodeLimit: 8,
        edgeLimit: 4,
        byteLimit: 65_536,
      });
      for (const node of graph.nodes.slice(0, 4)) {
        options.push({
          id: `world:${node.id}`,
          group: "World",
          label: node.name,
          detail: world.title,
          pin: { kind: "world_node", loupistId, recordId: world.recordId, revisionId: world.revisionId, objectId: node.id, label: node.name },
        });
      }
    } catch {
      // World search is optional context. The rest of the palette still works.
    }
  }
  for (const record of research) {
    const score = rankText(query, [record.title, record.summary ?? ""]);
    if (!score) continue;
    const journal = Boolean(record.journal);
    options.push({
      id: `research:${record.id}`,
      group: "Research",
      label: record.title,
      detail: journal ? t("searchComposerContext.savedJudgment") : t("searchComposerContext.savedWork"),
      pin: {
        kind: journal ? "journal" : "research",
        loupistId,
        objectId: journal ? record.journal?.sectionId ?? record.id : record.id,
        label: record.title,
      },
    });
  }
  for (const artifact of artifacts) {
    const score = rankText(query, [artifact.title, artifact.kind]);
    if (!score) continue;
    const ref: ChatArtifactRef = {
      artifact_id: artifact.id,
      revision: artifact.current_revision,
      kind: artifact.kind,
      title: artifact.title,
      digest: "",
    };
    options.push({
      id: `artifact:${artifact.id}`,
      group: "Documents",
      label: artifact.title,
      detail: artifact.kind === "chart" ? t("searchComposerContext.chart") : artifact.kind === "grid" ? t("searchComposerContext.table") : t("searchComposerContext.document"),
      artifact: ref,
    });
  }
  for (const connection of connections) {
    const score = rankText(query, [connection.name, connection.description, connection.id]);
    if (!score) continue;
    options.push({
      id: `connection:${connection.id}`,
      group: "Connections",
      label: connection.name,
      detail: connection.description,
      connectionId: connection.id,
    });
  }
  const groups = ["Files", "Lens", "World", "Sources", "Research", "Documents", "Connections"] as const;
  return groups.flatMap((group) => options.filter((option) => option.group === group).slice(0, limit));
};
