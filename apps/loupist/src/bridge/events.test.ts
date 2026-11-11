import registry from "../../src-tauri/src/events.rs?raw";
import bridgeSource from "./tauriBridge.ts?raw";
import { BRIDGE_EVENTS } from "./events";

const rustSources = import.meta.glob<string>("../../src-tauri/src/**/*.rs", { query: "?raw", import: "default", eager: true });
const isTest = (path: string) => /(?:^|\/)tests?(?:\/|\.rs$)|_tests?\.rs$/.test(path);

/** `pub(crate) const NAME: &str = "value";` declarations. */
const constants = (source: string) =>
  new Map([...source.matchAll(/const ([A-Z_]+): &str = "([^"]+)";/g)].map(([, name, value]) => [name!, value!]));

/** The event name of every Tauri `emit` call (`.emit(` or `Emitter::emit(`) in production Rust. */
function emittedNames() {
  const events = constants(registry);
  const names = new Set<string>();
  const problems: string[] = [];
  for (const [path, source] of Object.entries(rustSources)) {
    if (isTest(path)) continue;
    for (const [, argument] of source.matchAll(/(?:\.|::)emit\(\s*(?:&?[a-z_.0-9()]+,\s*)?((?:crate::)?events::[A-Z_]+|[A-Za-z_]+|"[^"]*")\s*,/g)) {
      const event = argument!.replace(/^(?:crate::)?events::/, "");
      if (events.has(event) && argument!.includes("events::")) names.add(events.get(event)!);
      // `briefings::desktop::emit` forwards a registry name it was given.
      else if (path.endsWith("briefings/desktop.rs") && event === "event") continue;
      else problems.push(`${path}: emit(${argument})`);
    }
  }
  return { names, problems, registered: new Set(events.values()) };
}

it("emits only registry event names, each one the renderer subscribes to", () => {
  const { names, problems, registered } = emittedNames();
  expect(problems).toEqual([]);
  expect([...registered].filter((name) => !names.has(name))).toEqual([]);
  expect([...names].sort()).toEqual(Object.values(BRIDGE_EVENTS).sort());
});

it("subscribes to every registry event by its registry entry", () => {
  expect(bridgeSource).not.toMatch(/subscribe\(\s*["'`]/);
  for (const key of Object.keys(BRIDGE_EVENTS)) expect(bridgeSource).toContain(`subscribe(BRIDGE_EVENTS.${key},`);
});
