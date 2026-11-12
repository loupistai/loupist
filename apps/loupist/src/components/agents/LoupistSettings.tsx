import { skillPresentation } from "../../lib/agentSkills";
import i18n, { useTranslation } from "../../i18n";
import { CONFIG_CHANGED_EVENT, notifyConfigurationChanged } from "../../app/useConfiguration";
import { Disclosure } from "../ui/disclosure";
import { LoupistAvatarPicker } from "../app/LoupistAvatar";
import { LoupistTracking } from "./LoupistTracking";
import { LoupistLearningSettings } from "./LoupistLearningSettings";
import { AgentSettings } from "../settings/AgentSettings";
import { SkillsSettings } from "../settings/SkillsSettings";
import { WorldProcessingSettings } from "../knowledge/WorldProcessingSettings";
import { MemoryResetDialog } from "../memory/MemoryResetDialog";
import { LoupistToneField } from "../onboarding/LoupistToneField";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  FolderOpenIcon,
  ArrowLeftIcon,
  SearchIcon,
  RefreshCwIcon,
  StoreIcon,
  Trash2Icon,
  CheckIcon,
  ShieldCheckIcon,
  XIcon,
} from "lucide-react";
import type { LoupistAvatarId } from "../../contracts/loupist";
import { Textarea } from "../ui/textarea";
import { Input } from "../ui/input";
import { Button } from "@/components/ui/button";
import { SegmentedControl, SegmentedControlItem } from "../ui/segmented-control";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type {
  LoupistCapabilityBinding,
  MarketplaceSnapshot,
} from "../../marketplace/types";
import { sortConnectionEntries, unavailableCapabilityNote } from "../../marketplace/readiness";
import { connectionText } from "../marketplace/connectionCopy";
import { connectionGroups } from "../../marketplace/groups";
import { errorMessage } from "../../lib/errors";
import type {
  AgentSkillSummary,
  ConfigScope,
  LoupistBridge,
  LoupistSummary,
  ModelCatalog,
} from "../../types";
import {
  AgentCapabilityList,
  type AgentCapabilityItem,
} from "./AgentCapabilityList";

import { LoupistAvailabilityBoundary } from "../app/LoupistAvailabilityBoundary";

/** A section of Loupist settings that a link elsewhere in the app can open. */
export type LoupistSettingsSection = "chat" | "world" | "capabilities" | "connections";

type Props = {
  bridge: LoupistBridge;
  selectedAgent: LoupistSummary | null;
  mutationBusy: boolean;
  mutationError: string | null;
  recoveryBusy: boolean;
  recoveryError: string | null;
  onRecover: () => void;
  onReconnect?: () => void;
  catalog: ModelCatalog | null;
  onChanged: () => void;
  onBack: () => void;
  initialSection?: LoupistSettingsSection;
  initialRemove?: boolean;
  onOpenMarketplace: (entryId?: string) => void;
  onManageProviders: () => void;
  onDraftSkill?: (scope: ConfigScope, brief: string) => void | Promise<void>;
  onDelete: () => Promise<void>;
  onAgentUpdated: (agent: LoupistSummary) => void;
};

export function LoupistSettings({
  bridge,
  selectedAgent,
  mutationBusy,
  mutationError,
  recoveryBusy,
  recoveryError,
  onRecover,
  onReconnect,
  catalog,
  onChanged,
  onBack,
  initialSection,
  initialRemove = false,
  onOpenMarketplace,
  onManageProviders,
  onDraftSkill,
  onDelete,
  onAgentUpdated,
}: Props) {
  useTranslation("settings");
  const [capabilityQuery, setCapabilityQuery] = useState("");
  const [capabilityFilter, setCapabilityFilter] = useState("all");
  const [savedMessage, setSavedMessage] = useState<string | null>(null);
  const [failedCapabilityId, setFailedCapabilityId] = useState<string | null>(null);
  const [openingFolder, setOpeningFolder] = useState(false);
  const [skills, setSkills] = useState<AgentSkillSummary[]>([]);
  const [marketplace, setMarketplace] = useState<MarketplaceSnapshot | null>(
    null,
  );
  const [toolBindings, setToolBindings] = useState<LoupistCapabilityBinding[]>([]);
  const [configurationError, setConfigurationError] = useState<string | null>(
    null,
  );
  const [configurationLoading, setConfigurationLoading] = useState(false);
  const [reloadToken, setReloadToken] = useState(0);
  const [busyCapabilityId, setBusyCapabilityId] = useState<string | null>(null);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [resetOpen, setResetOpen] = useState(false);
  const [manageSkills, setManageSkills] = useState(false);
  const [worldOpen, setWorldOpen] = useState(initialSection === "world");
  const selectedAgentId = selectedAgent?.id ?? null;
  const selectedAgentIdRef = useRef(selectedAgentId);
  const titleRef = useRef<HTMLHeadingElement>(null);
  const bodyRef = useRef<HTMLDivElement>(null);
  const openSection = (section: string) => {
    if (section === "world") setWorldOpen(true);
    const target = bodyRef.current?.querySelector<HTMLElement>(`[data-loupist-section="${section}"]`);
    // A section inside a disclosure (connections) opens the disclosure that holds it.
    const details = target instanceof HTMLDetailsElement ? target : target?.closest("details");
    if (details) details.open = true;
    target?.scrollIntoView?.({ block: "start" });
  };

  useEffect(() => { titleRef.current?.focus(); }, [selectedAgentId]);
  useEffect(() => {
    if (!initialSection) return;
    const frame = requestAnimationFrame(() => openSection(initialSection));
    return () => cancelAnimationFrame(frame);
  }, [initialSection, selectedAgentId]);

  selectedAgentIdRef.current = selectedAgentId;

  useEffect(() => {
    setCapabilityQuery("");
    setCapabilityFilter("all");
    setSavedMessage(null);
    setFailedCapabilityId(null);
    setOpeningFolder(false);
    setSkills([]);
    setMarketplace(null);
    setToolBindings([]);
    setConfigurationError(null);
    setBusyCapabilityId(null);
  }, [selectedAgentId]);

  useEffect(() => {
    let active = true;
    setConfigurationError(null);
    setConfigurationLoading(Boolean(selectedAgentId));
    if (!selectedAgentId || selectedAgent?.availability.status !== "available") {
      setSkills([]);
      setMarketplace(null);
      setToolBindings([]);
      setConfigurationLoading(false);
      return () => undefined;
    }

    void Promise.all([
      bridge.agentSkillCatalog(selectedAgentId),
      bridge.marketplaceSnapshot(),
      bridge.loupistCapabilityList(selectedAgentId),
    ])
      .then(([nextSkills, nextMarketplace, nextBindings]) => {
        if (!active) return;
        setSkills(nextSkills);
        setMarketplace(nextMarketplace);
        setToolBindings(nextBindings);
      })
      .catch((cause: unknown) => {
        if (!active) return;
        setFailedCapabilityId(null);
        setConfigurationError(
          errorMessage(cause, i18n.t("settings:loupistSettings.couldNotLoadThisLoupistSSkillsAnd")),
        );
      })
      .finally(() => {
        if (active) setConfigurationLoading(false);
      });

    return () => {
      active = false;
    };
  }, [bridge, reloadToken, selectedAgent?.availability.status, selectedAgentId]);

  useEffect(() => { setDeleteOpen(initialRemove); }, [initialRemove, selectedAgentId]);

  useEffect(() => {
    const reload = () => setReloadToken((current) => current + 1);
    window.addEventListener("focus", reload);
    window.addEventListener(CONFIG_CHANGED_EVENT, reload);
    return () => {
      window.removeEventListener("focus", reload);
      window.removeEventListener(CONFIG_CHANGED_EVENT, reload);
    };
  }, []);

  const bindingById = useMemo(
    () =>
      new Map(
        toolBindings.map((binding) => [binding.entry_id, binding]),
      ),
    [toolBindings],
  );
  const connectorById = useMemo(
    () =>
      new Map(
        marketplace?.connectors.map((connector) => [
          connector.entry_id,
          connector,
        ]) ?? [],
      ),
    [marketplace],
  );

  const toolItems = useMemo<AgentCapabilityItem[]>(() => {
    if (!marketplace) return [];
    const entriesById = new Map(
      marketplace.catalog.entries.map((entry) => [entry.id, entry]),
    );
    const listed = marketplace.installed.flatMap((installed) => {
      const entry = entriesById.get(installed.entry_id);
      if (!entry) {
        return [];
      }
      const binding = bindingById.get(entry.id);
      const available = binding?.available ?? false;
      return [
        {
          entry,
          item: {
            id: entry.id,
            name: connectionText(entry.name),
            description: connectionText(entry.summary),
            enabled: available && (binding?.enabled ?? false),
            locked: !available,
            note: available
              ? undefined
              : unavailableCapabilityNote(connectorById.get(entry.id)),
          },
        },
      ];
    });
    const rowById = new Map(listed.map((row) => [row.entry.id, row]));
    const ordered = sortConnectionEntries(listed.map((row) => row.entry), connectorById, (entry) => connectionText(entry.name));
    return connectionGroups(marketplace.plugins, ordered).flatMap((group) => {
      const rows = group.entries.map((entry) => rowById.get(entry.id)!);
      const hideGroup = rows.length === 1 && rows[0]?.item.name === group.label;
      return rows.map((row) => ({ ...row.item, group: hideGroup ? undefined : group.label }));
    });
  // eslint-disable-next-line react-hooks/exhaustive-deps -- the interface language is the recompute trigger; `i18n` itself is not reactive.
  }, [bindingById, connectorById, marketplace, i18n.language]);

  const skillItems = useMemo<AgentCapabilityItem[]>(
    () =>
      skills.filter((skill) => !skill.fixed).map((skill) => ({
        id: skill.id,
        ...skillPresentation(skill),
        enabled: skill.enabled,
        locked: false,
      })),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the interface language is the recompute trigger; `i18n` itself is not reactive.
    [skills, i18n.language],
  );

  const matchesCapability = (item: AgentCapabilityItem) => (capabilityFilter === "all" || (capabilityFilter === "enabled" ? item.enabled : item.locked)) && `${item.name} ${item.description} ${item.note ?? ""}`.toLowerCase().includes(capabilityQuery.trim().toLowerCase());
  const revealWorkspace = async () => {
    if (!selectedAgent || openingFolder) return;
    const agentId = selectedAgent.id;
    setOpeningFolder(true); setConfigurationError(null); setFailedCapabilityId(null);
    try { await bridge.workspaceReveal({ loupist_id: agentId }); }
    catch (cause) { if (selectedAgentIdRef.current === agentId) setConfigurationError(errorMessage(cause, i18n.t("settings:loupistSettings.couldNotOpenThisWorkspaceFolder"))); }
    finally { if (selectedAgentIdRef.current === agentId) setOpeningFolder(false); }
  };

  const toggleSkill = async (skillId: string) => {
    if (!selectedAgent || busyCapabilityId) return;
    const operationAgentId = selectedAgent.id;
    const selected = skills.find((skill) => skill.id === skillId);
    if (!selected || selected.fixed) return;
    setBusyCapabilityId(skillId);
    setConfigurationError(null);
    setFailedCapabilityId(null);
    setSavedMessage(null);
    try {
      const snapshot = await bridge.configRead({ scope: { kind: "project", loupistId: operationAgentId } });
      const updated = await bridge.configUpdate({
        scope: snapshot.scope,
        expectedRevision: snapshot.revision,
        set: { skills: { [skillId]: { enabled: !selected.enabled } } },
        unset: [],
      });
      notifyConfigurationChanged();
      if (selectedAgentIdRef.current !== operationAgentId) return;
      const enabledIds = Object.entries(updated.effective.skills).filter(([, skill]) => skill.enabled).map(([id]) => id);
      onAgentUpdated({ ...selectedAgent, enabled_skill_ids: enabledIds });
      setSavedMessage(i18n.t(selected.enabled ? "settings:loupistSettings.disabledForLoupist" : "settings:loupistSettings.enabledForLoupist", { name: skillPresentation(selected).name, loupist: selectedAgent.name }));
    } catch (cause) {
      if (selectedAgentIdRef.current !== operationAgentId) return;
      setFailedCapabilityId(skillId);
      setConfigurationError(
        errorMessage(cause, i18n.t("settings:loupistSettings.couldNotUpdateThisLoupistSSkills")),
      );
    } finally {
      if (selectedAgentIdRef.current === operationAgentId) setBusyCapabilityId(null);
    }
  };

  const toggleTool = async (entryId: string) => {
    if (!selectedAgent || !marketplace || busyCapabilityId) return;
    const operationAgentId = selectedAgent.id;
    const enabled = bindingById.get(entryId)?.enabled ?? false;
    setBusyCapabilityId(entryId);
    setConfigurationError(null);
    setFailedCapabilityId(null);
    setSavedMessage(null);
    try {
      const request = { loupist_id: operationAgentId, entry_id: entryId };
      const updated = enabled
        ? await bridge.loupistCapabilityDisable(request)
        : await bridge.loupistCapabilityEnable(request);
      if (selectedAgentIdRef.current !== operationAgentId) return;
      setToolBindings((current) =>
        current.map((binding) =>
          binding.entry_id === entryId
            ? updated
            : binding,
        ),
      );
      setSavedMessage(i18n.t(updated.enabled ? "settings:loupistSettings.enabledForLoupist" : "settings:loupistSettings.disabledForLoupist", { name: toolItems.find((item) => item.id === entryId)?.name ?? i18n.t("settings:loupistSettings.connection"), loupist: selectedAgent.name }));
    } catch (cause) {
      if (selectedAgentIdRef.current !== operationAgentId) return;
      setFailedCapabilityId(entryId);
      setConfigurationError(
        errorMessage(cause, i18n.t("settings:loupistSettings.couldNotUpdateThisLoupistSConnections")),
      );
    } finally {
      if (selectedAgentIdRef.current === operationAgentId) setBusyCapabilityId(null);
    }
  };

  const available = selectedAgent?.availability.status === "available";
  // A folder made by an earlier build cannot be opened; removing it is the way forward.
  const removable = available || selectedAgent?.availability.status === "earlier_version" || selectedAgent?.availability.status === "newer_version";
  return (
    <section className="settings-page loupist-settings-page" data-testid="loupist-settings-page" aria-label={i18n.t("app:shell.loupistSettings")} onFocus={(event) => { if (event.target instanceof HTMLElement) event.target.scrollIntoView({ block: "nearest", inline: "nearest" }); }}>
      <header className="loupist-settings-heading">
        <Button variant="ghost" size="sm" data-loupist-settings-back onClick={onBack}><ArrowLeftIcon />{i18n.t("knowledge:memoryPage.back")}</Button>
        <h1 ref={titleRef} tabIndex={-1}>{i18n.t("app:shell.loupistSettings")}</h1>
        <p>{selectedAgent?.name}</p>
      </header>
      {selectedAgent ? <nav className="loupist-settings-sections" aria-label={i18n.t("app:appNavigation.settingsSections")}>
        <Button variant="ghost" size="sm" onClick={() => openSection("profile")}>{i18n.t("app:loupistProfile.profile")}</Button>
        {available ? <>
          <Button variant="ghost" size="sm" onClick={() => openSection("chat")}>{i18n.t("settings:loupistSettings.chatPreferences")}</Button>
          <Button variant="ghost" size="sm" onClick={() => openSection("learning")}>{i18n.t("knowledge:memoryPage.about.learning")}</Button>
          <Button variant="ghost" size="sm" onClick={() => openSection("capabilities")}>{i18n.t("settings:loupistSettings.skillsConnections")}</Button>
          <Button variant="ghost" size="sm" onClick={() => openSection("world")}>{i18n.t("app:navigation.worldProcessing")}</Button>
        </> : null}
        <Button variant="ghost" size="sm" onClick={() => openSection("workspace")}>{i18n.t("settings:loupistSettings.localWorkspace")}</Button>
      </nav> : null}
      {mutationError && !deleteOpen ? <div className="inline-error" role="alert">{mutationError}</div> : null}
      {configurationError && !failedCapabilityId ? <div className="agent-configuration-error" role="alert"><span>{configurationError}</span><Button type="button" size="sm" variant="outline" onClick={() => setReloadToken((current) => current + 1)}><RefreshCwIcon /> {i18n.t("settings:loupistSettings.retry")}</Button></div> : null}
      <div className="loupist-settings-body" ref={bodyRef}>
        {selectedAgent ? (
          <div className="agent-editor" role="group" aria-label={i18n.t("settings:loupistSettings.settingsForName", { name: selectedAgent.name })}>
            <LoupistProfileEditor key={selectedAgent.id} bridge={bridge} loupist={selectedAgent} disabled={mutationBusy || !available} onUpdated={onAgentUpdated} />
            {available ? <>
              <Disclosure title={i18n.t("settings:loupistSettings.chatPreferences")} defaultOpen={initialSection === "chat"} data-loupist-section="chat">
                <AgentSettings bridge={bridge} loupists={[selectedAgent]} catalog={catalog} scope={{ kind: "project", loupistId: selectedAgent.id }} />
              </Disclosure>
              <Disclosure title={i18n.t("knowledge:memoryPage.about.learning")} data-loupist-section="learning">
                <LoupistLearningSettings key={selectedAgent.id} bridge={bridge} loupistId={selectedAgent.id} onChanged={onChanged} />
                <Button variant="destructive" size="sm" data-memory-reset onClick={() => setResetOpen(true)}>{i18n.t("knowledge:memoryPage.about.reset")}</Button>
                <MemoryResetDialog bridge={bridge} loupistId={selectedAgent.id} name={selectedAgent.name} open={resetOpen} onOpenChange={setResetOpen} onChanged={onChanged} />
              </Disclosure>
            </> : null}
            {!available ? <LoupistAvailabilityBoundary availability={selectedAgent.availability} busy={recoveryBusy || mutationBusy} error={recoveryError} onRecover={onRecover} onReconnect={onReconnect} onRemove={() => setDeleteOpen(true)} className="agent-workspace-recovery" compact /> : null}
            {available ? <Disclosure title={i18n.t("settings:loupistSettings.skillsConnections")} data-loupist-section="capabilities">
              <div className="agent-capabilities-toolbar">
                <div className="agent-capabilities-heading"><span role="status" className="agent-save-status">{savedMessage ? <><CheckIcon aria-hidden="true" />{savedMessage}</> : i18n.t("settings:loupistSettings.changesSaveAutomaticallyForThisLoupist")}</span></div>
                <div className="agent-capability-search"><SearchIcon aria-hidden="true" /><Input type="search" aria-label={i18n.t("settings:loupistSettings.searchSkillsAndConnections")} placeholder={i18n.t("settings:loupistSettings.findASkillOrConnection")} value={capabilityQuery} onChange={(event) => setCapabilityQuery(event.target.value)} />{capabilityQuery ? <Button variant="ghost" size="icon-sm" aria-label={i18n.t("settings:loupistSettings.clearCapabilitySearch")} onClick={() => setCapabilityQuery("")}><XIcon /></Button> : null}</div>
                <SegmentedControl value={capabilityFilter} onValueChange={setCapabilityFilter} aria-label={i18n.t("settings:loupistSettings.capabilityStatus")}><SegmentedControlItem value="all">{i18n.t("settings:loupistSettings.all")}</SegmentedControlItem><SegmentedControlItem value="enabled">{i18n.t("settings:loupistSettings.enabled")}</SegmentedControlItem><SegmentedControlItem value="setup">{i18n.t("settings:loupistSettings.needsSetup")}</SegmentedControlItem></SegmentedControl>
              </div>

              {capabilityFilter !== "setup" ? manageSkills ? <div className="agent-section-heading"><h2>{i18n.t("settings:loupistSettings.skills")}</h2><Button variant="ghost" size="sm" aria-expanded onClick={() => setManageSkills(false)}>{i18n.t("settings:loupistSettings.manageSkills")}</Button></div> : <AgentCapabilityList title={i18n.t("settings:loupistSettings.skills")} action={<Button variant="ghost" size="sm" aria-expanded={manageSkills} onClick={() => setManageSkills(value => !value)}>{i18n.t("settings:loupistSettings.manageSkills")}</Button>} description={i18n.t("settings:loupistSettings.optionalResearchAndDocumentWorkflowsEnabledByDefault")} items={skillItems.filter(matchesCapability)} busyId={busyCapabilityId} error={configurationError && failedCapabilityId ? { id: failedCapabilityId, message: configurationError } : undefined} emptyLabel={configurationLoading ? i18n.t("settings:loupistSettings.loadingSkills") : capabilityQuery || capabilityFilter !== "all" ? i18n.t("settings:loupistSettings.noMatchingSkills") : i18n.t("settings:loupistSettings.noSkillsAvailable")} onToggle={(id) => void toggleSkill(id)} /> : null}
              {manageSkills && capabilityFilter !== "setup" ? <SkillsSettings bridge={bridge} loupists={[selectedAgent]} scope={{ kind: "project", loupistId: selectedAgent.id }} onDraftSkill={onDraftSkill} /> : null}
              <AgentCapabilityList section="connections" title={i18n.t("settings:loupistSettings.connections")} description={i18n.t("settings:loupistSettings.chooseTheDataSourcesThisLoupistCanUse")} action={<Button type="button" size="sm" variant="ghost" onClick={() => onOpenMarketplace()}><StoreIcon /> {i18n.t("settings:loupistSettings.connections")}</Button>} items={toolItems.filter(matchesCapability)} busyId={busyCapabilityId} error={configurationError && failedCapabilityId ? { id: failedCapabilityId, message: configurationError } : undefined} emptyLabel={configurationLoading ? i18n.t("settings:loupistSettings.loadingConnections") : capabilityQuery || capabilityFilter !== "all" ? i18n.t("settings:loupistSettings.noMatchingConnections") : i18n.t("settings:loupistSettings.connectAServiceToGiveThisLoupistMore")} onToggle={(id) => void toggleTool(id)} onSetup={onOpenMarketplace} />
            </Disclosure> : null}
            {available ? <Disclosure title={i18n.t("app:navigation.worldProcessing")} open={worldOpen} onOpenChange={setWorldOpen} data-loupist-section="world">
              <div className="world-settings-card"><WorldProcessingSettings key={selectedAgent.id} bridge={bridge} loupistId={selectedAgent.id} active={worldOpen} onChanged={onChanged} onManageProviders={onManageProviders} /></div>
              <Disclosure className="loupist-tracking-advanced" title={i18n.t("app:tracking.advanced")} data-loupist-tracking-advanced>
                <LoupistTracking key={`${selectedAgent.id}:tracking`} bridge={bridge} loupistId={selectedAgent.id} />
              </Disclosure>
            </Disclosure> : null}
            <Disclosure title={i18n.t("settings:loupistSettings.localWorkspace")} data-loupist-section="workspace">
            <section className="agent-workspace-card" aria-labelledby="agent-workspace-heading">
              <header className="agent-section-heading"><div><h2 id="agent-workspace-heading">{i18n.t("settings:loupistSettings.localWorkspace")}</h2><p className="agent-config-description">{i18n.t("settings:loupistSettings.workspaceCaption")}</p></div><span className="agent-workspace-state">{available ? i18n.t("settings:loupistSettings.connected") : i18n.t("settings:loupistSettings.needsAttention")}</span></header>
              <p className="workspace-path agent-workspace-path" aria-label={i18n.t("settings:loupistSettings.workspaceFolder")}>{selectedAgent.workspace_path}</p>
              {available ? <>
                <div className="agent-workspace-actions"><Button variant="outline" size="sm" disabled={openingFolder} onClick={() => void revealWorkspace()}><FolderOpenIcon />{openingFolder ? i18n.t("settings:loupistSettings.opening") : i18n.t("settings:loupistSettings.showInFileManager")}</Button>{onReconnect ? <Button variant="ghost" size="sm" disabled={mutationBusy} onClick={onReconnect}>{i18n.t("settings:loupistSettings.reconnectFolder")}</Button> : null}</div>
              </> : null}
            </section>

            </Disclosure>
            <section className="agent-removal-card" aria-labelledby="agent-removal-heading"><div><h2 id="agent-removal-heading">{i18n.t("settings:loupistSettings.removeFromThisApp")}</h2><p>{i18n.t("settings:loupistSettings.yourWorkspaceFolderChatsAndMemoryStayOn")}</p>{!removable ? <p>{i18n.t("settings:loupistSettings.reconnectOrRecoverThisLoupistBeforeRemovingIt")}</p> : null}</div><Button type="button" size="sm" variant="outline" disabled={mutationBusy || !removable} onClick={() => setDeleteOpen(true)}><Trash2Icon /> {i18n.t("settings:loupistSettings.removeLoupist")}</Button></section>
          </div>
        ) : <div className="agent-editor-empty"><p>{i18n.t("settings:loupistSettings.selectALoupistToEditIt")}</p></div>}
      </div>

      <Dialog open={deleteOpen} onOpenChange={(open) => { if (!mutationBusy) setDeleteOpen(open); }}>
        <DialogContent size="sm" className="loupist-dialog loupist-delete-dialog" showCloseButton={!mutationBusy}>
          <DialogHeader><DialogTitle>{i18n.t("settings:loupistSettings.removeLoupistTitle")}</DialogTitle><DialogDescription>{selectedAgent ? i18n.t("settings:loupistSettings.removeNameFromThisApp", { name: selectedAgent.name }) : i18n.t("settings:loupistSettings.removeThisLoupistFromTheApp")}</DialogDescription></DialogHeader>
          <div className="loupist-preserved-note"><ShieldCheckIcon aria-hidden="true" /><div><strong>{i18n.t("settings:loupistSettings.yourResearchStaysInItsFolder")}</strong><p>{i18n.t("settings:loupistSettings.chatsMemoryAndLocalFilesAreKeptOpen")}</p>{selectedAgent ? <p className="workspace-path">{selectedAgent.workspace_path}</p> : null}</div></div>
          {mutationError ? <p className="inline-error" role="alert">{mutationError}</p> : null}
          <DialogFooter><Button type="button" variant="outline" disabled={mutationBusy} onClick={() => setDeleteOpen(false)}>{i18n.t("settings:loupistSettings.cancel")}</Button><Button type="button" variant="destructive" disabled={mutationBusy} onClick={() => void onDelete()}>{mutationBusy ? i18n.t("settings:loupistSettings.removing") : i18n.t("settings:loupistSettings.removeLoupist")}</Button></DialogFooter>
        </DialogContent>
      </Dialog>
    </section>
  );
}

function profileDraft(loupist: LoupistSummary) {
  return { name: loupist.name, focus: loupist.philosophy, avatar: loupist.avatar ?? "initial" as LoupistAvatarId, tone: loupist.tone ?? "balanced", instructions: loupist.instructions ?? "" };
}

function LoupistProfileEditor({ bridge, loupist, disabled, onUpdated }: { bridge: LoupistBridge; loupist: LoupistSummary; disabled: boolean; onUpdated: (loupist: LoupistSummary) => void }) {
  const { t } = useTranslation("app");
  const [draft, setDraft] = useState(() => profileDraft(loupist));
  const [baseline, setBaseline] = useState(() => profileDraft(loupist));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  useEffect(() => {
    const next = profileDraft(loupist);
    setDraft(next); setBaseline(next); setError(null);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- a refreshed profile object with the same fields must not discard the draft.
  }, [loupist.id, loupist.name, loupist.philosophy, loupist.avatar, loupist.tone, loupist.instructions]);
  useEffect(() => { if (!saved) return; const timer = setTimeout(() => setSaved(false), 2000); return () => clearTimeout(timer); }, [saved]);
  const changed = Object.keys(draft).some(key => draft[key as keyof typeof draft] !== baseline[key as keyof typeof baseline]);
  const focusLength = [...draft.focus].length;
  const valid = [...draft.name.trim()].length > 0 && [...draft.name.trim()].length <= 60 && focusLength <= 280 && [...draft.instructions].length <= 4000;
  const edit = <K extends keyof typeof draft>(key: K, value: typeof draft[K]) => { setDraft(current => ({ ...current, [key]: value })); setSaved(false); setError(null); };
  const save = async () => {
    if (busy || disabled || !changed || !valid) return;
    setBusy(true); setError(null); setSaved(false);
    try {
      const updated = await bridge.loupistProfileUpdate({ loupistId: loupist.id, ...draft });
      onUpdated(updated);
      if (!mounted.current) return;
      const next = profileDraft(updated);
      setDraft(next); setBaseline(next); setSaved(true);
    } catch (cause) {
      if (mounted.current) setError(errorMessage(cause, t("loupistProfile.saveError")));
    } finally { if (mounted.current) setBusy(false); }
  };
  return <section className="agent-profile-card" data-loupist-section="profile" aria-label={t("loupistProfile.profile")}>
    <h2>{t("loupistProfile.profile")}</h2>
    <form className="loupist-profile-form" data-testid="loupist-profile-form" onSubmit={event => { event.preventDefault(); void save(); }}>
      <fieldset disabled={disabled || busy}>
        <label htmlFor="loupist-profile-name">{t("loupistProfile.name")}</label>
        <Input id="loupist-profile-name" value={draft.name} onChange={event => edit("name", event.target.value)} aria-invalid={[...draft.name.trim()].length > 60 || !draft.name.trim()} />
        <LoupistAvatarPicker loupist={{ ...loupist, name: draft.name, avatar: draft.avatar }} onChange={avatar => edit("avatar", avatar)} />
        <label htmlFor="loupist-profile-focus">{t("loupistProfile.focus")}</label>
        <Textarea id="loupist-profile-focus" rows={2} value={draft.focus} onChange={event => edit("focus", event.target.value)} aria-invalid={focusLength > 280} aria-describedby={focusLength >= 240 ? "loupist-profile-focus-count" : undefined} />
        {focusLength >= 240 ? <span className="loupist-profile-counter" id="loupist-profile-focus-count">{t("loupistProfile.counter", { count: focusLength, limit: 280 })}</span> : null}
        <LoupistToneField name="loupist-settings-tone" legend={i18n.t("onboarding:tone.title", { name: loupist.name })} value={draft.tone} busy={disabled || busy} onChange={value => edit("tone", value)} />
        <label htmlFor="loupist-profile-instructions">{i18n.t("knowledge:memoryPage.about.instructions")}</label>
        <Textarea id="loupist-profile-instructions" rows={4} maxLength={4000} value={draft.instructions} placeholder={i18n.t("knowledge:memoryPage.about.answersPlaceholder")} onChange={event => edit("instructions", event.target.value)} />
        <p className="agent-config-description">{i18n.t("knowledge:memoryPage.about.answersHint")}</p>
      </fieldset>
      {error ? <div className="loupist-profile-error" role="alert"><span>{error}</span><Button type="button" size="sm" variant="outline" disabled={busy || disabled || !valid} onClick={() => void save()}>{t("loupistProfile.tryAgain")}</Button></div> : null}
      <footer><Button type="submit" disabled={disabled || busy || !changed || !valid}>{t("loupistProfile.save")}</Button><Button type="button" variant="ghost" disabled={busy || !changed} onClick={() => { setDraft(baseline); setError(null); setSaved(false); }}>{t("loupistProfile.discard")}</Button>{saved ? <span className="loupist-profile-saved" role="status">{t("loupistProfile.saved")}</span> : null}</footer>
    </form>
  </section>;
}
