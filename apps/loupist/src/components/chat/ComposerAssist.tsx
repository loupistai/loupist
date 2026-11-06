import i18n, { useTranslation } from "@/i18n";
import {
  BookOpenIcon,
  BrainIcon,
  CornerDownLeftIcon,
  CpuIcon,
  FileIcon,
  FileTextIcon,
  GlobeIcon,
  LibraryIcon,
  MessageSquarePlusIcon,
  PaperclipIcon,
  PlugIcon,
  SearchIcon,
  WorkflowIcon,
} from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type Ref } from "react";
import { knownComposerMentions, splitComposerMentions } from "../../chat/composerMentions";
import type { ContextOption } from "../../chat/composerChips";
import { validInlineReferences, type ComposerInlineReference } from "../../chat/composerInlineReferences";

const t = i18n.getFixedT(null, "chat");

export type ComposerOption = {
  id: string;
  name: string;
  description: string;
};

export type TriggerSymbol = "@" | "/";

export type ActiveTrigger = {
  symbol: TriggerSymbol;
  query: string;
  start: number;
  end: number;
};

type AssistKind = "skill" | "connection";

export type AssistOption = {
  id: string;
  kind: AssistKind | "context" | "command";
  label: string;
  name: string;
  detail: string;
  insert: string;
  group?: string;
  context?: ContextOption;
  command?: "new-chat" | "model" | "memory" | "attach" | "brief" | "compare" | "check" | "asof";
};

const assistHeading = (symbol: TriggerSymbol) => {
  if (symbol === "@") return t("chatComposer.context");
  return t("chatComposer.commands");
};

const assistHint = (symbol: TriggerSymbol) => {
  if (symbol === "@") return t("chatComposer.filesMemoryAndConnections");
  return t("chatComposer.commandsAndSkills");
};

const assistEmpty = (symbol: TriggerSymbol, skillCount: number) => {
  if (symbol === "@") return t("chatComposer.noMatchingContext");
  if (skillCount === 0 && symbol === "/") return t("chatComposer.noMatchingItem");
  return t("chatComposer.noMatchingItem");
};

function highlightMatch(text: string, query: string) {
  const needle = query.trim();
  if (!needle) return text;
  const haystack = text.toLowerCase();
  const folded = needle.toLowerCase();
  if (haystack.length !== text.length || folded.length !== needle.length) return text;
  const index = haystack.indexOf(folded);
  if (index < 0) return text;
  return (
    <>
      {text.slice(0, index)}
      <span className="composer-assist-match">{text.slice(index, index + folded.length)}</span>
      {text.slice(index + folded.length)}
    </>
  );
}

function assistIcon(option: AssistOption) {
  if (option.command === "new-chat") return MessageSquarePlusIcon;
  if (option.command === "model") return CpuIcon;
  if (option.command === "memory") return BrainIcon;
  if (option.command === "attach") return PaperclipIcon;
  if (option.kind === "skill") return WorkflowIcon;
  switch (option.context?.group) {
    case "Files":
      return FileIcon;
    case "Lens":
      return BookOpenIcon;
    case "World":
      return GlobeIcon;
    case "Sources":
      return LibraryIcon;
    case "Research":
      return SearchIcon;
    case "Documents":
      return FileTextIcon;
    case "Connections":
      return PlugIcon;
    default:
      return option.kind === "connection" || option.context?.connectionId ? PlugIcon : WorkflowIcon;
  }
}

function assistRowKind(option: AssistOption) {
  if (option.command) return "command";
  if (option.kind === "skill") return "skill";
  if (option.context?.connectionId || option.kind === "connection") return "connection";
  return "context";
}

function assistSections(options: readonly AssistOption[]) {
  const sections: { group: string; items: { option: AssistOption; index: number }[] }[] = [];
  options.forEach((option, index) => {
    const group = option.group ?? t("chatComposer.results");
    const last = sections.at(-1);
    if (!last || last.group !== group) sections.push({ group, items: [{ option, index }] });
    else last.items.push({ option, index });
  });
  return sections;
}

/** Whether the list has rows below what it shows, kept current as it scrolls, resizes or changes. */
function useMoreBelow(options: readonly AssistOption[]) {
  const ref = useRef<HTMLDivElement>(null);
  const [below, setBelow] = useState(false);
  const update = useCallback(() => {
    const element = ref.current;
    setBelow(Boolean(element) && element!.scrollHeight - element!.scrollTop - element!.clientHeight > 1);
  }, []);
  useLayoutEffect(update, [options, update]);
  useEffect(() => {
    const element = ref.current;
    if (!element || typeof ResizeObserver === "undefined") return undefined;
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [update]);
  return { ref, below, update };
}

export function ComposerAssistMenu({
  id,
  menuRef,
  trigger,
  options,
  activeOption,
  skillCount,
  onHover,
  onSelect,
}: {
  id: string;
  menuRef: Ref<HTMLDivElement | null>;
  trigger: ActiveTrigger;
  options: readonly AssistOption[];
  activeOption: number;
  skillCount: number;
  onHover: (index: number) => void;
  onSelect: (option: AssistOption) => void;
}) {
  useTranslation("chat");
  const sections = assistSections(options);
  const showHeaders = sections.length > 1 || (sections[0] !== undefined && sections[0].group !== assistHeading(trigger.symbol));
  const moreBelow = useMoreBelow(options);
  return (
    <div
      id={id}
      ref={menuRef}
      className="composer-assist"
      data-symbol={trigger.symbol}
      role="listbox"
      aria-label={t("chatComposer.promptShortcuts")}
    >
      <div className="composer-assist-heading">
        <span className="composer-assist-token" aria-hidden="true">{trigger.symbol}</span>
        <span className="composer-assist-title">{assistHeading(trigger.symbol)}</span>
        {trigger.query ? (
          <span className="composer-assist-query">{trigger.query}</span>
        ) : (
          <span className="composer-assist-hint">{assistHint(trigger.symbol)}</span>
        )}
      </div>
      {/* Rows below the fold fade out at the edge, so the list reads as one that scrolls. */}
      <div ref={moreBelow.ref} className="composer-assist-scroll" data-more-below={moreBelow.below || undefined} onScroll={moreBelow.update}>
        {options.length === 0 ? (
          <p className="composer-assist-empty">{assistEmpty(trigger.symbol, skillCount)}</p>
        ) : sections.map((section) => (
          <div key={`${section.group}-${section.items[0]?.index ?? 0}`} className="composer-assist-section" role="group" aria-label={section.group}>
            {showHeaders ? <div className="composer-assist-group">{section.group}</div> : null}
            {section.items.map(({ option, index }) => {
              const Icon = assistIcon(option);
              return (
                <button
                  key={option.id}
                  id={`${id}-${index}`}
                  className="composer-assist-option"
                  type="button"
                  role="option"
                  data-kind={assistRowKind(option)}
                  data-row={trigger.symbol === "/" ? "plain" : undefined}
                  aria-label={option.name}
                  aria-describedby={option.detail && option.detail !== option.name ? `${id}-${index}-detail` : undefined}
                  aria-selected={index === activeOption}
                  tabIndex={-1}
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => onSelect(option)}
                  onMouseEnter={() => onHover(index)}
                >
                  {trigger.symbol === "/" ? null : <span className="composer-assist-icon" aria-hidden="true"><Icon /></span>}
                  <span className="composer-assist-copy">
                    <strong>{highlightMatch(option.name, trigger.query)}</strong>
                    {option.detail && option.detail !== option.name ? <small id={`${id}-${index}-detail`} aria-hidden="true">{option.detail}</small> : null}
                  </span>
                  <span className="composer-assist-enter" aria-hidden="true">
                    {index === activeOption ? <CornerDownLeftIcon /> : null}
                  </span>
                </button>
              );
            })}
          </div>
        ))}
      </div>
      <footer className="composer-assist-footer">
        <span><kbd>↑</kbd><kbd>↓</kbd> {t("chatComposer.shortcutMove")}</span>
        <span><kbd>↵</kbd> {t("chatComposer.shortcutSelect")}</span>
        <span><kbd>{t("chatComposer.escapeKey")}</kbd> {t("chatComposer.shortcutClose")}</span>
      </footer>
    </div>
  );
}

export function ComposerPromptHighlight({
  prompt,
  skills,
  plugins,
  highlightRef,
  references = [],
}: {
  prompt: string;
  skills: ComposerOption[];
  plugins: ComposerOption[];
  highlightRef: Ref<HTMLDivElement>;
  references?: readonly ComposerInlineReference[];
}) {
  useTranslation("chat");
  const knownMentions = useMemo(
    () => knownComposerMentions(skills, plugins),
    [plugins, skills],
  );
  const ranges = validInlineReferences(prompt, references);
  const parts: ({ type: "reference"; value: string; kind: string; key: string } | ReturnType<typeof splitComposerMentions>[number])[] = [];
  let cursor = 0;
  for (const reference of ranges) {
    parts.push(...splitComposerMentions(prompt.slice(cursor, reference.start), knownMentions));
    parts.push({ type: "reference", value: reference.text, kind: reference.kind, key: reference.key });
    cursor = reference.end;
  }
  parts.push(...splitComposerMentions(prompt.slice(cursor), knownMentions));
  return (
    <div
      ref={highlightRef}
      className="composer-prompt-highlight"
      aria-hidden="true"
    >
      {parts.map((part, index) =>
        part.type === "reference" ? <span key={`${part.key}:${index}`} data-inline-reference={part.kind}>{part.value}</span> : part.type === "mention" ? (
          <span key={index} data-mention={part.kind}>
            {part.value}
          </span>
        ) : (
          <span key={index}>{part.value}</span>
        ),
      )}
      {"\n"}
    </div>
  );
}

export const activeTriggerAt = (
  value: string,
  cursor: number,
): ActiveTrigger | null => {
  const prefix = value.slice(0, cursor);
  const match = prefix.match(/(?:^|\s)([@/])([\p{L}\p{N}._:@/-]*)$/u);
  if (!match) return null;
  const symbol = match[1];
  if (symbol !== "@" && symbol !== "/") return null;
  const query = match[2] ?? "";
  return {
    symbol,
    query,
    start: cursor - query.length - 1,
    end: cursor,
  };
};
