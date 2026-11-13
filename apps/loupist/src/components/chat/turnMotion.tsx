import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import {
  CalculatorIcon,
  ChartColumnIcon,
  ChartLineIcon,
  CircleDashedIcon,
  DatabaseIcon,
  FileTextIcon,
  FolderIcon,
  GlobeIcon,
  PenLineIcon,
  PuzzleIcon,
  RefreshCwIcon,
  SearchIcon,
  SquareIcon,
  Table2Icon,
  TerminalSquareIcon,
  type LucideIcon,
} from "lucide-react";
import type { LiveGlyph } from "../../chat/turnView";

/** One rule for the live line, the chips and the timeline (DIRECTION 6.3). */
const GLYPHS: Record<Exclude<LiveGlyph, "waiting">, LucideIcon> = {
  chart: ChartColumnIcon,
  table: Table2Icon,
  search: SearchIcon,
  calculate: CalculatorIcon,
  finance: ChartLineIcon,
  memory: DatabaseIcon,
  web: GlobeIcon,
  files: FolderIcon,
  artifact: FileTextIcon,
  capability: PuzzleIcon,
  compute: TerminalSquareIcon,
  system: RefreshCwIcon,
  phase: CircleDashedIcon,
  writing: PenLineIcon,
  stopping: SquareIcon,
};

/** The glyph for a step or a phase; "waiting" is the amber dot, drawn by the live line. */
export function TurnGlyph({ glyph }: { glyph: LiveGlyph }) {
  if (glyph === "waiting") return <span className="turn-live-dot" />;
  const Icon = GLYPHS[glyph];
  return <Icon aria-hidden="true" />;
}

/**
 * Whether the turn had already painted when this part mounted. Parts that arrive
 * while the reader watches get their entrance; a turn opened mid-run, or read
 * from the store, is drawn settled at once with no entrance and no replay.
 */
const ArrivalContext = createContext<() => boolean>(() => false);

export function TurnArrivalProvider({ children }: { children: ReactNode }) {
  const painted = useRef(false);
  useEffect(() => { painted.current = true; }, []);
  const [read] = useState(() => () => painted.current);
  return <ArrivalContext.Provider value={read}>{children}</ArrivalContext.Provider>;
}

/** True only for a part that mounted after its turn first painted; fixed for the part's life. */
export function useArrived(): boolean {
  const painted = useContext(ArrivalContext);
  const [arrived] = useState(painted);
  return arrived;
}

/** How long the old line takes to leave (`--gg-motion-fast`). */
const TICK_OUT_MS = 120;

/**
 * A one-line slot whose text is replaced in place: the old line leaves upward
 * while the new one rises in (DIRECTION 9.3, animations 3 and 6). Nothing
 * around it moves; under reduced motion the CSS makes it a plain swap.
 */
export function Ticker({ id, className, children }: { id: string; className?: string; children: ReactNode }) {
  const [current, setCurrent] = useState({ id, children, replaced: false });
  const [leaving, setLeaving] = useState<{ id: string; children: ReactNode } | null>(null);
  if (current.id !== id) {
    setLeaving(current);
    setCurrent({ id, children, replaced: true });
  } else if (current.children !== children) setCurrent({ ...current, children });
  useEffect(() => {
    if (!leaving) return;
    const timer = window.setTimeout(() => setLeaving(null), TICK_OUT_MS);
    return () => window.clearTimeout(timer);
  }, [leaving]);
  return <span className={`turn-tick${className ? ` ${className}` : ""}`}>
    {leaving ? <span key={`out:${leaving.id}`} className="turn-tick-out" aria-hidden="true">{leaving.children}</span> : null}
    <span key={`in:${current.id}`} className="turn-tick-in" data-replaced={current.replaced || undefined}>{current.children}</span>
  </span>;
}
