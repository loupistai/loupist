import { cn } from "@/lib/utils";
import {
  createContext,
  type ComponentProps,
  type ReactNode,
  type UIEvent,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";

/**
 * Anchored reading (DIRECTION 10.1). On send the new question glides to the
 * top of the viewport. While the reply runs, the page moves by itself only
 * when a block is appended (once, to make room for it), never while a block
 * grows and never at commit, and only until the reader scrolls. "Jump to
 * latest", End, or scrolling to the very end follows the reply instead.
 */
type ConversationActions = {
  /** Follow the end of the thread until the reader scrolls up. */
  jumpToLatest: () => void;
  /** The reader opened something in the thread: stop following, leave the page where it is. */
  holdScrollPosition: () => void;
  /** A new block was appended at the end of the running turn; make room for it once. */
  blockAppended: (block: HTMLElement, kind: "prose" | "card") => void;
};

type ConversationState = {
  /** The end of the latest turn is more than 12 px below the fold and the reader is not following. */
  away: boolean;
  /** Content continues below the fold. */
  moreBelow: boolean;
};

const ConversationContext = createContext<ConversationActions | null>(null);
const ConversationStateContext = createContext<ConversationState | null>(null);
export const useConversationScroll = () => useContext(ConversationContext);
export const useConversationState = () => useContext(ConversationStateContext);

/** How far below the top of the viewport the sent question settles. */
export const ANCHOR_TOP_PX = 16;
/** A question taller than this share of the viewport aligns its bottom here instead. */
const TALL_QUESTION = 0.4;
/** New prose gets at least the lower 60% of the viewport to write into. */
const PROSE_LINE = 0.4;
/** Room kept under a card or a question for the live line. */
const LIVE_LINE_ROOM_PX = 56;
const GLIDE_MS = 240;
/** Following moves this share of the remaining distance each frame. */
const FOLLOW_SHARE = 0.18;
const AWAY_PX = 12;
const BOTTOM_TOLERANCE_PX = 2;
/** A scroll event this soon after reader input is the reader's. */
const INPUT_WINDOW_MS = 300;

const reducedMotion = () => typeof window !== "undefined" && !!window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;

type Box = { top: number; bottom: number; height: number };

/** Where the scroll position goes on send: the question 16 px under the top, or a tall one's bottom at 40%. */
export function anchorDelta(question: Box, view: Box): number {
  if (question.height > view.height * TALL_QUESTION) return question.bottom - (view.top + view.height * TALL_QUESTION);
  return question.top - (view.top + ANCHOR_TOP_PX);
}

/**
 * How far to glide when a block is appended. Prose: so its top is no lower than
 * 40% of the viewport. A card or question: the least distance that shows it
 * whole with room for the live line, or its top 16 px under the top when taller.
 * Zero when it already fits; the page never moves up for a new block.
 */
export function roomDelta(kind: "prose" | "card", block: Box, view: Box): number {
  const delta = kind === "prose"
    ? block.top - (view.top + PROSE_LINE * view.height)
    : block.height + LIVE_LINE_ROOM_PX > view.height
      ? block.top - (view.top + ANCHOR_TOP_PX)
      : block.bottom + LIVE_LINE_ROOM_PX - view.bottom;
  return delta > 4 ? delta : 0;
}

/** The pill shows while the end of the turn is more than 12 px below the fold and the reader is not following. */
export const pillVisible = (endBelowFold: number, following: boolean) => endBelowFold > AWAY_PX && !following;

export type ConversationProps = ComponentProps<"div"> & {
  /** Controls drawn over the bottom of the thread, sharing its scroll state; they never take its height. */
  footer?: ReactNode;
  /** Empty-state content stays at the top when it overflows. */
  autoScroll?: boolean;
  /** Changes when the user sends: the new question glides to the top. */
  scrollResetKey?: string | number;
  /** The latest reply is in flight. */
  running?: boolean;
};

/** The chat sub-header shows a rule and a fade only while the thread is scrolled under it. */
const markScrolled = (element: HTMLElement) => element.toggleAttribute("data-scrolled", element.scrollTop > 0);

const latestTurn = (element: HTMLElement) => element.querySelector<HTMLElement>("[data-turn][data-turn-latest]");

/**
 * A block's box in the viewport from layout offsets, not its painted rectangle:
 * a question still rising in (translateY) must not shift where it settles.
 */
function layoutBox(block: HTMLElement, scroller: HTMLElement): Box {
  let top = 0;
  let node: HTMLElement | null = block;
  while (node && node !== scroller) {
    top += node.offsetTop;
    const parent = node.offsetParent as HTMLElement | null;
    if (!parent || !scroller.contains(parent) && parent !== scroller) break;
    node = parent;
  }
  const view = scroller.getBoundingClientRect();
  const height = block.offsetHeight;
  const at = view.top + top - scroller.scrollTop;
  return { top: at, bottom: at + height, height };
}

export const Conversation = ({
  className,
  children,
  footer,
  onScroll,
  role = "log",
  autoScroll = true,
  scrollResetKey,
  running = false,
  ...props
}: ConversationProps) => {
  const scrollRef = useRef<HTMLDivElement>(null);
  const mode = useRef<"bottom" | "anchored">("bottom");
  // The reader scrolled since the question was sent: blocks no longer make room.
  const touched = useRef(false);
  const following = useRef(false);
  const lastInput = useRef(-Infinity);
  const lastTop = useRef(0);
  const glide = useRef<number | undefined>(undefined);
  const follow = useRef<number | undefined>(undefined);
  const lastResetKey = useRef(scrollResetKey);
  const runningRef = useRef(running);
  runningRef.current = running;
  const [state, setState] = useState<ConversationState>({ away: false, moreBelow: false });
  const stateRef = useRef(state);

  const measure = useCallback(() => {
    const element = scrollRef.current;
    if (!element) return;
    const view = element.getBoundingClientRect();
    const footers = latestTurn(element)?.querySelectorAll<HTMLElement>(".turn-footer");
    const end = footers?.length ? footers[footers.length - 1]!.getBoundingClientRect().bottom - view.bottom
      : element.scrollHeight - element.clientHeight - element.scrollTop;
    const next = { away: pillVisible(end, following.current), moreBelow: end > AWAY_PX };
    if (next.away !== stateRef.current.away || next.moreBelow !== stateRef.current.moreBelow) {
      stateRef.current = next;
      setState(next);
    }
  }, []);

  const stopGlide = useCallback(() => {
    if (glide.current !== undefined) cancelAnimationFrame(glide.current);
    glide.current = undefined;
  }, []);

  const glideBy = useCallback((delta: number) => {
    const element = scrollRef.current;
    if (!element || !delta) return;
    stopGlide();
    const from = element.scrollTop;
    const to = Math.max(0, from + delta);
    if (reducedMotion() || typeof requestAnimationFrame !== "function") {
      element.scrollTop = to;
      lastTop.current = element.scrollTop;
      markScrolled(element);
      return;
    }
    const start = performance.now();
    const step = (now: number) => {
      const progress = Math.min(1, (now - start) / GLIDE_MS);
      const eased = 1 - (1 - progress) ** 3;
      element.scrollTop = from + (to - from) * eased;
      lastTop.current = element.scrollTop;
      markScrolled(element);
      glide.current = progress < 1 ? requestAnimationFrame(step) : undefined;
    };
    glide.current = requestAnimationFrame(step);
  }, [stopGlide]);

  /** Make sure the position exists before gliding to it: the latest turn grows its reserve. */
  const reserve = useCallback((delta: number) => {
    const element = scrollRef.current;
    const turn = element && latestTurn(element);
    if (!element || !turn) return;
    const missing = element.scrollTop + delta + element.clientHeight - element.scrollHeight;
    if (missing > 0) turn.style.minHeight = `${turn.getBoundingClientRect().height + missing}px`;
  }, []);

  const toBottom = useCallback(() => {
    const element = scrollRef.current;
    if (!element) return;
    element.scrollTop = element.scrollHeight;
    lastTop.current = element.scrollTop;
    markScrolled(element);
  }, []);

  const followFrame = useCallback(() => {
    follow.current = undefined;
    const element = scrollRef.current;
    if (!element || !following.current) return;
    const remaining = element.scrollHeight - element.clientHeight - element.scrollTop;
    if (remaining < 1) { measure(); return; }
    // WKWebView keeps scrollTop in whole pixels and drops a smaller step, which would stall a few pixels short.
    element.scrollTop = Math.ceil(element.scrollTop + (reducedMotion() ? remaining : Math.max(1, remaining * FOLLOW_SHARE)));
    lastTop.current = element.scrollTop;
    markScrolled(element);
    follow.current = requestAnimationFrame(followFrame);
  }, [measure]);

  const startFollowing = useCallback(() => {
    following.current = true;
    stopGlide();
    if (follow.current === undefined && typeof requestAnimationFrame === "function") follow.current = requestAnimationFrame(followFrame);
    measure();
  }, [followFrame, measure, stopGlide]);

  const stopFollowing = useCallback(() => {
    following.current = false;
    if (follow.current !== undefined) cancelAnimationFrame(follow.current);
    follow.current = undefined;
    measure();
  }, [measure]);

  const jumpToLatest = useCallback(() => {
    mode.current = "anchored";
    startFollowing();
  }, [startFollowing]);
  const holdScrollPosition = useCallback(() => {
    if (mode.current === "bottom") mode.current = "anchored";
    stopFollowing();
  }, [stopFollowing]);
  const blockAppended = useCallback((block: HTMLElement, kind: "prose" | "card") => {
    const element = scrollRef.current;
    if (!element || mode.current !== "anchored" || touched.current || following.current) return;
    const delta = roomDelta(kind, layoutBox(block, element), element.getBoundingClientRect());
    if (!delta) return;
    reserve(delta);
    glideBy(delta);
  }, [glideBy, reserve]);

  // The viewport's height, for the latest turn's reserve: one write per resize.
  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    const write = () => element.style.setProperty("--turn-viewport", `${element.clientHeight}px`);
    write();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(write);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  // Opening: a settled chat opens at its end; a chat whose reply is in flight opens with the latest question at the top.
  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    if (!autoScroll) {
      element.scrollTop = 0;
      markScrolled(element);
      return;
    }
    if (runningRef.current) {
      mode.current = "anchored";
      touched.current = true;
      const turn = latestTurn(element);
      const question = turn?.querySelector<HTMLElement>(".message.user");
      if (turn && question) {
        turn.style.setProperty("--turn-lead", `${layoutBox(question, element).top - layoutBox(turn, element).top}px`);
        element.scrollTop += anchorDelta(layoutBox(question, element), element.getBoundingClientRect());
        lastTop.current = element.scrollTop;
        markScrolled(element);
      }
    } else toBottom();
    measure();
  // eslint-disable-next-line react-hooks/exhaustive-deps -- only the first layout of this thread opens it.
  }, [autoScroll]);

  // Send: the new question glides to the top; the page then stays where it is.
  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (!element || lastResetKey.current === scrollResetKey) return;
    lastResetKey.current = scrollResetKey;
    if (!autoScroll) return;
    mode.current = "anchored";
    touched.current = false;
    stopFollowing();
    const turn = latestTurn(element);
    const question = turn?.querySelector<HTMLElement>(".message.user");
    if (!turn || !question) return;
    // A divider above the question is part of the turn; the reserve counts from the question.
    turn.style.setProperty("--turn-lead", `${layoutBox(question, element).top - layoutBox(turn, element).top}px`);
    const delta = anchorDelta(layoutBox(question, element), element.getBoundingClientRect());
    reserve(delta);
    glideBy(delta);
  }, [autoScroll, glideBy, reserve, scrollResetKey, stopFollowing]);

  // Images, tables and late renderers change height after their commit: a thread opened at its end stays there,
  // a followed reply keeps following, and anything else stays put.
  useEffect(() => {
    const element = scrollRef.current;
    const content = element?.firstElementChild;
    if (!autoScroll || !element || !content || typeof ResizeObserver === "undefined") return;
    const changed = () => {
      if (mode.current === "bottom") toBottom();
      else if (following.current && follow.current === undefined && typeof requestAnimationFrame === "function") follow.current = requestAnimationFrame(followFrame);
      measure();
    };
    const observer = new ResizeObserver(changed);
    observer.observe(element);
    observer.observe(content);
    // WKWebView can deliver the viewport resize without a ResizeObserver callback for this flex child.
    window.addEventListener("resize", changed);
    return () => { observer.disconnect(); window.removeEventListener("resize", changed); };
  }, [autoScroll, followFrame, measure, toBottom]);

  useEffect(() => () => {
    if (glide.current !== undefined) cancelAnimationFrame(glide.current);
    if (follow.current !== undefined) cancelAnimationFrame(follow.current);
  }, []);

  /** Any wheel, touch, key or scrollbar drag on the thread is the reader's. Upward input stops following. */
  const noteInput = useCallback((upward: boolean) => {
    lastInput.current = performance.now();
    touched.current = true;
    stopGlide();
    if (upward && following.current) stopFollowing();
  }, [stopFollowing, stopGlide]);

  const handleScroll = useCallback((event: UIEvent<HTMLDivElement>) => {
    const element = event.currentTarget;
    const movedUp = element.scrollTop < lastTop.current - 1;
    // Movement no glide or follow made (a scrollbar drag reports no other input) is the reader's too.
    if (Math.abs(element.scrollTop - lastTop.current) > 1 && glide.current === undefined && follow.current === undefined && mode.current === "anchored") {
      touched.current = true;
      if (movedUp) lastInput.current = performance.now();
    }
    lastTop.current = element.scrollTop;
    markScrolled(element);
    const atBottom = element.scrollHeight - element.clientHeight - element.scrollTop <= BOTTOM_TOLERANCE_PX;
    const reader = performance.now() - lastInput.current < INPUT_WINDOW_MS;
    if (reader && movedUp) {
      if (following.current) stopFollowing();
      if (mode.current === "bottom") mode.current = "anchored";
    }
    if (reader && atBottom && runningRef.current && !following.current) startFollowing();
    // A reader who leaves the end of a thread opened there is no longer pinned to it.
    if (mode.current === "bottom" && !atBottom && reader) mode.current = "anchored";
    measure();
    onScroll?.(event);
  }, [measure, onScroll, startFollowing, stopFollowing]);

  const actions = useMemo(() => ({ jumpToLatest, holdScrollPosition, blockAppended }), [jumpToLatest, holdScrollPosition, blockAppended]);

  return (
    <ConversationContext.Provider value={actions}>
      <ConversationStateContext.Provider value={state}>
      <div
        ref={scrollRef}
        className={cn("relative flex-1 overflow-y-auto", className)}
        onScroll={handleScroll}
        onWheelCapture={(event) => noteInput(event.deltaY < 0)}
        onTouchMoveCapture={() => noteInput(false)}
        onPointerMoveCapture={(event) => { if (event.buttons) noteInput(false); }}
        onKeyDownCapture={(event) => {
          if (event.key === "End" && runningRef.current) { touched.current = true; jumpToLatest(); return; }
          noteInput(["ArrowUp", "PageUp", "Home"].includes(event.key));
        }}
        role={role}
        {...props}
      >
        {children}
      </div>
      {footer ? <div className="conversation-dock">
        <div className="conversation-more-below" data-on={state.moreBelow || undefined} aria-hidden="true" />
        {footer}
      </div> : null}
      </ConversationStateContext.Provider>
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
