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

it("loads local Markdown images through the scoped host reader and opens Files", async () => {
  const { screen, fireEvent } = await import("@testing-library/react");
  const { MockLoupistBridge } = await import("../../bridge/mockBridge");
  const { renderWithOpen, destinations } = await import("../../test/open");
  const { WorkspaceFileProvider } = await import("../workspace/WorkspaceFileContext");
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const read = vi.spyOn(bridge, "workspaceRead").mockResolvedValue({ path: "outputs/chart.png", size_bytes: 8, content: null, media_type: "image/png", truncated: false, binary: true, content_sha256: null, image_data_url: "data:image/png;base64,aGVsbG8=" });
  const { open } = renderWithOpen(<WorkspaceFileProvider value={{ bridge, loupistId: "owned" }}><MessageResponse mode="static" workspacePath="/research">{"![Corrected chart](outputs/chart.png)"}</MessageResponse></WorkspaceFileProvider>);
  expect(await screen.findByRole("img", { name: "Corrected chart" })).toHaveAttribute("src", "data:image/png;base64,aGVsbG8=");
  expect(read).toHaveBeenCalledWith({ loupist_id: "owned", path: "outputs/chart.png" });
  fireEvent.click(screen.getByRole("button", { name: "Corrected chart" }));
  expect(destinations(open)).toEqual([{ kind: "file", path: "outputs/chart.png" }]);
});

it("does not read escaped image paths or render an active image format returned by the host", async () => {
  const { screen } = await import("@testing-library/react");
  const { MockLoupistBridge } = await import("../../bridge/mockBridge");
  const { renderWithOpen } = await import("../../test/open");
  const { WorkspaceFileProvider } = await import("../workspace/WorkspaceFileContext");
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const read = vi.spyOn(bridge, "workspaceRead").mockResolvedValue({ path: "outputs/chart.svg", size_bytes: 8, content: null, media_type: "image/svg+xml", truncated: false, binary: true, content_sha256: null, image_data_url: "data:image/svg+xml;base64,PHN2Zz4=" });
  renderWithOpen(<WorkspaceFileProvider value={{ bridge, loupistId: "owned" }}><MessageResponse mode="static" workspacePath="/research">{"![Outside](../private.png)\n\n![Vector](outputs/chart.svg)"}</MessageResponse></WorkspaceFileProvider>);
  await waitFor(() => expect(read).toHaveBeenCalledOnce());
  expect(read).toHaveBeenCalledWith({ loupist_id: "owned", path: "outputs/chart.svg" });
  expect(screen.queryByRole("img", { name: "Vector" })).not.toBeInTheDocument();
  expect(await screen.findByRole("button", { name: "Vector" })).toBeVisible();
});

it("opens only retained artifact citations across answers and clears them when the chat changes", async () => {
  const { screen, fireEvent } = await import("@testing-library/react");
  const { ArtifactReadLinksProvider } = await import("../chat/ArtifactReadLinks");
  const { evidenceFixture } = await import("../../test/evidenceFixtures");
  const { renderWithOpen, destinations } = await import("../../test/open");
  const artifactId = `artifact-${"a".repeat(32)}`;
  const evidence = evidenceFixture({ id: "ev_work", kind: "derived", tool_name: "artifact_read", excerpt: JSON.stringify({ artifact: { id: artifactId, kind: "markdown" } }) });
  const prior = { id: "earlier", role: "assistant" as const, content: "Saved work", created_at: "2026-11-09", status: "complete" as const, evidence_refs: [evidence, evidenceFixture({ id: "ev_web" })] };
  const body = <MessageResponse mode="static">{"[Final note](#evidence-ev_work) · [Earlier source](#evidence-ev_web)"}</MessageResponse>;
  const { open, rerender } = renderWithOpen(<ArtifactReadLinksProvider messages={[prior]}>{body}</ArtifactReadLinksProvider>);
  fireEvent.click(screen.getByRole("link", { name: "Final note" }));
  expect(destinations(open)).toEqual([{ kind: "artifact-link", artifactId }]);
  expect(screen.queryByRole("link", { name: /Earlier source/ })).toBeNull();
  rerender(<ArtifactReadLinksProvider messages={[]}>{body}</ArtifactReadLinksProvider>);
  expect(screen.queryByRole("link", { name: "Final note" })).toBeNull();
});
