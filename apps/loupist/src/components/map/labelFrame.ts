import type { PositionedNode } from "./layout";
import type { PlacedLabel } from "./labels";

/** One render's label geometry. React renders invalidate priorities, font and
 * viewport exclusions; canvas frames also check the camera and mutable nodes.
 * Keep painting every frame so halos, fades and pointer interaction stay live. */
export function createLabelFrame() {
  let previous: { scale: number; x: number; y: number; nodes: PositionedNode[]; positions: number[]; labels: PlacedLabel[] } | undefined;
  return (nodes: PositionedNode[], scale: number, origin: { x: number; y: number }, place: () => PlacedLabel[]) => {
    if (previous && previous.scale === scale && previous.x === origin.x && previous.y === origin.y &&
      nodes.length === previous.nodes.length && nodes.every((node, index) =>
        node === previous!.nodes[index] && node.x === previous!.positions[index * 2] && node.y === previous!.positions[index * 2 + 1])) return previous.labels;
    const labels = place();
    previous = { scale, ...origin, nodes: [...nodes], positions: nodes.flatMap(node => [node.x, node.y]), labels };
    return labels;
  };
}
