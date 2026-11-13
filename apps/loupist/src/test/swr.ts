// Tests replace `swr` with this module (vite.config.ts `test.alias`). `@ai-sdk/react`
// imports SWR only for `useCompletion` and `useObject`, which Loupist does not use;
// the production bundle drops it. Loading the real SWR 2.5 in jsdom registers
// `setTimeout.bind(undefined, revalidate)` as focus and visibilitychange listeners,
// so each such Event became a timeout delay and Node printed `TimeoutNaNWarning`.
export default function useSWR(): never {
  throw new Error("SWR is stubbed in renderer tests; Loupist does not use it.");
}
