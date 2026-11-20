import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { AssetPricePoint } from "../../contracts/assets";
import { chartPoints, offeredRanges, PriceChart, rangePoints } from "./PriceChart";

/** One point per weekday ending on Nov 13, rising by one each day. */
const weekdays = (count: number, end = "2026-11-13"): AssetPricePoint[] => {
  const points: AssetPricePoint[] = [];
  for (let at = Date.parse(`${end}T00:00:00Z`); points.length < count; at -= 86_400_000) {
    const day = new Date(at).getUTCDay();
    if (day !== 0 && day !== 6) points.unshift({ date: new Date(at).toISOString().slice(0, 10), value: 0 });
  }
  return points.map((point, index) => ({ ...point, value: 100 + index }));
};

it("offers only the ranges the points can fill, plus the first that shows them all", () => {
  expect(offeredRanges(weekdays(20))).toEqual(["1W", "1M"]);
  expect(offeredRanges(weekdays(82))).toEqual(["1W", "1M", "3M", "6M"]);
  expect(offeredRanges(weekdays(262))).toEqual(["1W", "1M", "3M", "6M", "1Y"]);
  expect(offeredRanges(weekdays(1))).toEqual([]);
});

it("keeps one point per stated day in date order, and cuts a range back from the last point", () => {
  const points = chartPoints([
    { date: "2026-11-14T15:30:00Z", value: 12 },
    { date: "2026-11-12", value: 10 },
    { date: "2026-11-14", value: 11 },
    { date: "not a date", value: 9 },
    { date: "2026-11-13", value: Number.NaN },
  ]);
  expect(points).toEqual([{ date: "2026-11-12", value: 10 }, { date: "2026-11-14", value: 11 }]);
  expect(rangePoints(weekdays(40), "1W").map(point => point.date)).toEqual(["2026-11-06", "2026-11-09", "2026-11-10", "2026-11-11", "2026-11-12", "2026-11-13"]);
});

it("draws one line from the range's points with a dashed start line, reads each point and switches range", async () => {
  const points = weekdays(22);
  const onRange = vi.fn();
  const { container } = render(<PriceChart points={points} currency="USD" range="1M" ranges={["1W", "1M"]} onRange={onRange} name="NVIDIA" />);
  const region = screen.getByRole("region", { name: "Price chart" });
  const plot = within(region).getByRole("group", { name: /^NVIDIA prices over 1 month/u });
  expect(within(region).getByText("$121.00")).toBeVisible();
  expect(plot).toHaveAttribute("data-direction", "up");
  expect(plot.getAttribute("aria-label")).toContain("$100.00 to $121.00");
  expect(container.querySelector(".price-chart-base")).not.toBeNull();
  expect(container.querySelector(".price-chart-line")?.getAttribute("d")?.match(/[ML]/gu)).toHaveLength(22);
  // Fewer than 20 points would say so; 22 do not.
  expect(container.querySelector(".price-chart-note")).toBeNull();
  plot.focus();
  await userEvent.setup().keyboard("{End}");
  expect(region.querySelector("[aria-live]")).toHaveTextContent(/^Nov 13 · \$121\.00 · /u);
  expect(screen.getByText("Arrow keys read each price.")).toBeInTheDocument();
  await userEvent.setup().click(within(screen.getByRole("group", { name: "Range" })).getByRole("button", { name: "1 week" }));
  expect(onRange).toHaveBeenCalledWith("1W");
});

it("says how few points it has so far", () => {
  render(<PriceChart points={weekdays(3)} range="1W" ranges={["1W"]} onRange={vi.fn()} name="NVIDIA" />);
  expect(screen.getByText("Only 3 prices are stored so far.")).toBeInTheDocument();
});
