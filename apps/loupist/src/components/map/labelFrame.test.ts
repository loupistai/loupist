import { expect, it, vi } from "vitest";
import { createLabelFrame } from "./labelFrame";
import type { PositionedNode } from "./layout";

it("reuses steady label geometry and recomputes after camera, node or visibility changes", () => {
  const frame = createLabelFrame();
  const nodes = [{ id: "a", x: 1, y: 2 }] as PositionedNode[];
  const place = vi.fn(() => []);
  const read = (scale = 1, x = 0, y = 0) => frame(nodes, scale, { x, y }, place);
  const first = read();
  expect(read()).toBe(first);
  expect(place).toHaveBeenCalledTimes(1);
  read(2); read(2, 10); read(2, 10, 20);
  nodes[0].x += 1; read(2, 10, 20);
  nodes[0].y += 1; read(2, 10, 20);
  nodes[0] = { ...nodes[0] }; read(2, 10, 20);
  nodes.push({ ...nodes[0], id: "b" }); read(2, 10, 20);
  nodes.pop(); read(2, 10, 20);
  expect(place).toHaveBeenCalledTimes(9);
  // A new React render creates a fresh frame for changed selection, locale,
  // font, obstacles and glow priorities, even with identical coordinates.
  createLabelFrame()(nodes, 2, { x: 10, y: 20 }, place);
  expect(place).toHaveBeenCalledTimes(10);
});
