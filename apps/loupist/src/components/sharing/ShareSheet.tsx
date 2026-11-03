import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type MouseEvent, type ReactNode } from "react";
import { ArrowLeftIcon, CheckIcon, ExternalLinkIcon } from "lucide-react";
import i18n, { Trans, useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import type { LoupistBridge } from "../../types";
import type {
  GithubAccount, PackLicense, ShareFinding, SharePublishEvent, SharePublishResult, SharePublishTarget, ShareRequest, ShareScope,
  ShareSectionId, ShareSections, ShareSummary,
} from "../../contracts/sharing";
import { errorMessage } from "../../lib/errors";
import { Button } from "../ui/button";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, focusDialogTitle } from "../ui/dialog";
import { Input } from "../ui/input";
import { NativeSelect } from "../ui/native-select";
import { RadioGroup, RadioGroupItem } from "../ui/radio-group";
import { Spinner } from "../ui/spinner";
import { Switch } from "../ui/switch";
import { errorCode, licenseName, licenseSummary, repoSlug, shortTitle } from "./sharingText";
import "@/styles/sharing.css";

const SECTIONS: readonly ShareSectionId[] = ["profile", "perspective", "methods", "principles", "map"];
const FINDING_KEYS = { credential: "findingKey", account: "findingAccount", holding: "findingHolding", personal: "findingPersonal" } as const;
const FINDINGS_SHOWN = 5;

type Step = "options" | "signin" | "repo" | "progress" | "published";
type SignInState =
  | { status: "starting" }
  | { status: "code"; userCode: string; expiresAt: number }
  | { status: "expired" }
  | { status: "denied" };

/**
 * The Share sheet (W19.2, W19.3). Rust counts, scans and writes; this sheet
 * submits the chosen sections, left-out items and acknowledgements, and shows
 * findings by class and record label, never the matched text.
 */
export function ShareSheet({ bridge, loupistId, loupistName, scope, open, onOpenChange }: {
  bridge: LoupistBridge; loupistId: string; loupistName: string; scope: ShareScope; open: boolean; onOpenChange: (open: boolean) => void;
}) {
  const { t } = useTranslation("sharing");
  const ids = { body: useId(), license: useId(), byline: useId(), repo: useId(), reason: useId(), gist: useId() };
  const stepRef = useRef<HTMLDivElement>(null);
  const shownStep = useRef<Step>("options");
  const [summary, setSummary] = useState<ShareSummary | null>(null);
  const [account, setAccount] = useState<GithubAccount | null>(null);
  const [slow, setSlow] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [sections, setSections] = useState<ShareSections>({ profile: true, perspective: true, methods: true, principles: true, map: true });
  const [excluded, setExcluded] = useState<string[]>([]);
  const [acknowledged, setAcknowledged] = useState<string[]>([]);
  const [allFindings, setAllFindings] = useState(false);
  const [license, setLicense] = useState<PackLicense>();
  const [byline, setByline] = useState("");
  const [busy, setBusy] = useState<"folder" | "zip" | "copy" | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const [copiedText, setCopiedText] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [step, setStep] = useState<Step>("options");
  const [signIn, setSignIn] = useState<SignInState | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [repoName, setRepoName] = useState("");
  const [repoError, setRepoError] = useState<string | null>(null);
  const [newRepo, setNewRepo] = useState(false);
  const [gist, setGist] = useState<"public" | "secret">("public");
  const [progress, setProgress] = useState<SharePublishEvent | null>(null);
  const [published, setPublished] = useState<SharePublishResult | null>(null);
  const [linkCopied, setLinkCopied] = useState(false);
  const generation = useRef(0);
  const item = scope.kind === "item";
  const scopeKey = scope.kind === "item" ? `item:${scope.recordId}` : "loupist";

  useEffect(() => {
    if (!open) return;
    const scope: ShareScope = scopeKey === "loupist" ? { kind: "loupist" } : { kind: "item", recordId: scopeKey.slice(5) };
    const current = ++generation.current;
    setSummary(null); setAccount(null); setLoadError(null); setSlow(false); setExcluded([]); setAcknowledged([]); setAllFindings(false);
    setSaved(null); setError(null); setStep("options"); setSignIn(null); setNewRepo(false); setRepoError(null); setPublished(null); setProgress(null); setLinkCopied(false); setCopiedText(false);
    const timer = setTimeout(() => { if (current === generation.current) setSlow(true); }, 300);
    void Promise.all([bridge.shareSummary({ loupistId, scope }), bridge.githubAccount()]).then(([loaded, github]) => {
      if (current !== generation.current) return;
      setSummary(loaded); setAccount(github);
      setSections({ profile: true, perspective: loaded.counts.perspective > 0, methods: loaded.counts.methods > 0, principles: loaded.counts.principles > 0, map: loaded.counts.names > 0 });
      setLicense(loaded.defaultLicense);
      setByline(loaded.byline ?? "");
      setRepoName(loaded.repo?.fullName.split("/")[1] ?? repoSlug(loaded.loupistName));
    }).catch(cause => { if (current === generation.current) setLoadError(errorMessage(cause, t("scanning"))); });
    return () => { clearTimeout(timer); generation.current += 1; };
  }, [open, bridge, loupistId, scopeKey, t]);

  const summaryReady = summary !== null;
  useEffect(() => {
    if (!open || !summaryReady) return;
    let current = true;
    const scope: ShareScope = scopeKey === "loupist" ? { kind: "loupist" } : { kind: "item", recordId: scopeKey.slice(5) };
    void bridge.shareSummary({ loupistId, scope, sections, excludedItemKeys: excluded }).then(next => {
      if (!current) return;
      setSummary(previous => previous && {
        ...previous, counts: { ...previous.counts, leftOutLicense: next.counts.leftOutLicense },
        allowedLicenses: next.allowedLicenses, defaultLicense: next.defaultLicense, licenseConflict: next.licenseConflict,
      });
      setLicense(previous => previous && next.allowedLicenses.some(license => license.id === previous) ? previous : next.defaultLicense);
    }).catch(() => undefined);
    return () => { current = false; };
  }, [open, summaryReady, bridge, loupistId, scopeKey, sections, excluded]);

  // The Loupist's name leaves only with its Profile: the default repository name follows that switch.
  useEffect(() => {
    if (!summary || summary.repo) return;
    const named = repoSlug(summary.loupistName);
    const neutral = repoSlug("shared");
    setRepoName(current => sections.profile ? (current === neutral ? named : current) : (current === named ? neutral : current));
  }, [sections.profile, summary]);

  // The device code's remaining minutes, from GitHub's expiry.
  useEffect(() => {
    if (signIn?.status !== "code") return;
    const timer = setInterval(() => setNow(Date.now()), 15_000);
    return () => clearInterval(timer);
  }, [signIn?.status]);

  // A step change removes the focused control; focus the new step's title (or the field it focused itself), never the sheet.
  useLayoutEffect(() => {
    if (shownStep.current === step) return;
    shownStep.current = step;
    const root = stepRef.current;
    if (!root || root.contains(document.activeElement)) return;
    root.querySelector<HTMLElement>("[data-slot='dialog-title']")?.focus();
  }, [step]);

  const counts = summary?.counts;
  const nothing = counts ? counts.perspective + counts.methods + counts.principles + counts.names === 0 : false;
  const visibleFindings = useMemo(() => (summary?.findings ?? []).filter(finding => !excluded.includes(finding.itemKey) && !acknowledged.includes(finding.id)), [summary, excluded, acknowledged]);
  const blocking = visibleFindings.some(finding => finding.blocking);
  const anySection = item || SECTIONS.some(section => sections[section]);
  // Rust names each item whose license clashes (an item once per license in its origin chain); one row per item still included.
  const conflictItems = useMemo(() => {
    const rows = new Map<string, { key: string; name: string; licenses: PackLicense[] }>();
    for (const entry of summary?.licenseConflict?.items ?? []) {
      if (excluded.includes(entry.key) || (entry.key === "profile" && !sections.profile)) continue;
      const row = rows.get(entry.key) ?? { key: entry.key, name: entry.name, licenses: [] };
      if (!row.licenses.includes(entry.license)) row.licenses.push(entry.license);
      rows.set(entry.key, row);
    }
    return [...rows.values()];
  }, [summary, excluded, sections.profile]);
  const conflict = Boolean(summary?.licenseConflict);
  // Shared items allow only one license: the choice is set, and says why.
  const fixedLicense = summary?.allowedLicenses.length === 1;
  const licenseUrl = summary?.allowedLicenses.find(entry => entry.id === license)?.url;
  const outputsDisabled = !summary || summary.allowedLicenses.length === 0 || nothing || blocking || !anySection || busy !== null;
  // The disabled outputs say why under them; loading and "nothing to share" already explain themselves in the body.
  const disabledReason = !summary || nothing ? null : blocking ? t("blockedReason") : conflict ? t("conflictReason") : !anySection ? t("turnOnOne") : null;
  // The Profile is left out with its switch, everything else by its key.
  const leaveOut = (key: string) => key === "profile" ? setSections(current => ({ ...current, profile: false })) : setExcluded(current => [...current, key]);
  const reasonProps = disabledReason ? { "aria-describedby": ids.reason } : {};
  const request = (): ShareRequest => ({
    loupistId, scope, license: license, excludedItemKeys: excluded, acknowledgedFindingIds: acknowledged,
    sections: item ? { profile: false, perspective: false, methods: summary?.item?.kind !== "principle", principles: summary?.item?.kind === "principle", map: false } : sections,
    ...(byline.trim() ? { byline: byline.trim() } : {}),
  });

  const save = async (destination: "folder" | "zip") => {
    setBusy(destination); setError(null); setSaved(null);
    try { const result = await bridge.shareExport({ ...request(), destination }); if (result) setSaved(result.path); }
    catch (cause) { setError(errorMessage(cause, t("scanning"))); }
    finally { setBusy(null); }
  };
  const copyText = async () => {
    setBusy("copy"); setError(null); setCopiedText(false);
    try { const { text } = await bridge.shareCopyText(request()); await navigator.clipboard?.writeText(text); setCopiedText(true); }
    catch (cause) { setError(errorMessage(cause, t("copyText"))); }
    finally { setBusy(null); }
  };

  const publish = async (target: SharePublishTarget) => {
    setError(null); setRepoError(null); setProgress(null); setStep("progress");
    try {
      const result = await bridge.sharePublish({ ...request(), target }, event => setProgress(event));
      setPublished(result); setStep("published");
    } catch (cause) {
      const code = errorCode(cause);
      if (code === "name_taken") { setRepoError(t("nameTaken")); setStep("repo"); return; }
      if (code === "repo_gone" || code === "repo_mismatch") {
        setNewRepo(true); setStep("repo");
        if (code === "repo_mismatch") setRepoError(t("repoMismatch"));
        return;
      }
      if (code === "publish_denied" || code === "signed_out") setAccount(current => current && { ...current, login: null });
      setError(code === "offline" ? t("publishOffline") : code === "rate_limited" ? t("rateLimited") : code === "publish_denied" ? t("publishDenied") : errorMessage(cause, t("publishOffline")));
      setStep(item ? "options" : code === "publish_denied" || code === "signed_out" ? "options" : "repo");
    }
  };

  const startSignIn = (next: "repo" | "gist") => {
    const current = generation.current;
    setStep("signin"); setSignIn({ status: "starting" }); setError(null);
    void bridge.githubSignIn(event => {
      if (current !== generation.current) return;
      if (event.kind === "code") { setNow(Date.now()); setSignIn({ status: "code", userCode: event.userCode, expiresAt: Date.now() + event.expiresInSec * 1000 }); }
      else if (event.kind === "signed_in") {
        setAccount(previous => previous && { ...previous, login: event.login });
        setSignIn(null);
        if (next === "gist") void publish({ gist });
        else setStep("repo");
      } else if (event.kind === "failed") {
        if (event.reason === "expired") setSignIn({ status: "expired" });
        else if (event.reason === "denied") setSignIn({ status: "denied" });
        else { setSignIn(null); setStep("options"); setError(t(event.reason === "offline" ? "publishOffline" : "githubMissing")); }
      }
    }).then(result => { if (current === generation.current) setAccount(result); }).catch(cause => {
      if (current !== generation.current) return;
      setSignIn(null); setStep("options"); setError(errorMessage(cause, t("publishOffline")));
    });
  };
  const cancelSignIn = () => { void bridge.githubSignInCancel(); setSignIn(null); setStep("options"); };
  const startPublish = (next: "repo" | "gist") => {
    if (!account?.login) { startSignIn(next); return; }
    if (next === "gist") void publish({ gist }); else setStep("repo");
  };
  const signOut = async () => { setAccount(await bridge.githubSignOut()); };

  // "Rule “Trim when…” contains what looks like…": the kind in the app's words and the record's own title, never the matched text.
  const findingText = (finding: ShareFinding) => t(FINDING_KEYS[finding.class], { kind: t(`itemKind.${finding.itemKind}`), item: shortTitle(finding.itemLabel), symbol: finding.symbol ?? "" });
  const shownFindings = allFindings ? visibleFindings : visibleFindings.slice(0, FINDINGS_SHOWN);
  const leftOutLicense = counts?.leftOutLicense ? <p className="share-left-out" data-share-left-out-license>{t("leftOutLicense", { count: counts.leftOutLicense, formattedCount: formatNumber(counts.leftOutLicense) })}</p> : null;
  const countCell = (value: number | undefined, label?: string) => !summary ? (slow ? <span className="share-skeleton" aria-hidden="true" /> : null) : label ? <span className="share-count">{label}</span> : <span className="share-count gg-num">{formatNumber(value ?? 0)}</span>;
  const rule = summary?.item?.kind === "principle";
  const title = item ? t(rule ? "shareRule" : "shareMethod") : t("sheetTitle", { name: summary?.loupistName ?? loupistName });
  const description = item ? t(rule ? "shareRuleBody" : "shareMethodBody") : t("sheetBody");

  const sectionRow = (section: ShareSectionId) => {
    const value = section === "profile" ? undefined : section === "map" ? counts?.names : counts?.[section];
    const note = section === "profile" ? t("profileNote") : section === "principles" ? t("principlesNote") : null;
    const labelId = `${ids.body}-${section}`;
    const noteId = `${labelId}-note`;
    const countId = `${labelId}-count`;
    const empty = section !== "profile" && summary !== null && !value;
    const described = [note ? noteId : null, section !== "profile" ? countId : null].filter(Boolean).join(" ") || undefined;
    return <li key={section} className="share-section-row" data-section={section}>
      <div className="share-section-text">
        <span id={labelId} className="share-section-label">{t(section)}</span>
        {note ? <span id={noteId} className="share-section-note">{note}</span> : null}
        {section === "map" ? <span id={countId} className="share-section-note">{counts ? <span className="gg-num">{t("mapCounts", { names: formatNumber(counts.names), links: formatNumber(counts.links), sources: formatNumber(counts.sources) })}</span> : countCell(undefined)}</span> : null}
        {section === "map" && counts?.leftOut ? <span className="share-left-out">{t("leftOut", { formattedCount: formatNumber(counts.leftOut) })}</span> : null}
      </div>
      {section !== "profile" && section !== "map" ? <span id={countId}>{countCell(value)}</span> : null}
      <Switch aria-labelledby={labelId} aria-describedby={described} checked={sections[section]} disabled={!summary || empty}
        onCheckedChange={checked => setSections(current => ({ ...current, [section]: checked }))} />
    </li>;
  };

  const savedLine = saved ? <p className="share-saved" role="status"><CheckIcon aria-hidden="true" />{t("saved", { path: saved })}<Button variant="link" size="sm" onClick={() => void bridge.shareReveal({ loupistId })}>{t("showInFinder")}</Button></p> : null;
  const signedIn = account?.login ? <p className="share-github-line">{t("githubAs", { login: account.login })} · <Button variant="link" size="sm" onClick={() => void signOut()}>{t("signOut")}</Button></p> : null;
  const reasonLine = disabledReason ? <p id={ids.reason} className="share-disabled-reason" data-share-disabled-reason>{disabledReason}</p> : null;
  const optionsFooter = <DialogFooter className="share-footer">
    {error ? <p className="share-error" role="alert">{error}</p> : null}
    {item ? <>
      {account?.clientConfigured ? <>
        <div className="share-gist-choice">
          <p id={`${ids.gist}-note`} className="share-gist-note">{t("gistNote")}</p>
          <RadioGroup className="share-gist-options" aria-label={t("gistChoice")} aria-describedby={`${ids.gist}-note`} value={gist} onValueChange={value => setGist(value as "public" | "secret")}>
            {(["public", "secret"] as const).map(choice => <div key={choice} className="share-radio-row">
              <RadioGroupItem id={`${ids.gist}-${choice}`} value={choice} />
              <label htmlFor={`${ids.gist}-${choice}`}>{t(choice === "public" ? "gistPublic" : "gistSecret")}</label>
            </div>)}
          </RadioGroup>
        </div>
        <Button size="lg" className="share-primary" disabled={outputsDisabled} {...reasonProps} onClick={() => startPublish("gist")}>{t("gist")}</Button>
      </> : account ? <p className="share-github-missing">{t("githubMissing")}</p> : null}
      <div className="share-outputs">
        <Button variant="outline" disabled={outputsDisabled} {...reasonProps} onClick={() => void save("zip")}>{busy === "zip" ? <Spinner /> : null}{t("saveZip")}</Button>
        <Button variant="outline" disabled={outputsDisabled} {...reasonProps} onClick={() => void copyText()}>{busy === "copy" ? <Spinner /> : null}{t("copyText")}</Button>
      </div>
      {reasonLine}
      {copiedText ? <p className="share-saved" role="status"><CheckIcon aria-hidden="true" />{i18n.t("common:copyTextButton.copied")}</p> : null}
    </> : <>
      {account?.clientConfigured ? <Button size="lg" className="share-primary" disabled={outputsDisabled} {...reasonProps} data-share-publish onClick={() => startPublish("repo")}>{t("publish")}</Button>
        : account ? <p className="share-github-missing" data-github-missing>{t("githubMissing")}</p> : null}
      <div className="share-outputs">
        <Button variant="outline" disabled={outputsDisabled} {...reasonProps} data-share-save="folder" onClick={() => void save("folder")}>{busy === "folder" ? <Spinner /> : null}{t("saveFolder")}</Button>
        <Button variant="outline" disabled={outputsDisabled} {...reasonProps} data-share-save="zip" onClick={() => void save("zip")}>{busy === "zip" ? <Spinner /> : null}{t("saveZip")}</Button>
      </div>
      {reasonLine}
    </>}
    {savedLine}
    {signedIn}
  </DialogFooter>;

  const minutes = signIn?.status === "code" ? Math.max(1, Math.ceil((signIn.expiresAt - now) / 60_000)) : 0;
  return <Dialog open={open} onOpenChange={next => { if (step !== "progress") onOpenChange(next); }}>
    <DialogContent className="share-sheet" showCloseButton={step !== "progress"} onOpenAutoFocus={focusDialogTitle} aria-describedby={ids.body} data-share-sheet={scope.kind} data-step={step}>
      {step === "signin" ? <div className="share-step" key="signin" ref={stepRef}>
        <DialogHeader><DialogTitle>{t("ghTitle")}</DialogTitle></DialogHeader>
        <DialogBody className="share-signin">
          <p id={ids.body}>{t("ghEnterCode")}</p>
          {signIn?.status === "code" ? <>
            <p className="share-device-code gg-num" aria-label={[...signIn.userCode].join(" ")} data-device-code>{signIn.userCode}</p>
            <Button size="lg" className="share-primary" onClick={() => { void navigator.clipboard?.writeText(signIn.userCode).catch(() => undefined); void bridge.githubSignInOpen(); }}>{t("ghCopyOpen")}</Button>
            <p className="share-status" role="status"><Spinner className="share-spinner" />{t("ghWaiting")} · {t("ghExpiresIn", { minutes: formatNumber(minutes) })}</p>
          </> : signIn?.status === "expired" ? <>
            <p className="share-status" role="status">{t("ghExpired")}</p>
            <Button size="lg" className="share-primary" onClick={() => startSignIn(item ? "gist" : "repo")}>{t("ghNewCode")}</Button>
          </> : signIn?.status === "denied" ? <p className="share-status" role="status">{t("ghDenied")}</p> : <p className="share-status" role="status"><Spinner className="share-spinner" />{t("ghWaiting")}</p>}
          <p className="share-scope">{t("ghScope")}</p>
        </DialogBody>
        <DialogFooter className="share-footer"><Button variant="ghost" onClick={cancelSignIn}>{i18n.t("onboarding:appDialogs.cancel")}</Button></DialogFooter>
      </div> : step === "repo" ? <div className="share-step" key="repo" ref={stepRef}>
        <DialogHeader><DialogTitle>{title}</DialogTitle><DialogDescription id={ids.body}>{t("publicNote")}</DialogDescription></DialogHeader>
        <DialogBody className="share-repo">
          {summary?.repo && !newRepo ? null : <label className="share-field" htmlFor={ids.repo}><span>{t("repoName")}</span>
            <Input id={ids.repo} value={repoName} aria-invalid={Boolean(repoError) || undefined} aria-describedby={repoError ? `${ids.repo}-error` : undefined} onChange={event => { setRepoName(event.target.value); setRepoError(null); }} autoFocus />
            {repoError ? <span id={`${ids.repo}-error`} className="share-field-error" role="alert">{repoError}</span> : null}
          </label>}
        </DialogBody>
        <DialogFooter className="share-footer">
          {error ? <p className="share-error" role="alert">{error}</p> : null}
          {summary?.repo && !newRepo
            ? <Button size="lg" className="share-primary" onClick={() => void publish({ update: true })}>{t("updateRepo", { repo: summary.repo.url.replace(/^https:\/\//, ""), version: formatNumber(summary.nextVersion) })}</Button>
            : <Button size="lg" className="share-primary" disabled={!repoName.trim()} onClick={() => void publish({ repo: { name: repoName.trim() } })}>{t("publish")}</Button>}
          <div className="share-step-actions">
            <Button variant="ghost" data-share-back onClick={() => { setNewRepo(false); setRepoError(null); setError(null); setStep("options"); }}><ArrowLeftIcon aria-hidden="true" />{t("back")}</Button>
            {summary?.repo && !newRepo ? <Button variant="ghost" onClick={() => setNewRepo(true)}>{t("newRepo")}</Button> : null}
          </div>
          {signedIn}
        </DialogFooter>
      </div> : step === "progress" ? <div className="share-step" key="progress" ref={stepRef}>
        <DialogHeader><DialogTitle>{title}</DialogTitle><DialogDescription id={ids.body}>{description}</DialogDescription></DialogHeader>
        <DialogFooter className="share-footer">
          <p className="share-status" role="status" data-share-progress><Spinner className="share-spinner" />{progress?.kind === "uploading" ? t("uploading", { formattedCount: formatNumber(progress.files) }) : t("creating")}</p>
        </DialogFooter>
      </div> : step === "published" && published ? <div className="share-step" key="published" ref={stepRef}>
        <DialogHeader><DialogTitle>{title}</DialogTitle><DialogDescription id={ids.body}>{description}</DialogDescription></DialogHeader>
        <DialogBody>
          <p className="share-published" role="status" data-share-published><CheckIcon aria-hidden="true" />{t("published", { url: published.url.replace(/^https:\/\//, "") })}</p>
          {published.fullName && account?.clientConfigured && account.indexConfigured ? <GalleryListing bridge={bridge} loupistId={loupistId} termsUrl={account.galleryTermsUrl} /> : null}
        </DialogBody>
        <DialogFooter className="share-footer">
          <div className="share-outputs">
            <Button variant="outline" onClick={() => { void navigator.clipboard?.writeText(published.url).catch(() => undefined); setLinkCopied(true); }}>{t("copyLink")}</Button>
            <Button variant="ghost" onClick={() => void bridge.openExternalUrl(published.url)}>{t("openGithub")}<ExternalLinkIcon aria-hidden="true" /></Button>
          </div>
          {linkCopied ? <p className="share-saved" role="status">{t("copied")}</p> : null}
          <Button className="share-primary" onClick={() => onOpenChange(false)}>{i18n.t("connections:marketplaceSetupDialog.done")}</Button>
        </DialogFooter>
      </div> : <div className="share-step" key="options" ref={stepRef}>
        <DialogHeader><DialogTitle>{title}</DialogTitle><DialogDescription id={ids.body}>{description}</DialogDescription></DialogHeader>
        <DialogBody className="share-body">
          {loadError ? <p className="share-error" role="alert">{loadError}</p> : null}
          {!summary && !loadError && slow ? <p className="share-status" role="status"><Spinner className="share-spinner" />{t("scanning")}</p> : null}
          {item ? <div>
            <p className="share-item-row"><span className="share-section-label">{summary?.item?.title ?? null}</span>{summary?.item ? <span className="share-section-note">{t(`itemKind.${summary.item.kind}`)}</span> : slow ? <span className="share-skeleton" aria-hidden="true" /> : null}</p>
            {leftOutLicense}
          </div>
            : nothing ? <p className="share-nothing" data-share-nothing>{t("nothing")}</p> : <section aria-labelledby={`${ids.body}-included`}>
              <h3 id={`${ids.body}-included`} className="share-label">{t("included")}</h3>
              <ul className="share-sections">{SECTIONS.map(sectionRow)}</ul>
              {leftOutLicense}
            </section>}
          {visibleFindings.length ? <section className="share-findings" aria-labelledby={`${ids.body}-check`} data-share-findings>
            <h3 id={`${ids.body}-check`} className="share-label is-warning">{t("checkTitle", { formattedCount: formatNumber(visibleFindings.length) })}</h3>
            <ul>{shownFindings.map(finding => <li key={finding.id} className="share-finding" data-blocking={finding.blocking || undefined} data-finding-class={finding.class}>
              <span>{findingText(finding)}</span>
              <span className="share-finding-actions">
                <Button variant="outline" size="sm" onClick={() => setExcluded(current => [...current, finding.itemKey])}>{t("leaveOut")}</Button>
                {finding.class === "personal" ? <Button variant="ghost" size="sm" onClick={() => setAcknowledged(current => [...current, finding.id])}>{t("itsFine")}</Button> : null}
              </span>
            </li>)}</ul>
            {!allFindings && visibleFindings.length > FINDINGS_SHOWN ? <Button variant="link" size="sm" onClick={() => setAllFindings(true)}>{t("showMore", { formattedCount: formatNumber(visibleFindings.length - FINDINGS_SHOWN) })}</Button> : null}
          </section> : null}
          {summary && !nothing ? <div className="share-fields">
            {conflict ? <section className="share-license-conflict" aria-labelledby={`${ids.license}-label`} data-share-license-conflict>
              <h3 id={`${ids.license}-label`} className="share-label">{t("licenseLabel")}</h3>
              <p className="share-license-note">{t("licenseConflict")}</p>
              <ul>{conflictItems.map(row => <li key={row.key} className="share-conflict-item">
                <span className="share-conflict-text"><span>{shortTitle(row.name)}</span><span className="share-section-note">{row.licenses.map(licenseName).join(", ")}</span></span>
                <Button variant="outline" size="sm" onClick={() => leaveOut(row.key)}>{t("leaveOut")}</Button>
              </li>)}</ul>
            </section> : <div className="share-field">
              <label className="share-label" htmlFor={ids.license}>{t("licenseLabel")}</label>
              <NativeSelect id={ids.license} value={license} aria-describedby={[license ? `${ids.license}-summary` : null, fixedLicense ? `${ids.license}-fixed` : null].filter(Boolean).join(" ") || undefined}
                disabled={fixedLicense} onChange={event => setLicense(event.target.value as PackLicense)}>
                {summary.allowedLicenses.map(({ id }) => <option key={id} value={id}>{licenseName(id)}</option>)}
              </NativeSelect>
              {/* The chosen license's name opens its legal text (Rust supplies each allowed license's URL), then one plain line. */}
              {license ? <div className="share-license-about">
                {licenseUrl ? <Button variant="link" size="sm" className="share-license-link" title={licenseUrl} data-share-license-link onClick={() => void bridge.openExternalUrl(licenseUrl)}>{licenseName(license)}<ExternalLinkIcon aria-hidden="true" /></Button> : null}
                <p id={`${ids.license}-summary`} className="share-license-note" data-share-license-summary>{licenseSummary(license)}</p>
              </div> : null}
              {fixedLicense ? <p id={`${ids.license}-fixed`} className="share-left-out">{t("licenseFixed")}</p> : null}
            </div>}
            <label className="share-field" htmlFor={ids.byline}><span>{t("byline")}</span>
              <Input id={ids.byline} value={byline} maxLength={80} onChange={event => setByline(event.target.value)} />
            </label>
          </div> : null}
        </DialogBody>
        {optionsFooter}
      </div>}
    </DialogContent>
  </Dialog>;
}

/**
 * "List it in the Gallery" (W19.6): offered only when this build names listing
 * terms, and only after the user agrees to them. Rust records that acceptance,
 * adds this Loupist's entry to its fork of the community index and opens a pull
 * request; the index's automatic checks decide, and nobody reviews it by hand.
 */
function GalleryListing({ bridge, loupistId, termsUrl }: { bridge: LoupistBridge; loupistId: string; termsUrl: string | null }) {
  const { t } = useTranslation("sharing");
  const id = useId();
  const [listed, setListed] = useState<boolean | null>(null);
  const [agreed, setAgreed] = useState(false);
  const [request, setRequest] = useState<{ state: "busy" } | { state: "requested"; url: string } | { state: "failed"; message: string } | null>(null);
  useEffect(() => {
    let live = true;
    void bridge.galleryListingStatus({ loupistId }).then(status => { if (live) setListed(status === "listed"); }).catch(() => { if (live) setListed(false); });
    return () => { live = false; };
  }, [bridge, loupistId]);
  const list = async () => {
    if (!termsUrl || !agreed) return;
    setRequest({ state: "busy" });
    // The acceptance names the exact terms shown here; Rust refuses any other.
    try { setRequest({ state: "requested", url: (await bridge.galleryListingRequest({ loupistId, acceptedTerms: true, termsUrl })).url }); }
    catch (cause) {
      const code = errorCode(cause);
      setRequest({ state: "failed", message: code === "offline" ? t("publishOffline") : code === "rate_limited" ? t("rateLimited") : code === "publish_denied" ? t("publishDenied") : t("listFailed") });
    }
  };
  if (listed === null) return null;
  if (listed) return <p className="share-published" role="status" data-gallery-listed><CheckIcon aria-hidden="true" />{t("listed")}</p>;
  if (!termsUrl) return null;
  if (request?.state === "requested") return <div className="share-listing" data-gallery-listing>
    <p className="share-saved" role="status"><CheckIcon aria-hidden="true" />{t("listRequested")}
      <Button variant="link" size="sm" onClick={() => void bridge.openExternalUrl(request.url)}>{t("openGithub")}<ExternalLinkIcon aria-hidden="true" /></Button></p>
  </div>;
  const busy = request?.state === "busy";
  return <section className="share-listing" aria-labelledby={`${id}-title`} data-gallery-listing>
    <h3 id={`${id}-title`} className="share-label">{t("galleryTitle")}</h3>
    <div className="share-listing-choice">
      <input id={id} type="checkbox" checked={agreed} disabled={busy} onChange={event => setAgreed(event.target.checked)} data-gallery-terms />
      <label htmlFor={id}><Trans ns="sharing" i18nKey="listingTerms" components={[<TermsLink onOpen={() => void bridge.openExternalUrl(termsUrl)} />]} /></label>
    </div>
    <Button variant="outline" className="share-listing-request" disabled={!agreed || busy} aria-describedby={`${id}-note`} data-gallery-list onClick={() => void list()}>{busy ? <Spinner /> : null}{t("listInGallery")}</Button>
    <p id={`${id}-note`} className="share-section-note">{t("listNote")}</p>
    {request?.state === "failed" ? <p className="share-error" role="alert">{request.message}</p> : null}
  </section>;
}

/** The terms phrase inside the agreement sentence opens the terms; clicking it never ticks the box. */
function TermsLink({ onOpen, children }: { onOpen: () => void; children?: ReactNode }) {
  return <Button variant="link" size="sm" className="share-terms-link" onClick={(event: MouseEvent) => { event.preventDefault(); onOpen(); }}>{children}</Button>;
}
