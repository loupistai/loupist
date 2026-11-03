import i18n, { useTranslation } from "@/i18n";
import { useEffect, useId, useRef, useState, type Ref } from "react";
import { ChevronDownIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { formatDateTime, formatNumber } from "@/i18n/format";
import { focusForKeyboard, useInputModality } from "@/lib/inputModality";
import type { ChatArtifactVersion, ChatArtifactView, LoupistBridge } from "../../types";

const t = i18n.getFixedT(null, "chat");
const when = (ms: number) => formatDateTime(ms, { year: undefined, month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });

type Scope = { bridge: LoupistBridge; loupistId: string; /** The chat that made the item. */ threadId: string };

/**
 * "Version 2 ▾" in the Work status row (side-panel.md §3.2): the revisions Rust still keeps, newest first,
 * each with its date. Choosing an earlier one shows it read-only; choosing the current one returns to it.
 */
export function WorkVersionMenu({ bridge, loupistId, threadId, view, triggerRef, onChoose }: Scope & {
  view: ChatArtifactView;
  triggerRef?: Ref<HTMLButtonElement>;
  /** `null` is the current version. */
  onChoose: (revision: number | null) => void;
}) {
  useTranslation("chat");
  const [versions, setVersions] = useState<ChatArtifactVersion[] | null>(null);
  const [failed, setFailed] = useState(false);
  const request = useRef(0);
  // Closing the menu returns focus to its trigger; after a press that focus shows no ring.
  const modality = useInputModality();
  const current = view.artifact.current_revision;
  // Read on every open: a later answer or a restore may have added one.
  const load = () => {
    const id = ++request.current;
    setFailed(false);
    bridge.chatArtifactVersions(loupistId, threadId, view.artifact.id).then(
      (next) => { if (id === request.current) setVersions(next); },
      () => { if (id === request.current) setFailed(true); },
    );
  };
  return <DropdownMenu onOpenChange={(open) => { if (open) load(); }}>
    <DropdownMenuTrigger asChild>
      <Button ref={triggerRef} type="button" variant="ghost" size="sm" className="work-version-trigger" data-work-versions data-input={modality}>
        {t("workVersions.version", { formattedCount: formatNumber(view.revision.revision) })}<ChevronDownIcon aria-hidden="true" />
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="start" collisionPadding={8} className="work-version-menu" data-input={modality}>
      <DropdownMenuLabel>{t("workVersions.label")}</DropdownMenuLabel>
      {failed ? <p className="work-version-menu-state" role="alert">{t("workVersions.couldNotLoad")}</p>
        : !versions ? <p className="work-version-menu-state" role="status"><Spinner />{t("workVersions.loading")}</p>
        : <DropdownMenuRadioGroup value={String(view.revision.revision)} onValueChange={(value) => onChoose(Number(value) === current ? null : Number(value))}>
          {versions.map((version) => <DropdownMenuRadioItem key={version.revision_id} value={String(version.revision_id)} className="work-version-item">
            <span className="work-version-name">
              {t("workVersions.version", { formattedCount: formatNumber(version.revision_id) })}
              {version.revision_id === current ? <span className="work-version-current"> · {t("workVersions.current")}</span> : null}
            </span>
            <small>{[
              when(version.created_at_ms),
              version.restored_from_revision_id ? t("workVersions.restoredFrom", { formattedCount: formatNumber(version.restored_from_revision_id) }) : null,
            ].filter(Boolean).join(" · ")}</small>
          </DropdownMenuRadioItem>)}
        </DropdownMenuRadioGroup>}
    </DropdownMenuContent>
  </DropdownMenu>;
}

/**
 * Under the status row while an earlier version is shown: it is read-only, and "Restore this version" asks once
 * before Rust appends it as the newest revision. Answers keep the revisions they showed.
 */
export function WorkVersionBar({ bridge, loupistId, threadId, view, onBack, onRestored }: Scope & {
  view: ChatArtifactView;
  onBack: () => void;
  /** The new current revision and the version it came from. */
  onRestored: (restored: ChatArtifactView, from: number) => void;
}) {
  useTranslation("chat");
  const [step, setStep] = useState<"idle" | "confirm" | "restoring">("idle");
  const [failed, setFailed] = useState(false);
  const confirmRef = useRef<HTMLButtonElement>(null);
  const restoreRef = useRef<HTMLButtonElement>(null);
  const asked = useRef(false);
  const questionId = useId();
  // The pressed control is replaced, so keyboard focus follows to its replacement.
  useEffect(() => {
    if (step === "confirm") { asked.current = true; focusForKeyboard(confirmRef.current); }
    else if (step === "idle" && asked.current) { asked.current = false; focusForKeyboard(restoreRef.current); }
  }, [step]);
  const restore = async () => {
    setStep("restoring");
    setFailed(false);
    try {
      const restored = await bridge.chatArtifactRestore({
        loupist_id: loupistId, thread_id: threadId, artifact_id: view.artifact.id,
        revision_id: view.revision.revision,
        // Rust refuses the restore if another answer changed the item since this view was read.
        expected_current_revision: view.artifact.current_revision,
      });
      onRestored(restored, view.revision.revision);
    } catch {
      setFailed(true);
      setStep("idle");
    }
  };
  const version = formatNumber(view.revision.revision);
  return <div className="work-version-bar" data-work-version-bar data-step={step}>
    {step === "idle" ? <>
      <p className="work-version-note">{t("workVersions.viewing", { version, date: when(view.revision.created_at_ms) })}</p>
      <div className="work-version-actions">
        <Button ref={restoreRef} type="button" variant="outline" size="sm" onClick={() => setStep("confirm")}>{t("workVersions.restore")}</Button>
        <Button type="button" variant="ghost" size="sm" onClick={onBack}>{t("workVersions.backToCurrent")}</Button>
      </div>
    </> : <div className="work-version-confirm" role="group" aria-labelledby={questionId}>
      <p id={questionId} className="work-version-note">{t("workVersions.confirm", { version })}</p>
      <div className="work-version-actions">
        <Button ref={confirmRef} type="button" size="sm" disabled={step === "restoring"} onClick={() => void restore()}>{step === "restoring" ? <Spinner /> : null}{t(step === "restoring" ? "workVersions.restoring" : "workVersions.confirmRestore")}</Button>
        <Button type="button" variant="ghost" size="sm" disabled={step === "restoring"} onClick={() => setStep("idle")}>{i18n.t("onboarding:appDialogs.cancel")}</Button>
      </div>
    </div>}
    {failed ? <p className="work-version-error" role="alert">{t("workVersions.restoreFailed")}</p> : null}
  </div>;
}
