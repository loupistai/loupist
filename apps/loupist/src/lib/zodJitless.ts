/**
 * Zod (through the AI SDK) probes `new Function("")` when it builds its first object schema, to
 * decide whether it may compile parsers. The app's Content-Security-Policy has no 'unsafe-eval', so
 * WebKit blocks the probe and reports a violation on every page, although Zod then parses the same
 * way without it. Zod reads this shared config object (its `z.config({ jitless: true })`), so setting
 * it before the first schema skips the probe. The entry points import this module first.
 */
type ZodGlobal = typeof globalThis & { __zod_globalConfig?: { jitless?: boolean } };
const shared = globalThis as ZodGlobal;
Object.assign((shared.__zod_globalConfig ??= {}), { jitless: true });

export {};
