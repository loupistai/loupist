import { test } from "node:test";
import assert from "node:assert/strict";
import { reducedMotionViolations } from "./capture-motion.mjs";

const policy = { maxDurationMs: 1, allowlist: [] };
const animation = overrides => ({ type: "animation", target: "span.status", animationName: "pulse", transitionProperty: null, durationMs: 0.01, iterations: 1, ...overrides });

test("permits disabled/near-zero motion and the inclusive threshold", () => {
  assert.deepEqual(reducedMotionViolations([
    animation({ durationMs: 0 }), animation({}), animation({ durationMs: 1 }),
    animation({ type: "transition", animationName: null, transitionProperty: "opacity" }),
  ], policy), []);
});

test("rejects infinite motion, long animations/transitions and repeated near-zero motion", () => {
  const violations = [
    animation({ durationMs: 0, iterations: "infinite" }),
    animation({ durationMs: "infinite" }),
    animation({ durationMs: 1.01 }),
    animation({ type: "transition", animationName: null, transitionProperty: "transform", durationMs: 80 }),
    animation({ durationMs: 0.01, iterations: 101 }),
  ];
  assert.deepEqual(reducedMotionViolations([animation({}), ...violations], policy), violations);
});

test("an exception needs an exact target, effect, type and a written reason", () => {
  const spinner = animation({ animationName: "spin", iterations: "infinite" });
  const allowlist = [{ ...spinner, reason: "Test-only example of an explicitly required status spinner" }];
  const others = [animation({ ...spinner, target: "div.status" }), animation({ ...spinner, animationName: "pulse" }),
    animation({ ...spinner, type: "transition", animationName: null, transitionProperty: "opacity" })];
  assert.deepEqual(reducedMotionViolations([spinner, ...others], { ...policy, allowlist }), others);
  assert.deepEqual(reducedMotionViolations([spinner], { ...policy, allowlist: [{ ...spinner, reason: " " }] }), [spinner]);
});
