import type { OpenLinkOptions } from "@/chat/links";
import i18n, { useTranslation } from "@/i18n";
import type { ChatEvidenceRef, MemoryRef, NumberCheck } from "../../types";
import { citationLabel, citationNumber, evidencePresentation, evidenceTitle, sourceIdentityLabel } from "../../chat/evidence";
import { memoryReadForEvidence } from "../../chat/memoryActivity";
import { checkFor, looksLikeNumber } from "../../chat/numberChecks";
import { keepNumberParticles } from "../../chat/keepParticles";
import { CitationChip } from "../chat/CitationChip";
import { NumberChip, PendingNumber, type EvidenceCell } from "../chat/NumberChip";
import { decodedWorkspaceLink, workspaceLinksPlugin, WORKSPACE_LINK_PROTOCOL } from "../../lib/workspaceFileLink";
import { cn } from "@/lib/utils";
import { parseCredentialFreeHttpUrl } from "@/lib/credentialFreeUrl";
import { LENS_LINK_PROTOCOL, lensLinkRecordId } from "@/lib/lensLink";
import { cjk } from "@streamdown/cjk";
import type { CodeHighlighterPlugin } from "@streamdown/code";
import type { MathPlugin } from "@streamdown/math";
import type { DiagramPlugin } from "@streamdown/mermaid";
import { ExternalLinkIcon, FileIcon } from "lucide-react";
import { HoverCard } from "radix-ui";
import type { ComponentProps, ReactNode } from "react";
import { createContext, isValidElement, memo, useContext, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { Streamdown, defaultRehypePlugins, defaultUrlTransform, type StreamdownTranslations } from "streamdown";

const t = i18n.getFixedT(null, "chat");

type ResearchEvidence = {
  references?: ChatEvidenceRef[];
  memoryRefs?: MemoryRef[];
  onOpen?: (evidence: ChatEvidenceRef, anchor?: HTMLElement) => void;
  /** Rust's checks of this answer's number links (finance-show-the-math.md). */
  numberChecks?: NumberCheck[];
  /** While the answer streams, number links are pending: no popover yet. */
  streaming?: boolean;
  /** The answer's as-of date; every number popover says "Known as of". */
  asOf?: string;
  onOpenCell?: (evidence: ChatEvidenceRef, cell?: EvidenceCell) => void;
  onShowStep?: (evidence: ChatEvidenceRef) => void;
};
const ResearchEvidenceContext = createContext<ResearchEvidence>({});

export type LensLinkState = "available" | "archived" | "unresolved";
export type LensLinkPreview = { title: string; detail?: string };
export type LensLinkActions = {
  state?: (recordId: string) => LensLinkState;
  preview?: (recordId: string) => LensLinkPreview | null;
  onUnresolved?: (recordId: string, label: string) => void;
};
const LensLinkContext = createContext<LensLinkActions>({});

function nodeText(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(nodeText).join("");
  if (isValidElement<{ children?: ReactNode }>(node)) return nodeText(node.props.children);
  return "";
}

type MarkdownLinkProps = ComponentProps<"a"> & { node?: unknown };

function LoupistMarkdownLink({
  children,
  className,
  href,
  node: _node,
  onOpenLink,
  onOpenFile,
  onLensLink,
  rel: _rel,
  target: _target,
  ...anchorProps
}: MarkdownLinkProps & {
  onOpenLink: (url: string, options?: OpenLinkOptions) => Promise<void> | void;
  onOpenFile?: (path: string) => void;
  onLensLink?: (recordId: string) => void;
  evidenceRefs?: ChatEvidenceRef[];
  memoryRefs?: MemoryRef[];
  onOpenEvidence?: (evidence: ChatEvidenceRef, anchor?: HTMLElement) => void;
}) {
  useTranslation("chat");
  const { references: evidenceRefs, memoryRefs, onOpen: onOpenEvidence, numberChecks, streaming, asOf, onOpenCell, onShowStep } = useContext(ResearchEvidenceContext);
  const [openFailed, setOpenFailed] = useState(false);
  const externalUrl =
    href && /^https?:\/\//i.test(href)
      ? (parseCredentialFreeHttpUrl(href)?.href ?? null)
      : null;

  if (href?.startsWith("#evidence-")) {
    const evidence = evidenceRefs?.find((ref) => href === `#evidence-${ref.id}`);
    if (!evidence || !onOpenEvidence) return <span className="loupist-unavailable-citation" title={t("messageResponse.sourceUnavailableInThisReply")}>{children}</span>;
    // A number link is the control for its own evidence: Rust checked it when the answer finished.
    const linkText = nodeText(children).trim();
    const check = checkFor(numberChecks, evidence.id, linkText);
    if (check) {
      return <NumberChip check={check} checks={numberChecks} evidence={evidence} evidenceRefs={evidenceRefs ?? []} asOf={asOf}
        actions={{ onOpenEvidence: (target, cell) => (onOpenCell ? onOpenCell(target, cell) : onOpenEvidence(target)), onShowStep }}>{children}</NumberChip>;
    }
    if (streaming && looksLikeNumber(linkText)) return <PendingNumber>{children}</PendingNumber>;
    const title = evidenceTitle(evidence);
    const host = sourceIdentityLabel(evidence);
    const preview = evidencePresentation(evidence);
    const read = memoryReadForEvidence(evidence, memoryRefs);
    const label = citationLabel(evidence, evidenceRefs ?? [], read?.title);
    // Model-written reference numbers can disagree with the retained source list.
    // Keep the visible number and spoken source number tied to the same evidence.
    const number = /^\d+$/u.test(linkText) ? citationNumber(evidence, evidenceRefs ?? []) : null;
    const accessibleLabel = number === null && linkText && !label.toLocaleLowerCase().includes(linkText.toLocaleLowerCase()) ? `${linkText} · ${label}` : label;
    return <CitationChip evidence={evidence} label={accessibleLabel} title={title} source={host || preview.label} onOpen={(anchor) => onOpenEvidence(evidence, anchor)}>{number ?? children}</CitationChip>;
  }

  if (href?.startsWith("#")) {
    return (
      <a
        {...anchorProps}
        className={className}
        data-streamdown="link"
        href={href}
      >
        {children}
      </a>
    );
  }

  const lensRecordId = href && onLensLink ? lensLinkRecordId(href) : null;
  if (lensRecordId) {
    return <LensLinkButton recordId={lensRecordId} className={className} onLensLink={onLensLink}>{children}</LensLinkButton>;
  }

  const localPath = href && onOpenFile ? decodedWorkspaceLink(href) : null;
  if (localPath) {
    return <a href={href} className={cn("loupist-external-link", className)} data-streamdown="link" title={t("messageResponse.openFile", { localPath: localPath })}
      onClick={(event) => { event.preventDefault(); onOpenFile?.(localPath); }}
      onAuxClick={(event) => event.preventDefault()}>{children}<FileIcon aria-hidden="true" /></a>;
  }

  if (!externalUrl) {
    return (
      <span
        className={cn("loupist-blocked-link", className)}
        data-streamdown="link"
        title={t("messageResponse.thisLinkTypeCannotBeOpened")}
      >
        {children}
        <span className="sr-only">{t("messageResponse.unsupportedLink")}</span>
      </span>
    );
  }

  return (
    <button
      type="button"
      role="link"
      className={cn("loupist-external-link", className)}
      data-streamdown="link"
      title={openFailed ? t("messageResponse.couldNotOpenThisLink") : t("messageResponse.openExternalLink", { externalUrl: externalUrl })}
      data-open-failed={openFailed || undefined}
      onClick={(event) => {
        setOpenFailed(false);
        // Cmd-click (Ctrl on Windows) opens the system browser; a plain click opens the panel's browser.
        const external = event.metaKey || event.ctrlKey;
        void Promise.resolve()
          .then(() => external ? onOpenLink(externalUrl, { external: true }) : onOpenLink(externalUrl))
          .catch(() => {
            setOpenFailed(true);
          });
      }}
      onAuxClick={(event) => {
        if (event.button !== 1) return;
        event.preventDefault();
        void Promise.resolve().then(() => onOpenLink(externalUrl, { external: true })).catch(() => setOpenFailed(true));
      }}
    >
      {children}
      <ExternalLinkIcon aria-hidden="true" />
      {openFailed && (
        <span className="sr-only" role="alert">
          {t("messageResponse.couldNotOpenThisLink2")}</span>
      )}
    </button>
  );
}

function LensLinkButton({ recordId, className, onLensLink, children }: { recordId: string; className?: string; onLensLink?: (recordId: string) => void; children?: ReactNode }) {
  useTranslation("chat");
  const actions = useContext(LensLinkContext);
  const state = actions.state?.(recordId) ?? "available";
  const preview = actions.preview?.(recordId) ?? null;
  const label = nodeText(children).trim();
  const title = state === "unresolved" ? t("messageResponse.thisLensIsNotInTheCollection") : state === "archived" ? t("messageResponse.archivedLens") : t("messageResponse.openLinkedLens");
  const open = () => {
    if (state === "unresolved" && actions.onUnresolved) actions.onUnresolved(recordId, label);
    else onLensLink?.(recordId);
  };
  const button = <button type="button" role="link" className={cn("loupist-lens-link", state !== "available" && `is-${state}`, className)}
    data-streamdown="link" title={title} onClick={open}>{children}</button>;
  if (!preview) return button;
  return <HoverCard.Root openDelay={300} closeDelay={80}>
    <HoverCard.Trigger asChild>{button}</HoverCard.Trigger>
    <HoverCard.Portal>
      <HoverCard.Content className="lens-link-preview" sideOffset={6}>
        <strong>{preview.title}</strong>
        {preview.detail ? <p>{preview.detail}</p> : null}
      </HoverCard.Content>
    </HoverCard.Portal>
  </HoverCard.Root>;
}

export type MessageResponseProps = Omit<
  ComponentProps<typeof Streamdown>,
  "plugins"
> & {
  onOpenLink?: (url: string, options?: OpenLinkOptions) => Promise<void> | void;
  onOpenFile?: (path: string) => void;
  onLensLink?: (recordId: string) => void;
  lensLinks?: LensLinkActions;
  evidenceRefs?: ChatEvidenceRef[];
  memoryRefs?: MemoryRef[];
  onOpenEvidence?: (evidence: ChatEvidenceRef, anchor?: HTMLElement) => void;
  /** Rust's checks of this answer's number links; links without one stay numbered citations. */
  numberChecks?: NumberCheck[];
  /** The answer's as-of date (`YYYY-MM-DD`). */
  asOf?: string;
  /** Opens Evidence at a matched data cell. */
  onOpenEvidenceCell?: (evidence: ChatEvidenceRef, cell?: EvidenceCell) => void;
  /** Reveals a calculation's or query's step inline. */
  onShowStep?: (evidence: ChatEvidenceRef) => void;
  workspacePath?: string;
  workspaceBasePath?: string;
  mermaidPlugin?: DiagramPlugin;
};

type LoadedPlugins = { code?: CodeHighlighterPlugin; math?: MathPlugin };

// One store for every answer: a plugin loads once, and an instance that mounts
// after it renders highlighted on its first paint instead of after a swap.
const pluginStore = {
  loaded: {} as LoadedPlugins,
  listeners: new Set<() => void>(),
  loading: { code: undefined as Promise<void> | undefined, math: undefined as Promise<void> | undefined },
  subscribe(listener: () => void) {
    pluginStore.listeners.add(listener);
    return () => { pluginStore.listeners.delete(listener); };
  },
  snapshot: () => pluginStore.loaded,
  set(next: LoadedPlugins) {
    pluginStore.loaded = { ...pluginStore.loaded, ...next };
    for (const listener of pluginStore.listeners) listener();
  },
  load(kind: keyof LoadedPlugins) {
    if (pluginStore.loaded[kind] || pluginStore.loading[kind]) return;
    const request = kind === "code"
      ? import("@streamdown/code").then(({ code }) => pluginStore.set({ code }))
      : Promise.all([import("@streamdown/math"), import("katex/dist/katex.min.css")]).then(([{ math }]) => pluginStore.set({ math }));
    pluginStore.loading[kind] = request.catch(() => { pluginStore.loading[kind] = undefined; });
  },
};

function useContentPlugins(text: string) {
  useTranslation("chat");
  const requiresCode = /^[\t ]*(?:`{3,}|~{3,})/mu.test(text);
  // Streamdown's current default uses $$; ordinary currency stays ordinary text.
  const requiresMath = /(?<!\\)\$\$[\s\S]+?(?<!\\)\$\$/u.test(text);
  const loaded = useSyncExternalStore(pluginStore.subscribe, pluginStore.snapshot, pluginStore.snapshot);

  useEffect(() => { if (requiresCode) pluginStore.load("code"); }, [requiresCode]);
  useEffect(() => { if (requiresMath) pluginStore.load("math"); }, [requiresMath]);

  const codePlugin = requiresCode ? loaded.code : undefined;
  const mathPlugin = requiresMath ? loaded.math : undefined;
  return useMemo(() => ({
    cjk,
    ...(codePlugin ? { code: codePlugin } : {}),
    ...(mathPlugin ? { math: mathPlugin } : {}),
  }), [codePlugin, mathPlugin]);
}
const disabledLinkSafety = { enabled: false } as const;

export const MessageResponse = memo(
  ({
    className,
    components,
    children,
    linkSafety,
    translations,
    onOpenLink,
    onOpenFile,
    onLensLink,
    lensLinks,
    evidenceRefs,
    memoryRefs,
    onOpenEvidence,
    numberChecks,
    asOf,
    onOpenEvidenceCell,
    onShowStep,
    workspacePath,
    workspaceBasePath,
    mermaidPlugin,
    ...props
  }: MessageResponseProps) => {
    const { t: translate } = useTranslation("chat");
    // Streamdown retains unchanged Markdown blocks, including their link nodes.
    // Their event handlers must follow the current workspace actions.
    const linkActions = useRef({ onOpenLink, onOpenFile, onLensLink, onOpenEvidence, evidenceRefs });
    linkActions.current = { onOpenLink, onOpenFile, onLensLink, onOpenEvidence, evidenceRefs };
    const hasLinkAction = Boolean(onOpenLink);
    const hasFileAction = Boolean(onOpenFile);
    const hasLensAction = Boolean(onLensLink);
    const contentPlugins = useContentPlugins(typeof children === "string" ? children : "");
    const rehypePlugins = useMemo(() => {
      // A Korean particle stays on the line of the number it follows.
      if (!hasFileAction && !hasLensAction) return [...Object.values(defaultRehypePlugins), keepNumberParticles];
      const sanitizer = defaultRehypePlugins.sanitize;
      // Normalize Windows drive links before the sanitizer treats the drive as
      // an unsupported protocol. Keep every default rule, extending only href;
      // the URL transform and link component validate the relative path again.
      const fileSanitizer: typeof sanitizer = Array.isArray(sanitizer) ? [sanitizer[0], {
        ...sanitizer[1],
        protocols: { ...sanitizer[1]?.protocols, href: [...(sanitizer[1]?.protocols?.href ?? []), ...(hasFileAction ? [WORKSPACE_LINK_PROTOCOL] : []), ...(hasLensAction ? [LENS_LINK_PROTOCOL] : [])] },
      }] : sanitizer;
      // Streamdown's processor cache keys plugins by their name and options.
      // Explicit options keep each Loupist and relative Markdown base separate.
      const fileLinks: typeof sanitizer = [workspaceLinksPlugin, { rootPath: workspacePath, basePath: workspaceBasePath }];
      return [defaultRehypePlugins.raw, ...(hasFileAction ? [fileLinks] : []), fileSanitizer, defaultRehypePlugins.harden, keepNumberParticles];
    }, [hasFileAction, hasLensAction, workspacePath, workspaceBasePath]);
    const fileUrlTransform = useMemo(() => hasFileAction || hasLensAction ? ((url: string, key: string, node: Parameters<typeof defaultUrlTransform>[2]) =>
      key === "href" && ((hasFileAction && decodedWorkspaceLink(url)) || (hasLensAction && lensLinkRecordId(url)))
        ? url : defaultUrlTransform(url, key, node)) : undefined, [hasFileAction, hasLensAction]);
    const resolvedComponents = useMemo(
      () =>
        hasLinkAction || hasLensAction
          ? {
              ...components,
              a: (linkProps: MarkdownLinkProps) => (
                <LoupistMarkdownLink
                  {...linkProps}
                  onOpenLink={(url, options) => options ? linkActions.current.onOpenLink?.(url, options) : linkActions.current.onOpenLink?.(url)}
                  onOpenFile={hasFileAction ? (path) => linkActions.current.onOpenFile?.(path) : undefined}
                  onLensLink={hasLensAction ? (recordId) => linkActions.current.onLensLink?.(recordId) : undefined}
                />
              ),
            }
          : components,
      // Evidence updates reach retained links through context. Keep this
      // component stable so continued text does not replace a focused citation.
      [components, hasLinkAction, hasFileAction, hasLensAction],
    );
    const streamdownPlugins = useMemo(
      () =>
        mermaidPlugin
          ? { ...contentPlugins, mermaid: mermaidPlugin }
          : contentPlugins,
      [contentPlugins, mermaidPlugin],
    );
    const controlTranslations = useMemo<Partial<StreamdownTranslations>>(() => ({
      close: translate("messageResponse.streamdown.close"),
      copied: translate("messageResponse.streamdown.copied"),
      copyCode: translate("messageResponse.streamdown.copyCode"),
      copyLink: translate("messageResponse.streamdown.copyLink"),
      copyTable: translate("messageResponse.streamdown.copyTable"),
      copyTableAsCsv: translate("messageResponse.streamdown.copyTableAsCsv"),
      copyTableAsMarkdown: translate("messageResponse.streamdown.copyTableAsMarkdown"),
      copyTableAsTsv: translate("messageResponse.streamdown.copyTableAsTsv"),
      downloadDiagram: translate("messageResponse.streamdown.downloadDiagram"),
      downloadDiagramAsMmd: translate("messageResponse.streamdown.downloadDiagramAsMmd"),
      downloadDiagramAsPng: translate("messageResponse.streamdown.downloadDiagramAsPng"),
      downloadDiagramAsSvg: translate("messageResponse.streamdown.downloadDiagramAsSvg"),
      downloadFile: translate("messageResponse.streamdown.downloadFile"),
      downloadImage: translate("messageResponse.streamdown.downloadImage"),
      downloadTable: translate("messageResponse.streamdown.downloadTable"),
      downloadTableAsCsv: translate("messageResponse.streamdown.downloadTableAsCsv"),
      downloadTableAsMarkdown: translate("messageResponse.streamdown.downloadTableAsMarkdown"),
      exitFullscreen: translate("messageResponse.streamdown.exitFullscreen"),
      externalLinkWarning: translate("messageResponse.streamdown.externalLinkWarning"),
      imageNotAvailable: translate("messageResponse.streamdown.imageNotAvailable"),
      mermaidFormatMmd: translate("messageResponse.streamdown.mermaidFormatMmd"),
      mermaidFormatPng: translate("messageResponse.streamdown.mermaidFormatPng"),
      mermaidFormatSvg: translate("messageResponse.streamdown.mermaidFormatSvg"),
      openExternalLink: translate("messageResponse.streamdown.openExternalLink"),
      openLink: translate("messageResponse.streamdown.openLink"),
      tableFormatCsv: translate("messageResponse.streamdown.tableFormatCsv"),
      tableFormatMarkdown: translate("messageResponse.streamdown.tableFormatMarkdown"),
      tableFormatTsv: translate("messageResponse.streamdown.tableFormatTsv"),
      viewFullscreen: translate("messageResponse.streamdown.viewFullscreen"),
      ...translations,
    }), [translate, translations]);

    const streaming = Boolean(props.isAnimating);
    const evidenceContext = useMemo(() => ({ references: evidenceRefs, memoryRefs, onOpen: onOpenEvidence, numberChecks, streaming, asOf, onOpenCell: onOpenEvidenceCell, onShowStep }),
      [evidenceRefs, memoryRefs, onOpenEvidence, numberChecks, streaming, asOf, onOpenEvidenceCell, onShowStep]);
    const lensLinkContext = useMemo(() => lensLinks ?? {}, [lensLinks]);
    return (
      <ResearchEvidenceContext.Provider value={evidenceContext}>
      <LensLinkContext.Provider value={lensLinkContext}>
      <Streamdown
        key={`${Boolean(onLensLink)}:${workspacePath ?? ""}:${workspaceBasePath ?? ""}`}
        className={cn(
          "w-full [&>*:first-child]:mt-0 [&>*:last-child]:mb-0",
          className,
        )}
        components={resolvedComponents}
        translations={controlTranslations}
        rehypePlugins={rehypePlugins}
        urlTransform={fileUrlTransform}
        linkSafety={onOpenLink ? disabledLinkSafety : linkSafety}
        plugins={streamdownPlugins}
        {...props}
      >
        {children}
      </Streamdown>
      </LensLinkContext.Provider>
      </ResearchEvidenceContext.Provider>
    );
  },
);

MessageResponse.displayName = "MessageResponse";
