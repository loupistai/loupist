import { useEffect, useId, useMemo, useRef, useState } from "react";
import { useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import { LEARNING_PROGRAM_LIMITS, type ProgramField } from "../../contracts/scheduled";
import type { LearningDepth, ReportPolicy, DeliveryTarget, TaskSubject } from "../../contracts/generated";
import type { LoupistBridge, ScheduledTask } from "../../types";
import { Button } from "../ui/button";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";
import { Input } from "../ui/input";
import { SegmentedControl, SegmentedControlItem } from "../ui/segmented-control";
import { SelectField, SelectItem } from "../ui/select";
import { Spinner } from "../ui/spinner";
import { Switch } from "../ui/switch";
import { Textarea } from "../ui/textarea";
import { LearningCadenceField } from "./LearningCadenceField";
import { briefBytes, draftProblems, patchIsEmpty, programDraft, programPatch, saveRefusal, type FieldProblems, type ProgramDraft } from "./learningProgramDraft";
import { learningDeliveryText, learningDepthText, learningReportText, subjectChipModel } from "./scheduledPresentation";

type ProblemKey = "titleRequired" | "titleTooLong" | "titleInvalid" | "promptRequired" | "promptTooLong" | "subjectsTooMany"
  | "scheduleRequired" | "scheduleInvalid" | "sourcesNotGranted" | "keepsInvalid" | "fallback";
const PROBLEM_KEYS: Partial<Record<string, ProblemKey>> = {
  "title:required": "titleRequired", "title:too_long": "titleTooLong", "title:invalid": "titleInvalid",
  "prompt:required": "promptRequired", "prompt:too_long": "promptTooLong",
  "subjects:too_many": "subjectsTooMany",
  "schedule:required": "scheduleRequired", "schedule:invalid": "scheduleInvalid",
  "keeps:invalid": "keepsInvalid",
  // The host refuses Sources only for a scope wider than the Loupist grants, and Keeps only for asset reads without an asset.
  "sources:refused": "sourcesNotGranted", "keeps:refused": "keepsInvalid",
};
const DEPTHS: LearningDepth[] = ["light", "standard", "deep"];
const REPORTS: ReportPolicy[] = ["when_new", "every_run", "on_condition"];
const DELIVERIES: DeliveryTarget[] = ["bot", "briefing", "none"];

/**
 * The program editor, opened from a program's detail in Schedule. A direct edit needs no proposal:
 * the user typed it. Save sends only the fields that changed with the revision the editor opened on;
 * the host checks them with the table `learning_program_save` uses and names the field it refuses.
 */
export function LearningProgramEditor({ bridge, task, names, open, onOpenChange, onSaved }: {
  bridge: LoupistBridge;
  task: ScheduledTask;
  /** Followed assets by id, in the interface language. */
  names: ReadonlyMap<string, string>;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSaved: (task: ScheduledTask) => void;
}) {
  const { t } = useTranslation("briefings");
  const id = useId();
  const [base, setBase] = useState(task);
  const [draft, setDraft] = useState<ProgramDraft>(() => programDraft(task));
  const [saving, setSaving] = useState(false);
  const [refused, setRefused] = useState<FieldProblems>({});
  const [stale, setStale] = useState(false);
  const [failed, setFailed] = useState(false);
  const [reloading, setReloading] = useState(false);
  const [discarding, setDiscarding] = useState(false);
  const [name, setName] = useState("");
  const titleInput = useRef<HTMLInputElement>(null);
  // A closed editor follows the stored task; an open one keeps the revision it was opened on.
  useEffect(() => { if (!open) { setBase(task); setDraft(programDraft(task)); setRefused({}); setStale(false); setFailed(false); setName(""); } }, [open, task]);
  const patch = useMemo(() => programPatch(base, draft), [base, draft]);
  const dirty = !patchIsEmpty(patch);
  const local = draftProblems(draft);
  const problems: FieldProblems = { ...refused, ...local };
  const blocked = Object.keys(local).length > 0;
  const paused = base.status === "paused";
  const noAccount = base.model_selection?.unavailable_reason === "no_connected_account";
  const problemText = (field: ProgramField) => {
    const reason = problems[field];
    if (!reason) return null;
    const key: ProblemKey = PROBLEM_KEYS[`${field}:${reason}`] ?? "fallback";
    return t(`programs.problems.${key}`, { formattedCount: formatNumber(LEARNING_PROGRAM_LIMITS.subjects) });
  };
  const change = <K extends keyof ProgramDraft>(key: K, value: ProgramDraft[K], field: ProgramField) => {
    setDraft(current => ({ ...current, [key]: value }));
    setRefused(current => { const { [field]: _gone, ...rest } = current; return rest; });
    setFailed(false);
  };
  const requestClose = () => {
    if (saving) return;
    if (dirty) setDiscarding(true);
    else onOpenChange(false);
  };
  const save = async () => {
    if (!dirty || blocked || saving || stale || !bridge.learningProgramUpdate) return;
    setSaving(true); setFailed(false); setRefused({});
    try {
      const saved = await bridge.learningProgramUpdate({ loupistId: base.loupist_id, taskId: base.id, revision: base.revision, patch });
      onSaved(saved);
      onOpenChange(false);
    } catch (cause) {
      const refusal = saveRefusal(cause);
      if (refusal.kind === "stale") setStale(true);
      else if (refusal.kind === "field") setRefused({ [refusal.field]: "refused" });
      else setFailed(true);
    } finally { setSaving(false); }
  };
  const reload = async () => {
    setReloading(true);
    try {
      const fresh = await bridge.scheduledTaskGet({ loupist_id: base.loupist_id, task_id: base.id });
      if (fresh) { setBase(fresh); setDraft(programDraft(fresh)); setStale(false); setRefused({}); onSaved(fresh); }
    } catch { setFailed(true); }
    finally { setReloading(false); }
  };
  const addSubject = (subject: TaskSubject) => change("subjects", [...draft.subjects, subject], "subjects");
  const addName = () => {
    const value = name.trim();
    if (!value) return;
    addSubject({ name: value });
    setName("");
  };
  const chips = subjectChipModel(draft.subjects, names, draft.subjects.length).shown;
  const followed = [...names].filter(([assetId]) => !draft.subjects.some(subject => "asset" in subject && subject.asset === assetId));
  const selected = draft.connections === "all_enabled" ? null : draft.connections;
  const storedSelection = base.program && base.program.scope.connections !== "all_enabled" ? base.program.scope.connections : null;
  const fieldProps = (field: ProgramField) => ({ "aria-invalid": problems[field] ? true : undefined, "aria-describedby": problems[field] ? `${id}-${field}-problem` : undefined });
  const problemLine = (field: ProgramField) => problems[field] ? <p id={`${id}-${field}-problem`} className="inline-error" role="alert" data-program-problem={field}>{problemText(field)}</p> : null;
  const used = briefBytes(draft.prompt);
  const disabled = saving || stale;

  return <Dialog open={open} onOpenChange={value => { if (value) onOpenChange(true); else requestClose(); }}>
    <DialogContent size="lg" className="dialog-form" data-program-editor showCloseButton={!saving}
      onOpenAutoFocus={event => { event.preventDefault(); titleInput.current?.focus(); }}
      onEscapeKeyDown={event => { if (discarding) return; event.preventDefault(); requestClose(); }}
      onInteractOutside={event => { event.preventDefault(); requestClose(); }}>
      <form className="dialog-form" noValidate onSubmit={event => { event.preventDefault(); void save(); }}>
        <DialogHeader>
          <DialogTitle>{t("programs.editor.heading")}</DialogTitle>
          <DialogDescription>{t("programs.editor.description")}</DialogDescription>
        </DialogHeader>
        <DialogBody>
          {stale ? <p className="inline-error" role="alert" data-program-stale>{t("programs.editor.stale")} <Button type="button" variant="link" size="sm" disabled={reloading} onClick={() => void reload()}>{reloading ? <Spinner /> : null}{t("programs.editor.reload")}</Button></p> : null}
          {paused ? <p className="briefings-meta" role="status" data-program-paused>{t("programs.editor.paused")}</p> : null}
          {noAccount ? <p className="briefings-meta" role="status" data-program-no-account>{t("programs.editor.noAccount")}</p> : null}

          <div className="form-field" data-program-field="title">
            <label htmlFor={`${id}-title`}>{t("programs.editor.title")}</label>
            <Input id={`${id}-title`} ref={titleInput} value={draft.title} disabled={disabled} {...fieldProps("title")} onChange={event => change("title", event.currentTarget.value, "title")} />
            {problemLine("title")}
          </div>

          <div className="form-field" data-program-field="prompt">
            <label htmlFor={`${id}-prompt`}>{t("schedule.learning.what")}</label>
            <Textarea id={`${id}-prompt`} value={draft.prompt} rows={6} disabled={disabled} {...fieldProps("prompt")}
              onChange={event => change("prompt", event.currentTarget.value, "prompt")}
              onKeyDown={event => { if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) { event.preventDefault(); void save(); } }} />
            <p className="field-hint" data-program-brief-size>{t("programs.editor.briefSize", { used: formatNumber(used / 1024, { maximumFractionDigits: 1 }), limit: formatNumber(LEARNING_PROGRAM_LIMITS.briefBytes / 1024) })}</p>
            {problemLine("prompt")}
          </div>

          <div className="form-field" role="group" aria-labelledby={`${id}-about`} data-program-field="subjects">
            <span id={`${id}-about`} className="schedule-section-label">{t("schedule.learning.about")}</span>
            <p className="field-hint">{t("programs.editor.aboutHint", { formattedCount: formatNumber(LEARNING_PROGRAM_LIMITS.subjects) })}</p>
            {chips.length ? <ul className="schedule-subject-chips">{chips.map((chip, index) => <li key={chip.key}>
              <span className="schedule-subject-chip">{chip.label}
                <Button type="button" variant="ghost" size="icon-sm" disabled={disabled} aria-label={t("programs.editor.removeSubject", { name: chip.label })}
                  onClick={() => change("subjects", draft.subjects.filter((_, at) => at !== index), "subjects")}>×</Button></span>
            </li>)}</ul> : null}
            <div className="schedule-subject-chips">
              <Input aria-label={t("programs.editor.addName")} placeholder={t("programs.editor.addNamePlaceholder")} value={name} disabled={disabled}
                onChange={event => setName(event.currentTarget.value)}
                onKeyDown={event => { if (event.key === "Enter") { event.preventDefault(); addName(); } }} />
              <Button type="button" variant="outline" size="sm" disabled={disabled || !name.trim()} onClick={addName}>{t("programs.editor.add")}</Button>
              {followed.length ? <SelectField compact aria-label={t("programs.editor.addAsset")} placeholder={t("programs.editor.addAsset")} value="" disabled={disabled}
                onValueChange={assetId => { if (assetId) addSubject({ asset: assetId }); }}>
                {followed.map(([assetId, label]) => <SelectItem key={assetId} value={assetId}>{label}</SelectItem>)}
              </SelectField> : null}
            </div>
            {problemLine("subjects")}
          </div>

          <LearningCadenceField value={draft.schedule} original={base.schedule} disabled={disabled} problem={problemText("schedule")}
            onChange={schedule => change("schedule", schedule, "schedule")} />

          <div className="form-field" role="group" aria-labelledby={`${id}-depth`} data-program-field="depth">
            <span id={`${id}-depth`} className="schedule-section-label">{t("schedule.learning.depthLabel")}</span>
            <SegmentedControl value={draft.depth} disabled={disabled} aria-labelledby={`${id}-depth`} data-program-depth onValueChange={value => change("depth", value as LearningDepth, "depth")}>
              {DEPTHS.map(depth => <SegmentedControlItem key={depth} value={depth}>{learningDepthText(depth)}</SegmentedControlItem>)}
            </SegmentedControl>
            <p className="field-hint">{t(`programs.editor.depthHint.${draft.depth}`)}</p>
            {problemLine("depth")}
          </div>

          <div className="form-field" role="group" aria-labelledby={`${id}-sources`} data-program-field="sources">
            <span id={`${id}-sources`} className="schedule-section-label">{t("schedule.learning.sources")}</span>
            <SelectField aria-label={t("programs.editor.connections")} value={selected ? "chosen" : "all"} disabled={disabled || !storedSelection}
              onValueChange={value => change("connections", value === "all" ? "all_enabled" : [...(storedSelection ?? [])], "sources")}>
              <SelectItem value="all">{t("schedule.learning.scope.all")}</SelectItem>
              {storedSelection ? <SelectItem value="chosen">{t("programs.editor.chosenConnections", { count: storedSelection.length, formattedCount: formatNumber(storedSelection.length) })}</SelectItem> : null}
            </SelectField>
            <label className="loupist-learning-switch"><span>{t("programs.editor.web")}</span>
              <Switch checked={draft.web} disabled={disabled} onCheckedChange={web => change("web", web, "sources")} /></label>
            {problemLine("sources")}
          </div>

          <div className="form-field" role="group" aria-labelledby={`${id}-keeps`} data-program-field="keeps">
            <span id={`${id}-keeps`} className="schedule-section-label">{t("schedule.learning.keeps")}</span>
            {([["sources", "schedule.learning.keep.sources"], ["lens", "schedule.learning.keep.lens"], ["asset_reads", "schedule.learning.keep.reads"]] as const).map(([key, label]) =>
              <label key={key} className="loupist-learning-switch"><span>{t(label)}</span>
                <Switch checked={draft.keeps[key]} disabled={disabled} onCheckedChange={on => change("keeps", { ...draft.keeps, [key]: on }, "keeps")} /></label>)}
            {problemLine("keeps")}
          </div>

          <div className="form-field" data-program-field="report">
            <span className="schedule-section-label" id={`${id}-report`}>{t("schedule.learning.reports")}</span>
            <SelectField aria-labelledby={`${id}-report`} value={draft.report} disabled={disabled} {...fieldProps("report")} onValueChange={value => change("report", value as ReportPolicy, "report")}>
              {REPORTS.map(report => <SelectItem key={report} value={report}>{learningReportText(report)}</SelectItem>)}
            </SelectField>
            {problemLine("report")}
          </div>

          <div className="form-field" data-program-field="delivery">
            <span className="schedule-section-label" id={`${id}-delivery`}>{t("schedule.learning.delivers")}</span>
            <SelectField aria-labelledby={`${id}-delivery`} value={draft.delivery} disabled={disabled} {...fieldProps("delivery")} onValueChange={value => change("delivery", value as DeliveryTarget, "delivery")}>
              {DELIVERIES.map(to => <SelectItem key={to} value={to}>{learningDeliveryText(to)}</SelectItem>)}
            </SelectField>
            {problemLine("delivery")}
          </div>

          {failed ? <p className="inline-error" role="alert">{t("programs.editor.saveFailed")}</p> : null}
        </DialogBody>
        <DialogFooter>
          <Button type="button" variant="outline" disabled={saving} onClick={requestClose}>{t("schedule.cancel")}</Button>
          <Button type="submit" data-program-save disabled={!dirty || blocked || saving || stale}>{saving ? <><Spinner />{t("programs.editor.saving")}</> : t("programs.editor.save")}</Button>
        </DialogFooter>
      </form>
      <Dialog open={discarding} onOpenChange={setDiscarding}>
        <DialogContent size="sm" className="dialog-form" data-program-discard>
          <DialogHeader><DialogTitle>{t("programs.discard.title")}</DialogTitle><DialogDescription>{t("programs.discard.body")}</DialogDescription></DialogHeader>
          <DialogFooter>
            <Button variant="outline" autoFocus onClick={() => setDiscarding(false)}>{t("programs.discard.keep")}</Button>
            <Button variant="destructive" data-program-discard-confirm onClick={() => { setDiscarding(false); onOpenChange(false); }}>{t("programs.discard.discard")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </DialogContent>
  </Dialog>;
}
