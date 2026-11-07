import { render, waitFor } from "@testing-library/react";
import { MessageResponse } from "./message-response";

it("renders a plugin in place when it arrives and warm for the next answer", async () => {
  const text = "The intro paragraph.\n\n$$x^2$$";
  const first = render(<MessageResponse mode="static">{text}</MessageResponse>);
  const paragraph = first.container.querySelector("p");
  expect(paragraph).toHaveTextContent("The intro paragraph.");
  expect(first.container.querySelector(".katex")).toBeNull();
  await waitFor(() => expect(first.container.querySelector(".katex")).not.toBeNull());
  expect(first.container.querySelector("p")).toBe(paragraph);

  const second = render(<MessageResponse mode="static">{text}</MessageResponse>);
  expect(second.container.querySelector(".katex")).not.toBeNull();
});
