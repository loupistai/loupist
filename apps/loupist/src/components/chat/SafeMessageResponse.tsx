import {
  Component,
  lazy,
  Suspense,
  type ErrorInfo,
  type ReactNode,
  useEffect,
  useRef,
  useState,
} from "react";
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
  /** Draw a caret after the last block; only while animating. */
  caret?: boolean;
  numberChecks?: import("../../types").NumberCheck[];
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

export function SafeMessageResponse({ text, isAnimating, caret = false, numberChecks, workspacePath, workspaceBasePath }: Props) {
  // A cold instance must keep its Lazy type after the module becomes warm.
  // Switching to the resolved function remounts citations, losing focus and a
  // press already in progress when unrelated chat state next renders.
  const MessageResponse = useRef(readyRenderer ?? LazyMessageResponse).current;
  const fallback = <p className="message-response-fallback">{plainResponseText(text)}</p>;
  const mermaidPlugin = useDeferredMermaidPlugin(text, isAnimating);

  return (
    <MessageRenderBoundary fallback={fallback}>
      <Suspense fallback={fallback}>
        <MessageResponse
          isAnimating={isAnimating}
          caret={caret && isAnimating ? "block" : undefined}
          mermaidPlugin={mermaidPlugin}
          numberChecks={numberChecks}
          workspacePath={workspacePath}
          workspaceBasePath={workspaceBasePath}
        >
          {text}
        </MessageResponse>
      </Suspense>
    </MessageRenderBoundary>
  );
}
