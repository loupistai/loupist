import "../../../styles/connections.css";
import { useTranslation } from "../../../i18n";

import type { CustomConnectionOrigin } from "../../../contracts/knowledge";
export function ConnectionOrigin({ origin }: { origin?: CustomConnectionOrigin }) {
  const { t } = useTranslation("chat");
  return origin ? <span className="custom-meta">{t("customConnection.origin", { connection: origin.name })}</span> : null;
}
