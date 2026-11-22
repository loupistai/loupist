import { createContext, useContext } from "react";
import type { ConfigSnapshot } from "../contracts/config";

export type ExtensionAvailability = {
  userSkills: boolean;
  customConnections: boolean;
};

export function extensionAvailability(snapshot: ConfigSnapshot | null): ExtensionAvailability {
  return {
    userSkills: snapshot?.userSkillsAvailable === true,
    customConnections: snapshot?.customConnectionsAvailable === true,
  };
}

const ExtensionAvailabilityContext = createContext(extensionAvailability(null));

export const ExtensionAvailabilityProvider = ExtensionAvailabilityContext.Provider;

export function useExtensionAvailability() {
  return useContext(ExtensionAvailabilityContext);
}
