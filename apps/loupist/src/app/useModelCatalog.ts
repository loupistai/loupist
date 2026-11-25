import i18n, { useTranslation } from "@/i18n";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type {
  ConfigScope,
  LoupistBridge,
  ModelCatalog,
  ModelRunSelection,
} from "../types";
import type { ModelProviderActions } from "../components/settings/ModelSettingsPanel";
import type { ComposerModelSelection, ComposerModelWrite } from "../contracts/generated";
import { errorMessage } from "../lib/errors";
import { usableCatalogModel, visibleCatalogModels } from "../lib/modelSelection";
import { CONFIG_CHANGED_EVENT, notifyConfigurationChanged } from "./useConfiguration";
import { isEmptyChatThreadId } from "./emptyChat";

/** A chat's composer choice: a model, or `null` for the Loupist's default (its Default row). */
export type ChatModelChoice = ModelRunSelection | null;

/** The chats of one Loupist, by the prefix of their `chatSessionKey`. */
const loupistChatKeys = (loupistId: string) => `${encodeURIComponent(loupistId)}:`;
const chatIdentity = (key: string) => {
  const [loupist_id, thread_id] = key.split(":").map(decodeURIComponent);
  return { loupist_id, thread_id };
};
const toWire = (selection: ModelRunSelection): ComposerModelSelection => ({
  modelProfileId: selection.model_profile_id,
  thinkingLevel: selection.thinking_level,
  runOptions: selection.run_options,
});
const fromWire = (selection: ComposerModelSelection): ModelRunSelection => ({
  model_profile_id: selection.modelProfileId,
  thinking_level: selection.thinkingLevel,
  run_options: selection.runOptions,
});

/** Model catalog loading and each chat's model run choice. */
export function useModelCatalog(bridge: LoupistBridge) {
  useTranslation();
  const [catalog, setCatalog] = useState<ModelCatalog | null>(null);
  const [resolved, setResolved] = useState(false);
  const [settingsError, setSettingsError] = useState<string | null>(null);
  const [selectionReadError, setSelectionReadError] = useState<{ key: string; message: string } | null>(null);
  // The renderer caches saved choices and keeps the empty composer's choice until Chat creation.
  const [selections, setSelections] = useState<Record<string, ChatModelChoice>>({});
  const [lastSent, setLastSent] = useState<ModelRunSelection | undefined>(undefined);
  const [lastSentLoaded, setLastSentLoaded] = useState(false);
  const [selectionEpoch, setSelectionEpoch] = useState(0);
  const resetEpoch = useRef(0);
  const writes = useRef<Promise<void>>(Promise.resolve());
  const lastSentChanged = useRef(false);
  const localPicks = useRef(new Map<string, ChatModelChoice>());

  const writePreference = useCallback((request: ComposerModelWrite) => {
    const write = writes.current.then(() => bridge.chatComposerModelWrite(request));
    writes.current = write.catch((cause) => {
      setSettingsError(errorMessage(cause, i18n.t("app:useModelCatalog.couldNotReloadModelSettings")));
    });
    return write;
  }, [bridge]);

  useEffect(() => {
    let active = true;
    void bridge.chatComposerModelRead({ chat: null }).then((saved) => {
      if (active && saved && !lastSentChanged.current) setLastSent(fromWire(saved));
    }).catch((cause) => {
      if (active) setSettingsError(errorMessage(cause, i18n.t("app:useModelCatalog.couldNotLoadModelSettings")));
    }).finally(() => {
      if (active) setLastSentLoaded(true);
    });
    return () => { active = false; };
  }, [bridge]);

  const restoreSelection = useCallback((key: string) => {
    let active = true;
    const isCurrent = () => active && selectionEpoch === resetEpoch.current;
    const read = () => {
      void writes.current.then(() => bridge.chatComposerModelRead({ chat: chatIdentity(key) })).then((saved) => {
        if (!isCurrent()) return;
        setSelections((current) => Object.hasOwn(current, key) ? current : { ...current, [key]: saved ? fromWire(saved) : null });
        setSelectionReadError((current) => current?.key === key ? null : current);
      }).catch((cause) => {
        if (!isCurrent()) return;
        setSelectionReadError({ key, message: errorMessage(cause, i18n.t("app:useModelCatalog.couldNotLoadModelSettings")) });
        window.addEventListener("focus", read, { once: true });
      });
    };
    read();
    return () => {
      active = false;
      window.removeEventListener("focus", read);
    };
  }, [bridge, selectionEpoch]);

  useEffect(() => {
    let active = true;
    let initialReadFinished = false;
    let refreshRequested = false;
    let refreshGeneration = 0;
    const refresh = () => {
      if (!active) return;
      if (!initialReadFinished) {
        refreshRequested = true;
        return;
      }
      const generation = ++refreshGeneration;
      void bridge.modelCatalogGet().then((next) => {
        if (!active || generation !== refreshGeneration) return;
        setCatalog(next);
        setSettingsError(null);
      }).catch((cause) => {
        if (!active || generation !== refreshGeneration) return;
        setSettingsError(errorMessage(cause, i18n.t("app:useModelCatalog.couldNotReloadModelSettings")));
      });
    };
    const readSavedCatalog = async () => {
      if (!active) return;
      try {
        const settings = await bridge.modelCatalogGet();
        if (!active) return;
        setCatalog(settings);
        setSettingsError(null);
        // Restore every connected provider missing from the catalog. A usable
        // first account must not hide the others until Settings happens to open.
        // Existing models, including deliberately hidden ones, are kept.
        for (const provider of [...settings.providers, ...settings.custom_providers]) {
          if (provider.credential_source === "missing" || settings.models.some(model => model.provider === provider.id)) continue;
          try {
            const next = await bridge.providerModels(provider.id);
            if (!active) return;
            setCatalog(next);
            setSettingsError(null);
          } catch {
            if (!active) return;
            setSettingsError(i18n.t("app:useModelCatalog.couldNotLoadAvailableModels"));
          }
        }
      } catch (cause) {
        if (active) setSettingsError(errorMessage(cause, i18n.t("app:useModelCatalog.couldNotLoadModelSettings")));
      } finally {
        if (active) {
          setResolved(true);
          initialReadFinished = true;
          if (refreshRequested) refresh();
        }
      }
    };
    const start = () => { void readSavedCatalog(); };
    const stop = bridge.onModelCatalogChanged?.(refresh, start);
    if (!bridge.onModelCatalogChanged) start();
    window.addEventListener("focus", refresh);
    window.addEventListener(CONFIG_CHANGED_EVENT, refresh);
    return () => {
      active = false;
      stop?.();
      window.removeEventListener("focus", refresh);
      window.removeEventListener(CONFIG_CHANGED_EVENT, refresh);
    };
  }, [bridge]);

  // Rust emits provider-status on every state or notice change; patch it in place so cards, the composer and onboarding update live.
  useEffect(() => bridge.onProviderStatus?.(({ provider, status }) => {
    setCatalog((current) => {
      if (!current) return current;
      const providers = current.providers.some((item) => item.id === provider)
        ? current.providers.map((item) => (item.id === provider ? { ...item, status } : item)) : current.providers;
      const custom_providers = current.custom_providers.some((item) => item.id === provider)
        ? current.custom_providers.map((item) => (item.id === provider ? { ...item, status } : item)) : current.custom_providers;
      return { ...current, providers, custom_providers };
    });
  }), [bridge]);

  const acknowledgeModelNotice = useCallback((id: string) => {
    void bridge.modelNoticeAck({ id }).then(() => notifyConfigurationChanged()).catch((cause) => {
      setSettingsError(errorMessage(cause, i18n.t("app:useModelCatalog.couldNotReloadModelSettings")));
    });
  }, [bridge]);

  const changeSelection = useCallback((chatKey: string, selection: ChatModelChoice) => {
    localPicks.current.set(chatKey, selection);
    setSelectionReadError((current) => current?.key === chatKey ? null : current);
    setSelections((current) => current[chatKey] === selection ? current : { ...current, [chatKey]: selection });
    const chat = chatIdentity(chatKey);
    if (!isEmptyChatThreadId(chat.thread_id)) {
      void writePreference({ kind: "set", chat, selection: selection ? toWire(selection) : null, lastSent: null }).catch(() => {});
    }
  }, [writePreference]);

  /** The chat's Default row: the host default answers there, unpinned, whatever the chat ran on before. */
  const resetSelection = useCallback((chatKey: string) => {
    changeSelection(chatKey, null);
  }, [changeSelection]);

  /** A new chat became a saved one: it keeps the choice it was shown with, and the next new chat starts afresh. */
  const moveSelection = useCallback((fromKey: string, toKey: string, choice: ChatModelChoice | undefined) => {
    localPicks.current.delete(fromKey);
    if (choice !== undefined) localPicks.current.set(toKey, choice);
    setSelections((current) => {
      const { [fromKey]: _draft, ...rest } = current;
      return choice === undefined ? rest : { ...rest, [toKey]: choice };
    });
  }, []);

  const saveSent = useCallback(async (chatKey: string, selection: ChatModelChoice, resolved: ModelRunSelection) => {
    // A pick made while Send resolved the configuration wins over the earlier snapshot.
    const choice = localPicks.current.has(chatKey) ? localPicks.current.get(chatKey) : selection;
    const epoch = resetEpoch.current;
    await writePreference({
      kind: "set",
      chat: chatIdentity(chatKey),
      selection: choice ? toWire(choice) : null,
      lastSent: resolved.model_profile_id ? toWire(resolved) : null,
    });
    if (resolved.model_profile_id && epoch === resetEpoch.current) {
      lastSentChanged.current = true;
      setLastSent(resolved);
    }
  }, [writePreference]);

  /**
   * A Chat default saved in Settings ends the choices it covers: every chat's for the app scope,
   * else that Loupist's chats'. Those chats, and new ones, answer with the default until another pick.
   */
  const yieldToChatDefault = useCallback((scope: ConfigScope) => {
    const prefix = scope.kind === "project" ? loupistChatKeys(scope.loupistId) : "";
    for (const key of localPicks.current.keys()) {
      if (key.startsWith(prefix)) localPicks.current.delete(key);
    }
    void writePreference({ kind: "reset", loupistId: scope.kind === "project" ? scope.loupistId : null }).catch(() => {});
    setSelections((current) => {
      const kept = Object.entries(current).filter(([key]) => prefix && !key.startsWith(prefix));
      return kept.length === Object.keys(current).length ? current : Object.fromEntries(kept);
    });
    setSelectionEpoch(++resetEpoch.current);
    lastSentChanged.current = true;
    setLastSent(undefined);
  }, [writePreference]);

  // A composer pick lasts only while Chat can still answer with it. Once its account is
  // disconnected or its model is hidden or no longer offered, the pick becomes the chat's
  // Default row, so the Loupist's default answers from now on, including after the account comes back.
  const usableSelections = useMemo(() => {
    if (!catalog) return selections;
    const dropped = Object.entries(selections).filter(([, selection]) => selection && !usableCatalogModel(catalog, selection.model_profile_id));
    return dropped.length ? { ...selections, ...Object.fromEntries(dropped.map(([key]) => [key, null])) } : selections;
  }, [catalog, selections]);
  useEffect(() => {
    if (usableSelections === selections) return;
    for (const [key, selection] of Object.entries(selections)) {
      if (selection && usableSelections[key] === null) resetSelection(key);
    }
  }, [selections, usableSelections, resetSelection]);
  const usableLastSent = lastSent && catalog && !usableCatalogModel(catalog, lastSent.model_profile_id) ? undefined : lastSent;

  /** Provider operations used by Settings; each refreshes the catalog it returns. */
  const providerActions = useMemo<ModelProviderActions>(() => {
    const apply = async (pending: Promise<ModelCatalog>) => {
      const next = await pending;
      setCatalog(next);
      setSettingsError(null);
      notifyConfigurationChanged();
      return next;
    };
    return {
      onLoadModels: (provider) => apply(bridge.providerModels(provider)),
      onUpdateModelVisibility: (request) => apply(bridge.modelVisibilityUpdate(request)),
      onDisconnect: (provider) => apply(bridge.providerDisconnect(provider)),
      onRemoveCustomProvider: (provider) => apply(bridge.providerCustomRemove(provider)),
      onCatalogChange: (next) => { setCatalog(next); setSettingsError(null); notifyConfigurationChanged(); },
    };
  }, [bridge]);

  const needsProviderSetup =
    resolved &&
    catalog !== null &&
    catalog.providers.every(
      (provider) => provider.credential_source === "missing",
    ) &&
    catalog.models.every((model) => model.credential_source === "missing");
  const needsVisibleModels =
    resolved &&
    catalog !== null &&
    !needsProviderSetup &&
    visibleCatalogModels(catalog).length === 0;

  return {
    catalog,
    setCatalog,
    resolved,
    settingsError,
    selections: usableSelections,
    restoreSelection,
    selectionReadError,
    lastSent: usableLastSent,
    lastSentLoaded,
    changeSelection,
    resetSelection,
    moveSelection,
    saveSent,
    yieldToChatDefault,
    providerActions,
    acknowledgeModelNotice,
    needsProviderSetup,
    needsVisibleModels,
  };
}
