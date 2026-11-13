import { headerAction } from "../renderApp";
import { render, screen, waitFor, within } from "@testing-library/react";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
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

  it("lands on the home Thread with the morning message, and a new Chat offers only questions", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const create = vi.spyOn(bridge, "chatCreate");
    render(<App bridge={bridge} />);
    expect(await screen.findByRole("heading", { name: "Bot" })).toBeVisible();
    const message = await screen.findByRole("article", { name: "Message from Quality Compounder" });
    // The first visit has no "since"; it counts what there is to look at.
    expect(await within(message).findByText(/^(One thing|\d+ things) to look at\.$/)).toBeVisible();
    await user.click(within(screen.getByRole("region", { name: "You could ask" })).getByRole("button", { name: "What have you learned so far?" }));
    expect(composer()).toHaveValue("What have you learned so far?");
    // The Thread already exists, so writing in it never creates a Chat.
    await user.click(screen.getByRole("button", { name: "Send" }));
    await screen.findByText("Response complete.");
    expect(create).not.toHaveBeenCalled();
    // The first message replaces the morning message; the setup steps still open stay below the conversation.
    expect(screen.queryByText(/^(One thing|\d+ things) to look at\.$/)).not.toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "You could ask" })).not.toBeInTheDocument();
    expect(within(screen.getByRole("article", { name: "Message from Quality Compounder" })).getByRole("region", { name: "Get set up" })).toBeVisible();

    await user.click(newChatButton());
    const questions = await screen.findByRole("region", { name: "You could ask" });
    expect(screen.queryByRole("article", { name: "Message from Quality Compounder" })).not.toBeInTheDocument();
    await user.click(within(questions).getAllByRole("button")[0]!);
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(create).toHaveBeenCalledWith({ loupist_id: "loupist-quality" }));
  });

  it("tells what changed since the last visit and opens names on the Map and checks in the Journal", async () => {
    const user = userEvent.setup();
    localStorage.setItem("loupist.loupistVisit.v1", JSON.stringify({ "loupist-quality": { lastSeenAtMs: Date.now() - 86_400_000 } }));
    const bridge = await openApp();
    const growth = vi.spyOn(bridge, "knowledgeGrowthEvents").mockResolvedValue({ nextCursor: null, events: [{
      eventId: "event-apple", loupistId: "loupist-quality", atMs: Date.now(), kind: "entity_added", subjectId: "map-entity-00", recordId: null,
      originChatId: null, originMessageId: null, actor: "loupist", summary: "", beforeSummary: null, afterSummary: null,
    }] });
    await user.click(screen.getByRole("button", { name: "Bot" }));
    const message = await screen.findByRole("article", { name: "Message from Quality Compounder" });
    expect(await within(message).findByText(/^2 things since /)).toBeVisible();
    expect(growth).toHaveBeenCalledWith(expect.objectContaining({ loupistId: "loupist-quality", sinceMs: expect.any(Number) }));
    expect(within(message).getByText("+1 on the map:")).toBeVisible();
    expect(within(screen.getByRole("region", { name: "You could ask" })).getAllByRole("button")[0]).toHaveTextContent(/^What changed for Apple since /);

    await user.click(within(message).getByRole("button", { name: "Review this check" }));
    const workspace = await screen.findByRole("complementary", { name: "Chat workspace panel" });
    // The entry's heading and its Then · Now both name the judgment.
    expect((await within(workspace).findAllByText(/Treat the dependency as a reported protocol fact/))[0]).toBeVisible();

    await user.click(within(message).getByRole("button", { name: "Apple" }));
    expect(await screen.findByRole("complementary", { name: "Apple" })).toBeVisible();
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
