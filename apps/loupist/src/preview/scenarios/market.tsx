/** The shared market primitives on their own, before any screen is built from them. */
import { AssetTile } from "../../components/market/AssetTile";
import { ChangePill } from "../../components/market/ChangePill";
import { PriceText } from "../../components/market/PriceText";
import { Sparkline } from "../../components/market/Sparkline";
import { prepareFixture, previewKo } from "../fixtures";
import { select, type PreviewScenario } from "./kit";

const walk = (from: number, to: number, count: number, wobble: number, seed: number) =>
  Array.from({ length: count }, (_, index) => from + (to - from) * index / (count - 1) + Math.sin(index * 0.55 + seed) * wobble * Math.sin(Math.PI * index / (count - 1)));

const ASSETS = [
  { name: ["엔비디아", "NVIDIA"], symbol: "NVDA", market: "NASDAQ", currency: "USD", price: 162.85, change: -3.34, percent: -0.0201, closes: walk(171, 162.85, 30, 4, 1) },
  { name: ["삼성전자", "Samsung Electronics"], symbol: "005930", market: "KRX", currency: "KRW", price: 112400, change: 1400, percent: 0.0126, closes: walk(104800, 112400, 30, 2600, 2) },
  { name: ["SK하이닉스", "SK hynix"], symbol: "000660", market: "KRX", currency: "KRW", price: 618000, change: 13000, percent: 0.0215, closes: walk(566000, 618000, 30, 16000, 3) },
  { name: ["애플", "Apple Inc."], symbol: "AAPL", market: "NASDAQ", currency: "USD", price: 268.47, change: -0.83, percent: -0.0031, closes: walk(259, 268.47, 30, 5, 4) },
  { name: ["비트코인", "Bitcoin"], symbol: "BTC", market: "Crypto", currency: "USD", price: 98420, change: 1854, percent: 0.0192, closes: walk(103900, 98420, 30, 3800, 5), digits: 0 },
  { name: ["보합 예시", "Level example"], symbol: "LVL", market: "NYSE", currency: "USD", price: 50, change: 0, percent: 0, closes: [50, 50.4, 49.7, 50.2, 50] },
] as const;

// Preview fixture headings, as the fixtures name assets in both languages.
const HEADINGS = { current: ["현재 설정", "Current setting"], redUp: ["상승 빨강", "Red up"], greenUp: ["상승 초록", "Green up"] } as const;

// Gallery layout only; the primitives carry their own styles (styles/market.css).
const GALLERY_CSS = `
.preview-market { box-sizing: border-box; height: 100vh; overflow: auto; padding: var(--gg-space-6); display: grid; gap: var(--gg-space-5); align-content: start; background: var(--gg-color-bg-app); color: var(--gg-color-text-primary); font-family: var(--gg-font-ui); }
.preview-market h2 { margin: 0 0 var(--gg-space-2); font-size: var(--gg-text-meta); font-weight: var(--gg-weight-semibold); color: var(--gg-color-text-tertiary); }
.preview-market-headline, .preview-market-sizes { display: flex; flex-wrap: wrap; align-items: center; gap: var(--gg-space-4); padding: var(--gg-space-4); border: 1px solid var(--gg-color-border-subtle); border-radius: var(--gg-radius-lg); background: var(--gg-color-bg-raised); box-shadow: var(--gg-shadow-card); }
.preview-market-headline > div { display: flex; align-items: baseline; gap: var(--gg-space-3); flex-wrap: wrap; }
.preview-market-columns { display: grid; grid-template-columns: repeat(auto-fit, minmax(560px, 1fr)); gap: var(--gg-space-4); }
.preview-market-rows { border: 1px solid var(--gg-color-border-subtle); border-radius: var(--gg-radius-lg); background: var(--gg-color-bg-raised); box-shadow: var(--gg-shadow-card); padding: 0 var(--gg-space-3); }
.preview-market-row { display: grid; grid-template-columns: auto minmax(0, 1fr) auto auto auto; align-items: center; gap: var(--gg-space-3); min-height: 60px; border-top: 1px solid var(--gg-color-border-subtle); }
.preview-market-row:first-child { border-top: 0; }
.preview-market-row > .market-price, .preview-market-row > .market-change { justify-self: end; }
.preview-market-name { min-width: 0; display: grid; line-height: 1.3; }
.preview-market-name b { font-weight: var(--gg-weight-strong); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.preview-market-name small { font-size: var(--gg-text-meta); color: var(--gg-color-text-tertiary); }
`;

function MarketGallery() {
  const ko = previewKo();
  const rows = (preview?: "red-up" | "green-up") => (
    <div className="preview-market-rows" data-price-preview={preview}>
      {ASSETS.map((asset) => (
        <div key={asset.symbol} className="preview-market-row">
          <AssetTile name={asset.name[ko ? 0 : 1]} symbol={asset.symbol} />
          <span className="preview-market-name"><b>{asset.name[ko ? 0 : 1]}</b><small>{asset.symbol} · {asset.market}</small></span>
          <Sparkline closes={asset.closes} currency={asset.currency} />
          <PriceText value={asset.price} currency={asset.currency} digits={"digits" in asset ? asset.digits : undefined} />
          <ChangePill percent={asset.percent} />
        </div>
      ))}
    </div>
  );
  const nvidia = ASSETS[0];
  return (
    <main className="preview-market" data-market-gallery>
      <style>{GALLERY_CSS}</style>
      <section className="preview-market-headline">
        <AssetTile name={nvidia.name[ko ? 0 : 1]} size="lg" />
        <div>
          <PriceText value={nvidia.price} currency={nvidia.currency} size="figure" />
          <ChangePill percent={nvidia.percent} change={nvidia.change} currency={nvidia.currency} size="lg" />
        </div>
        <Sparkline closes={walk(151, 162.85, 65, 6, 7)} currency="USD" width={170} height={40} />
      </section>
      <div className="preview-market-columns">
        <section><h2>{HEADINGS.current[ko ? 0 : 1]}</h2>{rows()}</section>
        <section><h2>{HEADINGS.redUp[ko ? 0 : 1]}</h2>{rows("red-up")}</section>
        <section><h2>{HEADINGS.greenUp[ko ? 0 : 1]}</h2>{rows("green-up")}</section>
      </div>
      <section className="preview-market-sizes">
        <AssetTile name="Tesla" size="sm" /><AssetTile name="Tesla" /><AssetTile name="Tesla" size="lg" />
        <ChangePill percent={0.0084} size="sm" /><ChangePill percent={0.0084} /><ChangePill percent={-0.0084} size="lg" />
        <PriceText value={412.3} currency="USD" /><PriceText value={412.3} currency="USD" size="title" />
      </section>
    </main>
  );
}

export const marketScenarios: PreviewScenario[] = [
  {
    id: "market-primitives",
    title: "Market primitives: asset tile, price, change pill and trend line in both price conventions",
    setup: (bridge) => prepareFixture(bridge),
    enter: async () => {},
    landmark: () => select("[data-market-gallery] .market-sparkline"),
    render: () => <MarketGallery />,
  },
];
