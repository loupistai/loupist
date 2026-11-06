/**
 * Public renderer contract barrel.
 *
 * Domain modules under `contracts/` own the wire shapes. Existing renderer
 * imports stay stable through this intentionally thin entry point.
 */
export type * from "./contracts/browser";
export type * from "./contracts/bridge";
export type * from "./contracts/chat";
export type * from "./contracts/loupist";
export type * from "./contracts/knowledge";
export type * from "./contracts/lens";
export type * from "./contracts/memory";
export type * from "./contracts/model";
export type * from "./contracts/runtime";
export type {
  LoupistCapabilityBinding,
  MarketplaceCredentialStatus,
  MarketplaceSnapshot,
} from "./marketplace/types";

export type * from "./contracts/workspace";
export type * from "./contracts/scheduled";
export type * from "./contracts/earnings";
export type * from "./contracts/watch";
export type * from "./contracts/config";

export type * from "./contracts/skills";
export type * from "./contracts/userContext";
