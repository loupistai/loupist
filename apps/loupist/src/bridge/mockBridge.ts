import type { AttentionItem, AttentionNotifyRequest } from "../contracts/attention";
import { CURRENT_NOTICE_VERSION } from "./notice";
import type { PrincipleListRequest, PrincipleCreateRequest, PrincipleUpdateRequest, PrincipleArchiveRequest } from "../contracts/knowledge";
import { MockUserContext } from "./mock/userContext";
import { MockCustomConnections } from "./mock/customConnections";
import { MockAssets } from "./mock/assets";
import { MockEarnings } from "./mock/earnings";
import { MockWatch } from "./mock/watch";
import type { AssetPageGetRequest, AssetDeltaRequest, AssetRefreshRequest, AssetClassSetRequest, AssetVerdictUndoRequest, AssetPageSeenRequest, UpcomingEventsRequest } from "../contracts/assets";
import type { CustomConnectionsBridge } from "../contracts/customConnections";
import { MockSharing } from "./mock/sharing";
import type { SharingBridge } from "../contracts/sharing";
import type { WatchlistAddRequest } from "../contracts/userContext";
import { workspaceFolderName } from "../lib/loupistFolder";
import { AGENT_SKILLS, DEFAULT_AGENT_SKILL_IDS, FIXED_AGENT_SKILLS } from "../lib/agentSkills";
import type * as Contract from "../types";
import {
  createMockLoupistCapabilityBindings,
  createMockMarketplaceSnapshot,
  isValidMockSetupValue,
} from "./mock/marketplace";
import { MockBrowserTabs, validBrowserUrl } from "./mock/browser";
import * as loupistChat from "./mock/loupistChat";
import * as knowledge from "./mock/knowledge";
import { curatedSeedView, MockWorldSchema } from "./mock/worldSchema";
import { createMockBridgeState, type MockBridgeState } from "./mock/state";
import { MockScheduledTasks } from "./mock/scheduled";
import { MockConfig } from "./mock/config";
import worldDefaults from "../../world-model-defaults.json";
import { defaultRunOptions } from "../lib/modelSelection";
import { executionModelLock, initialModelCatalog, listProviderModels, mockProviderStatus } from "./mock/models";
import { commandError } from "./mock/commandError";

export class MockLoupistBridge implements Contract.LoupistBridge {
  private noticeVersion = CURRENT_NOTICE_VERSION;
  async noticeSettingsGet() { return { notice_acknowledged_version: this.noticeVersion }; }
  async noticeAcknowledge() { this.noticeVersion = Math.max(this.noticeVersion, CURRENT_NOTICE_VERSION); return this.noticeSettingsGet(); }
  private customConnections = new MockCustomConnections();
  private readonly loupistWorldSchema = new MockWorldSchema();
  customConnectionAddUrl(request: Parameters<CustomConnectionsBridge["customConnectionAddUrl"]>[0]) { return this.customConnections.customConnectionAddUrl(request); }
  customConnectionAddJson(request: Parameters<CustomConnectionsBridge["customConnectionAddJson"]>[0]) { return this.customConnections.customConnectionAddJson(request); }
  customConnectionAddCommand(request: Parameters<CustomConnectionsBridge["customConnectionAddCommand"]>[0]) { return this.customConnections.customConnectionAddCommand(request); }
  customConnectionConfirm(request: Parameters<CustomConnectionsBridge["customConnectionConfirm"]>[0]) { return this.customConnections.customConnectionConfirm(request); }
  customConnectionList(request: Parameters<CustomConnectionsBridge["customConnectionList"]>[0] = {}) { return this.customConnections.customConnectionList(request); }
  customConnectionRemove(request: Parameters<CustomConnectionsBridge["customConnectionRemove"]>[0]) { return this.customConnections.customConnectionRemove(request); }
  customConnectionSetMemory(request: Parameters<CustomConnectionsBridge["customConnectionSetMemory"]>[0]) { return this.customConnections.customConnectionSetMemory(request); }
  customConnectionSetEnabled(request: Parameters<CustomConnectionsBridge["customConnectionSetEnabled"]>[0]) { return this.customConnections.customConnectionSetEnabled(request); }
  customConnectionAuthorize(request: Parameters<CustomConnectionsBridge["customConnectionAuthorize"]>[0]) { return this.customConnections.customConnectionAuthorize(request); }
  customConnectionSignIn(request: Parameters<CustomConnectionsBridge["customConnectionSignIn"]>[0]) { return this.customConnections.customConnectionSignIn(request); }
  customConnectionSignInCancel(request: Parameters<CustomConnectionsBridge["customConnectionSignInCancel"]>[0]) { return this.customConnections.customConnectionSignInCancel(request); }
  customToolApprove(request: Parameters<CustomConnectionsBridge["customToolApprove"]>[0]) { return this.customConnections.customToolApprove(request); }
  /** Sharing fixtures; previews and tests adjust its public fields. */
  readonly sharing = new MockSharing({
    createLoupist: async (name, tone) => (await this.loupistCreate({ name, tone })).id,
    loupistName: id => this.state.loupists.find(item => item.id === id)?.name,
    delay: () => this.state.delay_ms,
  });
  shareSummary(request: Parameters<SharingBridge["shareSummary"]>[0]) { return this.sharing.shareSummary(request); }
  shareExport(request: Parameters<SharingBridge["shareExport"]>[0]) { return this.sharing.shareExport(request); }
  shareCopyText(request: Parameters<SharingBridge["shareCopyText"]>[0]) { return this.sharing.shareCopyText(request); }
  shareReveal(request: Parameters<SharingBridge["shareReveal"]>[0]) { return this.sharing.shareReveal(request); }
  sharePublish(request: Parameters<SharingBridge["sharePublish"]>[0], observer: Parameters<SharingBridge["sharePublish"]>[1]) { return this.sharing.sharePublish(request, observer); }
  githubAccount() { return this.sharing.githubAccount(); }
  githubSignIn(observer: Parameters<SharingBridge["githubSignIn"]>[0]) { return this.sharing.githubSignIn(observer); }
  githubSignInOpen() { return this.sharing.githubSignInOpen(); }
  githubSignInCancel() { return this.sharing.githubSignInCancel(); }
  githubSignOut() { return this.sharing.githubSignOut(); }
  packOpen(request: Parameters<SharingBridge["packOpen"]>[0]) { return this.sharing.packOpen(request); }
  packAdd(request: Parameters<SharingBridge["packAdd"]>[0], observer: Parameters<SharingBridge["packAdd"]>[1]) { return this.sharing.packAdd(request, observer); }
  packList(request: Parameters<SharingBridge["packList"]>[0]) { return this.sharing.packList(request); }
  packSuggestions(request: Parameters<SharingBridge["packSuggestions"]>[0]) { return this.sharing.packSuggestions(request); }
  packItemRead(request: Parameters<SharingBridge["packItemRead"]>[0]) { return this.sharing.packItemRead(request); }
  packDecide(request: Parameters<SharingBridge["packDecide"]>[0]) { return this.sharing.packDecide(request); }
  packRemove(request: Parameters<SharingBridge["packRemove"]>[0]) { return this.sharing.packRemove(request); }
  packReport(request: Parameters<SharingBridge["packReport"]>[0]) { return this.sharing.packReport(request); }
  packOrigin(request: Parameters<SharingBridge["packOrigin"]>[0]) { return this.sharing.packOrigin(request); }
  packCheckUpdate(request: Parameters<SharingBridge["packCheckUpdate"]>[0]) { return this.sharing.packCheckUpdate(request); }
  packChooseNewer(request: Parameters<SharingBridge["packChooseNewer"]>[0]) { return this.sharing.packChooseNewer(request); }
  packApplyUpdate(request: Parameters<SharingBridge["packApplyUpdate"]>[0]) { return this.sharing.packApplyUpdate(request); }
  galleryIndex(request: Parameters<SharingBridge["galleryIndex"]>[0]) { return this.sharing.galleryIndex(request); }
  galleryImage(request: Parameters<SharingBridge["galleryImage"]>[0]) { return this.sharing.galleryImage(request); }
  galleryListingStatus(request: Parameters<SharingBridge["galleryListingStatus"]>[0]) { return this.sharing.galleryListingStatus(request); }
  galleryListingRequest(request: Parameters<SharingBridge["galleryListingRequest"]>[0]) { return this.sharing.galleryListingRequest(request); }
  deepLinkPending() { return this.sharing.deepLinkPending(); }
  deepLinkDismiss() { return this.sharing.deepLinkDismiss(); }
  onDeepLinkImport(handler: Parameters<SharingBridge["onDeepLinkImport"]>[0]) { return this.sharing.onDeepLinkImport(handler); }
  private menuHandlers = new Set<(action: string) => void>();
  onMenuAction(handler: (action: string) => void): () => void {
    this.menuHandlers.add(handler);
    return () => { this.menuHandlers.delete(handler); };
  }
  emitMenuAction(action: string) { this.menuHandlers.forEach(handler => handler(action)); }

  private readonly userContext = new MockUserContext(() => {
    const entryId = "koreainvestment.market-data";
    const fields = this.financeCredentialFields.get(entryId);
    return this.financeCredentials.has(entryId) && !!fields?.has("account_number") && fields.has("account_product_code");
  });
  watchlistList() { return this.userContext.watchlistList(); }
  watchlistRemove(request: { symbol: string; market: string }) { return this.userContext.watchlistRemove(request); }
  async exportSave(request: Parameters<Contract.LoupistBridge["exportSave"]>[0]): Promise<string | null> { return `/mock/${request.suggestedName}`; }
  async pickFile(_request: { title: string; filters: { name: string; extensions: string[] }[] }): Promise<string | null> { return "/mock/holdings.csv"; }
  watchlistAdd(request: WatchlistAddRequest) { return this.userContext.watchlistAdd(request); }
  holdingsImportCsv(request: { path: string }) { return this.userContext.holdingsImportCsv(request); }
  holdingsCommitCsv(request: { previewId: string }) { return this.userContext.holdingsCommitCsv(request); }
  holdingsImportKis() { return this.userContext.holdingsImportKis(); }
  holdingsSummary() { return this.userContext.holdingsSummary(); }
  holdingsImportCryptoFile(request: { path: string }) { return this.userContext.holdingsImportCryptoFile(request); }
  holdingsMapColumns(request: { previewId: string; asset: string; balance: string }) { return this.userContext.holdingsMapColumns(request); }
  walletsList() { return this.userContext.walletsList(); }
  walletAdd(request: { address: string; name?: string | null }) { return this.userContext.walletAdd(request); }
  walletRename(request: { walletId: string; name?: string | null }) { return this.userContext.walletRename(request); }
  walletRemove(request: { walletId: string }) { return this.userContext.walletRemove(request); }
  walletRefresh(request?: { walletId?: string }) { return this.userContext.walletRefresh(request); }

  /** Asset pages; previews install fixtures here. */
  readonly assets = new MockAssets(
    loupistId => (this.state.knowledgeStores[loupistId] as { journals?: Contract.JournalRecord[] } | undefined)?.journals ?? [],
    () => this.watchlistList(),
  );
  assetPageGet(request: AssetPageGetRequest) { return this.assets.assetPageGet(request); }
  assetDelta(request: AssetDeltaRequest) { return this.assets.assetDelta(request); }
  assetRefresh(request: AssetRefreshRequest) { return this.assets.assetRefresh(request); }
  assetClassSet(request: AssetClassSetRequest) { return this.assets.assetClassSet(request); }
  assetVerdictUndo(request: AssetVerdictUndoRequest) { return this.assets.assetVerdictUndo(request); }
  assetPageSeen(request: AssetPageSeenRequest) { return this.assets.assetPageSeen(request); }
  upcomingEvents(request: UpcomingEventsRequest) { return this.assets.upcomingEvents(request); }

  /** Results snapshots; previews and tests install them here. */
  readonly earnings = new MockEarnings();
  earningsSnapshotGet(request: Parameters<Contract.LoupistBridge["earningsSnapshotGet"]>[0]) { return this.earnings.earningsSnapshotGet(request); }
  earningsEventsList(request: Parameters<Contract.LoupistBridge["earningsEventsList"]>[0]) { return this.earnings.earningsEventsList(request); }
  earningsPrepare(request: Parameters<Contract.LoupistBridge["earningsPrepare"]>[0]) { return this.earnings.earningsPrepare(request); }

  /** Local watchers; previews and tests install settings and alerts here. */
  readonly watch = new MockWatch(() => this.watchlistList());
  async watchDefaultsApply(request: Parameters<Contract.LoupistBridge["watchDefaultsApply"]>[0]): ReturnType<Contract.LoupistBridge["watchDefaultsApply"]> {
    const loupist = this.state.loupists.find(item => item.id === request.loupistId);
    if (!loupist) throw new Error("Loupist not found.");
    const settings = await this.watch.watchSettingsList(request);
    const assets = [...settings.watched, ...settings.notWatched];
    let changed = settings.consentOff;
    for (const asset of assets) {
      if (asset.revision !== 1 || asset.kinds.some(kind => kind.enabled)) continue;
      const equity = asset.kinds.some(kind => kind.kind === "filings");
      const templateClass = loupist.template === "crypto" && asset.kinds.some(kind => kind.kind === "cryptoMove" || kind.kind === "depeg")
        || loupist.template === "commodities" && asset.kinds.some(kind => kind.kind === "futuresMove");
      for (const kind of asset.kinds) kind.enabled = equity ? ["filings", "results", "dividends"].includes(kind.kind) : templateClass;
      asset.revision += 1;
      changed = true;
    }
    settings.consentOff = false;
    settings.watched = assets.filter(asset => asset.kinds.some(kind => kind.enabled));
    settings.notWatched = assets.filter(asset => asset.kinds.every(kind => !kind.enabled));
    if (changed) this.watch.install(request.loupistId, settings, await this.watch.watchEventsList({ ...request, limit: Number.MAX_SAFE_INTEGER }));
    if (assets.some(asset => asset.kinds.some(kind => kind.kind === "filings"))) {
      const task = (await this.scheduled.list({ loupist_id: request.loupistId })).find(item => item.kind === "earnings");
      if (task?.status === "paused" && task.revision === 1 && task.last_run_at_ms === null) {
        await this.scheduled.setStatus({ loupist_id: request.loupistId, task_id: task.id, expected_revision: 1, status: "active" });
      } else if (!task) {
        const now = Date.now();
        this.scheduled.installTask({ id: `earnings-${request.loupistId}`, loupist_id: request.loupistId, kind: "earnings", title: "Results before and after", prompt: "Automatic results captures", schedule: { kind: "interval", every_minutes: 60 }, target: { kind: "existing_chat", thread_id: loupistChat.ensureHomeThread(this.state, request.loupistId).id }, model_profile_id: "", thinking_level: "", run_options: {}, use_memory: true, update_memory: true, status: "active", revision: 1, created_at_ms: now, updated_at_ms: now, next_run_at_ms: now + 3_600_000, last_run_at_ms: null, last_error: null, session_thread_id: null, awaiting_confirmation: false });
      }
    }
    const needs: import("../contracts/generated").WatchDefaultsConnectionNeed[] = [];
    for (const asset of settings.watched) for (const kind of asset.kinds) {
      if (!kind.enabled || !kind.needsConnection) continue;
      let need = needs.find(item => item.kind === kind.kind && item.connection.id === kind.needsConnection!.id);
      if (!need) { need = { kind: kind.kind, connection: kind.needsConnection, assetIds: [] }; needs.push(need); }
      need.assetIds.push(asset.assetId);
    }
    return { watchedAssets: settings.watched.length, needsConnections: needs };
  }
  watchSettingsList(request: { loupistId: string }) { return this.watch.watchSettingsList(request); }
  watchSettingSet(request: Parameters<Contract.LoupistBridge["watchSettingSet"]>[0]) { return this.watch.watchSettingSet(request); }
  watchPauseSet(request: { loupistId: string; paused: boolean }) { return this.watch.watchPauseSet(request); }
  watchCheckNow(request: { loupistId: string }) { return this.watch.watchCheckNow(request); }
  watchDefaultsApply(request: { loupistId: string }) { return this.watch.watchDefaultsApply(request); }
  watchEventsList(request: Parameters<Contract.LoupistBridge["watchEventsList"]>[0]) { return this.watch.watchEventsList(request); }
  watchEventsSeen(request: { loupistId: string; eventIds: string[] }) { return this.watch.watchEventsSeen(request); }
  onWatchChanged(handler: (event: { loupistId: string }) => void) { return this.watch.onWatchChanged(handler); }

  private readonly configuration = new MockConfig();
  async configRead({ scope }: { scope: Contract.ConfigScope }) {
    const workspace = scope.kind === "project" ? this.state.loupists.find((loupist) => loupist.id === scope.loupistId)?.workspace_path : undefined;
    if (scope.kind === "project" && !workspace) throw new Error("Loupist not found.");
    return this.configuration.read(scope, workspace);
  }
  async configUpdate(request: Contract.ConfigUpdateRequest) {
    const scope = request.scope;
    const workspace = scope.kind === "project" ? this.state.loupists.find((loupist) => loupist.id === scope.loupistId)?.workspace_path : undefined;
    if (scope.kind === "project" && !workspace) throw new Error("Loupist not found.");
    const result = this.configuration.update(request, workspace);
    this.modelCatalog.hidden_model_profile_ids = result.effective.models.hidden_model_profile_ids;
    for (const loupist of this.state.loupists) {
      const config = this.configuration.read({ kind: "project", loupistId: loupist.id }, loupist.workspace_path).effective;
      loupist.enabled_skill_ids = DEFAULT_AGENT_SKILL_IDS.filter((id) => config.skills[id]?.enabled ?? true);
    }
    if (request.scope.kind === "global" && (request.set.connectors || request.unset.some((path) => path[0] === "connectors"))) {
      for (const [entry_id, config] of Object.entries(result.effective.connectors)) await this.applyConnectorConfiguration({ entry_id, config });
    }
    return result;
  }
  async runLogsReveal(_request: { loupist_id: string | null }) {}
  async runLogsLastError(_request: { loupist_id: string | null }): Promise<string | null> { return null; }
  async configReveal({ scope }: { scope: Contract.ConfigScope }) { await this.configRead({ scope }); }
  private readonly inboxListeners = new Set<(event: Contract.InboxChangedEvent) => void>();
  onInboxChanged(handler: (event: Contract.InboxChangedEvent) => void) { this.inboxListeners.add(handler); return () => { this.inboxListeners.delete(handler); }; }
  onInboxOpen(_handler: (event: Contract.InboxOpenEvent) => void) { return () => {}; }
  briefingSettingsGet(request: { loupistId: string }) { return this.scheduled.briefingSettingsGet(request); }
  briefingSettingsSet(request: Contract.BriefingSettingsRequest) { return this.scheduled.briefingSettingsSet(request); }
  // The fixture already has a briefing, so the automatic default has turned menu-bar residence on.
  private resident: Contract.BriefingResidentSettings = { keepRunning: true, notificationPermissionAsked: false, keepRunningChoice: null };
  async briefingResidentSettingsGet() { return { ...this.resident }; }
  async briefingResidentSettingsSet({ keepRunning }: { keepRunning: boolean }) { this.resident = { ...this.resident, keepRunning, keepRunningChoice: keepRunning }; return { ...this.resident }; }
  inboxList(request: Contract.InboxRequest) { return this.scheduled.inboxList(request); }
  inboxMarkRead(request: { loupistId: string; runIds: string[] }) { return this.scheduled.inboxMarkRead(request); }
  inboxUnreadCounts() { return this.scheduled.inboxUnreadCounts(); }
  /** Previews and tests set this; the mock has no refused tools or due checks of its own. */
  attentionItems: AttentionItem[] = [];
  async attentionList() { return structuredClone(this.attentionItems); }
  notifications: AttentionNotifyRequest[] = [];
  async attentionNotify(request: AttentionNotifyRequest) { this.notifications.push(request); return true; }
  private readonly scheduled: MockScheduledTasks;
  scheduledTasksList(request: { loupist_id: string }) { return this.scheduled.list(request); }
  scheduledTaskSetStatus(request: Contract.ScheduledTaskIdentity & { expected_revision: number; status: "active" | "paused" }) { return this.scheduled.setStatus(request); }
  scheduledTaskDelete(request: Contract.ScheduledTaskIdentity & { expected_revision: number }) { return this.scheduled.delete(request); }
  scheduledTaskRuns(request: Contract.ScheduledTaskIdentity) { return this.scheduled.taskRuns(request); }
  scheduledTaskRunNow(request: Contract.ScheduledTaskIdentity) { return this.scheduled.runNow(request, (input, observer) => this.chatSend(input, observer)); }
  /** Preview and test helper: a task the Loupist proposed in conversation, waiting for the user's OK. */
  proposeScheduledTask(request: Contract.ScheduledTaskCreateRequest) { return this.scheduled.propose(request); }
  /** Preview and test helper: a task Rust created by itself (the results task) and its runs. */
  installScheduledTask(task: Contract.ScheduledTask, runs: Contract.ScheduledRun[] = []) { this.scheduled.installTask(task, runs); }
  scheduledTaskGet(request: Contract.ScheduledTaskIdentity) { return this.scheduled.get(request); }
  scheduledTaskConfirm(request: Contract.ScheduledTaskIdentity) { return this.scheduled.confirm(request); }
  scheduledTaskDiscard(request: Contract.ScheduledTaskIdentity) { return this.scheduled.discard(request); }
  scheduledTasksActivity(request: { loupist_id: string }) { return this.scheduled.activity(request); }
  scheduledTaskWatch(request: Contract.ScheduledRunIdentity, observer: Contract.StreamObserver<Contract.ChatStreamEvent>) { return this.scheduled.watch(request, observer); }
  private readonly state: MockBridgeState;
  private workspaceUi: Contract.WorkspaceUiState | null = null;
  private modelCatalog: Contract.ModelCatalog = initialModelCatalog();
  private readonly disabledCapabilities = new Map<string, Set<string>>();
  private readonly enabledCapabilities = new Map<string, Set<string>>();
  private readonly financeCredentials = new Set<string>();
  private readonly pendingFinanceCredentials = new Set<string>();
  private readonly financeCredentialFields = new Map<string, Set<string>>();
  private readonly pendingFinanceCredentialFields = new Map<
    string,
    Set<string>
  >();
  private readonly capabilityConfigs = new Map<
    string,
    Record<string, string>
  >();
  private readonly browserTabs = new MockBrowserTabs();

  constructor(options: { delay_ms?: number; notice_acknowledged_version?: number } = {}) {
    this.noticeVersion = options.notice_acknowledged_version ?? CURRENT_NOTICE_VERSION;
    this.state = createMockBridgeState(options.delay_ms ?? 110);
    this.scheduled = new MockScheduledTasks(this.state, loupistId => { for (const handler of this.inboxListeners) handler({ loupistId }); });
  }

  /** Adjusts pacing of future mock stream chunks; use pauseChatStreams for lifecycle control. */
  setStreamDelay(delay_ms: number): void {
    this.state.delay_ms = delay_ms;
  }

  /** Hold newly started Chat streams until released; Stop remains active. */
  pauseChatStreams(): () => void {
    if (this.state.chat_stream_gate) throw new Error("Mock Chat streams are already paused.");
    let release!: () => void;
    const gate = new Promise<void>((resolve) => { release = resolve; });
    this.state.chat_stream_gate = gate;
    return () => {
      if (this.state.chat_stream_gate === gate) delete this.state.chat_stream_gate;
      release();
    };
  }

  async modelCatalogGet(): Promise<Contract.ModelCatalog> {
    return structuredClone(this.modelCatalog);
  }

  async modelVisibilityUpdate(
    request: Contract.ModelVisibilityUpdateRequest,
  ): Promise<Contract.ModelCatalog> {
    const modelExists = this.modelCatalog.models.some(
      (model) => model.id === request.model_profile_id,
    );
    if (!modelExists) throw new Error("The selected model profile was not found.");
    const hidden = new Set(this.modelCatalog.hidden_model_profile_ids);
    if (request.visible_in_chat) hidden.delete(request.model_profile_id);
    else hidden.add(request.model_profile_id);
    const current = await this.configRead({ scope: { kind: "global" } });
    await this.configUpdate({ scope: current.scope, expectedRevision: current.revision, set: { models: { hidden_model_profile_ids: [...hidden].sort() } }, unset: [] });
    return structuredClone(this.modelCatalog);
  }

  private executionModelLock(model_profile_id: string, thinking_level: string, run_options: Record<string, string>) {
    return executionModelLock(this.modelCatalog, model_profile_id, thinking_level, run_options);
  }

  async providerModels(provider: string): Promise<Contract.ModelCatalog> {
    listProviderModels(this.modelCatalog, provider);
    return structuredClone(this.modelCatalog);
  }

  /** Preview and test helper: replaces one provider's Rust status and notifies `provider-status` listeners. */
  setProviderStatus(provider: string, status: Partial<Contract.ProviderStatusView>) {
    const option = this.modelCatalog.providers.find((item) => item.id === provider);
    const custom = this.modelCatalog.custom_providers.find((item) => item.id === provider);
    const current = option?.status ?? custom?.status ?? mockProviderStatus("none");
    const next = { ...current, ...status };
    if (option) option.status = next;
    if (custom) custom.status = next;
    for (const handler of this.providerStatusListeners) handler({ provider, status: structuredClone(next) });
  }

  /** Preview and test helper: the "Now using …" line Rust records when Auto or a saved choice changes. */
  setModelNotice(notice: Contract.ModelNotice | null) {
    this.modelCatalog.model_notice = notice;
  }

  private readonly providerStatusListeners = new Set<(event: Contract.ProviderStatusEvent) => void>();
  onProviderStatus(handler: (event: Contract.ProviderStatusEvent) => void) {
    this.providerStatusListeners.add(handler);
    return () => { this.providerStatusListeners.delete(handler); };
  }

  private markConnected(provider: string, credential_kind: Contract.ProviderCredentialKind) {
    const option = this.modelCatalog.providers.find((item) => item.id === provider);
    if (!option) return;
    option.credential_source = "saved";
    option.status = mockProviderStatus(credential_kind, { connected_at_ms: Date.now() });
  }

  async providerConfigure(request: Contract.ProviderConfigureRequest) {
    const option = this.modelCatalog.providers.find(
      (item) => item.id === request.provider,
    );
    if (option && request.clear_saved_key) {
      option.credential_source = "missing";
      option.status = mockProviderStatus("none");
    } else if (option) this.markConnected(option.id, "api_key");
    return this.providerModels(request.provider);
  }

  /** The scripted sign-in a preview or test drives; by default the browser comes back at once. */
  connectScript: (provider: string, observer: Contract.StreamObserver<Contract.ProviderConnectionEvent>) => Promise<void> = async (provider, observer) => {
    const option = this.modelCatalog.providers.find((item) => item.id === provider);
    if (option?.sign_in_flow === "device") {
      observer({ type: "device_code", user_code: "WDJB-MJHT", verification_uri: "https://accounts.x.ai/oauth2/device", expires_in_s: 900 });
    } else {
      observer({ type: "opening_browser", message: "Continue in your browser to finish signing in.", opened: true, url: null });
      observer({ type: "manual_entry" });
    }
    observer({ type: "connected", message: `${option?.label ?? "Provider"} is connected.` });
  };

  async providerConnect(
    provider: string,
    observer: Contract.StreamObserver<Contract.ProviderConnectionEvent>,
  ) {
    await this.connectScript(provider, observer);
    this.markConnected(provider, "plan");
    return this.providerModels(provider);
  }

  async providerConnectCancel() {}

  async providerConnectOpenBrowser() {}

  async providerConnectSubmitCode(request: { value: string }) {
    const value = request.value.trim();
    // ChatGPT's loopback address.
    const callback = /^http:\/\/127\.0\.0\.1:1455\/auth\/callback\?/u.test(value);
    const redirect = callback && /[?&]code=/u.test(value) && /[?&]state=/u.test(value);
    const code = /^[^\s#]+#[^\s#]+$/u.test(value) && value.length <= 2048;
    if (!redirect && !code) throw commandError("paste_invalid", "That isn't the sign-in address.");
  }

  /** Preview and test knob for `providerLocalDiscover`. */
  localDiscovery: Contract.LocalDiscovery = {
    engines: [
      { engine: "ollama", label: "Ollama", base_url: "http://localhost:11434/v1", running: true, models: [
        { id: "qwen3:32b", name: "qwen3:32b", context_tokens: 40_960 },
        { id: "llama3.2:3b", name: "llama3.2:3b", context_tokens: 4_096 },
      ] },
      { engine: "lm_studio", label: "LM Studio", base_url: "http://localhost:1234/v1", running: false, models: [] },
      { engine: "llama_cpp", label: "llama.cpp", base_url: "http://localhost:8080/v1", running: false, models: [] },
    ],
  };

  async providerLocalDiscover() {
    return structuredClone(this.localDiscovery);
  }

  /** Preview and test knob for `providerCustomTest`. */
  customTestResult: Contract.ProviderCustomTestResult = { outcome: "ok", models: ["qwen2.5-72b-instruct", "llama-3.3-70b-instruct", "mistral-small-3.2"] };

  async providerCustomTest(request: Contract.ProviderCustomTestRequest): Promise<Contract.ProviderCustomTestResult> {
    const address = request.base_url.trim();
    if (!/^https:\/\//u.test(address) && !/^http:\/\/(localhost|127\.0\.0\.1|\[::1\])(:|\/|$)/u.test(address)) return { outcome: "invalid", models: [] };
    return structuredClone(this.customTestResult);
  }

  async providerOpenConsole(_request: { provider: string }) {}

  async providerOpenUsage(_request: { provider: string }) {}

  async providerPresetSave(request: Contract.ProviderPresetRequest) {
    if (!request.api_key.trim()) throw commandError("key_rejected", "Upstage didn't accept this key.");
    const next = await this.providerCustomSave({ name: "Upstage Solar", base_url: "https://api.upstage.ai/v1", api: "openai-completions", api_key: request.api_key, model_ids: ["solar-pro2", "solar-mini"] });
    const custom = next.custom_providers.find((item) => item.name === "Upstage Solar");
    if (custom) custom.status = mockProviderStatus("custom", { connected_at_ms: Date.now() });
    return structuredClone(this.modelCatalog);
  }

  async modelNoticeAck(request: { id: string }) {
    if (this.modelCatalog.model_notice?.id === request.id) this.modelCatalog.model_notice = null;
    return structuredClone(this.modelCatalog);
  }

  async providerDisconnect(provider: string) {
    const option = this.modelCatalog.providers.find(
      (item) => item.id === provider,
    );
    if (option) {
      option.credential_source = "missing";
      option.status = mockProviderStatus("none");
    }
    this.modelCatalog.models = this.modelCatalog.models.filter(
      (model) => model.provider !== provider,
    );
    return structuredClone(this.modelCatalog);
  }

  async providerCustomSave(request: Contract.CustomProviderRequest) {
    const providers = this.modelCatalog.custom_providers;
    const id = request.provider ?? `custom-${request.name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "provider"}`;
    const base_url = request.base_url.trim().replace(/\/+$/, "");
    const local = /^http:\/\/(localhost|127\.0\.0\.1|\[::1\])(:|\/|$)/u.test(base_url);
    const provider: Contract.CustomModelProvider = { id, name: request.name.trim(), base_url, api: request.api, model_ids: request.model_ids.map(item => item.trim()).filter(Boolean), credential_source: "saved", status: mockProviderStatus(local ? "local" : "custom") };
    this.modelCatalog.custom_providers = [...providers.filter(item => item.id !== id), provider];
    this.modelCatalog.models = [
      ...this.modelCatalog.models.filter(model => model.provider !== id),
      ...provider.model_ids.map(model => ({ id: `${id}/${model}`, name: model, provider: id, model, input: ["text"], reasoning: false, context_window: 128_000, max_tokens: 16_384, thinking_levels: ["off"], thinking_level_map: {}, run_controls: [], credential_source: "saved" as const })),
    ];
    return structuredClone(this.modelCatalog);
  }

  async providerCustomRemove(provider: string) {
    this.modelCatalog.custom_providers = this.modelCatalog.custom_providers.filter(item => item.id !== provider);
    this.modelCatalog.models = this.modelCatalog.models.filter(model => model.provider !== provider);
    return structuredClone(this.modelCatalog);
  }

  async marketplaceSnapshot(): Promise<Contract.MarketplaceSnapshot> {
    return createMockMarketplaceSnapshot(
      this.financeCredentials,
      this.pendingFinanceCredentials,
      this.capabilityConfigs,
      this.financeCredentialFields,
      this.pendingFinanceCredentialFields,
    );
  }

  async loupistCapabilityList(
    loupist_id: string,
  ): Promise<Contract.LoupistCapabilityBinding[]> {
    return createMockLoupistCapabilityBindings(
      this.disabledCapabilities.get(loupist_id) ?? new Set(),
      this.enabledCapabilities.get(loupist_id) ?? new Set(),
      await this.marketplaceSnapshot(),
    );
  }

  private readonly customSkills = new Map<string, { owner: string; document: Contract.SkillDocument }>();
  async skillsList({ scope }: { scope: Contract.ConfigScope }): Promise<Contract.SkillCatalog> {
    const configuration = await this.configRead({ scope });
    const owner = scope.kind === "global" ? "global" : scope.loupistId;
    const skills: Contract.ManagedSkill[] = [...FIXED_AGENT_SKILLS, ...AGENT_SKILLS].map((skill) => ({ ...skill, path: null, source: "bundled", fixed: Boolean(skill.fixed), editable: false, shadowed: false, revision: `bundled-${skill.id}`, error: null, enabled: Boolean(skill.fixed) || (configuration.effective.skills[skill.id]?.enabled ?? true) }));
    const names = new Set(skills.map((s) => s.id));
    const custom = [...this.customSkills.values()].filter((s) => s.owner === "global" || s.owner === owner).sort((a, b) => Number(a.owner === "global") - Number(b.owner === "global"));
    for (const entry of custom) {
      const skill = structuredClone(entry.document.skill);
      skill.shadowed = names.has(skill.id); names.add(skill.id);
      skill.editable = entry.owner === owner && !skill.shadowed;
      skill.enabled = !skill.shadowed && (configuration.effective.skills[skill.id]?.enabled ?? true);
      skills.push(skill);
    }
    return { skills, configuration };
  }
  async skillsRead(request: Contract.SkillReadRequest): Promise<Contract.SkillDocument> {
    const entry = (await this.skillsList(request)).skills.find((s) => (s.path ?? s.id) === request.key);
    if (!entry) throw new Error("Skill not found.");
    const stored = entry.path ? this.customSkills.get(entry.path) : undefined;
    return stored ? { ...structuredClone(stored.document), skill: entry } : { skill: entry, instructions: entry.description, files: ["SKILL.md"] };
  }
  async skillsCreate(request: Contract.SkillCreateRequest): Promise<Contract.SkillCatalog> {
    const configuration = await this.configRead(request);
    const id = request.name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || `skill-${crypto.randomUUID().slice(0, 8)}`;
    if (!request.name.trim() || !request.description.trim() || !request.instructions.trim()) throw new Error("Enter a name, description and instructions.");
    const owner = request.scope.kind === "global" ? "global" : request.scope.loupistId;
    const path = `${configuration.path.slice(0, -"config.toml".length)}skills/${id}/SKILL.md`;
    if ([...AGENT_SKILLS, ...FIXED_AGENT_SKILLS].some((s) => s.id === id) || this.customSkills.has(path)) throw new Error("A skill with that name already exists.");
    const skill: Contract.ManagedSkill = { id, name: request.name, description: request.description, path, source: owner === "global" ? "personal" : "loupist", enabled: true, fixed: false, editable: true, shadowed: false, revision: crypto.randomUUID(), error: null };
    this.customSkills.set(path, { owner, document: { skill, instructions: request.instructions, files: ["SKILL.md", "agents/openai.yaml"] } });
    return this.skillsList(request);
  }
  async skillsSave(request: Contract.SkillSaveRequest): Promise<Contract.SkillCatalog> {
    const current = await this.skillsRead(request);
    if (!current.skill.editable) throw new Error("This skill is read-only here.");
    if (current.skill.revision !== request.expectedRevision) throw new Error("Skill changed outside this view; reload before saving.");
    current.skill.description = request.description;
    current.skill.revision = crypto.randomUUID();
    current.instructions = request.instructions;
    this.customSkills.get(request.key)!.document = current;
    return this.skillsList(request);
  }
  async skillsDelete(request: Contract.SkillDeleteRequest): Promise<Contract.SkillCatalog> {
    const current = await this.skillsRead(request);
    if (!current.skill.editable || current.skill.revision !== request.expectedRevision) throw new Error("Reload this skill in its original scope before deleting.");
    this.customSkills.delete(request.key);
    return this.skillsList(request);
  }
  async skillsImport(request: Contract.SkillImportRequest): Promise<Contract.SkillCatalog | null> {
    if (request.kind === "github") throw new Error("GitHub downloads are available in the desktop app.");
    return null;
  }
  async skillsReveal(request: Contract.SkillReadRequest) { await this.skillsRead(request); }
  async agentSkillCatalog(loupist_id: string): Promise<Contract.AgentSkillSummary[]> {
    const catalog = await this.skillsList({ scope: { kind: "project", loupistId: loupist_id } });
    return catalog.skills.filter((skill) => !skill.shadowed && !skill.error).map(({ id, name, description, enabled, fixed }) => ({ id, name, description, enabled, fixed }));
  }

  async loupistCapabilityEnable(request: Contract.LoupistCapabilityRequest) {
    this.disabledCapabilities.get(request.loupist_id)?.delete(request.entry_id);
    const enabled =
      this.enabledCapabilities.get(request.loupist_id) ?? new Set<string>();
    enabled.add(request.entry_id);
    this.enabledCapabilities.set(request.loupist_id, enabled);
    const binding = (await this.loupistCapabilityList(request.loupist_id)).find(
      (candidate) => candidate.entry_id === request.entry_id,
    );
    if (!binding) throw new Error("Capability binding is unavailable.");
    return binding;
  }

  async marketplaceConnectorConfigure(
    request: Contract.MarketplaceConnectorConfigureRequest,
  ) {
    const current = await this.configRead({ scope: { kind: "global" } });
    await this.applyConnectorConfiguration(request);
    await this.configUpdate({ scope: current.scope, expectedRevision: current.revision, set: { connectors: { [request.entry_id]: request.config } }, unset: [["connectors", request.entry_id]] });
  }

  private async applyConnectorConfiguration(request: Contract.MarketplaceConnectorConfigureRequest) {
    const entry = createMockMarketplaceSnapshot().catalog.entries.find(
      (candidate) => candidate.id === request.entry_id,
    );
    const fields = entry?.setup?.config_fields ?? [];
    if (
      Object.keys(request.config).some(
        (fieldId) => !fields.some((field) => field.id === fieldId),
      ) ||
      fields.some((field) => {
        const value = request.config[field.id];
        return value === undefined
          ? field.required
          : !isValidMockSetupValue(field, value);
      })
    ) {
      throw new Error(
        "Configuration does not match the connector setup contract.",
      );
    }
    const wasReady = await this.connectorIsReady(request.entry_id);
    const scopeFields = entry?.setup?.credential_scope_fields ?? [];
    if (
      scopeFields.some(
        (fieldId) =>
          this.capabilityConfigs.get(request.entry_id)?.[fieldId] !==
          request.config[fieldId],
      )
    ) {
      this.financeCredentials.delete(request.entry_id);
      this.pendingFinanceCredentials.delete(request.entry_id);
      this.financeCredentialFields.delete(request.entry_id);
      this.pendingFinanceCredentialFields.delete(request.entry_id);
      for (const [loupistId, enabled] of this.enabledCapabilities) {
        enabled.delete(request.entry_id);
        const disabled =
          this.disabledCapabilities.get(loupistId) ?? new Set<string>();
        disabled.add(request.entry_id);
        this.disabledCapabilities.set(loupistId, disabled);
      }
    }
    this.capabilityConfigs.set(
      request.entry_id,
      structuredClone(request.config),
    );
    if (!wasReady && (await this.connectorIsReady(request.entry_id))) {
      this.enableReadyConnectorForAllLoupists(request.entry_id);
    }
  }

  async loupistCapabilityDisable(request: Contract.LoupistCapabilityRequest) {
    const disabled =
      this.disabledCapabilities.get(request.loupist_id) ?? new Set<string>();
    disabled.add(request.entry_id);
    this.disabledCapabilities.set(request.loupist_id, disabled);
    this.enabledCapabilities.get(request.loupist_id)?.delete(request.entry_id);
    const binding = (await this.loupistCapabilityList(request.loupist_id)).find(
      (candidate) => candidate.entry_id === request.entry_id,
    );
    if (!binding) throw new Error("Capability binding is unavailable.");
    return binding;
  }

  async marketplaceCredentialSave(
    request: Contract.MarketplaceCredentialSaveRequest,
  ) {
    const entry = createMockMarketplaceSnapshot().catalog.entries.find(
      (candidate) => candidate.id === request.entry_id,
    );
    const fields = entry?.setup?.credential_fields ?? [];
    const submittedIds = Object.keys(request.secrets).sort();
    const declaredIds = new Set(fields.map((field) => field.id));
    const existingFields = new Set([
      ...(this.financeCredentialFields.get(request.entry_id) ?? []),
      ...(this.pendingFinanceCredentialFields.get(request.entry_id) ?? []),
    ]);
    const mergedFields = new Set([...existingFields, ...submittedIds]);
    if (
      !fields.length ||
      !submittedIds.length ||
      submittedIds.some((credentialId) => !declaredIds.has(credentialId)) ||
      submittedIds.some((credentialId) => {
        const field = fields.find((candidate) => candidate.id === credentialId);
        return !field || !isValidMockSetupValue(field, request.secrets[credentialId] ?? "");
      }) ||
      fields.some((field) => field.required && !mergedFields.has(field.id))
    ) {
      throw new Error("Credentials do not match the connector setup contract.");
    }
    this.pendingFinanceCredentials.add(request.entry_id);
    this.pendingFinanceCredentialFields.set(request.entry_id, mergedFields);
    return (await this.marketplaceSnapshot()).connectors.find(
      (connector) => connector.entry_id === request.entry_id,
    )!
      .credentials;
  }

  async marketplaceCredentialVerify(
    request: Contract.MarketplaceCredentialRequest,
  ) {
    if (
      !this.pendingFinanceCredentials.has(request.entry_id) &&
      !this.financeCredentials.has(request.entry_id)
    ) {
      throw new Error("Credential is missing.");
    }
    const wasReady = await this.connectorIsReady(request.entry_id);
    const pendingFields = this.pendingFinanceCredentialFields.get(
      request.entry_id,
    );
    if (pendingFields) {
      this.financeCredentialFields.set(
        request.entry_id,
        new Set(pendingFields),
      );
    }
    this.pendingFinanceCredentials.delete(request.entry_id);
    this.pendingFinanceCredentialFields.delete(request.entry_id);
    this.financeCredentials.add(request.entry_id);
    if (!wasReady && (await this.connectorIsReady(request.entry_id))) {
      this.enableReadyConnectorForAllLoupists(request.entry_id);
    }
    return (await this.marketplaceSnapshot()).connectors.find(
      (connector) => connector.entry_id === request.entry_id,
    )!
      .credentials;
  }

  async marketplaceCredentialDelete(
    request: Contract.MarketplaceCredentialRequest,
  ) {
    this.financeCredentials.delete(request.entry_id);
    this.pendingFinanceCredentials.delete(request.entry_id);
    this.financeCredentialFields.delete(request.entry_id);
    this.pendingFinanceCredentialFields.delete(request.entry_id);
    for (const [loupistId, enabled] of this.enabledCapabilities) {
      enabled.delete(request.entry_id);
      const disabled =
        this.disabledCapabilities.get(loupistId) ?? new Set<string>();
      disabled.add(request.entry_id);
      this.disabledCapabilities.set(loupistId, disabled);
    }
    return (await this.marketplaceSnapshot()).connectors.find(
      (connector) => connector.entry_id === request.entry_id,
    )!
      .credentials;
  }

  async siteFavicon(_url: string): Promise<string | null> {
    return null;
  }

  async openExternalUrl(url: string) {
    window.open(validBrowserUrl(url).href, "_blank", "noopener,noreferrer");
  }

  browserTabOpen(request: Contract.BrowserTabOpenRequest, observer: Contract.StreamObserver<Contract.BrowserTabEvent>) { return this.browserTabs.open(request, observer); }
  browserTabNavigate(tab_id: string, url: string) { return this.browserTabs.navigate(tab_id, url); }
  browserTabHistory(tab_id: string, direction: Contract.BrowserHistoryDirection) { return this.browserTabs.history(tab_id, direction); }
  browserTabReload(tab_id: string) { return this.browserTabs.reload(tab_id); }
  browserTabSetBounds(request: Contract.BrowserTabBoundsRequest) { return this.browserTabs.setBounds(request); }
  browserTabClose(tab_id: string) { return this.browserTabs.close(tab_id); }
  browserTabsReset() { return this.browserTabs.reset(); }

  async updateStatus(): Promise<Contract.UpdateState> {
    return {
      supported: true,
      current_version: "0.0.1",
      phase: "idle",
      offer: null,
      downloaded_bytes: 0,
      total_bytes: null,
      last_checked_at_ms: null,
      next_auto_check_at_ms: null,
      error: null,
      blockers: [],
    };
  }

  updateCheck(): Promise<Contract.UpdateState> {
    return this.updateStatus();
  }

  async updateInstall(
    _request: Contract.UpdateInstallRequest,
  ): Promise<Contract.UpdateInstallResult> {
    return { outcome: "cancelled", blockers: [] };
  }

  loupistList(): Promise<Contract.LoupistSummary[]> {
    return loupistChat.loupistList(this.state);
  }

  loupistSelect(loupist_id: string): Promise<Contract.LoupistWorkspace> {
    return loupistChat.loupistSelect(this.state, loupist_id);
  }

  async loupistRecover(
    request: Contract.LoupistRecoverRequest,
  ): Promise<Contract.LoupistSummary> {
    const loupist = this.state.loupists.find(
      (candidate) => candidate.id === request.loupist_id,
    );
    if (!loupist) throw new Error("Loupist not found");
    if (request.action !== "recover_memory") {
      throw new Error("Unsupported Loupist recovery action");
    }
    loupist.availability = { status: "available" };
    return structuredClone(loupist);
  }

  async loupistChooseFolder(): Promise<string | null> { return "/Users/research/new-research"; }
  async loupistReconnect(request: Contract.LoupistReconnectRequest) {
    const loupist = this.state.loupists.find((item) => item.id === request.loupist_id);
    if (!loupist) throw new Error("Loupist not found.");
    loupist.workspace_path = request.workspace_path;
    loupist.availability = { status: "available" };
    return structuredClone(loupist);
  }
  async workspaceList(request: Contract.WorkspacePathRequest): Promise<Contract.WorkspaceDirectory> {
    if (!this.state.loupists.some((item) => item.id === request.loupist_id)) throw new Error("Loupist not found.");
    return { path: request.path ?? "", entries: [], truncated: false };
  }
  async workspaceRead(_request: Contract.WorkspacePathRequest & { path: string }): Promise<Contract.WorkspaceFile> { throw new Error("File not found."); }
  async workspaceReveal(_request: Contract.WorkspacePathRequest) {}
  async chatActivityRead(_request: Contract.ChatActivityRequest): Promise<Contract.ChatActivityDetail> { throw new Error("Activity not found."); }
  private draftFiles = new Map<string, import("../contracts/workspace").DraftAttachment[]>();
  async chatDraftFilesRead(request: { loupist_id: string; thread_id: string }) { return structuredClone(this.draftFiles.get(JSON.stringify(request)) ?? []); }
  async chatDraftFilesWrite(request: { loupist_id: string; thread_id: string }, files: import("../contracts/workspace").DraftAttachment[]) { this.draftFiles.set(JSON.stringify(request), structuredClone(files)); }
  private draftText = new Map<string, Contract.StoredChatDraft>();
  async chatDraftRead(request: { loupist_id: string; thread_id: string }) { return structuredClone(this.draftText.get(JSON.stringify(request)) ?? null); }
  async chatDraftWrite(request: { loupist_id: string; thread_id: string }, draft: Contract.StoredChatDraft) { this.draftText.set(JSON.stringify(request), structuredClone(draft)); }
  async workspaceUiRead(): Promise<Contract.WorkspaceUiState | null> { return structuredClone(this.workspaceUi); }
  async workspaceUiWrite(state: Contract.WorkspaceUiState): Promise<Contract.WorkspaceUiState> {
    this.workspaceUi = structuredClone(state);
    return structuredClone(this.workspaceUi);
  }

  /** Mirrors Rust: the first connected World default model, if any. */
  private async worldDefaultModel() {
    const catalog = await this.modelCatalogGet();
    catalog.models = catalog.models.filter((model) => worldDefaults.some((entry) => entry.provider === model.provider) && model.input.includes("text") && model.credential_source !== "missing" && catalog.providers.some((provider) => provider.id === model.provider && provider.credential_source !== "missing"));
    const selection = worldDefaults.flatMap((entry) => entry.models.map((candidate) => {
      const model = catalog.models.find((item) => item.provider === entry.provider && item.model === candidate.id && item.thinking_levels.includes(candidate.thinking_level));
      return model ? { model_profile_id: model.id, thinking_level: candidate.thinking_level, run_options: defaultRunOptions(model) } : null;
    })).find(Boolean) ?? null;
    return { catalog, selection };
  }

  loupistProfileUpdate(request: Contract.LoupistProfileUpdateRequest) { return loupistChat.loupistProfileUpdate(this.state, request); }

  async loupistImport(request: Contract.LoupistImportRequest): Promise<Contract.LoupistSummary> {
    const existing = this.state.loupists.find(loupist => loupist.workspace_path === request.workspace_path);
    if (existing) return structuredClone(existing);
    const loupist = await loupistChat.loupistCreate(this.state, { name: workspaceFolderName(request.workspace_path) });
    this.state.loupists.find(item => item.id === loupist.id)!.workspace_path = request.workspace_path;
    return { ...loupist, workspace_path: request.workspace_path };
  }

  async loupistCreate(
    request: Contract.LoupistCreateRequest,
  ): Promise<Contract.LoupistSummary> {
    const { catalog, selection } = await this.worldDefaultModel();
    if (selection && !catalog.models.some((model) => model.id === selection.model_profile_id)) throw new Error("The World model is no longer connected or supported. Choose another model.");
    const loupist = await loupistChat.loupistCreate(this.state, request);
    const settings = await knowledge.knowledgeGraphSettingsGet(this.state, { loupistId: loupist.id });
    await knowledge.knowledgeGraphSettingsSet(this.state, { loupistId: loupist.id, settings: { ...settings, enrichmentEnabled: true, helperModelProfileId: selection?.model_profile_id ?? null, thinkingLevel: selection?.thinking_level ?? "off", runOptions: selection?.run_options ?? {}, researchFocus: request.focus ?? "" } });
    const seeded = curatedSeedView(request.template);
    if (seeded) this.loupistWorldSchema.install(loupist.id, seeded);
    loupist.enabled_skill_ids = (await this.agentSkillCatalog(loupist.id)).filter((skill) => !skill.fixed && skill.enabled).map((skill) => skill.id);
    this.state.loupists.find((item) => item.id === loupist.id)!.enabled_skill_ids = [...loupist.enabled_skill_ids];
    return loupist;
  }

  async loupistDelete(request: Contract.LoupistDeleteRequest) {
    await loupistChat.loupistDelete(this.state, request);
    this.disabledCapabilities.delete(request.loupist_id);
  }

  async chatCreate(request: Contract.ChatCreateRequest): Promise<Contract.ChatThread> {
    const defaults = (await this.configRead({ scope: { kind: "project", loupistId: request.loupist_id } })).effective.chat;
    const thread = await loupistChat.chatCreate(this.state, request);
    thread.use_memory = defaults.use_memory;
    thread.update_memory = defaults.update_memory;
    const stored = this.state.threads[request.loupist_id].find((candidate) => candidate.id === thread.id)!;
    stored.use_memory = thread.use_memory;
    stored.update_memory = thread.update_memory;
    return thread;
  }

  async chatFeedbackSet(request: Contract.ChatFeedbackRequest): Promise<Contract.ChatFeedback> {
    const thread = this.state.threads[request.loupist_id]?.find((item) => item.id === request.thread_id);
    const message = thread?.messages.find((item) => item.id === request.message_id);
    if (!message || message.role !== "assistant" || message.status !== "complete") throw new Error("Feedback requires a completed assistant message in this Loupist.");
    const correction = request.correction?.trim() || null;
    if (correction && [...correction].length > 4000) throw new Error("Correction exceeds 4000 characters.");
    const feedback = { rating: request.rating, correction, updatedAt: Date.now() };
    message.feedback = feedback;
    return structuredClone(feedback);
  }

  chatRename(
    request: Contract.ChatRenameRequest,
  ): Promise<Contract.ChatThread> {
    return loupistChat.chatRename(this.state, request);
  }

  /** Mock of the Rust version store: tails set aside per version id. */
  private chatVersionMessages = new Map<string, Contract.ChatMessage[]>();

  private keepChatVersion(thread: Contract.ChatThread, position: number, tail: Contract.ChatMessage[]) {
    if (!tail.length) return;
    const id = `version-${crypto.randomUUID()}`;
    this.chatVersionMessages.set(id, tail);
    thread.versions = [...(thread.versions ?? []), {
      id,
      after_message_id: position > 0 ? thread.messages[position - 1]!.id : null,
      first_message_at_ms: Date.parse(tail[0]!.created_at),
      message_count: tail.length,
    }];
  }

  async chatBranch(request: Contract.ChatBranchRequest): Promise<Contract.ChatThread> {

    const thread = this.state.threads[request.loupist_id]?.find((item) => item.id === request.thread_id);
    if (!thread) throw new Error("Chat not found.");
    const byId = thread.messages.findIndex((message) => message.id === request.message_id);
    const byPlace = request.position !== undefined && thread.messages[request.position]?.role === "user" && thread.messages[request.position]?.content === request.content ? request.position : -1;
    const position = byId >= 0 ? byId : byPlace;
    if (position < 0 || thread.messages[position]!.role !== "user") throw new Error("Only a message you sent can be edited.");
    const tail = thread.messages.slice(position);
    thread.messages = thread.messages.slice(0, position);
    this.keepChatVersion(thread, position, tail);
    return structuredClone(thread);
  }

  async chatVersionSwitch(request: Contract.ChatVersionSwitchRequest): Promise<Contract.ChatThread> {

    const thread = this.state.threads[request.loupist_id]?.find((item) => item.id === request.thread_id);
    const version = thread?.versions?.find((item) => item.id === request.version_id);
    const messages = version ? this.chatVersionMessages.get(version.id) : undefined;
    if (!thread || !version || !messages) throw new Error("Chat version not found.");
    const position = version.after_message_id === null ? 0 : thread.messages.findIndex((message) => message.id === version.after_message_id) + 1;
    if (position === 0 && version.after_message_id !== null) throw new Error("This version no longer follows the chat as it is now.");
    thread.versions = thread.versions!.filter((item) => item.id !== version.id);
    this.chatVersionMessages.delete(version.id);
    const shown = thread.messages.slice(position);
    thread.messages = thread.messages.slice(0, position);
    this.keepChatVersion(thread, position, shown);
    thread.messages = [...thread.messages, ...messages];
    return structuredClone(thread);
  }

  async chatMemorySetAside(request: Contract.ChatMemorySetAsideRequest): Promise<Contract.ChatThread> {
    const thread = this.state.threads[request.loupist_id]?.find((item) => item.id === request.thread_id);
    if (!thread) throw new Error("Chat not found");
    const current = new Set(thread.set_aside_memory ?? []);
    if (request.set_aside) current.add(request.record_id); else current.delete(request.record_id);
    thread.set_aside_memory = [...current];
    return structuredClone(thread);
  }

  /** Mock decision: settles the waiting card in the stored reply, as the host's next progress patch would.
   * Like Rust, "always" counts as once for every kind but a custom connection's tool and a command. */
  async chatApprovalDecide(request: { loupist_id: string; request_id: string; decision: Contract.ChatApprovalDecision }): Promise<void> {
    for (const thread of this.state.threads[request.loupist_id] ?? []) {
      for (const message of thread.messages) {
        const approval = message.progress?.approvals?.find((item) => item.requestId === request.request_id);
        if (!approval) continue;
        if (approval.state !== "waiting") throw new Error("This approval is no longer waiting");
        approval.state = request.decision === "deny" ? "declined"
          : request.decision === "always" ? "always_allowed" : "allowed_once";
        return;
      }
    }
    throw new Error("This approval is no longer waiting");
  }

  /** Mock answer: settles the waiting question in the stored reply, as the host's next progress patch would. */
  async chatQuestionAnswer(request: { loupist_id: string; request_id: string; choice?: number; text?: string }): Promise<void> {
    if ((request.choice === undefined) === (request.text === undefined)) throw new Error("Send exactly one of choice and text");
    for (const thread of this.state.threads[request.loupist_id] ?? []) {
      for (const message of thread.messages) {
        const question = message.progress?.questions?.find((item) => item.requestId === request.request_id);
        if (!question) continue;
        if (question.state !== "waiting") throw new Error("This question is no longer waiting");
        if (request.choice !== undefined && !question.options[request.choice]) throw new Error("choice is not one of the options");
        const text = request.text?.trim();
        if (request.text !== undefined && (!text || text.length > 500)) throw new Error("text must be 1-500 characters");
        question.state = "answered";
        question.answer = request.choice !== undefined ? { choice: request.choice } : { text: text! };
        return;
      }
    }
    throw new Error("This question is no longer waiting");
  }

  chatDelete(request: Contract.ChatDeleteRequest): Promise<void> {
    return loupistChat.chatDelete(this.state, request);
  }

  chatAttachmentRead(
    loupist_id: string,
    thread_id: string,
    message_id: string,
    attachment_id: string,
  ) {
    return loupistChat.chatAttachmentRead(
      this.state,
      loupist_id,
      thread_id,
      message_id,
      attachment_id,
    );
  }

  chatArtifactList(loupist_id: string, thread_id: string) {
    return loupistChat.chatArtifactList(this.state, loupist_id, thread_id);
  }
  loupistArtifactList(request: { loupist_id: string; limit?: number }) {
    return loupistChat.loupistArtifactList(this.state, request);
  }

  chatArtifactRead(
    loupist_id: string,
    thread_id: string,
    artifact_id: string,
    revision_id?: number,
  ) {
    return loupistChat.chatArtifactRead(this.state, loupist_id, thread_id, artifact_id, revision_id);
  }

  chatArtifactVersions(loupist_id: string, thread_id: string, artifact_id: string) {
    return loupistChat.chatArtifactVersions(this.state, loupist_id, thread_id, artifact_id);
  }

  chatArtifactRestore(request: Contract.ChatArtifactRestoreRequest) {
    return loupistChat.chatArtifactRestore(this.state, request);
  }

  researchHandoffStart(request: Contract.ResearchHandoffRequest) { return loupistChat.researchHandoffStart(this.state, request); }
  private connections = new Map<string, import("../contracts/knowledge").ConnectionView>();
  private async connectionSetStatus(request: { loupistId: string; connectionId: string }, status: import("../contracts/knowledge").ConnectionStatus) {
    const found = this.connections.get(request.connectionId);
    if (!found) throw new Error("No such link.");
    const next = { ...found, status };
    this.connections.set(request.connectionId, next);
    return next;
  }
  knowledgeConnectionDismiss(request: { loupistId: string; connectionId: string }) { return this.connectionSetStatus(request, "dismissed"); }
  knowledgeConnectionOpen(request: { loupistId: string; connectionId: string }) { return this.connectionSetStatus(request, "opened"); }
  chatSend(
    request: Contract.ChatSendRequest,
    observer: Contract.StreamObserver<Contract.ChatStreamEvent>,
  ): Promise<{ run_id: string }> {
    const capabilities = createMockLoupistCapabilityBindings(
      this.disabledCapabilities.get(request.loupist_id) ?? new Set(),
      this.enabledCapabilities.get(request.loupist_id) ?? new Set(),
      createMockMarketplaceSnapshot(
        this.financeCredentials,
        this.pendingFinanceCredentials,
        this.capabilityConfigs,
        this.financeCredentialFields,
        this.pendingFinanceCredentialFields,
      ),
    ).flatMap((binding) => (binding.enabled ? [binding.entry_id] : []));
    return loupistChat.chatSend(
      this.state,
      request,
      observer,
      capabilities,
      this.executionModelLock(
        request.model_profile_id,
        request.thinking_level,
        request.run_options,
      ),
    );
  }

  chatSteer(request: Contract.ChatControlRequest) {
    return loupistChat.chatSteer(this.state, request);
  }

  chatRetryLast(request: { loupist_id: string; thread_id: string }) {
    return Promise.resolve(loupistChat.chatRetryLast(this.state, request));
  }

  chatAbort(run_id: string): Promise<void> {
    return loupistChat.chatAbort(this.state, run_id);
  }

  async runActivityList(): Promise<Contract.RunActivity[]> {
    return structuredClone(
      [...this.state.run_activities.values()].sort(
        (left, right) => left.started_at_ms - right.started_at_ms,
      ),
    );
  }

  knowledgeCatalog(request: Contract.KnowledgeCatalogRequest) {
    return knowledge.knowledgeCatalog(this.state, request);
  }

  knowledgeWorldQuery(request: Contract.WorldGraphQuery) {
    return knowledge.knowledgeWorldQuery(this.state, request);
  }

  knowledgeMap(request: Contract.KnowledgeMapQuery) {
    return knowledge.knowledgeMap(this.state, request);
  }

  async knowledgeSourceExport(request: Contract.KnowledgeSourceReadRequest) {
    const detail = await knowledge.knowledgeSourceRead(this.state, request);
    if (!request.spanId || !detail.spans.some((span) => span.spanId === request.spanId)) throw new Error("Choose a retained source span to export.");
    return `/exports/${detail.title}`;
  }

  async knowledgeSourceSelectionResolve(request: Contract.KnowledgeSourceSelectionRequest): Promise<Contract.KnowledgePin> {
    const detail = await this.knowledgeSourceRead(request.source);
    const span = detail.spans.find(item => item.spanId === request.source.spanId);
    const text = span?.presentation?.text ?? span?.text;
    const { startUtf16, endUtf16, text: selected } = request.selection;
    if (!span || !text || startUtf16 < 0 || endUtf16 <= startUtf16 || text.slice(startUtf16, endUtf16) !== selected) throw new Error("Source selection is no longer available.");
    return { kind: "source", loupistId: request.source.loupistId, recordId: detail.recordId, revisionId: detail.revisionId, objectId: detail.sourceId, spanId: span.spanId, label: detail.title, selection: { ...request.selection } };
  }

  knowledgeSourceRead(request: Contract.KnowledgeSourceReadRequest) {
    return knowledge.knowledgeSourceRead(this.state, request);
  }

  learningReviewList(request: import("../contracts/knowledge").LearningReviewQuery) { return knowledge.learningReviewList(this.state, request); }
  memoryLearningSettingsGet(request: { loupistId: string }) { return knowledge.memoryLearningSettingsGet(this.state, request); }
  memoryLearningSettingsSet(request: { loupistId: string; learnMethods: boolean }) { return knowledge.memoryLearningSettingsSet(this.state, request); }
  onLearningReviewFinished(_handler: (event: import("../contracts/knowledge").LearningReviewFinishedEvent) => void): () => void { return () => {}; }
  async worldSchemaGet(request: { loupistId: string }) { return this.loupistWorldSchema.get(request.loupistId); }
  async worldSchemaUndo(request: { loupistId: string; changeId: string }) { return this.loupistWorldSchema.undo(request.loupistId, request.changeId); }
  async worldSchemaRemove(request: { loupistId: string; kind: "type" | "relation"; name: string }) { return this.loupistWorldSchema.remove(request.loupistId, request.kind, request.name); }
  onWorldSchemaChanged(handler: (event: { loupistId: string }) => void) { return this.loupistWorldSchema.subscribe(handler); }
  principleList(request: PrincipleListRequest) {
    return knowledge.principleList(this.state, request);
  }

  principleCreate(request: PrincipleCreateRequest) {
    return knowledge.principleCreate(this.state, request);
  }

  principleUpdate(request: PrincipleUpdateRequest) {
    return knowledge.principleUpdate(this.state, request);
  }

  principleArchive(request: PrincipleArchiveRequest) {
    return knowledge.principleArchive(this.state, request);
  }

  knowledgeMemoryControls(request: { loupistId: string }) { return knowledge.knowledgeMemoryControls(this.state, request); }
  knowledgeLensPause(request: { loupistId: string; recordId: string; paused: boolean }) { return knowledge.knowledgeLensPause(this.state, request); }
  knowledgeMemoryResetPreview(request: { loupistId: string }) { return knowledge.knowledgeMemoryResetPreview(this.state, request); }
  knowledgeMemoryReset(request: { loupistId: string; confirm: boolean; expected: import("../contracts/knowledge").MemoryResetCounts }) { return knowledge.knowledgeMemoryReset(this.state, request); }
  knowledgeMemoryUsage(request: { loupistId: string }) { return knowledge.knowledgeMemoryUsage(this.state, request); }
  knowledgeLensArchive(request: { loupistId: string; recordId: string; archived: boolean }) {
    return knowledge.knowledgeLensArchive(this.state, request);
  }

  knowledgeLensRead(request: Contract.KnowledgeLensReadRequest) {
    return knowledge.knowledgeLensRead(this.state, request);
  }

  private workingNotes = new Map<string, Contract.WorkingNote[]>();

  async knowledgeWorkingNoteRead(request: Contract.WorkingNoteReadRequest): Promise<Contract.WorkingNote> {
    for (const [key, notes] of this.workingNotes) {
      if (!key.startsWith(`${request.loupistId}:`)) continue;
      const found = notes.find((note) => note.researchId === request.noteId);
      if (found) return structuredClone(found);
    }
    throw new Error("Working note not found.");
  }

  async knowledgeWorkingNotes(request: Contract.WorkingNotesRequest): Promise<Contract.WorkingNotesPage> {
    const notes = this.workingNotes.get(`${request.loupistId}:${request.chatId}`) ?? [];
    return { current: notes[0] ? structuredClone(notes[0]) : null };
  }

  knowledgeGrowthEvents(request: Contract.GrowthEventsQuery) {
    return knowledge.knowledgeGrowthEvents(this.state, request);
  }

  knowledgeLensSearch(request: { loupistId: string; query: string; kinds?: import("../contracts/lens").LensKind[] }) {
    return knowledge.knowledgeLensSearch(this.state, request);
  }

  knowledgeLensPagesForNode(request: { loupistId: string; nodeId: string }) {
    return knowledge.knowledgeLensPagesForNode(this.state, request);
  }

  memorySectionForget(request: { loupistId: string; recordId: string; key: string }) {
    return knowledge.memorySectionForget(this.state, request);
  }

  memoryOpUndo(request: { loupistId: string; opId: string }) {
    return knowledge.memoryOpUndo(this.state, request);
  }

  memoryLearned(request: { loupistId: string; messageIds: string[] }) {
    return knowledge.memoryLearned(this.state, request);
  }

  knowledgeResearchSearch(request: Contract.KnowledgeResearchSearchRequest) {
    return knowledge.knowledgeResearchSearch(this.state, request);
  }

  knowledgeSourceRetain(request: Contract.KnowledgeSourceRetainRequest) {
    return knowledge.knowledgeSourceRetain(this.state, request);
  }

  knowledgeGraphJob(request: { loupistId: string; jobId: string }) {
    return knowledge.knowledgeGraphJob(this.state, request);
  }

  /** The mock Map is static, so a followed list counts as read once World processing can run. */
  async knowledgeFollowedStatus(request: { loupistId: string }): Promise<Contract.FollowedStatus> {
    const [watchlist, view] = await Promise.all([this.watchlistList(), this.knowledgeGraphSettingsGet(request)]);
    if (!watchlist.length) return { state: "none", assets: 0 };
    const state = view.readiness.status === "ready" ? "done" : view.readiness.status === "disabled" || view.readiness.status === "paused" ? "off" : "waiting";
    return { state, assets: watchlist.length };
  }



  async knowledgeGraphRecover(request: { loupistId: string }) {
    return knowledge.graphProcessingView(await knowledge.knowledgeGraphRecover(this.state, request), await this.modelCatalogGet(), await knowledge.knowledgeGraphJobs(this.state, request));
  }

  async knowledgeGraphPause(request: { loupistId: string; paused: boolean }) {
    return knowledge.graphProcessingView(await knowledge.knowledgeGraphPause(this.state, request), await this.modelCatalogGet(), await knowledge.knowledgeGraphJobs(this.state, request));
  }

  async knowledgeGraphSettingsGet(request: { loupistId: string }) {
    return knowledge.graphProcessingView(await knowledge.knowledgeGraphSettingsGet(this.state, request), await this.modelCatalogGet(), await knowledge.knowledgeGraphJobs(this.state, request));
  }

  async knowledgeGraphSettingsSet(request: {
    loupistId: string;
    settings: Contract.GraphProcessingSettings;
  }) {
    return knowledge.graphProcessingView(await knowledge.knowledgeGraphSettingsSet(this.state, request), await this.modelCatalogGet(), await knowledge.knowledgeGraphJobs(this.state, request));
  }

  worldKeepAwakeGet() {
    return knowledge.worldKeepAwakeGet(this.state);
  }

  worldKeepAwakeSet(request: { enabled: boolean }) {
    return knowledge.worldKeepAwakeSet(this.state, request);
  }

  journalList(request: Contract.JournalListRequest) {
    return knowledge.journalList(this.state, request);
  }
  journalCreate(request: Contract.JournalCreateRequest) {
    return knowledge.journalCreate(this.state, request);
  }
  journalUpdate(request: Contract.JournalUpdateRequest) {
    return knowledge.journalUpdate(this.state, request);
  }
  journalArchive(request: Contract.JournalArchiveRequest) {
    return knowledge.journalArchive(this.state, request);
  }
  knowledgeJournalRead(request: Contract.KnowledgeJournalReadRequest) {
    return knowledge.knowledgeJournalRead(this.state, request);
  }

  knowledgeJournalSourceRead(request: Contract.KnowledgeJournalSourceReadRequest) {
    return knowledge.knowledgeJournalSourceRead(this.state, request);
  }

  private async connectorIsReady(entryId: string) {
    const snapshot = await this.marketplaceSnapshot();
    return (
      snapshot.connectors.find((connector) => connector.entry_id === entryId)
        ?.readiness === "ready"
    );
  }

  private enableReadyConnectorForAllLoupists(entryId: string) {
    for (const loupist of this.state.loupists) {
      this.disabledCapabilities.get(loupist.id)?.delete(entryId);
      const enabled =
        this.enabledCapabilities.get(loupist.id) ?? new Set<string>();
      enabled.add(entryId);
      this.enabledCapabilities.set(loupist.id, enabled);
    }
  }

}
