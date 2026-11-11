import type { ChatArtifactRef, ChatArtifactView } from "../types";

export const chatArtifactRef = (view: ChatArtifactView): ChatArtifactRef => ({
  artifact_id: view.artifact.id,
  revision: view.revision.revision,
  kind: view.artifact.kind,
  title: view.artifact.title,
  digest: view.revision.digest,
});
