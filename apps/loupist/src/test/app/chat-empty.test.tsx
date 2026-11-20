import { headerAction } from "../renderApp";
import { render, screen, waitFor, within } from "@testing-library/react";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { mockAssetId } from "../../bridge/mock/assets";
import userEvent from "@testing-library/user-event";
import { chooseLoupist, mainTab, newChatButton, openApp } from "../renderApp";

const composer = () => screen.getByRole("textbox", { name: "Message Loupist" });

describe("Loupist · New Chat", () => {
  it("returns to the unsent new Chat when switching back to its Loupist", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const create = vi.spyOn(bridge, "chatCreate");
    await user.click(newChatButton());
    await user.type(composer(), "Compare the next earnings report");
    await chooseLoupist(user, "Contrarian Value");
    await screen.findByRole("heading", { name: "Bot" });
    expect(composer()).toHaveValue("");
    await chooseLoupist(user, "Quality Compounder");
    await user.click(newChatButton());
    expect(await screen.findByRole("heading", { name: "Untitled research session", level: 1 })).toBeVisible();
    expect(composer()).toHaveValue("Compare the next earnings report");
    expect(create).not.toHaveBeenCalled();
  });

  it("lands on Bot's greeting, whose questions fill the composer, and a new Chat offers only questions", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const create = vi.spyOn(bridge, "chatCreate");
    render(<App bridge={bridge} />);
    expect(await screen.findByRole("heading", { name: "Bot" })).toBeVisible();
    const greeting = await screen.findByRole("region", { name: "Quality Compounder greets you" });
    await waitFor(() => expect(greeting).toHaveAttribute("aria-busy", "false"));
    // A new user's cards are first steps; with assets followed they are questions.
    const ask = within(within(greeting).getByRole("region", { name: /^(Start here|Suggested questions)$/ })).getAllByRole("button")
      .find((button) => button.dataset.ask === "markets" || button.dataset.ask === "status")!;
    await user.click(ask);
    // The card names the question; the composer gets it as the user's own request.
    expect((composer() as HTMLTextAreaElement).value).toMatch(ask.dataset.ask === "markets" ? /^What matters in markets this week\? List the dates and figures to watch\.$/ : /^Where does .+ stand\? /);
    // The Thread already exists, so writing in it never creates a Chat.
    await user.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByText("Response complete.");
    expect(create).not.toHaveBeenCalled();
    // The first message replaces the greeting; nothing about setup stays under the conversation.
    expect(screen.queryByRole("region", { name: "Quality Compounder greets you" })).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Get set up" })).not.toBeInTheDocument();

    await user.click(newChatButton());
    const questions = await screen.findByRole("region", { name: "You could ask" });
    expect(screen.queryByRole("region", { name: "Quality Compounder greets you" })).not.toBeInTheDocument();
    await user.click(within(questions).getAllByRole("button")[0]!);
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(create).toHaveBeenCalledWith({ loupist_id: "loupist-quality" }));
  });

  it("shows the followed asset's stored price with its date and provider", async () => {
    const user = userEvent.setup();
    const bridge = await openApp();
    const assetId = mockAssetId("NVIDIA");
    bridge.assets.installAsset({ assetId, name: "NVIDIA" });
    bridge.assets.installPrice("loupist-quality", assetId, { price: { value: 162.85, date: "2026-11-13", currency: "USD", provider: "yfinance", sourceUrl: null, dataset: { dataset_id: "chat-empty", revision: 1, digest: "test" } } });
    await user.click(screen.getByRole("button", { name: "Bot" }));
    const greeting = await screen.findByRole("region", { name: "Quality Compounder greets you" });
    expect(within(greeting).getByRole("button", { name: /Where NVIDIA stands/ })).toBeVisible();
    const tile = await within(greeting).findByRole("button", { name: "Open NVIDIA" });
    expect(tile).toHaveTextContent("$162.85");
    expect(tile).toHaveTextContent("As of Nov 13 · Yahoo Finance");
  });

  it("opens model settings and the central Loupist page while preserving the draft", async () => {
    const user = userEvent.setup();
    await openApp();
    await user.type(composer(), "Keep this question");
    await user.click(screen.getByRole("button", { name: /Model settings for this message/ }));
    await user.click(screen.getByRole("menuitem", { name: "Manage AI accounts" }));
    expect(await screen.findByRole("heading", { name: "AI accounts", level: 1 })).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Back" }));
    expect(screen.queryByRole("button", { name: "Chat defaults for this Loupist" })).toBeNull();
    await user.click(headerAction("Loupist settings"));
    expect(await screen.findByRole("textbox", { name: "Name" })).toHaveValue("Quality Compounder");
    await user.click(screen.getByRole("button", { name: "Chat defaults" }));
    expect(await screen.findByRole("switch", { name: "Remember new conversations" })).toBeVisible();
    await user.click(await mainTab(/Chat/));
    expect(composer()).toHaveValue("Keep this question");
    await user.type(composer(), " /");
    expect(screen.queryByRole("option", { name: "Manage skills and connections" })).toBeNull();
  });
});
