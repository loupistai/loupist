import { render, screen } from "@testing-library/react";
import { ChartBoundary } from "./ChartBoundary";

function BrokenChart(): never {
  throw new TypeError("measureCtx.scale is not a function");
}

it("keeps a chart engine failure inside the chart card", () => {
  vi.spyOn(console, "error").mockImplementation(() => undefined);
  render(<div>
    <p>The answer stays readable.</p>
    <ChartBoundary fallback={<p role="alert">This chart is no longer available.</p>}><BrokenChart /></ChartBoundary>
  </div>);
  expect(screen.getByText("The answer stays readable.")).toBeVisible();
  expect(screen.getByRole("alert")).toHaveTextContent("This chart is no longer available.");
});
