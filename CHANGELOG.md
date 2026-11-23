# Changelog

## Unreleased

- Build and run Loupist on Linux x86_64. Install the WebKitGTK 4.1 development libraries, then run `npm run tauri build` in `apps/loupist` to produce a `.deb`.
- The signed download remains macOS 13 or later on Apple Silicon. A Linux package does not save data-connection keys, install updates, register `loupist://` links, or open at login. It does include the Linux Pi and OpenBB programs, so an AI account key is saved in Pi's `auth.json` and a chat can ask the model and read OpenBB data.

## 0.1.5 - 2026-11-23

- Chat again after connecting a ChatGPT or SuperGrok account: a Loupist whose Chat default names only a model no longer refuses every reply.
- Keep chatting when a model goes away: a chat that used a model of an account you disconnected falls back to your default, a Chat default that cannot run right now gives way to the recommended model until it can, and a changed Chat default applies to new replies at once.
- Keep chatting while a research session runs: a reply never holds its conversation while it waits to save to Memory. The next message can be sent at once, and "Saving to Memory", Skip saving and the result now sit on the reply they belong to.
- Keep clicking, typing and using VoiceOver while World builds from many Sources: the window no longer waits for World to finish saving an update.
- Let World pick its model from the AI account you have connected: each Loupist's World model is Recommended by default and follows your accounts and Chat default, and Settings > World building lets you choose another model or return to the recommended one.
- Have World finish ordinary filings without pausing: the default number of model requests per Source is now 32. You see why World paused on a Source and that World is waiting for an AI account when none is connected, each with the setting to open.
- Keep World to what your research is about: a ranking page adds only the names you asked about, and a site's list of similar companies is not recorded as a relationship.
- Find each company once in World: two companies with different tickers are no longer merged under one name, the same listing is no longer added twice, and the next World update repairs names that were mixed up before.
- Read World's relationship names in your language: the common relationships a model names itself ("operates", "files with", "lists on") now have reviewed labels, and any other reads as plain lowercase English.
- Trust What's new's totals: the header, the groups and "new this week" count a name once, by the Source that first added it, with one group per Source however many parts it was read in.
- Trust fewer false warnings on figures: capital spending, buybacks and other outflows a filing prints in brackets match when you state them as amounts, per-share rows and ratios keep their units, a change in percentage points matches a source in basis points, and an amount written with a scale word of the answer's language matches the source's table, in new answers and in ones you already have.
- See a figure whose citation did not resolve marked as having no linked source, instead of looking cited; a citation with a mistyped link is repaired when only one source fits and the number holds.
- Read a company's income statement, balance sheet and cash flow for the last five periods without an SEC contact, and get a clear reason when a market-data request cannot be served: an empty result is reported as empty, a refused argument is named, each tool states what the available providers require, and Activity shows why the step was not used.
- Search for an asset even when a saved search key cannot be read, get a price as soon as you follow a listed asset, and see a saved price with its currency, or "Currency unknown" when neither the price nor the listing states one.
- Read "today" and weekdays in an answer as your own date: your AI provider now receives this Mac's date and UTC offset with each message, and no time zone name or location. A scheduled task's previous run and the cutoff of a dated question use your own time too.
- Find a research session whose first answer you stopped, or that failed, under a title taken from your question.
- Open a cited figure in a wide table with the cell in view and its row label kept beside it.
- Set up SEC EDGAR from a card that asks for a contact email, not a key.
- Turn the built-in weekday briefing on from Schedule at any time, also after you have made tasks of your own.
- Read Private chat's rule under its chip without hovering, and long titles in the activity panel without them running under the time.

## 0.1.4 - 2026-11-23

- See that World is waiting for a model, instead of a spinner, when the model chosen for World is not connected.
- Keep your place in a conversation when the side panel opens and when an answer finishes.
- Find a research session that was cut off by a crash or a forced quit marked as stopped, with Try again.
- Quit without leaving a helper process behind.
- Read market data that needs no key even when a saved key cannot be read: only the connection that needs that key is left out, and Loupist says which.
- Keep the World to the names your research is about: a table that lists many companies with the same few figures no longer adds every row.
- Read What's new by Source: one update from one Source is one row that expands, and a new name shows its new facts on the same row.

## 0.1.3 - 2026-11-22

- Open the Developer ID-signed, Apple-notarized Beta directly from Applications, without approving it in System Settings.
- After updating from 0.1.2, open **Settings > Data connections**, choose **Allow access** and select **Always Allow** in each macOS prompt for your saved keys. Until then, Chat says which connections need permission and scheduled results name the connections they skipped.
- Open Loupist in the menu bar when you log in to your Mac so scheduled research can resume after a restart. Enable it in General settings or from Schedule.
- Find the page of names, facts and relationships under its new name, World (formerly Map).
- Read the Lens as how your Loupist looks at things: its perspective, your principles, its methods and its notes.
- Choose the model for a chat by name. The automatic choice is gone.
- Keep working in other conversations while one waits for your answer. A finished answer is shown as finished while it saves to Memory, and you can skip that save.
- See what a number check compared: the answer's value, the source's value and why they differ.
- Trust fewer false warnings: amounts stated in millions, in another notation of the same currency or with a decline written in words are recognised, in new answers and in ones you already have.
- Open a saved Source at the passage an answer cited, from any of its figures.
- See a reused figure's original publisher and date separately from the day Loupist saved it.
- Read SEC filings directly once your SEC contact is set in Data connections.
- Read a workbook Loupist made beside the chat as tables, and open it in your spreadsheet application.
- Compare series side by side in bar charts. Bars are stacked only when they are parts of one total.
- Follow a scheduled run from its result in Bot to its run in Schedule, and see in the sidebar when a run or World building is in progress.
- Hand research from Bot to a session that opens with the whole request.
- Use the menu bar in your interface language, with Print, Close Window and zoom.
- Open links outside a chat in your browser, which now comes to the front.
- Return to Loupist from a Loupist page after approving an AI account sign-in in your browser.
- Understand a data provider's refusal: a mistyped series or code is reported as an input to correct.
- Read a company's statements without an SEC contact: Loupist uses the other providers it has and says when SEC data needs your contact.
- Follow an asset and see its price at once: a price saved in the reply that follows it is kept, and the receipt names it.
- Let Loupist follow a page's own link to the filing or release it reports on.
- Open a name in World while World is being updated, and find facts about one name under Key facts instead of among its relationships.
- Find Loupist in the Dock by its face: the app icon is now the Loupist character.
- User-added Skills and custom connections are not available in this version. Any you added in 0.1.2 stay on your disk but are not shown or used.

## 0.1.2 - 2026-11-21

- Run Loupist, a local-first financial research app, on macOS 13 or newer on Apple Silicon only.
- Connect your own AI account with a supported sign-in or API key, or look around first without a Loupist account.
- Choose a research template or set a custom focus, then give your Loupist a name and face colour.
- Ask a quick question in Bot or hand research to a separate session and open its result in Bot when the session ends.
- Follow assets separately in each Loupist and let it find a price source for each new asset.
- Read filings, market prices and economic series through read-only data connections.
- Keep research sessions, asset pages and retained evidence in a local folder you can copy and reopen.
- Save a passage you paste into a research session as a Source marked **User-provided**.
- Find saved material across languages, including English filings from a Korean question.
- Open Map and Lens as full pages from the sidebar to trace a claim to its evidence or inspect the history of a research method.
- Teach a principle and see how a later answer applies it alongside methods learned from earlier research.
- Keep your judgments in a Journal and review dated checks against the evidence available then and now.
- Run calculations locally and open the resulting charts, tables and documents in your research session.
- Inspect a checked figure in an answer to see its source or the calculation and inputs behind it.
- Turn on Private chat to skip Memory reads and saves while keeping the conversation history and sending messages to your AI provider.
- Schedule briefings and research while the app is open or in the menu bar, and use Run now in Schedule to run a task on demand.
- Install the Beta signed with Loupist’s own certificate and approve its first launch in System Settings because Apple has not notarized it.
- Install signed updates with your consent.
- Use and contribute to Loupist under the AGPL-3.0-only license.
- Use Loupist for research. It never trades and is not a licensed adviser.
