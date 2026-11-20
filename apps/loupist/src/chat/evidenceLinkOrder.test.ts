import { evidenceLinkStarts } from "./evidenceLinkOrder";

it("counts the answer's evidence links in order outside code, as Rust numbers them", () => {
  // The same answer as number_checks.rs::links_are_read_in_order_outside_code, plus a reference link and an image.
  const content = "Margin was [8.4%](#evidence-ev_a) on [revenue](#evidence-ev_b) [1](#evidence-ev_c).\n\n```\n[9%](#evidence-ev_d)\n```\n\n"
    + "Code `[7%](#evidence-ev_e)`, ![chart](#evidence-ev_f), [a site](https://example.com), [growth][g].\n\n[g]: #evidence-ev_g\n";
  expect(evidenceLinkStarts(content).map((start) => content.slice(start, content.indexOf("]", start) + 1)))
    .toEqual(["[8.4%]", "[revenue]", "[1]", "[growth]"]);
});
