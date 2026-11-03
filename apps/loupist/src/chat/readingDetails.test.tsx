import { fireEvent, render, screen } from "@testing-library/react";
import { MessageResponse } from "@/components/ai-elements/message-response";
import type { ChatMessage } from "../types";
import { retainedEvidenceFor } from "./links";
import { chatSeenAt, dayLabel, dividersBefore, firstNewSince, markChatSeen } from "./dividers";
import { answerOutline } from "./outline";
import { AnswerOutline } from "../components/chat/AnswerOutline";
import { ChatDivider } from "../components/chat/ChatDivider";

const message = (id: string, role: ChatMessage["role"], createdAt: string, extra: Partial<ChatMessage> = {}): ChatMessage =>
  ({ id, role, content: "", created_at: createdAt, status: "complete", ...extra });

describe("links in answers (X4)", () => {
  it("opens the panel's browser on click and the system browser on Cmd-click", async () => {
    const onOpenLink = vi.fn();
    render(<MessageResponse onOpenLink={onOpenLink}>{"Read [the report](https://hanul.example/q3)."}</MessageResponse>);
    const link = await screen.findByRole("link", { name: /the report/ });
    fireEvent.click(link);
    await vi.waitFor(() => expect(onOpenLink).toHaveBeenLastCalledWith("https://hanul.example/q3"));
    fireEvent.click(link, { metaKey: true });
    await vi.waitFor(() => expect(onOpenLink).toHaveBeenLastCalledWith("https://hanul.example/q3", { external: true }));
  });

  it("finds the retained copy of a linked page, ignoring the fragment and a trailing slash", () => {
    const evidence = { id: "e1", title: "Q3", kind: "original" as const, url: "https://hanul.example/q3/", tool_name: "web_fetch", retrieved_at: "", excerpt: "", excerpt_truncated: false, content_sha256: "a".repeat(64), warnings: [] };
    const discovered = { ...evidence, id: "e2", kind: "discovery" as const, url: "https://other.example/" };
    const messages = [message("a", "assistant", "2026-10-28T00:00:00Z", { evidence_refs: [evidence, discovered] })];
    expect(retainedEvidenceFor(messages, "https://hanul.example/q3#margin")?.id).toBe("e1");
    // A search result was never kept, so it still opens as a live page.
    expect(retainedEvidenceFor(messages, "https://other.example/")).toBeUndefined();
  });
});

describe("dividers (X5)", () => {
  afterEach(() => localStorage.clear());

  it("marks a change of day and the first answer that finished while the chat was out of sight", () => {
    const now = Date.parse("2026-10-28T15:00:00");
    const messages = [
      message("q1", "user", "2026-10-26T09:00:00"),
      message("a1", "assistant", "2026-10-26T09:01:00"),
      message("q2", "user", "2026-10-27T10:00:00"),
      message("a2", "assistant", "2026-10-28T08:00:00"),
    ];
    // A conversation over several days names its first day too.
    expect(dividersBefore(messages, 0, undefined, now)).toEqual([{ kind: "day", label: "Oct 26" }]);
    expect(dividersBefore(messages, 1, undefined, now)).toEqual([]);
    expect(dividersBefore(messages, 2, undefined, now)).toEqual([{ kind: "day", label: "Yesterday" }]);
    expect(dividersBefore(messages, 3, "a2", now)).toEqual([{ kind: "new", day: "Today" }]);
    expect(dayLabel(Date.parse("2026-10-20T08:00:00"), now)).toBe("Oct 20");

    // Nothing is new on a first visit; after one, answers finished later are.
    expect(firstNewSince(messages, chatSeenAt("chat"))).toBeUndefined();
    markChatSeen("chat", Date.parse("2026-10-27T12:00:00"));
    expect(firstNewSince(messages, chatSeenAt("chat"))).toBe("a2");
  });

  it("labels the divider for readers and assistive technology", () => {
    render(<ChatDivider divider={{ kind: "new" }} />);
    expect(screen.getByRole("separator", { name: "New since you left" })).toBeVisible();
  });
});

describe("outline (X16)", () => {
  const long = ["## Revenue", "Grew 11%.", "## Margin", "Fell to 14.8%.", "```", "## not a heading", "```", "### Costs", "Qualification costs."].join("\n");

  it("lists H2 and H3 headings of long or sectioned answers, outside code", () => {
    expect(answerOutline(long)).toEqual([{ level: 2, text: "Revenue" }, { level: 2, text: "Margin" }, { level: 3, text: "Costs" }]);
    expect(answerOutline("## Only one\nShort answer.")).toEqual([]);
    expect(answerOutline(`## Only one\n${"word ".repeat(601)}`)).toEqual([{ level: 2, text: "Only one" }]);
  });

  it("jumps to the heading in the rendered answer", () => {
    render(<article data-message-id="a1">
      <AnswerOutline messageId="a1" markdown={long} />
      <div className="message-content"><h2>Revenue</h2><h2>Margin</h2><h3>Costs</h3></div>
    </article>);
    fireEvent.click(screen.getByText("Outline"));
    fireEvent.click(screen.getByRole("button", { name: "Margin" }));
    // The heading takes focus, so keyboard and screen-reader users land where the eye does.
    expect(document.activeElement?.tagName).toBe("H2");
    expect(document.activeElement).toHaveTextContent("Margin");
  });
});
