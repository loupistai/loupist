import { assetExchangeLabel, type AssetReferenceFields } from "../../contracts/assets";
import directory from "../../../src-tauri/src/finance_data/openfigi/equity-exchanges.json";

const exchanges: Readonly<Record<string, (string | null)[][]>> = directory.exchanges;
const regionOptions = { type: "region", fallback: "none" } as const;
const englishRegions = new Intl.DisplayNames("en", regionOptions);

/** Country-wide OpenFIGI listings use a region name. Venue names retain the provider's spelling. */
export function listingExchange(reference: AssetReferenceFields | null | undefined, language: string) {
  const venue = assetExchangeLabel(reference);
  const matches = exchanges[reference?.exchange?.trim() ?? ""];
  const code = matches?.length === 1 ? matches[0]?.[1] : null;
  const country = code && /^[A-Z]{2}$/u.test(code) && venue === englishRegions.of(code)
    ? new Intl.DisplayNames(language, regionOptions).of(code) ?? venue
    : null;
  return { exchange: country ?? venue, country };
}
