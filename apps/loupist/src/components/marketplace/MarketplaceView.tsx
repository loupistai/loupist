import { useExtensionAvailability } from "../../app/extensionAvailability";
import { CustomConnections } from "../connections/custom/CustomConnections";
import { connectionText } from "@/lib/connectionCopy";
import { keychainErrorMessage } from "@/lib/errors";
import { formatNumber } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import { PageHeader } from "../ui/page-header";
import { DatabaseIcon, RefreshCwIcon, SearchIcon, SlidersHorizontalIcon, XIcon } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { SegmentedControl, SegmentedControlItem } from "../ui/segmented-control";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type {
  MarketplaceEntry,
  MarketplaceFreeState,
  MarketplaceSnapshot,
} from "../../marketplace/types";
import type { LoupistSummary, LoupistBridge } from "../../types";
import { MarketplaceCard, accessLabel } from "./MarketplaceCard";
import { MarketplaceSetupDialog } from "./MarketplaceSetupDialog";
import { CONFIG_CHANGED_EVENT, notifyConfigurationChanged } from "../../app/useConfiguration";
import { connectionGroups } from "../../marketplace/groups";
import { CONNECTION_STATES, connectionState, needsSavedKeyPermission, sortConnectionEntries } from "../../marketplace/readiness";

type Props = {
  bridge: LoupistBridge;
  loupists?: LoupistSummary[];
  currentLoupistId?: string | null;
  permissionNoticeRequest?: number;
  initialEntryId?: string | null;
  onManageLoupists?: () => void;
  onOpenModelSettings?: () => void;
};

type FreeFilter = "all" | MarketplaceFreeState;
type StatusFilter = "all" | "ready" | "attention";

const FREE_FILTERS: Array<{ id: FreeFilter; label: string }> = [
  { id: "all", get label() { return i18n.t("connections:marketplaceView.allAccess"); } },
  { id: "keyless", get label() { return i18n.t("connections:marketplaceView.noKey"); } },
  { id: "free_account", get label() { return i18n.t("connections:marketplaceView.freeAccount"); } },
  { id: "paid", get label() { return i18n.t("connections:marketplaceView.paid"); } },
];

/** Search matches what a card shows, so every result can say why it matched. */
function entrySearchText(entry: MarketplaceEntry) {
  return [
    connectionText(entry.name),
    connectionText(entry.summary),
    accessLabel(entry),
    ...entry.markets.map(connectionText),
  ]
    .join(" ")
    .toLowerCase();
}

/**
 * A plugin's group label in the interface language: the known group names are translated by
 * `connectionGroups`; another (Asset lookup) is the English name of its one entry, which the
 * catalog translates.
 */
const groupLabel = (group: { label: string }) => connectionText(group.label);

/** A group of one entry named like its group needs no heading of its own. */
const showGroupHeading = (group: { label: string; entries: MarketplaceEntry[] }) =>
  group.entries.length > 1 || connectionText(group.entries[0]?.name ?? "") !== groupLabel(group);

export function MarketplaceView({ bridge, loupists = [], currentLoupistId, initialEntryId, permissionNoticeRequest, onManageLoupists, onOpenModelSettings }: Props) {
  const { customConnections } = useExtensionAvailability();
  const { i18n: activeLanguage } = useTranslation("connections");
  const batchId = useRef<string | null>(null);
  const snapshotRevision = useRef(0);
  const [permissionProgress, setPermissionProgress] = useState<{ settled: number; total: number } | null>(null);
  const [cancellingPermission, setCancellingPermission] = useState(false);
  const [permissionError, setPermissionError] = useState<string | null>(null);
  const [snapshot, setSnapshot] = useState<MarketplaceSnapshot | null>(null);
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [query, setQuery] = useState("");
  const [freeFilter, setFreeFilter] = useState<FreeFilter>("all");
  const [error, setError] = useState<string | null>(null);
  const [reloadToken, setReloadToken] = useState(0);
  const [selectedEntryId, setSelectedEntryId] = useState<string | null>(initialEntryId ?? null);
  /** Which connections the current Loupist has on, by entry id; null until read or when it cannot be. */
  const [enabledHere, setEnabledHere] = useState<ReadonlyMap<string, boolean> | null>(null);
  const currentLoupist = loupists.find((loupist) => loupist.id === currentLoupistId) ?? null;

  const readEnabledHere = useCallback(async () => {
    if (!currentLoupistId) return null;
    // A page that sent the user here ("off for this Loupist") is answered on the card; without the read, cards say nothing.
    return bridge.loupistCapabilityList(currentLoupistId)
      .then((bindings) => new Map(bindings.map((binding) => [binding.entry_id, binding.enabled])))
      .catch(() => null);
  }, [bridge, currentLoupistId]);

  useEffect(() => () => {
    const id = batchId.current;
    batchId.current = null;
    if (id) void bridge.marketplaceCredentialsCancelAccess(id).catch(() => undefined);
  }, [bridge]);

  useEffect(() => {
    let active = true;
    const refresh = () => {
      const requested = ++snapshotRevision.current;
      void bridge.marketplaceSnapshot().then((next) => {
        if (active && snapshotRevision.current === requested) { setSnapshot(next); setError(null); }
      }).catch(() => {
        if (active && snapshotRevision.current === requested) setError(i18n.t("connections:marketplaceView.couldNotLoadConnections"));
      });
      void readEnabledHere().then((next) => { if (active && snapshotRevision.current === requested) setEnabledHere(next); });
    };
    refresh();
    window.addEventListener("focus", refresh);
    window.addEventListener(CONFIG_CHANGED_EVENT, refresh);
    return () => {
      active = false;
      window.removeEventListener("focus", refresh);
      window.removeEventListener(CONFIG_CHANGED_EVENT, refresh);
    };
  }, [bridge, reloadToken, readEnabledHere]);

  useEffect(() => {
    if (reloadToken > 0) setSelectedEntryId(null);
  }, [reloadToken]);

  const connectorById = useMemo(
    () =>
      new Map(
        snapshot?.connectors.map((connector) => [
          connector.entry_id,
          connector,
        ]) ?? [],
      ),
    [snapshot],
  );
  const permissionNotice = useRef<HTMLDivElement>(null);
  const permissionCount = (snapshot?.connectors ?? []).filter(needsSavedKeyPermission).length;
  const showPermissionNotice = permissionCount > 0;
  useEffect(() => {
    if (permissionNoticeRequest && showPermissionNotice) permissionNotice.current?.focus();
  }, [permissionNoticeRequest, showPermissionNotice]);
  const browseableEntryCount = useMemo(
    () =>
      (snapshot?.catalog.entries ?? []).length,
    [snapshot],
  );
  const attentionCount = useMemo(
    () => (snapshot?.catalog.entries ?? []).filter((entry) => CONNECTION_STATES[connectionState(connectorById.get(entry.id))].needsAttention).length,
    [snapshot, connectorById],
  );
  const visibleEntries = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase();
    const matching = (snapshot?.catalog.entries ?? []).filter((entry) => {
      const state = connectionState(connectorById.get(entry.id));
      return (freeFilter === "all" || entry.free_state === freeFilter) &&
        (statusFilter === "all" || (statusFilter === "ready" ? state === "ready" : CONNECTION_STATES[state].needsAttention)) &&
        (!normalizedQuery || entrySearchText(entry).includes(normalizedQuery));
    });
    return sortConnectionEntries(matching, connectorById, (entry) => connectionText(entry.name));
  // eslint-disable-next-line react-hooks/exhaustive-deps -- the interface language is the recompute trigger; `i18n` itself is not reactive.
  }, [freeFilter, query, snapshot, statusFilter, connectorById, activeLanguage.language]);
  const discoveryGroups = useMemo(
    () => connectionGroups(snapshot?.plugins ?? [], visibleEntries),
    [snapshot, visibleEntries],
  );
  const selectedEntry = selectedEntryId
    ? (snapshot?.catalog.entries.find(
        (entry) => entry.id === selectedEntryId,
      ) ?? null)
    : null;

  async function refreshMarketplace() {
    const requested = ++snapshotRevision.current;
    const [next, here] = await Promise.all([bridge.marketplaceSnapshot(), readEnabledHere()]);
    if (snapshotRevision.current === requested) {
      setSnapshot(next);
      setEnabledHere(here);
    }
  }

  /** Turns a ready connection on for the current Loupist, from its dialog. */
  async function enableHere(entryId: string) {
    if (!currentLoupistId) return;
    await bridge.loupistCapabilityEnable({ loupist_id: currentLoupistId, entry_id: entryId });
    setEnabledHere(await readEnabledHere());
  }

  /** Rechecks the saved-key state and refreshes the row. */
  async function unlockEntry(entry: MarketplaceEntry, presenceOnly: boolean) {
    const request = { entry_id: entry.id };
    if (presenceOnly) await bridge.marketplaceCredentialUnlock(request, true);
    else await bridge.marketplaceCredentialUnlock(request);
    await refreshMarketplace();
    notifyConfigurationChanged();
  }

  async function allowSavedKeys() {
    if (batchId.current || !permissionCount) return;
    const id = crypto.randomUUID();
    batchId.current = id;
    setPermissionError(null);
    setPermissionProgress({ settled: 0, total: permissionCount });
    const couldNotCheck = i18n.t("connections:savedKeyPermission.error");
    try {
      await bridge.marketplaceCredentialsAllowAccess(id, (progress) => {
        if (batchId.current !== id) return;
        setPermissionProgress(progress);
        if (progress.failed_entry_id) setPermissionError(couldNotCheck);
        void refreshMarketplace().catch(() => {
          if (batchId.current === id) setPermissionError(couldNotCheck);
        });
      });
      if (batchId.current === id) await refreshMarketplace();
    } catch (cause) {
      if (batchId.current === id) setPermissionError(keychainErrorMessage(cause) ?? couldNotCheck);
    } finally {
      if (batchId.current === id) {
        batchId.current = null;
        setPermissionProgress(null);
        setCancellingPermission(false);
        notifyConfigurationChanged();
      }
    }
  }

  async function cancelSavedKeys() {
    if (!batchId.current) return;
    setCancellingPermission(true);
    try {
      await bridge.marketplaceCredentialsCancelAccess(batchId.current);
    } catch {
      setCancellingPermission(false);
      setPermissionError(i18n.t("connections:savedKeyPermission.error"));
    }
  }

  return (
    <section className="marketplace-page" aria-labelledby="marketplace-title">
      <div className="marketplace-shell">
        <PageHeader className="settings-heading" titleId="marketplace-title" title={i18n.t("connections:marketplaceView.connections")} hideTitle
          description={i18n.t("connections:marketplaceView.newConnectionsAreEnabledForAvailableLoupistsAdjust")}
          actions={onManageLoupists ? <Button type="button" variant="outline" onClick={onManageLoupists}><SlidersHorizontalIcon />{i18n.t("connections:marketplaceView.manageLoupistAccess")}</Button> : undefined}
 />

        {error ? (
          <div className="marketplace-error" role="alert">
            <DatabaseIcon aria-hidden="true" />
            <div>
              <strong>{snapshot ? i18n.t("connections:marketplaceView.couldNotRefreshConnections") : i18n.t("connections:marketplaceView.connectionsAreUnavailable")}</strong>
              <p>{error}</p>
            </div>
            <Button
              type="button"
              variant="outline"
              onClick={() => setReloadToken((current) => current + 1)}
            >
              <RefreshCwIcon /> {i18n.t("connections:marketplaceView.retry")}</Button>
          </div>
        ) : null}
        {permissionCount || permissionProgress ? (
          <div className="marketplace-permission" ref={permissionNotice} tabIndex={-1}>
            <DatabaseIcon aria-hidden="true" />
            <div>
              <p>{i18n.t("connections:savedKeyPermission.notice", { count: permissionCount || permissionProgress?.total })}</p>
              {permissionProgress ? <p role="status">{i18n.t(cancellingPermission
                ? "connections:savedKeyPermission.closePrompt"
                : "connections:savedKeyPermission.progress", permissionProgress)}</p> : null}
              {permissionError ? <p role="alert">{permissionError}</p> : null}
            </div>
            {permissionProgress
              ? <Button variant="outline" disabled={cancellingPermission} onClick={() => void cancelSavedKeys()}>{i18n.t("connections:savedKeyPermission.cancel")}</Button>
              : <Button variant="outline" disabled={Boolean(selectedEntry)} onClick={() => void allowSavedKeys()}>{i18n.t("connections:savedKeyPermission.allow")}</Button>}
          </div>
        ) : null}
        <section className="connection-catalog" aria-labelledby="available-connections-title">
          <h2 className="connection-section-heading" id="available-connections-title">{i18n.t("connections:marketplaceView.availableConnections")}</h2>
        <div className="marketplace-browser">
          <SegmentedControl value={statusFilter} onValueChange={(value) => setStatusFilter(value as StatusFilter)} className="marketplace-status-tabs" aria-label={i18n.t("connections:marketplaceView.connectionReadiness")}>
              <SegmentedControlItem value="all">{i18n.t("connections:marketplaceView.allConnections")}</SegmentedControlItem>
              <SegmentedControlItem value="ready">{i18n.t("connections:marketplaceView.readyToUse")}</SegmentedControlItem>
              <SegmentedControlItem value="attention">{i18n.t("connections:marketplaceView.needsAttention")}{attentionCount ? <span className="marketplace-filter-count"> {formatNumber(attentionCount)}</span> : null}</SegmentedControlItem>
            </SegmentedControl>
          <div className="marketplace-controls">
            <label className="marketplace-search">
              <SearchIcon aria-hidden="true" />
              <Input
                type="search"
                value={query}
                aria-label={i18n.t("connections:marketplaceView.searchConnections")}
                placeholder={i18n.t("connections:marketplaceView.searchConnections")}
                onChange={(event) => setQuery(event.target.value)}
              />
              {query ? <Button variant="ghost" size="icon-sm" aria-label={i18n.t("connections:marketplaceView.clearSearch")} onClick={() => setQuery("")}><XIcon /></Button> : null}
            </label>
            <Select
              value={freeFilter}
              onValueChange={(value) =>
                setFreeFilter(value as FreeFilter)
              }
            >
              <SelectTrigger
                aria-label={i18n.t("connections:marketplaceView.filterConnectionsByAccess")}
                className="marketplace-filter-select"
              >
                <SelectValue />
              </SelectTrigger>
              <SelectContent position="popper" align="end">
                {FREE_FILTERS.map((filter) => (
                  <SelectItem value={filter.id} key={filter.id}>
                    {filter.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {snapshot && visibleEntries.length !== browseableEntryCount ? <p className="marketplace-result-count">{i18n.t("connections:marketplaceView.resultCount", { visible: formatNumber(visibleEntries.length), total: formatNumber(browseableEntryCount) })}</p> : null}
          {!snapshot && error ? null : !snapshot ? (
            <div
              className="marketplace-loading"
              role="status"
              aria-label={i18n.t("connections:marketplaceView.loadingConnections")}
            >
              <span /> <span /> <span />
            </div>
          ) : discoveryGroups.length ? (
            <div className="marketplace-groups">
              {discoveryGroups.map((group) => (
                <section className="marketplace-group" key={group.id}>
                  {showGroupHeading(group) ? (
                    <h3>{groupLabel(group)}<span aria-hidden="true">{formatNumber(group.entries.length)}</span></h3>
                  ) : null}
                  <div className="marketplace-grid">
                    {group.entries.map((entry) => (
                      <MarketplaceCard
                        key={entry.id}
                        entry={entry}
                        connector={connectorById.get(entry.id)}
                        offFor={currentLoupist && enabledHere?.get(entry.id) === false ? currentLoupist.name : undefined}
                        headingLevel={showGroupHeading(group) ? 4 : 3}
                        onSetup={(next) => setSelectedEntryId(next.id)}
                        onUnlock={unlockEntry}
                        permissionBatchActive={Boolean(permissionProgress)}
                      />
                    ))}
                  </div>
                </section>
              ))}
            </div>
          ) : (
            <div className="marketplace-empty">
              <h3>{i18n.t("connections:marketplaceView.noMatchingConnections")}</h3>
              <p>{i18n.t("connections:marketplaceView.tryAnotherSearchOrChooseAllAccess")}</p>
              <Button type="button" variant="outline" onClick={() => { setQuery(""); setFreeFilter("all"); setStatusFilter("all"); }}>{i18n.t("connections:marketplaceView.clearFilters")}</Button>
            </div>
          )}
        </div>
        </section>

        {customConnections ? <CustomConnections bridge={bridge} loupists={loupists} initialLoupistId={currentLoupistId} /> : null}
      </div>

      <MarketplaceSetupDialog
        bridge={bridge}
        entry={selectedEntry}
        connector={
          selectedEntryId ? connectorById.get(selectedEntryId) : undefined
        }
        here={currentLoupist && selectedEntryId && enabledHere?.has(selectedEntryId) ? {
          loupist: currentLoupist.name,
          enabled: enabledHere.get(selectedEntryId) === true,
          enable: () => enableHere(selectedEntryId),
        } : undefined}
        onClose={() => setSelectedEntryId(null)}
        onChanged={refreshMarketplace}
        onManageLoupists={onManageLoupists}
        onOpenModelSettings={onOpenModelSettings}
      />
    </section>
  );
}
