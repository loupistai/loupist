export const LENS_LINK_PROTOCOL = "lens";

/** Parse a navigation destination, never a filesystem path or an evidence receipt. */
export function lensLinkRecordId(href: string): string | null {
  if (!href.startsWith(`${LENS_LINK_PROTOCOL}:`)) return null;
  // Canonical record IDs are URI-safe already. Keep their literal identity in
  // sync with Markdown discovery rather than interpreting encoded aliases.
  const recordId = href.slice(LENS_LINK_PROTOCOL.length + 1);
  return recordId.length > 0 && recordId.length <= 128 && !/[\s\u0000-\u001f\u007f/%?#\\:]/u.test(recordId)
    ? recordId
    : null;
}
