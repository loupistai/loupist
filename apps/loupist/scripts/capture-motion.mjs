// Serialized into the capture page; keep this function independent of module scope.
export function collectRunningCssMotion() {
  const number = value => Number.isFinite(value) ? value : "infinite";
  return {
    reducedMotion: matchMedia("(prefers-reduced-motion: reduce)").matches,
    animations: document.getAnimations().flatMap(animation => {
      const type = animation instanceof CSSAnimation ? "animation" : animation instanceof CSSTransition ? "transition" : null;
      if (!type || animation.playState !== "running") return [];
      const effect = animation.effect;
      const target = effect.target;
      const selector = target ? target.localName + (target.id ? `#${CSS.escape(target.id)}` : "")
        + [...target.classList].slice(0, 3).map(name => `.${CSS.escape(name)}`).join("") : "<unknown>";
      return [{
        type, target: selector + (effect.pseudoElement ?? ""),
        animationName: type === "animation" ? animation.animationName : null,
        transitionProperty: type === "transition" ? animation.transitionProperty : null,
        durationMs: number(effect.getComputedTiming().duration),
        iterations: number(effect.getTiming().iterations),
      }];
    }),
  };
}

// Input is the JSON-safe snapshot above; duration includes all finite repetitions.
export function reducedMotionViolations(animations, { maxDurationMs, allowlist }) {
  return animations.filter(animation => {
    const excessive = animation.iterations === "infinite" || animation.durationMs === "infinite"
      || animation.durationMs * animation.iterations > maxDurationMs;
    const allowed = allowlist.some(entry => entry.reason?.trim() && entry.type === animation.type
      && entry.target === animation.target && entry.animationName === animation.animationName
      && entry.transitionProperty === animation.transitionProperty);
    return excessive && !allowed;
  });
}
