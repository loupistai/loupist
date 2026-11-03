import { createContext, useContext } from "react";
import type { AssetPageTarget } from "../../contracts/assets";
import "@/styles/asset-rows.css";

/** Opens an asset page over the current tab; the app shell provides it. */
export type OpenAssetPage = (target: AssetPageTarget) => void;

const AssetPageOpenerContext = createContext<OpenAssetPage | null>(null);
export const AssetPageOpenerProvider = AssetPageOpenerContext.Provider;
/** Null outside the shell (tests, previews of one component): entry points hide themselves. */
export const useAssetPageOpener = () => useContext(AssetPageOpenerContext);
