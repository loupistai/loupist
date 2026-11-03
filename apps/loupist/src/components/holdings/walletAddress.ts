import type { WalletFamily } from "@/contracts/userContext";

/**
 * Renderer-side hints while typing. Rust validates checksums (base58check,
 * bech32/bech32m, EIP-55) and runs the same secret guard again on Add.
 */
const BASE58 = /^[1-9A-HJ-NP-Za-km-z]+$/;

/** bc1…, 1…, 3… → Bitcoin; 0x + 40 hex → EVM; 32–44 base58 → Solana. */
export function detectNetwork(text: string): WalletFamily | null {
  const value = text.trim();
  if (!value || /\s/.test(value)) return null;
  if (/^bc1[02-9ac-hj-np-z]{8,87}$/i.test(value)) return "bitcoin";
  if (/^0x[0-9a-f]{40}$/i.test(value)) return "evm";
  if (/^[13]/.test(value) && value.length >= 26 && value.length <= 35 && BASE58.test(value)) return "bitcoin";
  if (value.length >= 32 && value.length <= 44 && BASE58.test(value)) return "solana";
  return null;
}

/**
 * Twelve to twenty-four plain words (a recovery phrase), 64 hex characters,
 * a WIF or extended private key, or a 64-byte keypair. Addresses never
 * contain spaces, so the word rule cannot catch a real address.
 */
export function looksLikeSecret(text: string): boolean {
  const value = text.trim();
  const words = value.split(/\s+/).filter(Boolean);
  if (words.length >= 12 && words.length <= 24 && words.every(word => /^[A-Za-z]{3,8}$/.test(word))) return true;
  if (/^(0x)?[0-9a-f]{64}$/i.test(value)) return true;
  if (/^[xyztuv]prv/.test(value)) return true;
  if (/^[5KLc9][1-9A-HJ-NP-Za-km-z]{50,51}$/.test(value)) return true;
  if (value.length >= 80 && value.length <= 90 && BASE58.test(value)) return true;
  if (value.startsWith("[") && value.endsWith("]") && (value.match(/,/g)?.length ?? 0) === 63) return true;
  return false;
}

/** First six and last four characters; the only form the app shows. */
export function maskAddress(address: string): string {
  return address.length <= 12 ? address : `${address.slice(0, 6)}…${address.slice(-4)}`;
}
