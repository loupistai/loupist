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
 * when a block is appended (once, to make room for it), and only until the
 * reader scrolls. Reflow keeps the current reading block in place. "Jump to
 * latest", End, or scrolling to the very end keeps the end in view instead.
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
/** Ignore differences below a CSS pixel, including WebKit's scroll rounding. */
const differs = (a: number, b: number) => Math.abs(a - b) >= 1;
/** A scroll event this soon after reader input is the reader's. */
const INPUT_WINDOW_MS = 300;

const reducedMotion = () => typeof window !== "undefined" && !!window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;

type Box = { top: number; bottom: number; height: number };
type ReadingAnchor = { block: HTMLElement; top: number };
type Viewport = { top: number; width: number; height: number; extent: number };
const readViewport = (element: HTMLElement): Viewport => ({
  top: element.getBoundingClientRect().top,
  width: element.clientWidth,
  height: element.clientHeight,
  extent: element.scrollHeight,
});

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

/** The viewport's height, for the latest turn's reserve (chat.css `--turn-viewport`). */
const writeViewport = (element: HTMLElement, height: number) => element.style.setProperty("--turn-viewport", `${height}px`);

/**
 * Behind a full-page Map or Lens the thread is `display: none`: it has no box, every measure reads zero and the
 * browser may drop its scroll position. Nothing is measured or moved then; the reader's place comes back when it shows.
 */
const laidOut = (element: HTMLElement) => element.clientHeight > 0;

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

/**
 * Find a reading block within the first visible message. The enclosing boxes
 * can hold the place if a streaming block is replaced when the answer settles.
 */
function readingAnchors(scroller: HTMLElement): ReadingAnchor[] {
  const edge = scroller.getBoundingClientRect().top;
  const first = (nodes: Iterable<Element>) => {
    for (const node of nodes) {
      if (!(node instanceof HTMLElement) || node.matches(".thread-selection-end")) continue;
      const box = layoutBox(node, scroller);
      if (box.height > 0 && box.bottom > edge + 1) return node;
    }
  };
  const turn = first(scroller.firstElementChild?.children ?? []);
  const message = turn?.hasAttribute("data-turn") ? first(turn.children) : undefined;
  const block = message && first(message.querySelectorAll(
    ".message-content :is(p,h1,h2,h3,h4,h5,h6,li,pre,tr), [data-turn-item]:not([data-turn-item=prose]), .turn-footer",
  ));
  return [block, message, turn].flatMap(node => node ? [{ block: node, top: layoutBox(node, scroller).top }] : []);
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
  const ownScroll = useRef<number | undefined>(undefined);
  const viewport = useRef<Viewport | null>(null);
  const anchors = useRef<ReadingAnchor[]>([]);
  const glide = useRef<number | undefined>(undefined);
  /** Where the running glide ends. */
  const glideTarget = useRef(0);
  const follow = useRef<number | undefined>(undefined);
  const layoutFrame = useRef<number | undefined>(undefined);
  const measureFrame = useRef<number | undefined>(undefined);
  const lastResetKey = useRef(scrollResetKey);
  /** The thread was last seen without a box, hidden behind a full-page view. */
  const unseen = useRef(false);
  /** A question sent, or a running chat opened, while the thread was hidden goes to the top once it shows. */
  const anchorWhenShown = useRef(false);
  /** The `autoScroll` this thread last opened with. */
  const openedFor = useRef<boolean | null>(null);
  const runningRef = useRef(running);
  runningRef.current = running;
  const [state, setState] = useState<ConversationState>({ away: false, moreBelow: false });
  const stateRef = useRef(state);

  // Scroll events are asynchronous and WebKit may round or clamp the target.
  const writeScroll = useCallback((element: HTMLElement, target: number) => {
    const before = element.scrollTop;
    const next = Math.max(0, Math.min(target, element.scrollHeight - element.clientHeight));
    if (!differs(before, next)) return false;
    element.scrollTop = next;
    const accepted = element.scrollTop;
    if (accepted !== before) ownScroll.current = accepted;
    return differs(accepted, before);
  }, []);

  const rememberPosition = useCallback((element: HTMLElement) => {
    lastTop.current = element.scrollTop;
    anchors.current = mode.current === "anchored" && !following.current ? readingAnchors(element) : [];
    markScrolled(element);
  }, []);

  // Read status in a separate frame from the placement that may write layout.
  const measure = useCallback(() => {
    if (measureFrame.current !== undefined) return;
    measureFrame.current = requestAnimationFrame(() => {
      measureFrame.current = undefined;
      const element = scrollRef.current;
      if (!element || !laidOut(element)) return;
      const view = element.getBoundingClientRect();
      const footers = latestTurn(element)?.querySelectorAll<HTMLElement>(".turn-footer");
      const end = footers?.length ? footers[footers.length - 1]!.getBoundingClientRect().bottom - view.bottom
        : element.scrollHeight - element.clientHeight - element.scrollTop;
      const next = { away: pillVisible(end, following.current), moreBelow: end > AWAY_PX };
      if (next.away !== stateRef.current.away || next.moreBelow !== stateRef.current.moreBelow) {
        stateRef.current = next;
        setState(next);
      }
    });
  }, []);

  const stopGlide = useCallback(() => {
    if (glide.current !== undefined) cancelAnimationFrame(glide.current);
    glide.current = undefined;
  }, []);

  const glideTo = useCallback((target: number) => {
    const element = scrollRef.current;
    if (!element) return;
    stopGlide();
    const from = element.scrollTop;
    const to = Math.max(0, target);
    glideTarget.current = to;
    if (reducedMotion()) {
      writeScroll(element, to);
      rememberPosition(element);
      measure();
      return;
    }
    const start = performance.now();
    const step = (now: number) => {
      // Hidden mid-glide: the thread shows again where the glide would have ended.
      if (!laidOut(element)) {
        lastTop.current = to;
        anchors.current = [];
        glide.current = undefined;
        return;
      }
      const progress = Math.min(1, (now - start) / GLIDE_MS);
      const eased = 1 - (1 - progress) ** 3;
      writeScroll(element, from + (to - from) * eased);
      rememberPosition(element);
      measure();
      glide.current = progress < 1 ? requestAnimationFrame(step) : undefined;
    };
    glide.current = requestAnimationFrame(step);
  }, [measure, rememberPosition, stopGlide, writeScroll]);

  const glideBy = useCallback((delta: number) => {
    const element = scrollRef.current;
    if (!element || !differs(delta, 0)) return;
    glideTo(element.scrollTop + delta);
  }, [glideTo]);

  /** Make sure the position exists before gliding to it: the latest turn grows its reserve. */
  const reserve = useCallback((delta: number) => {
    const element = scrollRef.current;
    const turn = element && latestTurn(element);
    if (!element || !turn) return;
    const missing = element.scrollTop + delta + element.clientHeight - element.scrollHeight;
    if (missing >= 1) {
      const minimum = turn.offsetHeight + Math.ceil(missing);
      if (!turn.style.minHeight || minimum > Number.parseFloat(turn.style.minHeight)) turn.style.minHeight = `${minimum}px`;
    }
  }, []);

  /** The latest question 16 px under the top (or a tall one's bottom at 40%), with room made below it. */
  const anchorQuestion = useCallback(() => {
    const element = scrollRef.current;
    const turn = element && latestTurn(element);
    const question = turn?.querySelector<HTMLElement>(".message.user");
    if (!element || !turn || !question) return;
    // A divider above the question is part of the turn; the reserve counts from the question.
    turn.style.setProperty("--turn-lead", `${layoutBox(question, element).top - layoutBox(turn, element).top}px`);
    const delta = anchorDelta(layoutBox(question, element), element.getBoundingClientRect());
    reserve(delta);
    glideBy(delta);
  }, [glideBy, reserve]);

  const toBottom = useCallback(() => {
    const element = scrollRef.current;
    if (!element) return;
    writeScroll(element, element.scrollHeight);
    rememberPosition(element);
  }, [rememberPosition, writeScroll]);

  const followFrame = useCallback(() => {
    follow.current = undefined;
    const element = scrollRef.current;
    if (!element || !following.current || !laidOut(element)) return;
    const remaining = element.scrollHeight - element.clientHeight - element.scrollTop;
    if (remaining < 1) return;
    // WKWebView can drop steps below a whole pixel. If it clamps even this step, wait for a layout change.
    const step = reducedMotion() ? remaining : Math.max(1, remaining * FOLLOW_SHARE);
    if (!writeScroll(element, Math.ceil(element.scrollTop + step))) return;
    rememberPosition(element);
    measure();
    follow.current = requestAnimationFrame(followFrame);
  }, [measure, rememberPosition, writeScroll]);

  const queueFollow = useCallback(() => {
    if (follow.current === undefined) follow.current = requestAnimationFrame(followFrame);
  }, [followFrame]);

  const readLayoutChange = useCallback((element: HTMLElement) => {
    const before = viewport.current;
    const view = readViewport(element);
    const anchor = anchors.current.find(({ block }) => element.contains(block) && block.offsetHeight > 0);
    const box = anchor && layoutBox(anchor.block, element);
    // Adding scrollTop recovers the layout coordinate even if WebKit clamped the scroll position.
    const delta = anchor && box
      ? box.top + element.scrollTop - (anchor.top + lastTop.current)
      : view.top - (before?.top ?? view.top);
    const resized = !before || differs(view.width, before.width) || differs(view.height, before.height);
    const changed = !before || resized || differs(view.extent, before.extent) || differs(delta, 0);
    return { before, view, anchor, box, delta, resized, changed };
  }, []);

  /** Apply at most one correction for a new layout. Hidden threads keep their last visible snapshot. */
  const keepContentStill = useCallback(() => {
    const element = scrollRef.current;
    if (!element) return false;
    if (!laidOut(element)) {
      if (viewport.current) unseen.current = true;
      return false;
    }
    const returned = unseen.current;
    const { before, view, anchor, box, delta, resized, changed } = readLayoutChange(element);
    if (!returned && !changed) return false;
    unseen.current = false;
    if (!before || differs(view.height, before.height)) writeViewport(element, view.height);

    const wasAtEnd = before && before.extent - before.height - lastTop.current <= BOTTOM_TOLERANCE_PX;
    const displaced = element.scrollTop < lastTop.current - BOTTOM_TOLERANCE_PX;
    if (mode.current === "bottom" || following.current && (wasAtEnd || displaced || resized || !runningRef.current || returned)) {
      toBottom();
    } else if (anchorWhenShown.current) {
      writeScroll(element, lastTop.current);
      rememberPosition(element);
      anchorQuestion();
    } else if (!following.current && (before || returned)) {
      // If the block itself got shorter, keep at least its last line in the viewport.
      const anchorTop = anchor && box ? Math.max(anchor.top, view.top + 2 - box.height) : null;
      const target = Math.max(0, lastTop.current + delta + (anchor && anchorTop !== null ? anchor.top - anchorTop : 0));
      if (differs(target, element.scrollTop)) {
        reserve(target - element.scrollTop);
        const gliding = glide.current !== undefined ? glideTarget.current : null;
        stopGlide();
        writeScroll(element, target);
        if (gliding !== null && gliding - target >= 1) glideTo(gliding);
      }
      rememberPosition(element);
    }
    anchorWhenShown.current = false;
    // Reserve writes can change the extent. Use the resulting layout only as the baseline for the next pass.
    viewport.current = readViewport(element);
    if (following.current && element.scrollHeight - element.clientHeight - element.scrollTop > BOTTOM_TOLERANCE_PX) queueFollow();
    return true;
  }, [anchorQuestion, glideTo, queueFollow, readLayoutChange, rememberPosition, reserve, stopGlide, toBottom, writeScroll]);

  const startFollowing = useCallback(() => {
    following.current = true;
    anchors.current = [];
    stopGlide();
    queueFollow();
    measure();
  }, [measure, queueFollow, stopGlide]);

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
    const element = scrollRef.current;
    if (element && laidOut(element)) rememberPosition(element);
  }, [rememberPosition, stopFollowing]);
  const blockAppended = useCallback((block: HTMLElement, kind: "prose" | "card") => {
    const element = scrollRef.current;
    if (!element || mode.current !== "anchored" || touched.current || following.current) return;
    const delta = roomDelta(kind, layoutBox(block, element), element.getBoundingClientRect());
    if (!delta) return;
    reserve(delta);
    glideBy(delta);
  }, [glideBy, reserve]);

  // Context above the thread usually changes in the same commit as the thread (an answer, the reply's commit):
  // correct before that frame is drawn, not a frame later when the resize is observed.
  useLayoutEffect(() => {
    if (!autoScroll) return;
    keepContentStill();
    measure();
  }, [autoScroll, children, keepContentStill, measure, running]);

  // ResizeObserver and browser scroll events can both report a reflow. One
  // deferred pass handles both without writing into the observer delivery.
  const queueLayout = useCallback(() => {
    if (layoutFrame.current !== undefined) return;
    layoutFrame.current = requestAnimationFrame(() => {
      layoutFrame.current = undefined;
      if (keepContentStill()) measure();
    });
  }, [keepContentStill, measure]);

  // Opening: a settled chat opens at its end; a chat whose reply is in flight opens with the latest question at the top.
  // A Suspense boundary that showed its fallback (a lazy page opening) re-runs layout effects when it shows this
  // thread again, often while a full-page view still hides it: that is not an opening, and the reader keeps their place.
  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (!element || openedFor.current === autoScroll) return;
    openedFor.current = autoScroll;
    if (!autoScroll) {
      writeScroll(element, 0);
      viewport.current = null;
      markScrolled(element);
      return;
    }
    if (runningRef.current) {
      mode.current = "anchored";
      touched.current = true;
      if (!laidOut(element)) { unseen.current = true; anchorWhenShown.current = true; return; }
      const turn = latestTurn(element);
      const question = turn?.querySelector<HTMLElement>(".message.user");
      if (turn && question) {
        turn.style.setProperty("--turn-lead", `${layoutBox(question, element).top - layoutBox(turn, element).top}px`);
        writeScroll(element, element.scrollTop + anchorDelta(layoutBox(question, element), element.getBoundingClientRect()));
        rememberPosition(element);
      }
    } else if (laidOut(element)) toBottom();
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
    if (!laidOut(element)) { unseen.current = true; anchorWhenShown.current = true; return; }
    anchorQuestion();
  }, [anchorQuestion, autoScroll, scrollResetKey, stopFollowing]);

  // Images, tables and late renderers change height after their commit: a thread opened at its end stays there,
  // a followed reply keeps following, and anything else stays put.
  useEffect(() => {
    const element = scrollRef.current;
    const content = element?.firstElementChild;
    if (!autoScroll || !element || !content || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(queueLayout);
    observer.observe(element);
    observer.observe(content);
    // WKWebView can deliver the viewport resize without a ResizeObserver callback for this flex child.
    window.addEventListener("resize", queueLayout);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", queueLayout);
      if (layoutFrame.current !== undefined) cancelAnimationFrame(layoutFrame.current);
      layoutFrame.current = undefined;
    };
  }, [autoScroll, queueLayout]);

  useEffect(() => () => {
    if (glide.current !== undefined) cancelAnimationFrame(glide.current);
    if (follow.current !== undefined) cancelAnimationFrame(follow.current);
    if (layoutFrame.current !== undefined) cancelAnimationFrame(layoutFrame.current);
    if (measureFrame.current !== undefined) cancelAnimationFrame(measureFrame.current);
    glide.current = follow.current = layoutFrame.current = measureFrame.current = undefined;
  }, []);

  /** Any wheel, touch, key or scrollbar drag on the thread is the reader's. Upward input stops following. */
  const noteInput = useCallback((upward: boolean) => {
    if (!autoScroll) return;
    lastInput.current = performance.now();
    touched.current = true;
    stopGlide();
    if (upward && (following.current || mode.current === "bottom")) holdScrollPosition();
  }, [autoScroll, holdScrollPosition, stopGlide]);

  const handleScroll = useCallback((event: UIEvent<HTMLDivElement>) => {
    const element = event.currentTarget;
    const accepted = ownScroll.current;
    ownScroll.current = undefined;
    const reflow = autoScroll && (unseen.current || !laidOut(element) || readLayoutChange(element).changed);
    if (reflow) queueLayout();
    // Consume our write once, even if a later scroll superseded it before delivery.
    if (reflow || accepted !== undefined && !differs(element.scrollTop, accepted)) {
      onScroll?.(event);
      return;
    }
    // A scrollbar drag can report movement without a DOM pointer event.
    const moved = differs(element.scrollTop, lastTop.current);
    const movedUp = moved && element.scrollTop < lastTop.current;
    const reader = performance.now() - lastInput.current < INPUT_WINDOW_MS
      || moved && glide.current === undefined && follow.current === undefined;
    if (reader && moved) touched.current = true;
    const atBottom = element.scrollHeight - element.clientHeight - element.scrollTop <= BOTTOM_TOLERANCE_PX;
    if (reader && movedUp) {
      if (following.current) stopFollowing();
      if (mode.current === "bottom") mode.current = "anchored";
    }
    if (reader && atBottom && !following.current && !movedUp) {
      if (runningRef.current) startFollowing();
      else mode.current = "bottom";
    }
    // A reader who leaves the end of a thread opened there is no longer pinned to it.
    if (mode.current === "bottom" && !atBottom && reader) mode.current = "anchored";
    rememberPosition(element);
    measure();
    onScroll?.(event);
  }, [autoScroll, measure, onScroll, queueLayout, readLayoutChange, rememberPosition, startFollowing, stopFollowing]);

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
