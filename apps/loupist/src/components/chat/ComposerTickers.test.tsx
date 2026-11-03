import { useState } from "react";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TooltipProvider } from "../ui/tooltip";
import { ChatComposer } from "./ChatComposer";
import { ChatMessageCard } from "./ChatMessageCard";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { assetRefs, companyChip, searchTickers, type TickerOption } from "../../chat/assetTickers";
import { composeSubmission, splitAssetTokens } from "../../chat/composerChips";

const apple: TickerOption = { key: "watch:AAPL:NASDAQ", source: "watchlist", label: "Apple", symbol: "AAPL", market: "NASDAQ" };

function Composer({ onSend, tickers = [apple] }: { onSend: (text: string) => void; tickers?: TickerOption[] }) {
  const [prompt, setPrompt] = useState("");
  return <TooltipProvider><ChatComposer
    prompt={prompt} onPromptChange={setPrompt}
    privateChat={false} isRunning={false} error={null} streamAnnouncement=""
    models={[{ id: "model", provider: "test", model: "model", name: "Model", input: ["text"], reasoning: true, context_window: 10000, max_tokens: 1000, thinking_levels: ["medium"], thinking_level_map: {}, run_controls: [], credential_source: "saved" }]} providers={[]} modelSelection={{ model_profile_id: "model", thinking_level: "medium", run_options: {} }} modelAuto onModelResetToAuto={() => {}}
    skills={[{ id: "research", name: "Research", description: "Research a topic" }]} plugins={[]}
    onSearchTickers={async () => tickers}
    onPrivateChatChange={vi.fn()} onModelSelectionChange={vi.fn()}
    onSend={(text) => { onSend(text); return true; }} onFollowUp={vi.fn()} onAbort={vi.fn()}
  /></TooltipProvider>;
}

it("turns $ into an asset chip that sends as $SYMBOL:MARKET with its name and leaves typed text alone", async () => {
  const user = userEvent.setup();
  const onSend = vi.fn();
  render(<Composer onSend={onSend} />);
  const textarea = screen.getByRole("textbox", { name: "Message Loupist" });
  await user.type(textarea, "How is margin? $app");
  await user.click(await screen.findByRole("option", { name: /Apple/ }));
  const tray = screen.getByRole("list", { name: "Added to this message" });
  expect(within(tray).getByText("Apple · AAPL")).toBeVisible();
  expect(textarea).toHaveValue("How is margin? $Apple · AAPL ");
  // Removing the chip keeps what the user typed.
  await user.click(within(tray).getByRole("button", { name: "Remove Apple · AAPL" }));
  expect(textarea).toHaveValue("How is margin?  ");
  await user.type(textarea, "$app");
  await user.click(await screen.findByRole("option", { name: /Apple/ }));
  await user.keyboard("{Enter}");
  await waitFor(() => expect(onSend).toHaveBeenCalled());
  expect(onSend.mock.calls[0]![0]).toMatch(/^How is margin\? +\$AAPL:NASDAQ \(Apple\)$/u);
});

it("marks a company's shares in the company color and other instruments in the Map's asset color, from menu to sent bubble", async () => {
  const user = userEvent.setup();
  const gold: TickerOption = { key: "watch:GC:COMEX", source: "watchlist", label: "Gold", symbol: "GC", market: "COMEX" };
  const bitcoin: TickerOption = { key: "watch:BTC:UPBIT", source: "watchlist", label: "Bitcoin", symbol: "BTC", market: "UPBIT" };
  render(<Composer onSend={vi.fn()} tickers={[apple, gold, bitcoin]} />);
  await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "$");
  const mark = (element: Element) => element.querySelector(".composer-chip-marker")?.getAttribute("data-kind");
  expect(mark(await screen.findByRole("option", { name: /Apple/ }))).toBe("company");
  expect(mark(screen.getByRole("option", { name: /Gold/ }))).toBe("asset");
  expect(mark(screen.getByRole("option", { name: /Bitcoin/ }))).toBe("asset");
  await user.click(screen.getByRole("option", { name: /Gold/ }));
  expect(document.querySelector('.composer-prompt-highlight [data-inline-reference="asset"]')).toHaveTextContent("$Gold · GC");
  // The sent bubble keeps the marker its chip had.
  const sent = render(<TooltipProvider><ChatMessageCard message={{ id: "question", role: "user", status: "complete", content: "$GC:COMEX (Gold) $AAPL:NASDAQ (Apple) What moved?", created_at: "2026-10-10T00:00:00Z" }}
    loupistId="loupist-a" chatId="chat-a" loupistName="Loupist" onOpenMemory={vi.fn()} onOpenMemoryPage={vi.fn()} onOpenArtifact={vi.fn()} onOpenLink={vi.fn()} onReadAttachment={vi.fn()} /></TooltipProvider>);
  expect(mark(within(sent.container).getByTitle("GC:COMEX"))).toBe("asset");
  expect(mark(within(sent.container).getByTitle("AAPL:NASDAQ"))).toBe("company");
});

it("offers the finance commands, then Skills, under /", async () => {
  const user = userEvent.setup();
  render(<Composer onSend={vi.fn()} />);
  await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "/");
  const names = screen.getAllByRole("option").map((option) => option.textContent ?? "");
  expect(names.findIndex((name) => name.includes("/brief"))).toBeLessThan(names.findIndex((name) => name.includes("Research")));
  await user.click(screen.getByRole("option", { name: /\/brief/ }));
  // A command starts the sentence and opens the asset menu right away.
  expect(screen.getByRole("textbox", { name: "Message Loupist" })).toHaveValue("Give me a quick briefing on $");
  expect(await screen.findByRole("option", { name: /Apple/ })).toBeVisible();
});

describe("asset tickers", () => {
  it("serializes a chip with its display name and reads tokens back from a question", () => {
    const chip = companyChip(apple);
    expect(composeSubmission("Margin?", [chip])).toBe("$AAPL:NASDAQ (Apple)\n\nMargin?");
    expect(assetRefs("$AAPL:NASDAQ (Apple) and $NVDA, not $research")).toEqual([{ symbol: "AAPL", market: "NASDAQ", name: "Apple" }, { symbol: "NVDA", market: undefined, name: undefined }]);
    // A sent bubble shows the chip, never its serialization; prose like "$5" stays text.
    expect(splitAssetTokens("$AAPL:NASDAQ (Apple)\n\nMargin? It cost $5.")).toEqual([
      { type: "asset", symbol: "AAPL", market: "NASDAQ", name: "Apple" },
      { type: "text", value: " Margin? It cost $5." },
    ]);
  });

  it("lists what the user follows first, then the Map, then a typed symbol", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    await bridge.watchlistAdd({ symbol: "AAPL", market: "NASDAQ", displayName: "Apple" });
    const rows = await searchTickers(bridge, "loupist-quality", "nvda", "en");
    expect(rows.at(-1)).toMatchObject({ source: "free", symbol: "NVDA" });
    const followed = await searchTickers(bridge, "loupist-quality", "app", "en");
    expect(followed[0]).toMatchObject({ source: "watchlist", label: "Apple", symbol: "AAPL", market: "NASDAQ" });
  });
});
