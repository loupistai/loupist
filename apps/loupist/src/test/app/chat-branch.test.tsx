import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { openApp } from "../renderApp";

const composer = () => screen.getByRole("textbox", { name: "Message Loupist" });
const userMessages = () => [...document.querySelectorAll<HTMLElement>("article.message.user")];
const shown = (text: string) => userMessages().some((article) => article.textContent?.includes(text));

describe("Loupist · Edit forks (X18)", () => {
  it("edits a sent message into a new version and switches back to the earlier one", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const branch = vi.spyOn(bridge, "chatBranch");
    await user.type(composer(), "Why did the margin fall?");
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(shown("Why did the margin fall?")).toBe(true));
    await waitFor(() => expect(screen.queryByRole("button", { name: "Stop response" })).not.toBeInTheDocument(), { timeout: 30_000 });

    const original = userMessages().find((article) => article.textContent?.includes("Why did the margin fall?"))!;
    await user.click(within(original).getByRole("button", { name: "Edit message" }));
    expect(screen.getByText("Editing your message. Sending keeps the earlier version.")).toBeVisible();
    await user.clear(composer());
    await user.type(composer(), "Is the margin decline temporary?");
    await user.click(screen.getByRole("button", { name: "Send" }));

    await waitFor(() => expect(branch).toHaveBeenCalledWith(expect.objectContaining({ message_id: expect.any(String) })));
    await waitFor(() => expect(shown("Is the margin decline temporary?")).toBe(true));
    // The original message is set aside as an earlier version, not shown.
    expect(shown("Why did the margin fall?")).toBe(false);
    await waitFor(() => expect(screen.queryByRole("button", { name: "Stop response" })).not.toBeInTheDocument(), { timeout: 30_000 });

    const edited = userMessages().find((article) => article.textContent?.includes("Is the margin decline temporary?"))!;
    await user.click(await within(edited).findByRole("button", { name: "Earlier version" }));
    await waitFor(() => expect(shown("Why did the margin fall?")).toBe(true));
    expect(shown("Is the margin decline temporary?")).toBe(false);
    const restored = userMessages().find((article) => article.textContent?.includes("Why did the margin fall?"))!;
    expect(within(restored).getByRole("button", { name: "Later version" })).toBeVisible();
    // Two full mock replies stream in this test; under a loaded suite that takes longer than the default.
  }, 60_000);
});
