import { getBuiltinModels, getBuiltinProviders } from "@earendil-works/pi-ai/providers/all";

const BUILTIN_PROVIDERS = new Set(getBuiltinProviders());
const EXACT_BASE_URL_PROVIDERS = new Set(["openai", "openai-codex", "xai"]);

function sameHeaders(headers, bundled) {
  const entries = Object.entries(headers ?? {});
  return entries.length === Object.keys(bundled ?? {}).length
    && entries.every(([name, value]) => Object.hasOwn(bundled ?? {}, name) && bundled[name] === value);
}

export function hasTrustedModelEndpoint(model) {
  try {
    // Pi's public overlay is only applied to built-ins. Custom endpoints use
    // the address saved by the user and do not take models from that overlay.
    if (!BUILTIN_PROVIDERS.has(model?.provider)) return true;
    if (typeof model.baseUrl !== "string") return false;
    const url = URL.parse(model.baseUrl);
    return getBuiltinModels(model.provider).some((bundled) => {
      if (model.api !== bundled.api || !sameHeaders(model.headers, bundled.headers)) return false;
      // Some bundled addresses are templates or empty (the SDK takes the
      // address from local configuration). Keep those forms exactly as shipped.
      return model.baseUrl === bundled.baseUrl || (!EXACT_BASE_URL_PROVIDERS.has(model.provider)
        && url?.protocol === "https:"
        && !url.username && !url.password && !url.search && !url.hash
        && !bundled.baseUrl.includes("{") && url.origin === URL.parse(bundled.baseUrl)?.origin);
    });
  } catch {
    return false;
  }
}

const PERFORMANCE_APIS = new Set([
  "openai-responses",
  "openai-codex-responses",
]);

const PERFORMANCE_CONTROL = Object.freeze({
  id: "performance",
  label: "Performance",
  default_choice: "standard",
  choices: Object.freeze([
    Object.freeze({
      id: "standard",
      label: "Standard",
      description: "Use the provider's standard service tier.",
    }),
    Object.freeze({
      id: "fast",
      label: "Fast",
      description: "Request the provider's priority service tier.",
    }),
  ]),
});

export function runControlsFor(model) {
  return PERFORMANCE_APIS.has(model?.api) ? [PERFORMANCE_CONTROL] : [];
}

export function applyRunOptions(model, payload, options) {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    throw new Error("Pi provider request payload is invalid");
  }
  const controls = new Map(runControlsFor(model).map((control) => [control.id, control]));
  for (const [controlId, choiceId] of Object.entries(options)) {
    const control = controls.get(controlId);
    if (!control || !control.choices.some((choice) => choice.id === choiceId)) {
      throw new Error("Pi model run option is unsupported");
    }
  }

  const next = { ...payload };
  if (options.performance === "fast") next.service_tier = "priority";
  else if (options.performance === "standard") delete next.service_tier;
  return next;
}
