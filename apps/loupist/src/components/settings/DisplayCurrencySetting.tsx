import { useId } from "react";
import { useTranslation } from "../../i18n";
import { formatDay } from "../../i18n/format";
import type { DisplayFx } from "../../app/useDisplayFx";
import { Button } from "../ui/button";
import { SelectField, SelectItem } from "../ui/select";

/**
 * The euro and the 29 currencies of the ECB reference table (currency design §1.5),
 * in the table's order. A static list: the renderer never fetches or infers it.
 */
export const DISPLAY_CURRENCIES = [
  "EUR", "USD", "JPY", "CZK", "DKK", "GBP", "HUF", "PLN", "RON", "SEK", "CHF", "ISK", "NOK", "TRY", "AUD",
  "BRL", "CAD", "CNY", "HKD", "IDR", "ILS", "INR", "KRW", "MXN", "MYR", "NZD", "PHP", "SGD", "THB", "ZAR",
] as const;

/** A currency's name in the interface language ("South Korean won"); the code where none is known. */
export function displayCurrencyName(code: string, language: string) {
  try {
    return new Intl.DisplayNames([language === "en-XA" ? "en" : language || "en"], { type: "currency" }).of(code) ?? code;
  } catch {
    return code;
  }
}

/** The select's value for As listed: the setting is removed, not stored. */
const AS_LISTED = "";

type Props = {
  /** The configured code, which may be a well-shaped code outside the table. */
  value?: string;
  onChange?: (code: string | null) => void;
  /** The shell's shared query. */
  fx: DisplayFx & { retry?: () => void };
  /** Opens this Loupist's Connections; absent without an active Loupist. */
  onConnect?: () => void;
  disabled?: boolean;
};

export function DisplayCurrencySetting({ value, onChange, fx, onConnect, disabled }: Props) {
  const { t, i18n } = useTranslation("settings");
  const id = useId();
  const selected = value ?? AS_LISTED;
  const unsupported = value && !(DISPLAY_CURRENCIES as readonly string[]).includes(value) ? value : undefined;
  const option = (code: string) => {
    const name = displayCurrencyName(code, i18n.language);
    return name === code ? code : t("appearance.displayCurrency.option", { code, name });
  };
  return (
    <>
      <h3 className="text-sm font-semibold"><label htmlFor={`${id}-select`}>{t("appearance.displayCurrency.label")}</label></h3>
      <div className="display-currency-setting" data-display-currency-setting>
        <SelectField id={`${id}-select`} data-display-currency-select aria-describedby={`${id}-status`} value={selected} placeholder={t("appearance.displayCurrency.asListed")} disabled={disabled || !onChange}
          onValueChange={value => onChange?.(value === AS_LISTED ? null : value)}>
          <SelectItem value={AS_LISTED}>{t("appearance.displayCurrency.asListed")}</SelectItem>
          {unsupported ? <SelectItem value={unsupported}>{t("appearance.displayCurrency.unsupported", { code: unsupported })}</SelectItem> : null}
          {DISPLAY_CURRENCIES.map((code) => <SelectItem key={code} value={code}>{option(code)}</SelectItem>)}
        </SelectField>
        <p id={`${id}-status`} role="status" className="display-currency-status text-sm text-muted-foreground" data-fx-status={statusId(fx)}>
          <FxStatus fx={fx} onConnect={onConnect} />
        </p>
      </div>
    </>
  );
}

/** Which status sentence is showing, for tests and preview captures. */
function statusId(fx: DisplayFx) {
  if (fx.status !== "read") return fx.status;
  const display = fx.display;
  if (display.state === "ready") return display.behind ? `behind-${display.behind}` : "ready";
  return display.state === "unavailable" ? display.reason : display.state;
}

/** One sentence per state, as Rust decided it; nothing while no Loupist is open or the first read runs. */
function FxStatus({ fx, onConnect }: { fx: DisplayFx & { retry?: () => void }; onConnect?: () => void }) {
  const { t } = useTranslation("settings");
  if (fx.status === "off" || fx.status === "no_loupist" || fx.status === "reading") return null;
  if (fx.status === "failed") {
    return <>{t("appearance.displayCurrency.status.readFailed")}{" "}
      {fx.retry ? <Button type="button" variant="link" size="sm" className="display-currency-action" onClick={fx.retry}>{t("appearance.displayCurrency.retry")}</Button> : null}</>;
  }
  const display = fx.display;
  const connect = onConnect
    ? <>{" "}<Button type="button" variant="link" size="sm" className="display-currency-action" data-fx-connect onClick={onConnect}>{t("appearance.displayCurrency.connect")}</Button></>
    : null;
  if (display.state === "waiting") return <>{t("appearance.displayCurrency.status.waiting")}</>;
  if (display.state === "ready") {
    const values = { source: display.source, date: formatDay(display.date) };
    switch (display.behind) {
      case undefined: return <>{t("appearance.displayCurrency.status.ready", values)}</>;
      case "offline": return <>{t("appearance.displayCurrency.status.behindOffline", values)}</>;
      case "provider_failed": return <>{t("appearance.displayCurrency.status.behindProviderFailed", values)}</>;
      case "connection_off": return <>{t("appearance.displayCurrency.status.behindConnectionOff", values)}{connect}</>;
    }
  }
  const lastDate = display.lastDate ? formatDay(display.lastDate) : undefined;
  switch (display.reason) {
    case "connection_off": return <>{t("appearance.displayCurrency.status.connectionOff")}{connect}</>;
    case "offline": return <>{lastDate ? t("appearance.displayCurrency.status.offlineSince", { date: lastDate }) : t("appearance.displayCurrency.status.offline")}</>;
    case "provider_failed": return <>{lastDate ? t("appearance.displayCurrency.status.providerFailedSince", { date: lastDate }) : t("appearance.displayCurrency.status.providerFailed")}</>;
    case "no_rate_for_currency": return <>{lastDate
      ? t("appearance.displayCurrency.status.noRateOn", { code: display.currency, date: lastDate })
      : t("appearance.displayCurrency.status.noRate", { code: display.currency })}</>;
    case "too_old": return <>{lastDate ? t("appearance.displayCurrency.status.tooOldSince", { date: lastDate }) : t("appearance.displayCurrency.status.tooOld")}</>;
  }
}
