import { cleanup, render, screen } from "@testing-library/react";
import i18n from "../../i18n";
import { AssetTile } from "./AssetTile";
import { ChangePill } from "./ChangePill";
import { PriceText } from "./PriceText";
import { Sparkline } from "./Sparkline";

afterEach(async () => { cleanup(); await i18n.changeLanguage("en"); });

describe("PriceText", () => {
  it.each([
    ["en", 162.85, "USD", undefined, "$162.85"],
    ["en", 112400, "KRW", undefined, "₩112,400"],
    ["ko", 112400, "KRW", undefined, "112,400원"],
    ["ko", 162.85, "USD", undefined, "$162.85"],
    ["en", 98420, "USD", 0, "$98,420"],
    ["en", 4.5, undefined, undefined, "4.50"],
  ])("writes a %s price of %s %s in the locale's form", async (locale, value, currency, digits, expected) => {
    await i18n.changeLanguage(locale);
    render(<PriceText value={value} currency={currency} digits={digits} size="figure" />);
    const price = screen.getByText(expected);
    expect(price).toHaveClass("market-price", "gg-num");
    expect(price).toHaveAttribute("data-size", "figure");
  });
});

describe("ChangePill", () => {
  it("marks the direction with a glyph and says it; the figures after it are magnitudes", () => {
    const { container } = render(<ChangePill percent={0.0126} change={3.34} currency="USD" />);
    const pill = container.querySelector(".market-change")!;
    expect(pill).toHaveAttribute("data-direction", "up");
    expect(pill.querySelector("[aria-hidden=true]")).toHaveTextContent("▲");
    expect(pill).toHaveTextContent("Up 3.34 · 1.26%");
    expect(pill.textContent).not.toMatch(/[+\-−]/u);
  });

  it("falls with a down glyph and keeps the currency's decimals", () => {
    const { container } = render(<ChangePill percent={-0.0201} change={-2300} currency="KRW" />);
    const pill = container.querySelector(".market-change")!;
    expect(pill).toHaveAttribute("data-direction", "down");
    expect(pill.querySelector("[aria-hidden=true]")).toHaveTextContent("▼");
    expect(pill).toHaveTextContent("Down 2,300 · 2.01%");
    expect(pill.textContent).not.toMatch(/[+\-−]/u);
  });

  it("is flat when the change rounds to zero, and speaks Korean in Korean", async () => {
    await i18n.changeLanguage("ko");
    const { container } = render(<ChangePill percent={0.00001} />);
    const pill = container.querySelector(".market-change")!;
    expect(pill).toHaveAttribute("data-direction", "flat");
    expect(pill).toHaveTextContent("보합 0.00%");
  });

  it("takes its direction from the price change when the percent rounds to zero", () => {
    const { container } = render(<ChangePill percent={0.00004} change={0.05} currency="USD" />);
    expect(container.querySelector(".market-change")).toHaveAttribute("data-direction", "up");
  });
});

describe("Sparkline", () => {
  it("draws the closes inside its box, coloured by first to last, with a spoken summary", () => {
    render(<Sparkline closes={[100, 104, 98, 110]} currency="USD" width={96} height={28} />);
    const line = screen.getByRole("img", { name: "Trend from $100.00 to $110.00, up 10.0%" });
    expect(line).toHaveAttribute("data-direction", "up");
    expect(line).toHaveAttribute("viewBox", "0 0 96 28");
    const points = [...line.querySelector(".market-sparkline-line")!.getAttribute("d")!.matchAll(/[ML]([\d.]+) ([\d.]+)/g)].map(([, x, y]) => [Number(x), Number(y)]);
    expect(points).toHaveLength(4);
    for (const [x, y] of points) {
      expect(x).toBeGreaterThanOrEqual(0); expect(x).toBeLessThanOrEqual(96);
      expect(y).toBeGreaterThanOrEqual(0); expect(y).toBeLessThanOrEqual(28);
    }
    // The highest close is drawn highest.
    expect(points[3][1]).toBeLessThan(points[2][1]);
    expect(line.querySelector(".market-sparkline-end")).not.toBeNull();
  });

  it("falls, stays level, and draws nothing without two closes", () => {
    const { rerender, container } = render(<Sparkline closes={[110, 100]} showEnd={false} />);
    expect(screen.getByRole("img")).toHaveAttribute("data-direction", "down");
    expect(screen.getByRole("img")).toHaveAccessibleName("Trend from 110.00 to 100.00, down 9.1%");
    expect(container.querySelector(".market-sparkline-end")).toBeNull();
    rerender(<Sparkline closes={[5, 5, 5]} />);
    expect(screen.getByRole("img")).toHaveAttribute("data-direction", "flat");
    rerender(<Sparkline closes={[5, Number.NaN]} />);
    expect(container.querySelector("svg")).toBeNull();
  });
});

describe("AssetTile", () => {
  it.each([
    ["Samsung Electronics", undefined, "SE"],
    ["삼성전자", undefined, "삼"],
    ["SK hynix", undefined, "SK"],
    ["NVIDIA Corp", undefined, "NV"],
    ["Apple Inc.", undefined, "AP"],
    ["Tesla", undefined, "TE"],
    ["トヨタ自動車", undefined, "ト"],
    ["", "BTC", "BT"],
  ])("takes the letters of %j from the name", (name, symbol, expected) => {
    const { container } = render(<AssetTile name={name} symbol={symbol} size="sm" />);
    const tile = container.querySelector(".market-tile")!;
    expect(tile).toHaveTextContent(expected);
    expect(tile).toHaveAttribute("aria-hidden", "true");
    expect(tile).toHaveAttribute("data-size", "sm");
  });
});
