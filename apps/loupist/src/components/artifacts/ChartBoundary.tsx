import { Component, type ReactNode } from "react";

/**
 * Keeps a chart engine failure inside its card: the answer, the thread and the
 * rest of the view stay usable when the chart library cannot draw.
 */
export class ChartBoundary extends Component<{ children: ReactNode; fallback: ReactNode }, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  render() {
    return this.state.failed ? this.props.fallback : this.props.children;
  }
}
