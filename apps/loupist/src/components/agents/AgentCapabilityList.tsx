import { formatNumber } from "../../i18n/format";
import i18n, { useTranslation } from "../../i18n";
import { Fragment, useId, type ReactNode } from "react";
import { ChevronRightIcon, LoaderCircleIcon } from "lucide-react";
import { Switch } from "@/components/ui/switch";
import { Button } from "../ui/button";

export type AgentCapabilityItem = {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  locked?: boolean;
  note?: string;
  group?: string;
};

type Props = {
  /** Deep-link target: `data-loupist-section` on the list. */
  section?: string;
  title: string;
  description: string;
  action?: ReactNode;
  items: AgentCapabilityItem[];
  busyId: string | null;
  emptyLabel: string;
  onToggle: (id: string) => void;
  onSetup?: (id: string) => void;
  error?: { id: string; message: string };
};

export function AgentCapabilityList({
  section,
  title,
  description,
  action,
  items,
  busyId,
  emptyLabel,
  onToggle,
  onSetup,
  error,
}: Props) {
  useTranslation("settings");
  const headingId = useId();
  const switchPrefix = useId();

  return (
    <section className="agent-section" aria-labelledby={headingId} data-loupist-section={section}>
      <header className="agent-section-heading">
        <div>
          <h2 id={headingId}>{title}<span className="agent-section-count" aria-hidden="true">{formatNumber(items.length)}</span></h2>
          <p>{description}</p>
        </div>
        {action}
      </header>

      {items.length ? (
        <div className="agent-capability-list">
          {items.map((item, index) => {
            const switchId = `${switchPrefix}-${item.id}`;
            const showGroup = Boolean(item.group) && item.group !== items[index - 1]?.group;
            return (
              <Fragment key={item.id}>
              {showGroup ? <h3 className="agent-capability-group">{item.group}</h3> : null}
              <div className="agent-capability" data-locked={item.locked || undefined} aria-busy={busyId === item.id}>
                <label htmlFor={switchId}>
                  <strong>{item.name}</strong>
                  <span>{item.description}</span>
                  {item.note ? <small>{item.note}</small> : null}
                  {error?.id === item.id ? <small className="agent-capability-error" role="alert">{error.message}</small> : null}
                </label>
                <div className="agent-capability-action">
                  {busyId === item.id ? <LoaderCircleIcon className="agents-loading-icon" aria-label={i18n.t("settings:agentCapabilityList.savingName", { name: item.name })} role="status" /> : null}
                  <Switch
                  id={switchId}
                  checked={item.enabled}
                  disabled={item.locked || busyId !== null}
                  aria-label={item.enabled ? i18n.t("settings:agentCapabilityList.enabledLabel", { name: item.name }) : i18n.t("settings:agentCapabilityList.disabledLabel", { name: item.name })}
                  onCheckedChange={() => onToggle(item.id)}
                />
                  {item.locked && onSetup ? <Button type="button" size="sm" variant="ghost" aria-label={i18n.t("settings:agentCapabilityList.connectionSettingsForName", { name: item.name })} onClick={() => onSetup(item.id)}>{i18n.t("settings:agentCapabilityList.details")}<ChevronRightIcon /></Button> : null}
                </div>
              </div>
              </Fragment>
            );
          })}
        </div>
      ) : (
        <p className="agent-capability-empty">{emptyLabel}</p>
      )}
    </section>
  );
}
