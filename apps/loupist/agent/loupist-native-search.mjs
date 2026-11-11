// Provider wire contracts are ported from Oh My Pi (can1357/oh-my-pi) commit
// 76a294cb19bfded1e32e2111f1f729129595bf5e.
//
// MIT License
// Copyright (c) 2025 Mario Zechner
// Copyright (c) 2025-2026 Can Bölük
// Copyright (c) 2026 Stencil Labs, Inc.
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

import { NativeSearchError, SEARCH_HARD_TIMEOUT_MS, toSearchResult } from "./native-search/common.mjs";
import { anthropicSearchAdapter } from "./native-search/anthropic.mjs";
import { codexSearchAdapter, openaiSearchAdapter } from "./native-search/codex.mjs";
import { xaiSearchAdapter } from "./native-search/xai.mjs";

// Each adapter names the Pi provider it serves. Rust offers native search for
// a provider only when `provider-manifest.json` gives it `native_search`, and
// `provider-manifest.test.mjs` keeps that list and these adapters the same.
const ADAPTERS = Object.freeze(Object.fromEntries(
  [openaiSearchAdapter, codexSearchAdapter, anthropicSearchAdapter, xaiSearchAdapter].map((adapter) => [adapter.id, adapter]),
));

export function isNativeSearchProvider(providerId) {
  return Object.hasOwn(ADAPTERS, providerId);
}

export const nativeSearchProviderIds = () => Object.keys(ADAPTERS);

export function selectSearchModel(providerId, availableIds = []) {
  const allowlist = ADAPTERS[providerId]?.modelIds ?? [];
  const available = new Set(availableIds);
  return allowlist.find((id) => available.has(id)) ?? allowlist[0];
}

/** Bounded request fields shared by the host request file and direct callers. */
export function validateSearchRequest(request) {
  if (
    typeof request.query !== "string" ||
    request.query.trim().length === 0 ||
    request.query.length > 4_096
  ) {
    throw new NativeSearchError("malformed", "search query is invalid");
  }
  if (!Number.isInteger(request.limit) || request.limit < 1 || request.limit > 10) {
    throw new NativeSearchError("malformed", "search limit is invalid");
  }
  if (request.recency !== undefined && !["day", "week", "month", "year"].includes(request.recency)) {
    throw new NativeSearchError("malformed", "search recency is invalid");
  }
  for (const domains of [request.include_domains, request.exclude_domains]) {
    if (
      domains !== undefined &&
      (!Array.isArray(domains) ||
        domains.length > 10 ||
        domains.some(
          (domain) =>
            typeof domain !== "string" ||
            domain.length < 1 ||
            domain.length > 253 ||
            !/^[a-z0-9.-]+$/u.test(domain),
        ))
    ) {
      throw new NativeSearchError("malformed", "search domains are invalid");
    }
  }
}

/** OpenAI, Codex and Anthropic search run through Pi (`deps.ctx`); xAI posts directly
 * with the credential `deps.resolveAuth` returns. Every provider requires a
 * resolvable credential before any request is made.
 */
export async function runNativeSearch(request, deps) {
  const adapter = ADAPTERS[request.provider];
  if (!adapter) {
    throw new NativeSearchError("unavailable", "search provider is not allowlisted");
  }
  validateSearchRequest(request);
  const timeoutSignal = AbortSignal.timeout(deps.timeoutMs ?? SEARCH_HARD_TIMEOUT_MS);
  const signal = deps.signal ? AbortSignal.any([deps.signal, timeoutSignal]) : timeoutSignal;
  const auth = await deps.resolveAuth(adapter.id);
  if (!auth) throw new NativeSearchError("unavailable", "provider credentials are unavailable");
  const model = selectSearchModel(adapter.id, deps.availableModelIds ?? []);
  let parsed;
  try {
    parsed = await adapter.search({ request, model, auth, deps, signal });
  } catch (error) {
    if (deps.signal?.aborted && error instanceof NativeSearchError && error.kind === "timeout") {
      throw new NativeSearchError("cancelled", "provider request was cancelled");
    }
    throw error;
  }
  return toSearchResult(adapter.id, parsed, request.limit);
}
