import i18n, { useTranslation } from "@/i18n";
import type { NumberCheck } from "../../types";
import { citationLabel, citationNumber, evidencePresentation, evidenceTitle, sourceIdentityLabel } from "../../chat/evidence";
import { memoryReadForEvidence } from "../../chat/memoryActivity";
import { checkFor, looksLikeNumber } from "../../chat/numberChecks";
import { keepNumberParticles } from "../../chat/keepParticles";
import { CitationChip } from "../chat/CitationChip";
import { NumberChip, PendingNumber } from "../chat/NumberChip";
import { useTurn } from "../chat/TurnProvider";
import { useOpen, useOpenProps } from "../navigation/Open";
import type { ChatDestination } from "../../chat/navigation";
import { decodedWorkspaceLink, workspaceLinksPlugin, WORKSPACE_LINK_PROTOCOL } from "../../lib/workspaceFileLink";
import { cn } from "@/lib/utils";
import { parseCredentialFreeHttpUrl } from "@/lib/credentialFreeUrl";
import { LENS_LINK_PROTOCOL, lensLinkRecordId } from "@/lib/lensLink";
import { cjk } from "@streamdown/cjk";
import type { CodeHighlighterPlugin } from "@streamdown/code";
import type { MathPlugin } from "@streamdown/math";
import type { DiagramPlugin } from "@streamdown/mermaid";
import type { ComponentProps, ReactNode } from "react";
import { createContext, isValidElement, memo, useContext, useEffect, useMemo, useSyncExternalStore } from "react";
import { Streamdown, defaultRehypePlugins, defaultUrlTransform, type StreamdownTranslations } from "streamdown";

const t = i18n.getFixedT(null, "chat");

/** Per block of text, not per answer: a segment's own number checks and whether it still streams. */
type TextBlock = {
  /** Rust's checks of this text's number links (finance-show-the-math.md). */
  numberChecks?: NumberCheck[];
  /** While the answer streams, number links are pending: no popover yet. */
  streaming?: boolean;
};
const TextBlockContext = createContext<TextBlock>({});

function nodeText(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(nodeText).join("");
  if (isValidElement<{ children?: ReactNode }>(node)) return nodeText(node.props.children);
  return "";
}

type MarkdownLinkProps = ComponentProps<"a"> & { node?: unknown };

const unavailableCitation = (children: ReactNode) =>
  <span className="loupist-unavailable-citation" title={t("messageResponse.sourceUnavailableInThisReply")}>{children}</span>;

function EvidenceLink({ href, children }: { href: string; children?: ReactNode }) {
  useTranslation("chat");
  const turn = useTurn();
  const { numberChecks, streaming } = useContext(TextBlockContext);
  const evidenceRefs = turn?.evidenceRefs ?? [];
  const evidence = evidenceRefs.find((ref) => href === `#evidence-${ref.id}`);
  if (!evidence) return unavailableCitation(children);
  // A number link is the control for its own evidence: Rust checked it when the answer finished.
  const linkText = nodeText(children).trim();
  const check = checkFor(numberChecks, evidence.id, linkText);
  if (check) return <NumberChip check={check} checks={numberChecks} evidence={evidence} evidenceRefs={evidenceRefs} asOf={turn?.asOf}>{children}</NumberChip>;
  if (streaming && looksLikeNumber(linkText)) return <PendingNumber>{children}</PendingNumber>;
  const read = memoryReadForEvidence(evidence, turn?.memoryRefs);
  const label = citationLabel(evidence, evidenceRefs, read?.title);
  // Model-written reference numbers can disagree with the retained source list.
  // Keep the visible number and spoken source number tied to the same evidence.
  const number = /^\d+$/u.test(linkText) ? citationNumber(evidence, evidenceRefs) : null;
  const accessibleLabel = number === null && linkText && !label.toLocaleLowerCase().includes(linkText.toLocaleLowerCase()) ? `${linkText} · ${label}` : label;
  return <CitationChip evidence={evidence} label={accessibleLabel} title={evidenceTitle(evidence)} source={sourceIdentityLabel(evidence) || evidencePresentation(evidence).label}
    to={{ kind: "evidence", evidence }}>{number ?? children}</CitationChip>;
}

/** A prose link: the underline is its affordance, so it carries no icon. */
function ProseLink({ to, className, title, children }: { to: ChatDestination; className?: string; title: string; children?: ReactNode }) {
  useTranslation("chat");
  const link = useOpenProps(to);
  if (!link.available) return <span className={className}>{children}</span>;
  return <button type="button" role="link" className={cn(to.kind === "lens" ? "loupist-lens-link" : "loupist-external-link", className)} data-streamdown="link"
    title={link.failed ? t("messageResponse.couldNotOpenThisLink") : title} {...link.props}>
    {children}
    {link.failed ? <span className="sr-only" role="alert">{t("messageResponse.couldNotOpenThisLink2")}</span> : null}
  </button>;
}

function LoupistMarkdownLink({
  children,
  className,
  href,
  node: _node,
  rel: _rel,
  target: _target,
  ...anchorProps
}: MarkdownLinkProps) {
  useTranslation("chat");
  if (href?.startsWith("#evidence-")) return <EvidenceLink href={href}>{children}</EvidenceLink>;

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

  const lensRecordId = href ? lensLinkRecordId(href) : null;
  if (lensRecordId) return <ProseLink to={{ kind: "lens", recordId: lensRecordId }} className={className} title={t("messageResponse.openLinkedLens")}>{children}</ProseLink>;

  const localPath = href ? decodedWorkspaceLink(href) : null;
  if (localPath) return <ProseLink to={{ kind: "file", path: localPath }} className={className} title={t("messageResponse.openFile", { localPath })}>{children}</ProseLink>;

  const externalUrl = href && /^https?:\/\//i.test(href) ? (parseCredentialFreeHttpUrl(href)?.href ?? null) : null;
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
  return <ProseLink to={{ kind: "link", url: externalUrl }} className={className} title={t("messageResponse.openExternalLink", { externalUrl })}>{children}</ProseLink>;
}

export type MessageResponseProps = Omit<
  ComponentProps<typeof Streamdown>,
  "plugins"
> & {
  /** Rust's checks of this text's number links; links without one stay numbered citations. */
  numberChecks?: NumberCheck[];
  /** Relative file links resolve inside this workspace; without it they are not links. */
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
    numberChecks,
    workspacePath,
    workspaceBasePath,
    mermaidPlugin,
    ...props
  }: MessageResponseProps) => {
    const { t: translate } = useTranslation("chat");
    const hasLinkAction = useOpen() !== null;
    const hasFileAction = hasLinkAction && workspacePath !== undefined;
    const contentPlugins = useContentPlugins(typeof children === "string" ? children : "");
    const rehypePlugins = useMemo(() => {
      // A Korean particle stays on the line of the number it follows.
      if (!hasLinkAction) return [...Object.values(defaultRehypePlugins), keepNumberParticles];
      const sanitizer = defaultRehypePlugins.sanitize;
      // Normalize Windows drive links before the sanitizer treats the drive as
      // an unsupported protocol. Keep every default rule, extending only href;
      // the URL transform and link component validate the relative path again.
      const fileSanitizer: typeof sanitizer = Array.isArray(sanitizer) ? [sanitizer[0], {
        ...sanitizer[1],
        protocols: { ...sanitizer[1]?.protocols, href: [...(sanitizer[1]?.protocols?.href ?? []), ...(hasFileAction ? [WORKSPACE_LINK_PROTOCOL] : []), ...(hasLinkAction ? [LENS_LINK_PROTOCOL] : [])] },
      }] : sanitizer;
      // Streamdown's processor cache keys plugins by their name and options.
      // Explicit options keep each Loupist and relative Markdown base separate.
      const fileLinks: typeof sanitizer = [workspaceLinksPlugin, { rootPath: workspacePath, basePath: workspaceBasePath }];
      return [defaultRehypePlugins.raw, ...(hasFileAction ? [fileLinks] : []), fileSanitizer, defaultRehypePlugins.harden, keepNumberParticles];
    }, [hasFileAction, hasLinkAction, workspacePath, workspaceBasePath]);
    const fileUrlTransform = useMemo(() => hasLinkAction ? ((url: string, key: string, node: Parameters<typeof defaultUrlTransform>[2]) =>
      key === "href" && ((hasFileAction && decodedWorkspaceLink(url)) || lensLinkRecordId(url))
        ? url : defaultUrlTransform(url, key, node)) : undefined, [hasFileAction, hasLinkAction]);
    // Links read their actions and evidence from context, so retained blocks never
    // see a new component and a focused citation is never replaced.
    const resolvedComponents = useMemo(() => hasLinkAction ? { ...components, a: LoupistMarkdownLink } : components, [components, hasLinkAction]);
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
    const textBlock = useMemo(() => ({ numberChecks, streaming }), [numberChecks, streaming]);
    return (
      <TextBlockContext.Provider value={textBlock}>
      <Streamdown
        key={`${workspacePath ?? ""}:${workspaceBasePath ?? ""}`}
        className={cn(
          "w-full [&>*:first-child]:mt-0 [&>*:last-child]:mb-0",
          className,
        )}
        components={resolvedComponents}
        translations={controlTranslations}
        rehypePlugins={rehypePlugins}
        urlTransform={fileUrlTransform}
        linkSafety={hasLinkAction ? disabledLinkSafety : linkSafety}
        plugins={streamdownPlugins}
        {...props}
      >
        {children}
      </Streamdown>
      </TextBlockContext.Provider>
    );
  },
);

MessageResponse.displayName = "MessageResponse";
