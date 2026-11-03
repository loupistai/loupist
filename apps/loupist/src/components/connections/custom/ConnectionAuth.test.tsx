import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../../bridge/mockBridge";
import { ConnectionAuth } from "./ConnectionAuth";

it("submits pre-registered client details as write-only fields and clears the masked secret", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const record = await bridge.customConnectionAddUrl({ url: "https://research.example" });
  record.auth_state = "sign_in_needed";
  const authorize = vi.spyOn(bridge, "customConnectionAuthorize").mockResolvedValue({ ...record, auth_state: "signed_in", pending: false });
  const changed = vi.fn();
  render(<ConnectionAuth bridge={bridge} connection={record} onChanged={changed} />);
  const user = userEvent.setup(); await user.click(screen.getByText("Advanced"));
  await user.type(screen.getByRole("textbox", { name: "Authorization server URL" }), "https://login.example");
  await user.type(screen.getByRole("textbox", { name: "Client ID" }), "research-client");
  const secret = screen.getByLabelText("Client secret (optional)");
  expect(secret).toHaveAttribute("type", "password");
  await user.type(secret, "fixture-client-secret");
  await user.click(screen.getByRole("button", { name: "Authorize client" }));
  expect(authorize).toHaveBeenCalledWith({ connection_id: record.id, issuer: "https://login.example", client_id: "research-client", client_secret: "fixture-client-secret" });
  await waitFor(() => expect(changed).toHaveBeenCalled());
  expect(secret).toHaveValue(""); expect(document.body.textContent).not.toContain("fixture-client-secret");
});

it("cancels the host browser flow when the auth surface closes", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const record = await bridge.customConnectionAddUrl({ url: "https://research.example" }); record.auth_state = "sign_in_needed";
  const signIn = vi.spyOn(bridge, "customConnectionSignIn").mockImplementation(() => new Promise(() => {}));
  const cancel = vi.spyOn(bridge, "customConnectionSignInCancel");
  const view = render(<ConnectionAuth bridge={bridge} connection={record} autoStart onChanged={() => {}} />);
  await waitFor(() => expect(signIn).toHaveBeenCalled());
  view.unmount(); expect(cancel).toHaveBeenCalledWith({ connection_id: record.id });
});
