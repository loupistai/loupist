import i18n, { useTranslation } from "@/i18n";
import { PanelRightOpenIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { ChatArtifactRef } from "../../types";
import { useConversationScroll } from "../ai-elements/conversation";
import { useGo } from "../navigation/Open";

const t = i18n.getFixedT(null, "chat");

/**
 * Opens the revision this card shows in the side panel. A second press, while
 * that revision is already what the panel shows, moves focus there.
 */
export function OpenInPanelButton({ artifact }: { artifact: ChatArtifactRef }) {
  const go = useGo();
  const conversation = useConversationScroll();
  useTranslation("chat");
  if (!go) return null;
  const label = t("messageResponse.streamdown.openInSidePanel");
  return (
    <Button
      type="button"
      variant="ghost"
      size="icon-sm"
      className="open-in-panel"
      aria-label={label}
      title={label}
      onClick={() => {
        conversation?.holdScrollPosition();
        const panel = document.getElementById("workspace-panel");
        if (panel && panel.dataset.openArtifact === artifact.artifact_id && Number(panel.dataset.openRevision) === artifact.revision) {
          panel.focus({ preventScroll: true });
          return;
        }
        go({ kind: "artifact", artifact });
      }}
    >
      <PanelRightOpenIcon aria-hidden="true" />
    </Button>
  );
}
