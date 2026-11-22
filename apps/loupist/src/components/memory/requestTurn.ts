import { createContext, useContext } from "react";

/** Resolves the saved user-message ID in the open Loupist's conversations. */
type RequestTurn = (messageId: string) => { date: string; open: () => void } | undefined;
const RequestTurnContext = createContext<RequestTurn | null>(null);
export const RequestTurnProvider = RequestTurnContext.Provider;
export const useRequestTurn = () => useContext(RequestTurnContext);
