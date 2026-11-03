import { cleanup, render, waitFor } from "@testing-library/react";
import { MessageResponse } from "./message-response";

it("keeps untrusted math from loading external images even with inherited trust", async () => {
  const warmup = render(<MessageResponse mode="static">{"$$x^2$$"}</MessageResponse>);
  await waitFor(() => expect(warmup.container.querySelector(".katex math")).not.toBeNull());
  cleanup();
  const previous = Object.getOwnPropertyDescriptor(Object.prototype, "trust");
  Object.defineProperty(Object.prototype, "trust", { value: true, writable: true, configurable: true });
  try {
    const view = render(<MessageResponse mode="static">{String.raw`$$\includegraphics{https://security.invalid/probe.png}$$`}</MessageResponse>);
    await waitFor(() => expect(view.container.querySelector(".katex")).not.toBeNull());
    expect(view.container.querySelector('img[src*="security.invalid"]')).toBeNull();
  } finally {
    if (previous) Object.defineProperty(Object.prototype, "trust", previous);
    else Reflect.deleteProperty(Object.prototype, "trust");
    cleanup();
  }
});
