import { parseCredentialFreeHttpUrl } from "./credentialFreeUrl";

/** The host a reader knows a page by: no scheme and no leading `www.`. Empty unless the address is a credential-free http(s) URL. */
export const displayHost = (url: string | undefined): string =>
  url ? parseCredentialFreeHttpUrl(url)?.hostname.replace(/^www\./u, "") ?? "" : "";
