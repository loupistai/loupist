import { CustomConnections } from "../connections/custom/CustomConnections";
import { connectionText } from "./connectionCopy";
import { formatNumber } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import { PageHeader } from "../ui/page-header";
import { DatabaseIcon, RefreshCwIcon, SearchIcon, SlidersHorizontalIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "../ui/tabs";
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
import { MarketplaceCard } from "./MarketplaceCard";
import { MarketplaceSetupDialog } from "./MarketplaceSetupDialog";
import { CONFIG_CHANGED_EVENT } from "../../app/useConfiguration";
import { connectionGroups } from "../../marketplace/groups";

type Props = {
  bridge: LoupistBridge;
  loupists?: LoupistSummary[];
  currentLoupistId?: string | null;
  initialEntryId?: string | null;
  onManageLoupists?: () => void;
  onOpenModelSettings?: () => void;
};

type FreeFilter = "all" | MarketplaceFreeState;

const FREE_FILTERS: Array<{ id: FreeFilter; label: string }> = [
  { id: "all", get label() { return i18n.t("connections:marketplaceView.allAccess"); } },
  { id: "keyless", get label() { return i18n.t("connections:marketplaceView.noKey"); } },
  { id: "free_account", get label() { return i18n.t("connections:marketplaceView.freeAccount"); } },
  { id: "paid", get label() { return i18n.t("connections:marketplaceView.paid"); } },
];

function entrySearchText(entry: MarketplaceEntry) {
  return [
    connectionText(entry.name),
    connectionText(entry.summary),
    entry.publisher,
    connectionText(entry.data_authority),
    entry.plugin,
    ...entry.runtime.provider_ids,
    ...entry.markets,
    ...entry.asset_classes,
    ...entry.capabilities,
  ]
    .join(" ")
    .toLowerCase();
}

export function MarketplaceView({ bridge, loupists = [], currentLoupistId, initialEntryId, onManageLoupists, onOpenModelSettings }: Props) {
  const { i18n: activeLanguage } = useTranslation("connections");
  const [snapshot, setSnapshot] = useState<MarketplaceSnapshot | null>(null);
  const [statusFilter, setStatusFilter] = useState("all");
  const [query, setQuery] = useState("");
  const [freeFilter, setFreeFilter] = useState<FreeFilter>("all");
  const [error, setError] = useState<string | null>(null);
  const [reloadToken, setReloadToken] = useState(0);
  const [selectedEntryId, setSelectedEntryId] = useState<string | null>(initialEntryId ?? null);

  useEffect(() => {
    let active = true;
    let revision = 0;
    const refresh = () => {
      const requested = ++revision;
      void bridge.marketplaceSnapshot().then((next) => {
        if (active && revision === requested) { setSnapshot(next); setError(null); }
      }).catch(() => {
        if (active && revision === requested) setError(i18n.t("connections:marketplaceView.couldNotLoadConnections"));
      });
    };
    refresh();
    window.addEventListener("focus", refresh);
    window.addEventListener(CONFIG_CHANGED_EVENT, refresh);
    return () => {
      active = false;
      window.removeEventListener("focus", refresh);
      window.removeEventListener(CONFIG_CHANGED_EVENT, refresh);
    };
  }, [bridge, reloadToken]);

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
  const browseableEntryCount = useMemo(
    () =>
      (snapshot?.catalog.entries ?? []).length,
    [snapshot],
  );
  const visibleEntries = useMemo(() => {
    const normalizedQuery = query.trim().toLowerCase();
    return [...(snapshot?.catalog.entries ?? [])].filter(
      (entry) =>
        (freeFilter === "all" || entry.free_state === freeFilter) &&
        (statusFilter === "all" || (statusFilter === "ready" ? connectorById.get(entry.id)?.readiness === "ready" : connectorById.get(entry.id)?.readiness !== "ready")) &&
        (!normalizedQuery || entrySearchText(entry).includes(normalizedQuery)),
    );
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
    const next = await bridge.marketplaceSnapshot();
    setSnapshot(next);
  }

  return (
    <section className="marketplace-page" aria-labelledby="marketplace-title">
      <div className="marketplace-shell">
        <PageHeader className="settings-heading" titleId="marketplace-title" title={i18n.t("connections:marketplaceView.connections")}
          description={i18n.t("connections:marketplaceView.newConnectionsAreEnabledForAvailableLoupistsAdjust")}
          actions={onManageLoupists ? <Button type="button" variant="outline" onClick={onManageLoupists}><SlidersHorizontalIcon />{i18n.t("connections:marketplaceView.manageLoupistAccess")}</Button> : undefined}
 />

        <CustomConnections bridge={bridge} loupists={loupists} initialLoupistId={currentLoupistId} />

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
        <section className="connection-catalog" aria-labelledby="available-connections-title">
          <h2 className="connection-section-heading" id="available-connections-title">{i18n.t("connections:marketplaceView.availableConnections")}</h2>
        <div className="marketplace-browser">
          <Tabs value={statusFilter} onValueChange={setStatusFilter} className="marketplace-status-tabs">
            <TabsList aria-label={i18n.t("connections:marketplaceView.connectionReadiness")}>
              <TabsTrigger value="all">{i18n.t("connections:marketplaceView.allConnections")}</TabsTrigger>
              <TabsTrigger value="ready">{i18n.t("connections:marketplaceView.readyToUse")}</TabsTrigger>
              <TabsTrigger value="setup">{i18n.t("connections:marketplaceView.needsAttention")}</TabsTrigger>
            </TabsList>
          </Tabs>
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

          {snapshot ? <p className="marketplace-result-count">{i18n.t("connections:marketplaceView.resultCount", { visible: formatNumber(visibleEntries.length), total: formatNumber(browseableEntryCount) })}</p> : null}
          {!snapshot && error ? null : !snapshot ? (
            <div
              className="marketplace-loading"
              aria-label={i18n.t("connections:marketplaceView.loadingConnections")}
            >
              <span /> <span /> <span />
            </div>
          ) : discoveryGroups.length ? (
            <div className="marketplace-groups">
              {discoveryGroups.map((group) => (
                <section className="marketplace-group" key={group.id}>
                  {group.entries.length > 1 ||
                  group.entries[0]?.name !== group.label ? (
                    <h2>{connectionText(group.label)}<span aria-hidden="true">{formatNumber(group.entries.length)}</span></h2>
                  ) : null}
                  <div className="marketplace-grid">
                    {group.entries.map((entry) => (
                      <MarketplaceCard
                        key={entry.id}
                        entry={entry}
                        connector={connectorById.get(entry.id)}
                        onSetup={(next) => setSelectedEntryId(next.id)}
                      />
                    ))}
                  </div>
                </section>
              ))}
            </div>
          ) : (
            <div className="marketplace-empty">
              <h2>{i18n.t("connections:marketplaceView.noMatchingConnections")}</h2>
              <p>{i18n.t("connections:marketplaceView.tryAnotherSearchOrChooseAllAccess")}</p>
              <Button type="button" variant="outline" onClick={() => { setQuery(""); setFreeFilter("all"); setStatusFilter("all"); }}>{i18n.t("connections:marketplaceView.clearFilters")}</Button>
            </div>
          )}
        </div>
        </section>
      </div>

      <MarketplaceSetupDialog
        bridge={bridge}
        entry={selectedEntry}
        connector={
          selectedEntryId ? connectorById.get(selectedEntryId) : undefined
        }
        onClose={() => setSelectedEntryId(null)}
        onChanged={refreshMarketplace}
        onManageLoupists={onManageLoupists}
        onOpenModelSettings={onOpenModelSettings}
      />
    </section>
  );
}
