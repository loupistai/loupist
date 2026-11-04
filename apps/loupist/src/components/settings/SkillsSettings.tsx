import { skillPresentation } from "../../lib/agentSkills";
import i18n, { useTranslation } from "../../i18n";
import { loupistOptionLabel } from "../../lib/loupistFolder";
import { useCallback, useEffect, useRef, useState } from "react";
import { BookOpenIcon, FolderOpenIcon, PlusIcon, RefreshCwIcon, SearchIcon } from "lucide-react";
import type { ConfigScope, LoupistSummary, LoupistBridge, ManagedSkill, SkillCatalog, SkillDocument } from "../../types";
import { CONFIG_CHANGED_EVENT, notifyConfigurationChanged } from "../../app/useConfiguration";
import { errorMessage } from "../../lib/errors";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Textarea } from "../ui/textarea";
import { NativeSelect } from "../ui/native-select";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription, DialogBody, DialogFooter } from "../ui/dialog";
import { SegmentedControl, SegmentedControlItem } from "../ui/segmented-control";
import { ConfigurationDiagnostics } from "./AgentSettings";

const keyOf = (skill: ManagedSkill) => skill.path ?? skill.id;
const origin = (skill: ManagedSkill) => skill.source === "bundled" ? i18n.t("settings:skillsSettings.includedWithLoupist") : skill.source === "personal" ? i18n.t("settings:skillsSettings.personal") : i18n.t("settings:skillsSettings.thisLoupist");
export function SkillsSettings({ bridge, loupists, scope, onScopeChange, onDraftSkill }: {
  bridge: LoupistBridge; loupists: LoupistSummary[]; scope: ConfigScope; onScopeChange?: (scope: ConfigScope) => void;
  onDraftSkill?: (scope: ConfigScope, brief: string) => void | Promise<void>;
}) {
  useTranslation("settings");
  const [catalog, setCatalog] = useState<SkillCatalog | null>(null);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("all");
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const [detail, setDetail] = useState<SkillDocument | null>(null);
  const [editor, setEditor] = useState<"new" | "edit" | null>(null);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [instructions, setInstructions] = useState("");
  const [adding, setAdding] = useState(false);
  const [addKind, setAddKind] = useState<"github" | "zip" | "folder">("github");
  const [url, setUrl] = useState("");
  const [deleting, setDeleting] = useState(false);
  const scopeKey = scope.kind === "global" ? "global" : scope.loupistId;
  const current = useRef(scopeKey); current.current = scopeKey;
  const reload = useCallback(async () => {
    const key = scopeKey;
    try {
      const next = await bridge.skillsList({ scope });
      if (current.current === key) { setCatalog(next); setError(null); }
    } catch (cause) { if (current.current === key) setError(errorMessage(cause, i18n.t("settings:skillsSettings.couldNotLoadSkills"))); }
  }, [bridge, scopeKey]);
  useEffect(() => {
    setCatalog(null); setDetail(null); setEditor(null); setAdding(false); setDeleting(false); setStatus(""); setQuery("");
    void reload();
    const refresh = () => void reload();
    window.addEventListener("focus", refresh); window.addEventListener(CONFIG_CHANGED_EVENT, refresh);
    return () => { window.removeEventListener("focus", refresh); window.removeEventListener(CONFIG_CHANGED_EVENT, refresh); };
  }, [reload]);
  const mutate = async (action: () => Promise<SkillCatalog | null>, message: string) => {
    const key = scopeKey;
    setBusy(true); setError(null); setStatus("");
    try {
      const next = await action();
      if (next && current.current === key) { setCatalog(next); setStatus(message); setEditor(null); setAdding(false); setDeleting(false); setDetail(null); }
      if (next) notifyConfigurationChanged();
      return Boolean(next);
    } catch (cause) { if (current.current === key) setError(errorMessage(cause, i18n.t("settings:skillsSettings.couldNotUpdateThisSkill"))); return false; }
    finally { setBusy(false); }
  };
  const preference = async (skill: ManagedSkill, reset = false) => {
    if (!catalog) return;
    await mutate(async () => {
      await bridge.configUpdate({ scope, expectedRevision: catalog.configuration.revision,
        set: reset ? {} : { skills: { [skill.id]: { enabled: !skill.enabled } } },
        unset: reset ? [["skills", skill.id, "enabled"]] : [] });
      return bridge.skillsList({ scope });
    }, reset ? i18n.t(scope.kind === "global" ? "settings:skillsSettings.appDefaults" : "settings:skillsSettings.globalDefaults", { name: skillPresentation(skill).name }) : i18n.t(skill.enabled ? "settings:skillsSettings.disabledNotice" : "settings:skillsSettings.enabledNotice", { name: skillPresentation(skill).name }));
  };
  const open = async (skill: ManagedSkill) => {
    setBusy(true); setError(null); const key = scopeKey;
    try { const doc = await bridge.skillsRead({ scope, key: keyOf(skill) }); if (current.current === key) setDetail(doc); }
    catch (cause) { setError(errorMessage(cause, i18n.t("settings:skillsSettings.couldNotOpenThisSkill"))); }
    finally { setBusy(false); }
  };
  const beginCreate = () => { setName(""); setDescription(""); setInstructions(""); setError(null); setEditor("new"); };
  const startEdit = () => { if (!detail) return; setName(detail.skill.name); setDescription(detail.skill.description); setInstructions(detail.instructions); setEditor("edit"); };
  const draftWithLoupist = async () => {
    if (!onDraftSkill) return;
    setBusy(true); setError(null);
    try {
      await onDraftSkill(scope, [name, description, instructions].filter(Boolean).join("\n\n"));
      setEditor(null);
    } catch (cause) { setError(errorMessage(cause, i18n.t("settings:skillsSettings.couldNotOpenASkillDraft"))); }
    finally { setBusy(false); }
  };
  const skills = catalog?.skills.filter((skill) =>
    (filter === "all" || filter === "added" && skill.source !== "bundled" || filter === "included" && skill.source === "bundled" || filter === "enabled" && skill.enabled)
    && `${skillPresentation(skill).name} ${skillPresentation(skill).description}`.toLowerCase().includes(query.toLowerCase()))
    .sort((a, b) => Number(a.fixed) - Number(b.fixed)) ?? [];
  const disabled = busy || Boolean(catalog?.configuration.diagnostics.length);
  const errorNode = error ? <p className="inline-error" role="alert">{error}</p> : null;
  const editorOpen = editor !== null;
  return <div className="skills-settings">
    {onScopeChange ? <div className="skills-scope"><div><label htmlFor="skills-scope">{i18n.t("settings:skillsSettings.availableTo")}</label><p>{scope.kind === "global" ? i18n.t("settings:skillsSettings.personalSkillsAndDefaultsForAllYourLoupists") : i18n.t("settings:skillsSettings.skillsAndOverridesForThisLoupistPersonalSkills")}</p></div>
      <NativeSelect id="skills-scope" disabled={busy} value={scopeKey} onChange={(event) => onScopeChange(event.target.value === "global" ? { kind: "global" } : { kind: "project", loupistId: event.target.value })}><option value="global">{i18n.t("settings:skillsSettings.allLoupistsPersonal")}</option>{loupists.map((loupist) => <option key={loupist.id} value={loupist.id} disabled={loupist.availability.status !== "available"}>{loupistOptionLabel(loupist, loupists)}</option>)}</NativeSelect>
    </div> : null}
    <div className="skills-toolbar"><div className="skills-search"><SearchIcon aria-hidden="true" /><Input aria-label={i18n.t("settings:skillsSettings.searchSkills")} placeholder={i18n.t("settings:skillsSettings.searchSkillsPlaceholder")} value={query} onChange={(event) => setQuery(event.target.value)} /></div>
      <Button variant="outline" disabled={disabled} onClick={() => { setError(null); setAdding(true); }}><PlusIcon /> {i18n.t("settings:skillsSettings.addSkill")}</Button><Button disabled={disabled} onClick={beginCreate}>{i18n.t("settings:skillsSettings.createSkill")}</Button>
      <Button variant="ghost" size="icon" aria-label={i18n.t("settings:skillsSettings.reloadSkills")} disabled={busy} onClick={() => void reload()}><RefreshCwIcon /></Button>
    </div>
    <SegmentedControl value={filter} onValueChange={setFilter} aria-label={i18n.t("settings:skillsSettings.skillFilter")}><SegmentedControlItem value="all">{i18n.t("settings:skillsSettings.all")}</SegmentedControlItem><SegmentedControlItem value="enabled">{i18n.t("settings:skillsSettings.enabled")}</SegmentedControlItem><SegmentedControlItem value="added">{i18n.t("settings:skillsSettings.addedByYou")}</SegmentedControlItem><SegmentedControlItem value="included">{i18n.t("settings:skillsSettings.included")}</SegmentedControlItem></SegmentedControl>
    <p className="skills-status" role="status">{busy ? i18n.t("settings:skillsSettings.working") : status || i18n.t("settings:skillsSettings.skillsProvideReusableInstructionsEnabledSkillsAreAvailable")}</p>
    {!editorOpen && !adding && !detail ? errorNode : null}
    <ConfigurationDiagnostics snapshot={catalog?.configuration ?? null} />
    {!catalog ? !error ? <p role="status">{i18n.t("settings:skillsSettings.loadingSkills")}</p> : null : skills.length ? <ul className="skills-list">{skills.map((skill) => {
      const explicit = catalog.configuration.explicit?.skills?.[skill.id]?.enabled !== undefined;
      const source = catalog.configuration.sources[`skills.${skill.id}.enabled`];
      return <li key={keyOf(skill)} className="skill-row"><button className="skill-summary" disabled={busy} onClick={() => void open(skill)}><span className="skill-title"><BookOpenIcon aria-hidden="true" />{skillPresentation(skill).name}</span><span className="skill-description">{skillPresentation(skill).description || skill.error}</span><span className="skill-origin">{skill.fixed ? i18n.t("settings:skillsSettings.originAlways", { origin: origin(skill) }) : skill.shadowed ? i18n.t("settings:skillsSettings.originShadowed", { origin: origin(skill) }) : source === "project" ? i18n.t("settings:skillsSettings.originProject", { origin: origin(skill) }) : source === "global" ? i18n.t("settings:skillsSettings.originGlobal", { origin: origin(skill) }) : origin(skill)}</span></button><div className="skill-actions">{!skill.fixed ? <><Button variant="outline" size="sm" role="switch" aria-checked={skill.enabled} aria-label={skill.enabled ? i18n.t("settings:skillsSettings.enabledLabel", { name: skillPresentation(skill).name }) : i18n.t("settings:skillsSettings.disabledLabel", { name: skillPresentation(skill).name })} disabled={disabled || skill.shadowed || Boolean(skill.error)} onClick={() => void preference(skill)}>{skill.enabled ? i18n.t("settings:skillsSettings.enabled") : i18n.t("settings:skillsSettings.disabled")}</Button>{explicit ? <Button variant="ghost" size="sm" disabled={disabled} aria-label={i18n.t("settings:skillsSettings.resetName", { name: skillPresentation(skill).name })} onClick={() => void preference(skill, true)}>{i18n.t("settings:skillsSettings.useDefault")}</Button> : null}</> : <span>{i18n.t("settings:skillsSettings.alwaysOn")}</span>}</div></li>;
    })}</ul> : <div className="skills-empty"><p>{query ? i18n.t("settings:skillsSettings.noMatchingSkills") : i18n.t("settings:skillsSettings.noSkillsInThisCollectionYet")}</p>{!query && filter === "added" ? <Button variant="outline" onClick={beginCreate}>{i18n.t("settings:skillsSettings.createYourFirstSkill")}</Button> : null}</div>}

    <div className="settings-footnote"><span>{i18n.t("settings:skillsSettings.changesApplyToTheNextTurn")}</span><Button variant="ghost" size="sm" onClick={() => void bridge.configReveal({ scope }).catch((cause) => setError(errorMessage(cause, i18n.t("settings:skillsSettings.couldNotOpenThePreferencesFile"))))}>{i18n.t("settings:skillsSettings.openPreferencesFile")}</Button></div>
    <Dialog open={Boolean(detail) && !editorOpen && !deleting} onOpenChange={(value) => { if (!value && !busy) setDetail(null); }}><DialogContent size="lg" className="dialog-form"><DialogHeader><DialogTitle>{detail && skillPresentation(detail.skill).name}</DialogTitle><DialogDescription>{detail && skillPresentation(detail.skill).description}</DialogDescription></DialogHeader><DialogBody>{errorNode}{detail?.skill.error ? <p className="inline-error" role="alert">{i18n.t("settings:skillsSettings.repairSkill", { error: detail.skill.error })}</p> : null}<p className="skill-origin">{detail && (detail.skill.fixed ? i18n.t("settings:skillsSettings.originAlways", { origin: origin(detail.skill) }) : origin(detail.skill))}</p><h3>{i18n.t("settings:skillsSettings.instructions")}</h3><pre className="skill-instructions">{detail?.instructions}</pre><details><summary>{i18n.t("settings:skillsSettings.includedFilesAndLocation")}</summary><ul>{detail?.files.map((file) => <li key={file}>{file}</li>)}</ul>{detail?.skill.path ? <p className="workspace-path">{detail.skill.path}</p> : null}</details></DialogBody><DialogFooter>{detail?.skill.path ? <Button variant="ghost" onClick={() => void bridge.skillsReveal({ scope, key: keyOf(detail.skill) }).catch((cause) => setError(errorMessage(cause, i18n.t("settings:skillsSettings.couldNotOpenTheSkillFolder"))))}><FolderOpenIcon /> {i18n.t("settings:skillsSettings.openFolder")}</Button> : null}{detail?.skill.editable ? <><Button variant="ghost" disabled={busy} onClick={() => setDeleting(true)}>{i18n.t("settings:skillsSettings.deleteSkill")}</Button>{!detail.skill.error ? <Button onClick={startEdit}>{i18n.t("settings:skillsSettings.editInstructions")}</Button> : null}</> : <Button variant="outline" onClick={() => setDetail(null)}>{i18n.t("settings:skillsSettings.done")}</Button>}</DialogFooter></DialogContent></Dialog>

    <Dialog open={editorOpen} onOpenChange={(value) => { if (!value && !busy) setEditor(null); }}><DialogContent size="lg" className="dialog-form" showCloseButton={!busy}><DialogHeader><DialogTitle>{editor === "new" ? i18n.t("settings:skillsSettings.createASkill") : i18n.t("settings:skillsSettings.editName", { name: name })}</DialogTitle><DialogDescription>{scope.kind === "global" ? i18n.t("settings:skillsSettings.saveAPersonalWorkflowForAllYourLoupists") : i18n.t("settings:skillsSettings.saveAWorkflowInThisLoupistSFolder")}</DialogDescription></DialogHeader><DialogBody>{errorNode}<div className="skill-editor-fields">{editor === "new" ? <label>{i18n.t("settings:skillsSettings.name")}<Input value={name} maxLength={256} onChange={(event) => setName(event.target.value)} placeholder={i18n.t("settings:skillsSettings.quarterlyEarningsReview")} disabled={busy} /></label> : null}<label>{i18n.t("settings:skillsSettings.whenToUseIt")}<Textarea value={description} onChange={(event) => setDescription(event.target.value)} maxLength={4096} placeholder={i18n.t("settings:skillsSettings.useWhenReviewingACompanySQuarterlyEarnings")} disabled={busy} /></label><label>{i18n.t("settings:skillsSettings.instructions")}<Textarea className="skill-instructions-editor" value={instructions} onChange={(event) => setInstructions(event.target.value)} placeholder={i18n.t("settings:skillsSettings.describeTheWorkflowConventionsAndChecksThatMatter")} disabled={busy} /></label></div></DialogBody><DialogFooter>{editor === "new" && onDraftSkill ? <Button variant="ghost" disabled={busy} onClick={() => void draftWithLoupist()}>{i18n.t("settings:skillsSettings.draftWithLoupist")}</Button> : null}<Button variant="outline" disabled={busy} onClick={() => setEditor(null)}>{i18n.t("settings:skillsSettings.cancel")}</Button><Button disabled={busy || !name.trim() || !description.trim() || !instructions.trim()} onClick={() => void mutate(() => editor === "new" ? bridge.skillsCreate({ scope, name, description, instructions }) : bridge.skillsSave({ scope, key: keyOf(detail!.skill), expectedRevision: detail!.skill.revision, description, instructions }), i18n.t("settings:skillsSettings.skillSavedAvailableOnTheNextTurn"))}>{busy ? i18n.t("settings:skillsSettings.saving") : i18n.t("settings:skillsSettings.saveSkill")}</Button></DialogFooter></DialogContent></Dialog>

    <Dialog open={adding} onOpenChange={(value) => { if (!busy) setAdding(value); }}><DialogContent size="md" className="dialog-form" showCloseButton={!busy}><DialogHeader><DialogTitle>{i18n.t("settings:skillsSettings.addASkill")}</DialogTitle><DialogDescription>{i18n.t("settings:skillsSettings.addASkillYouTrustIncludedFilesAre")}</DialogDescription></DialogHeader><DialogBody>{errorNode}<label>{i18n.t("settings:skillsSettings.source")}<NativeSelect value={addKind} disabled={busy} onChange={(event) => setAddKind(event.target.value as typeof addKind)}><option value="github">{i18n.t("settings:skillsSettings.publicGitHubFolder")}</option><option value="zip">{i18n.t("settings:skillsSettings.zipPackage")}</option><option value="folder">{i18n.t("settings:skillsSettings.localFolder")}</option></NativeSelect></label>{addKind === "github" ? <label>{i18n.t("settings:skillsSettings.gitHubFolderURL")}<Input value={url} disabled={busy} onChange={(event) => setUrl(event.target.value)} placeholder="https://github.com/owner/repo/tree/main/skills/example" /></label> : <p>{addKind === "zip" ? i18n.t("settings:skillsSettings.chooseZipDescription") : i18n.t("settings:skillsSettings.chooseFolderDescription")}</p>}<p className="skill-origin">{scope.kind === "global" ? i18n.t("settings:skillsSettings.personalDestination") : i18n.t("settings:skillsSettings.loupistDestination", { name: loupists.find((loupist) => scope.kind === "project" && loupist.id === scope.loupistId)?.name })}</p></DialogBody><DialogFooter><Button variant="outline" disabled={busy} onClick={() => setAdding(false)}>{i18n.t("settings:skillsSettings.cancel")}</Button><Button disabled={busy || addKind === "github" && !url.trim()} onClick={() => void mutate(() => bridge.skillsImport({ scope, kind: addKind, ...(addKind === "github" ? { url: url.trim() } : {}) }), i18n.t("settings:skillsSettings.skillAddedAvailableOnTheNextTurn"))}>{busy ? i18n.t("settings:skillsSettings.adding") : addKind === "github" ? i18n.t("settings:skillsSettings.addSkill") : addKind === "zip" ? i18n.t("settings:skillsSettings.chooseZip") : i18n.t("settings:skillsSettings.chooseFolder")}</Button></DialogFooter></DialogContent></Dialog>

    <Dialog open={deleting} onOpenChange={(value) => { if (!busy) setDeleting(value); }}><DialogContent size="sm" className="dialog-form"><DialogHeader><DialogTitle>{i18n.t("settings:skillsSettings.deleteTitle", { name: detail?.skill.name })}</DialogTitle><DialogDescription>{scope.kind === "global" ? i18n.t("settings:skillsSettings.deletePersonalDescription") : i18n.t("settings:skillsSettings.deleteLoupistDescription")}</DialogDescription></DialogHeader><DialogBody>{errorNode}</DialogBody><DialogFooter><Button variant="outline" disabled={busy} onClick={() => setDeleting(false)}>{i18n.t("settings:skillsSettings.cancel")}</Button><Button disabled={busy} onClick={() => detail && void mutate(() => bridge.skillsDelete({ scope, key: keyOf(detail.skill), expectedRevision: detail.skill.revision }), i18n.t("settings:skillsSettings.skillDeleted"))}>{i18n.t("settings:skillsSettings.deleteSkill")}</Button></DialogFooter></DialogContent></Dialog>
  </div>;
}
