import {
  Component,
  lazy,
  Suspense,
  type ErrorInfo,
  type ReactNode,
  useEffect,
  useState,
} from "react";
import { usePacedText } from "../../chat/usePacedText";
import type { ChatEvidenceRef } from "../../types";
import type { DiagramPlugin } from "@streamdown/mermaid";
import { hasCompleteMermaidFence, loadMermaidPlugin } from "./mermaidPlugin";

type Renderer = typeof import("@/components/ai-elements/message-response").MessageResponse;
let readyRenderer: Renderer | undefined;
const loadMessageResponse = async () => {
  const { MessageResponse } = await import("@/components/ai-elements/message-response");
  readyRenderer = MessageResponse;
  return { default: MessageResponse };
};
const LazyMessageResponse = lazy(loadMessageResponse);

// A warm module can render synchronously, including a preamble shorter than
// React's first Suspense reveal. Cold loads still have a readable fallback.
if (typeof window !== "undefined") {
  const idle = window.requestIdleCallback ?? ((callback: () => void) => window.setTimeout(callback, 1));
  idle(() => { void loadMessageResponse().catch(() => undefined); });
}

/** A readable projection while the renderer loads or after it fails: no Markdown markers. */
export function plainResponseText(text: string): string {
  return text
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/(\*\*|__)(.+?)\1/g, "$2")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/^#{1,6}\s+/gm, "")
    .replace(/^>\s?/gm, "");
}

type BoundaryProps = {
  children: ReactNode;
  fallback: ReactNode;
};

type BoundaryState = {
  failed: boolean;
};

class MessageRenderBoundary extends Component<
  BoundaryProps,
  BoundaryState
> {
  state: BoundaryState = { failed: false };

  static getDerivedStateFromError(): BoundaryState {
    return { failed: true };
  }

  componentDidCatch(_error: Error, _info: ErrorInfo) {
    // The readable response projection remains visible below. Do not log
    // response content or renderer internals from this recovery boundary.
  }

  render() {
    return this.state.failed ? this.props.fallback : this.props.children;
  }
}

type Props = {
  text: string;
  isAnimating: boolean;
  onOpenLink: (url: string) => Promise<void> | void;
  onOpenFile?: (path: string) => void;
  onLensLink?: (recordId: string) => void;
  lensLinks?: import("../ai-elements/message-response").LensLinkActions;
  evidenceRefs?: ChatEvidenceRef[];
  memoryRefs?: import("../../types").MemoryRef[];
  onOpenEvidence?: (evidence: ChatEvidenceRef, anchor?: HTMLElement) => void;
  numberChecks?: import("../../types").NumberCheck[];
  asOf?: string;
  onOpenEvidenceCell?: (evidence: ChatEvidenceRef, cell?: import("./NumberChip").EvidenceCell) => void;
  onShowStep?: (evidence: ChatEvidenceRef) => void;
  workspacePath?: string;
  workspaceBasePath?: string;
};

function useDeferredMermaidPlugin(
  text: string,
  isAnimating: boolean,
): DiagramPlugin | undefined {
  const requiresMermaid = !isAnimating && hasCompleteMermaidFence(text);
  const [plugin, setPlugin] = useState<DiagramPlugin>();

  useEffect(() => {
    let disposed = false;
    if (!requiresMermaid) {
      setPlugin(undefined);
      return () => {
        disposed = true;
      };
    }

    void loadMermaidPlugin().then(
      (loaded) => {
        if (!disposed) setPlugin(loaded);
      },
      () => {
        if (!disposed) setPlugin(undefined);
      },
    );
    return () => {
      disposed = true;
    };
  }, [requiresMermaid]);

  return requiresMermaid ? plugin : undefined;
}

export function SafeMessageResponse({ text, isAnimating, onOpenLink, onOpenFile, onLensLink, lensLinks, evidenceRefs, memoryRefs, onOpenEvidence, numberChecks, asOf, onOpenEvidenceCell, onShowStep, workspacePath, workspaceBasePath }: Props) {
  const MessageResponse = readyRenderer ?? LazyMessageResponse;
  const visibleText = usePacedText(text, isAnimating);
  const fallback = <p className="message-response-fallback">{plainResponseText(visibleText)}</p>;
  const mermaidPlugin = useDeferredMermaidPlugin(text, isAnimating);

  return (
    <MessageRenderBoundary fallback={fallback}>
      <Suspense fallback={fallback}>
        <MessageResponse
          isAnimating={isAnimating}
          mermaidPlugin={mermaidPlugin}
          onOpenLink={onOpenLink}
          onOpenFile={onOpenFile}
          onLensLink={onLensLink}
          lensLinks={lensLinks}
          evidenceRefs={evidenceRefs}
          memoryRefs={memoryRefs}
          onOpenEvidence={onOpenEvidence}
          numberChecks={numberChecks}
          asOf={asOf}
          onOpenEvidenceCell={onOpenEvidenceCell}
          onShowStep={onShowStep}
          workspacePath={workspacePath}
          workspaceBasePath={workspaceBasePath}
        >
          {visibleText}
        </MessageResponse>
      </Suspense>
    </MessageRenderBoundary>
  );
}
