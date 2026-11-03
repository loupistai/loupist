import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { RunDiagnostics } from "./RunDiagnostics";

describe("Run diagnostics", () => {
  it("reveals the selected Loupist logs and copies the host-provided last failed line", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const reveal = vi.spyOn(bridge, "runLogsReveal").mockResolvedValue();
    const line = '{"kind":"chat","status":"failed","failureKind":"provider_timeout"}';
    const lastError = vi.spyOn(bridge, "runLogsLastError").mockResolvedValue(line);
    const write = vi.spyOn(navigator.clipboard, "writeText").mockResolvedValue();
    render(<RunDiagnostics bridge={bridge} loupistId="loupist-quality" />);
    await user.click(screen.getByRole("button", { name: "Reveal in Finder" }));
    expect(reveal).toHaveBeenCalledWith({ loupist_id: "loupist-quality" });
    await user.click(screen.getByRole("button", { name: "Copy last error details" }));
    expect(lastError).toHaveBeenCalledWith({ loupist_id: "loupist-quality" });
    expect(write).toHaveBeenCalledWith(line);
    expect(await screen.findByRole("status")).toHaveTextContent("Last error details copied.");
  });
  it("handles app logs, empty logs, and clipboard failures without copying an invented error", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const lastError = vi.spyOn(bridge, "runLogsLastError").mockResolvedValueOnce(null).mockResolvedValue('{"status":"failed"}');
    vi.spyOn(navigator.clipboard, "writeText").mockRejectedValue(new Error("denied"));
    render(<RunDiagnostics bridge={bridge} loupistId={null} />);
    await user.click(screen.getByRole("button", { name: "Copy last error details" }));
    expect(lastError).toHaveBeenCalledWith({ loupist_id: null });
    expect(await screen.findByRole("status")).toHaveTextContent("No failed runs recorded.");
    await user.click(screen.getByRole("button", { name: "Copy last error details" }));
    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent("Could not copy error details."));
  });
});
