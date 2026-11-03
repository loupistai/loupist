import { useEffect, useState } from "react";

/** Canvas/chart libraries require resolved colors, not CSS variable expressions. */
export function readChartTokens() {
  const style = getComputedStyle(document.documentElement);
  const color = (name: string) => style.getPropertyValue(`--gg-color-${name}`).trim() || "transparent";
  return {
    up: color("price-up"), down: color("price-down"),
    text: color("text-primary"), secondary: color("text-secondary"),
    border: color("border-subtle"), background: color("bg-app"),
    raised: color("bg-raised"), info: color("info"),
    series: ["company", "product", "event", "person", "other"].map((name) => color(`map-${name}`)),
    font: style.getPropertyValue("--gg-font-ui").trim(),
  };
}

export function useChartTokens() {
  const [tokens, setTokens] = useState(readChartTokens);
  useEffect(() => {
    const update = () => {
      const next = readChartTokens();
      setTokens((current) => JSON.stringify(current) === JSON.stringify(next) ? current : next);
    };
    update();
    const observer = new MutationObserver(update);
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "data-price-colors"] });
    return () => observer.disconnect();
  }, []);
  return tokens;
}

export function chartColorWithOpacity(color: string, opacity: number) {
  // Resolve color-mix before handing a fill to canvas-based chart libraries.
  const probe = document.createElement("span");
  probe.style.color = `color-mix(in srgb, ${color} ${opacity * 100}%, transparent)`;
  document.documentElement.appendChild(probe);
  const resolved = getComputedStyle(probe).color;
  probe.remove();
  return resolved || color;
}
