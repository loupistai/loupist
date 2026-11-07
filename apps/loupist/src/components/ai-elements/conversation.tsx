import { cn } from "@/lib/utils";
import {
  createContext,
  type ComponentProps,
  type ReactNode,
  type UIEvent,
  useCallback,
  useContext,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";

type ConversationContextValue = {
  isAtBottom: boolean;
  scrollToBottom: () => void;
  /** Keeps the end in view after content grew, only while the reader follows it. */
  followGrowth: () => void;
  holdScrollPosition: () => void;
};

const ConversationContext = createContext<ConversationContextValue | null>(null);
export const useConversationScroll = () => useContext(ConversationContext);
const BOTTOM_TOLERANCE_PX = 2;

export type ConversationProps = ComponentProps<"div"> & {
  /** Controls below the scroll viewport, sharing its scroll state. */
  footer?: ReactNode;
  /** Empty-state content stays at the top when it overflows. */
  autoScroll?: boolean;
  /** Advances when rendered Chat content changes and should stay in view. */
  scrollRevision?: string | number;
  /** A newly submitted prompt returns the reader to the active conversation. */
  scrollResetKey?: string | number;
};

/**
 * A deliberately small scroll owner for high-frequency Chat updates.
 *
 * Resize callbacks only move the scroll position. React state changes only
 * when the user leaves or returns to the bottom, avoiding render feedback.
 */
/** The chat sub-header shows a rule and a fade only while the thread is scrolled under it. */
const markScrolled = (element: HTMLElement) => element.toggleAttribute("data-scrolled", element.scrollTop > 0);

export const Conversation = ({
  className,
  children,
  footer,
  onScroll,
  role = "log",
  autoScroll = true,
  scrollRevision,
  scrollResetKey,
  ...props
}: ConversationProps) => {
  const scrollRef = useRef<HTMLDivElement>(null);
  const lastScrollTopRef = useRef(0);
  const pinnedBottomRef = useRef(0);
  const lastScrollInputRef = useRef(-Infinity);
  const hadOverflowRef = useRef(false);
  const lastResetKeyRef = useRef(scrollResetKey);
  const isAtBottomRef = useRef(true);
  const [isAtBottom, setIsAtBottom] = useState(true);

  const updateIsAtBottom = useCallback((next: boolean) => {
    if (isAtBottomRef.current === next) return;
    isAtBottomRef.current = next;
    setIsAtBottom(next);
  }, []);

  const scrollToBottom = useCallback(() => {
    const element = scrollRef.current;
    if (!element || !autoScroll) return;
    element.scrollTop = element.scrollHeight;
    lastScrollTopRef.current = element.scrollTop;
    pinnedBottomRef.current = element.scrollHeight - element.clientHeight;
    markScrolled(element);
    updateIsAtBottom(true);
  }, [autoScroll, updateIsAtBottom]);
  const holdScrollPosition = useCallback(() => updateIsAtBottom(false), [updateIsAtBottom]);
  // Called from the layout effect of the content that grew, so the scroll lands
  // in the same commit; ResizeObserver would move it a frame after the paint.
  const followGrowth = useCallback(() => { if (isAtBottomRef.current) scrollToBottom(); }, [scrollToBottom]);
  const noteScrollInput = useCallback(() => { lastScrollInputRef.current = performance.now(); }, []);

  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    if (!autoScroll) {
      element.scrollTop = 0;
      lastScrollTopRef.current = 0;
      markScrolled(element);
      updateIsAtBottom(true);
      return;
    }
    const overflow =
      element.scrollHeight - element.clientHeight > BOTTOM_TOLERANCE_PX;
    hadOverflowRef.current = overflow;
    const submitted = lastResetKeyRef.current !== scrollResetKey;
    lastResetKeyRef.current = scrollResetKey;
    // Empty or short threads can inherit a stale "scrolled up" flag from the
    // previous conversation; without overflow there is nothing to catch up to.
    if (submitted || !overflow || isAtBottomRef.current) {
      scrollToBottom();
    }
  }, [autoScroll, scrollRevision, scrollResetKey, scrollToBottom, updateIsAtBottom]);

  useLayoutEffect(() => {
    const element = scrollRef.current;
    const content = element?.firstElementChild;
    if (!autoScroll || !element || !content || typeof ResizeObserver === "undefined") return;
    const followResize = () => {
      const overflow = element.scrollHeight - element.clientHeight > BOTTOM_TOLERANCE_PX;
      const wasShort = !hadOverflowRef.current;
      hadOverflowRef.current = overflow;
      if (isAtBottomRef.current || wasShort || !overflow) scrollToBottom();
    };
    // Images, tables and lazily loaded renderers can change height after the
    // commit that added them. Follow those changes while pinned.
    const observer = new ResizeObserver(followResize);
    observer.observe(element);
    observer.observe(content);
    // WKWebView can deliver the viewport resize without a ResizeObserver
    // callback for this flex child, so follow the window change as well.
    window.addEventListener("resize", followResize);
    return () => { observer.disconnect(); window.removeEventListener("resize", followResize); };
  }, [autoScroll, scrollToBottom]);

  const handleScroll = useCallback(
    (event: UIEvent<HTMLDivElement>) => {
      const element = event.currentTarget;
      const bottom = element.scrollHeight - element.clientHeight;
      const atBottom = bottom - element.scrollTop <= BOTTOM_TOLERANCE_PX;
      const movedUp = element.scrollTop < lastScrollTopRef.current;
      lastScrollTopRef.current = element.scrollTop;
      markScrolled(element);
      // A queued programmatic scroll event can arrive after more text has
      // grown. An unchanged offset is not a user scrolling away from the end.
      if (autoScroll && (atBottom || movedUp)) {
        // A Markdown table can shrink, clamp scrollTop upward, then grow
        // again before WebKit delivers the scroll event. Keep following
        // that layout change unless it coincides with a reader gesture.
        const reflow = bottom !== pinnedBottomRef.current;
        const readerGesture = performance.now() - lastScrollInputRef.current < 300;
        if (!atBottom && isAtBottomRef.current && reflow && !readerGesture) scrollToBottom();
        else updateIsAtBottom(atBottom);
      }
      onScroll?.(event);
    },
    [autoScroll, onScroll, scrollToBottom, updateIsAtBottom],
  );

  const context = useMemo(
    () => ({ isAtBottom, scrollToBottom, followGrowth, holdScrollPosition }),
    [isAtBottom, scrollToBottom, followGrowth, holdScrollPosition],
  );

  return (
    <ConversationContext.Provider value={context}>
      <div
        ref={scrollRef}
        className={cn("relative flex-1 overflow-y-auto", className)}
        onScroll={handleScroll}
        onWheelCapture={noteScrollInput}
        onTouchMoveCapture={noteScrollInput}
        onPointerMoveCapture={event => { if (event.buttons) noteScrollInput(); }}
        onKeyDownCapture={noteScrollInput}
        role={role}
        {...props}
      >
        {children}
      </div>
      {footer}
    </ConversationContext.Provider>
  );
};

export type ConversationContentProps = ComponentProps<"div">;

export const ConversationContent = ({
  className,
  ...props
}: ConversationContentProps) => (
  <div className={cn("flex flex-col gap-8 p-4", className)} {...props} />
);
