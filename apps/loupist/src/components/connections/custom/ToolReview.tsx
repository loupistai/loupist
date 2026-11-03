import { useState } from "react";
import { useTranslation } from "../../../i18n";
import type { CustomConnection, CustomConnectionsBridge, CustomToolCard } from "../../../contracts/customConnections";
import { Button } from "../../ui/button";
import { humanizeToolName } from "./toolTitle";

function Tool({ tool, busy, onChoice }: { tool: CustomToolCard; busy: boolean; onChoice: (choice: "always" | "deny") => void }) {
  const { t } = useTranslation("settings");
  const [expanded, setExpanded] = useState(false);
  const title = humanizeToolName(tool.name);
  return <li className="custom-tool" data-custom-tool={tool.name}>
    <span className="custom-tool-title">{title}</span>
    <span className="custom-tool-id">{tool.name}</span>
    <span className="custom-meta">{t("custom.serverDescription")}</span>
    <p className={expanded ? "" : "custom-description-clamp"}>{tool.server_description || t("custom.noDescription")}</p>
    {tool.server_description ? <Button variant="link" size="sm" className="flush-start" aria-expanded={expanded} onClick={() => setExpanded(!expanded)}>{t(expanded ? "custom.showLess" : "custom.showMore")}</Button> : null}
    <p className={`custom-status ${tool.eligibility === "read_only" ? "positive" : tool.eligibility === "financial_action" ? "negative" : "muted"}`}>{t(`custom.eligibility.${tool.eligibility}`)}</p>
    {tool.definition_changed && tool.eligibility === "read_only" ? <p className="custom-status warning">{t("custom.changed")}</p> : null}
    {tool.eligibility === "read_only" ? <div role="group" aria-label={t("custom.approvalFor", { tool: title })} className="custom-segments">
      <Button variant={!tool.definition_changed && tool.approval === "always" ? "secondary" : "outline"} aria-pressed={!tool.definition_changed && tool.approval === "always"} disabled={busy} onClick={() => onChoice("always")}>{t("custom.allow")}</Button>
      <Button variant={!tool.definition_changed && tool.approval === "deny" ? "secondary" : "outline"} aria-pressed={!tool.definition_changed && tool.approval === "deny"} disabled={busy} onClick={() => onChoice("deny")}>{t("custom.deny")}</Button>
    </div> : null}
  </li>;
}

export function ToolReview({ bridge, connection, loupistId, onChanged, busy = false, onError }: {
  bridge: CustomConnectionsBridge; connection: CustomConnection; loupistId: string; onChanged: (record: CustomConnection) => void; busy?: boolean; onError: () => void;
}) {
  const [saving, setSaving] = useState(false);
  const { t } = useTranslation("settings");
  async function choose(tool: CustomToolCard, choice: "always" | "deny") {
    setSaving(true);
    try { onChanged(await bridge.customToolApprove({ loupist_id: loupistId, connection_id: connection.id, tool_name: tool.name, definition_hash: tool.definition_hash, choice })); }
    catch { onError(); }
    finally { setSaving(false); }
  }
  return <ul className="custom-tools" aria-label={t("custom.tools")}>{connection.tools.map(tool => <Tool key={`${connection.id}:${loupistId}:${tool.name}:${tool.definition_hash}`} tool={tool} busy={busy || saving || !loupistId} onChoice={choice => void choose(tool, choice)} />)}</ul>;
}

export function reviewComplete(connection: CustomConnection) {
  return connection.tools.every(tool => tool.eligibility !== "read_only" || (!tool.definition_changed && (tool.approval === "always" || tool.approval === "deny")));
}
