# Changelog

## 0.1.8 - 2026-11-28

**Memory upgrade.** Loupist 0.1.8 upgrades Memory storage to Knowledge 12 and saves a backup beside each database before upgrading it. Loupist 0.1.7 cannot open a profile that 0.1.8 has opened.

- See when each fact in World held: a fact keeps the dates it was true for, the day it was published and the day your Loupist learned it, and reads Current, Ended, Disputed or Withdrawn.
- Have World end a fact when a later Source replaces it, mark two facts that cannot both be true as a disagreement, and make an ended fact current again when a later Source states it again, instead of keeping every statement as if it still held.
- Open a fact's History to see the Sources that created it, supported it and ended it, and to open the fact that replaced it.
- Open History on a name in World to read, in order of publication, what was added about it, what ended and where Sources disagree.
- Turn on Fact changes for an asset you follow: the app checks every hour, with no AI, whether a fact about it ended or was contradicted or a Lens section went out of date, and when one did the Bot reports what was stated before and now, with the Sources and their dates. Only changes that arrive after you turn it on are reported.
- Ask the Bot for a fact check that reports only when a figure moved lower, or only when it moved higher; a change whose figures cannot be compared is still reported, and says so.
- Trust the figures World keeps: each recognized amount, percentage and per-share figure in a fact is checked against the passage it came from and the whole Source before World keeps it. A fact whose figure differs is left out, and the same amount in another unit (1,234억 원 and KRW 123.4 billion) counts as a match. Unrecognized number formats are left unchecked.
- See what a figure check did: the Library row and the Source reader say how many facts were left out and show the figures as stated, and a fact whose figure was not found in its passage says so in its inspector and opens the Source at that passage.
- Keep a wrong figure out of Lens: a fact section whose amount differs from the text it cites is not filed.
- Set up a learning program: a research brief your Loupist runs on a schedule, starting each run from the last one, with a request budget, a choice of what it keeps (Sources, Lens, asset reads) and where it reports (Bot, the next briefing or only Schedule). The Bot proposes one on a card, and nothing runs until you turn it on.
- Start a learning program from a template: Track daily, Theme, Company filings, Economic series, Question or Market.
- Turn on Track daily on an asset's page to have your Loupist read what is new about it each weekday at 06:30 and keep its Lens page, thesis read and price read current.
- Ask for a learning program where you are: Learn about this on a name in World, Keep this page current on a Lens page, or a request in a research session, which is handed to the Bot.
- Edit a learning program in Schedule without asking the Bot: its title, what to learn, up to eight subjects, when it runs, its depth, its sources and what it keeps. Changes apply from the next run.
- Undo what a learning program wrote to Lens: Undo this run on one run, or Undo all runs after a count of what goes back. A section changed since is kept, and Sources stay in the Library.
- See in Lens which sections a learning program wrote: each carries the date and the program, and opens that run in Schedule.
- See what each learning run used and why one waits: its requests and outcome, a run stopped at its request limit, and a program waiting for another learning run or for tomorrow's allowance.
- Set how much learning a Loupist may do in its settings: the requests a day for learning runs (132 by default, 0 turns them off) and how many programs can be on at once (20 by default).
- Read World as of a date from its toolbar: the map, search, both inspectors and History show only what was published by that day, with a line saying how many later facts are hidden.
- Have a long table read into World in full: it is read in shorter runs of rows, each with the table's header and unit line repeated so a continued part is read as part of the same table, where one long table could time out.
- Use the deep-research Skill for a question that rests on many sources: your Loupist writes a plan for you to approve, keeps its progress in a file so a stopped reply can resume, looks for evidence against its conclusion and ends in a cited report.
- Ask in Chat what changed in World since a date, or what a company's latest report changed against the one before.
- See on an asset's page what World learned since your last read: facts that ended, facts added, disagreements and Lens sections affected, since the read, over 30 days or since a date you pick.
- See on an asset's page what its latest report changed against the one before: what it ended, what is new, where Sources disagree, and what it did not repeat, which is not called ended.
- See when a Lens section rests on a fact that changed: the section reads Superseded with the date and the Source, or Sources disagree, the page counts such sections, and a section reads Current again when its fact is stated again.
- Tell Loupist a World fact is wrong from its inspector: the fact, its dates and its Sources are quoted into a message for you to finish, and nothing is sent until you send it.
- Read World's counters as names, new names this week and disagreements, with ended facts and superseded Lens sections listed in what is new.
- Have one company stay one name in World: a bare KRX code and its .KS form are the same listing, share classes such as BRK.A and BRK.B stay apart, and when two names are joined your connections, Journal entries and Lens links follow.
- Have methods learned from subjects other than companies, such as an economic series or a theme, and read in Lens why method learning is waiting: for a second case, for Sources still to be read, or for an allowance.
- Have the preferences you gave the Bot apply in research sessions too, when Memory is on; Forget removes one from the next message.
- Ask the Bot a quick question about a figure in a saved Source: it now searches what your Loupist has saved when no data connection answers, instead of saying the figure could not be verified.
- Keep outside text from changing your standing instructions: once a reply has read a web page or other third-party text, it cannot save a principle, a preference for the Bot or a learning program with its own brief in that turn. A bundled check, or a learning template that only names its subject, is still offered on a card, because you turn it on yourself.
- Find everything the Bot is waiting on you for in one Needs you list, and open Standing as a panel at the window's edge.
- Read a met check's explanation on its Bot card with its formatting, instead of raw Markdown marks.
- See each followed asset's price, its change from the previous close with that close's date, and whether a check, a watch or Track daily is on, in its Following row.
- Open Ahead in Following to see the next 60 days: results follow-ups with the results day, Journal checks that are due, overdue ones first, and the latest briefing's Looking ahead.
- Confirm before you stop following an asset, and choose there whether to pause its Track daily too.
- Find Schedule in sections, Briefings, Checks, Scheduled research and Learning, each with its count.
- Read a check in Schedule without opening its code: when it is met and what it then tells you, what it reads, the assets by name, every recent evaluation including those that started no run, and each day its test run would have been met.
- Keep automatic work inside a daily allowance: 400 model requests a day for background work, half of them for World, and 300 for reading Sources saved in Chat. Settings › Usage shows both with a warning at 80%, a task or Source that is held says "Waiting for tomorrow's allowance", Run now starts a held task after you confirm, and Read now reads a held Source at once.
- See why background work stopped when an AI plan reached its limit or ran out of credit: the task or Source says so and resumes the next day.
- Print one answer or save it as a PDF, or copy it as Markdown with its citations as numbered footnotes, from the answer's menu.
- Print an asset's page or save it as a PDF, and see "Getting price history…" on a new asset's page instead of an empty chart.
- The Helpful, Not helpful and Suggest a correction controls are gone from answers; say what was wrong in a message.
- Keep the side panel as each chat left it: a chat with nothing to show starts with it closed, switching between a research session and the Bot no longer carries it across, and a Source you left open is open again when you return.
- See a table saved as a Source previewed in the Library by its row count and the period it covers.
- See why a Source stopped being read into World: interrupted, timed out, a model reply that could not be used, an AI provider error, or a World that is full, with Try again where it can help.
- Keep World working when a model reply cannot be read: only that Source stops, where the whole World could be set aside until recovery.
- See on each saved script what it reads, which task uses it and its last result.
- Find data connections in three groups, Works without a key, Filings and market data, and More providers, with the markets you follow first.
- See an OpenRouter key's remaining credit and when it resets on its account card.
- See what a check cost in Settings › Usage: its last check, checks today, data reads, how often it was met and the model requests it started.
- Pick World's fast model for routine checks from the model menu instead of typing its name, or leave it the same as the processing model.
- Use dropdowns that match the app's menus in Settings, Schedule, an asset's page and the Journal, and read quiet hours as times in your interface language.
- See what an update is waiting on before it installs, and see that line go when the work ends.
- Edit or archive a principle from its reader.
- Read how often a principle was used as a count of the answers that cited or read it, not of every answer it was offered to.
- Start a Loupist from the Real assets template, and find the portfolio template renamed Watchlist research, which says it follows filings, results and your reasons and does not track holdings or returns.
- Read Loupist, Bot, Schedule and Digest as one Korean term each across the interface.
- Fixed: 0.1.7 could fail to save everything a long reply kept. A reply that kept more than 18 things, for example 16 Sources and three Lens pages, lost all of them behind "Not saved to Memory: saving failed"; 0.1.8 saves the whole reply. On 0.1.7, keep the Sources in one message and file to Lens in the next.
- Fixed: a save whose writes had all finished on a busy Mac was thrown away because it passed its time budget; it is now kept.
- See why a Memory save failed and what to do next: the notice names the cause (something refused, Loupist busy saving other work, the disk, or a conversation that changed meanwhile) and says that whatever the answer claims it saved was not kept.
- Keep the rest of a reply's save when one item is refused: the notice reads "Partly saved to Memory" and lists what was not kept, and a save that found Loupist busy is tried again, up to three times.
- Fixed: the daily allowance counts each World request on the day it was made, also across midnight, keeps counting a run that ended in a crash, and treats an AI plan limit reached in Chat as a reason to hold background work on that account for the day.
- Fixed: a research session started while another one was refreshing no longer disappears from the list.
- Fixed: a one-year price chart opens on the whole year instead of its last month with a clipped date label.
- Fixed: the Bot's list of scheduled tasks and the next briefing's estimated time use your time zone, not UTC, when macOS reports a zone without a name.
- Fixed: a Lens page taller than the side panel scrolls, a Lens link stays on the line with the punctuation after it, and the model menu no longer shows the page through it.

How this release was measured. We ran the release suite on the final build with GPT-6.2 Sol (medium effort, fast tier) as the agent and with xAI Grok 4.7 as a second model family. Sol passed the required checks of all ten release units and the security suite (62 of 62 checks over two trials). Grok passed the security suite (31 of 31) and six of the ten release units: two runs stopped at the suite's per-turn input limit, one cited a saved Source without its exact passage, and one did not keep a Bot memory entry. World extraction was measured with GPT-6 Luna (high effort): no false ending in the six control scenarios, 17 of 18 temporal scenarios on the first trial with the remaining one passing two of its three trials, and the ontology checks passed. Rubric items were judged by Claude Opus 5.5 with GPT-6.2 Astra as a second judge; they agreed on all 17 items, a small sample, and neither judge was calibrated against human labels.

### Known limits

- A corrected figure can appear as a disagreement between Sources while the earlier fact stays open.
- World reads long tables in sections and shows when a section timed out. A very wide table can still time out in part.
- Each Source has its own limit on World model requests. A Source that reaches the limit pauses and shows **Try again** to give it another allowance.

### Planned follow-ups

- A dedicated Deep Research mode is deferred. The deep-research Skill is included.
- Stored corrections to World facts, entity matches and Lens sections are deferred. **Tell Loupist this is wrong** drafts a Chat message from a fact and its evidence.
- Structured figures and the metric-by-period table are deferred. Source figure checks and the comparison between the latest report and the previous one are included.
- Normalized World storage is deferred. World still offers fact History and an as-of view.
- Resuming a failed Memory save after a restart and letting saves proceed independently across conversations are deferred. This release retries busy saves and reports what a partial save kept.

## 0.1.7 - 2026-11-25

- Pick a model for one chat without changing another: each chat keeps its own choice, Default included, and a new chat starts on the model your last message ran on.
- Reopen a saved chat after a restart on its own model, reasoning and run options, instead of the defaults.
- See what your AI accounts were used for in Settings › Usage: totals per account and per kind of work, and each account's last seven days, counted on this Mac.
- See in an answer's details how many model requests it made and, for an account connected with an API key, what they are estimated to cost.
- Send a message right after adding a file: Send waits while the file is still being read ("Reading 1 file…") and sends it with the message, instead of sending without it and leaving it for the next one.
- Close the + menu by choosing Files… or Paste image: it no longer stays open behind the file dialog or the Paste confirmation.
- Paste images straight from the + menu on macOS.
- Read the notes written while an answer is being worked out in the language of your question, also in a Loupist whose name or focus is in another language.
- Have a question that a saved Source answers open that Source before any web search, and have a connected official source such as OpenDART read and cited ahead of a copy of the same filing saved from another site.
- Save a company's full statements or a long price history as a Source: a result too large to show whole is kept with every line item, where it was refused or saved only as a narrower read. A result over 8 MiB is refused when you save it, with a narrower read to try.
- See a research session you just started named from its question in the waiting save line while its first answer runs, instead of "Untitled research session".
- Keep market data working when one request returns too much: only that request is refused, with its size and a shorter range or narrower selection to ask for, and the petroleum status report and the Svensson yield curve are offered again.
- See a data connection's reply too large to read reported as such, with a narrower request or another source to try, instead of "the provider rejected the request".
- Read a DeFi protocol with a long history from DefiLlama, Curve DEX among them, instead of being told the provider rejected the request.
- Follow "Samsung Electronics" and find its KRX listing (005930) first, instead of only its bonds and overseas trading lines.
- Read KRX daily prices across a trading halt: the halted days are named apart from holidays instead of failing the whole read, and a date before 2010-01-04, where KRX's data starts, is refused with a pointer to another source instead of returning no data.
- Keep Korea Investment reads within its per-second limit across chats and continuation pages, and see one that still came too fast reported as a rate limit to wait out, instead of a failed read.
- See why a Korea Investment read failed: an expired session is renewed once on its own, a refused app key or secret comes with the setup card to fix it, and a missing API permission, a wrong market code or a value of the wrong length is named.
- Use a Korea Investment demo account without dead ends: only the reads demo serves are offered, and a real-only read is refused before any request.
- Find a Korea Investment symbol by its Korean or English name or its code, across KIS's domestic and overseas markets, with the market or exchange code its reads need.
- Read KSD's dividend calendar through a real Korea Investment connection: record and payment dates, per-share cash and dividend rates for a date range, for one company or every issue.
- Read the dates on a chart's time axis: a year of daily prices in the side panel shows one label a quarter instead of twelve that run together, and a wide chart keeps one a month.
- Find Schedule laid out like the other pages: centred, with Off beside Turn on in the briefing's row and the footer right after the list.
- Read the Edit menu in your interface language: macOS's AutoFill, Start Dictation and Emoji & Symbols items, which showed in English, are no longer added to it.
- Ask the Bot to watch what you follow and hear from it only when something matters: material filings and results, where the measures behind your lines stand, or a company's next results. A watch reports on its first check so you see what it reads, then stays quiet until its condition is met; turn one on from the Bot or from the asset's page.
- See the Bot's standing work above its conversation: what is checking now, how many checks found nothing, and a task paused after repeated failures. Results that arrived while you were away are grouped together.
- Read the evidence under each Bot result: which of the figures it cites were checked against their source, and the run behind it, with Ask about this to continue from there.
- Turn on a task the Bot suggests from its card: nothing runs until you do, and declining one can be undone while you are still in the Bot.
- Choose how each kind of Bot update reaches you in Settings › Notifications: now, with the next update, or in Bot only, with quiet hours, a daily cap, a switch for showing details, and a link to the macOS notification settings. Line crossings, filings watches and briefings notify now by default; other results wait, with quiet hours from 22:00 to 07:00 and at most six a day. Each watch can keep its own setting.
- Bot sends no notification for its conversation while you are reading it, and a watch drops an asset you stop following.
- Let Python read your connected data and saved Sources through Loupist, so work over many rows is computed in code instead of copied into the answer. Show the math on a calculated figure opens the code and the reads behind it, and Run again on the same data repeats them.
- Keep scripts that work in Library › Scripts: every version stays, and you can edit, restore, test, copy or export one as a .py file. A version the agent saved can be undone from its answer.
- Have the Bot offer a watch or a weekly review on an asset you follow when it sees a reason, such as results due within two weeks. Turn on creates that task and nothing else, and Not now keeps the same offer from coming back. A scheduled run can also leave you one question with a suggested answer instead of waiting for you.
- Get an evening briefing beside the morning one: what happened today and what is due the next trading day, with its own time and its own switch. The next briefing also covers what your watches checked without telling you.
- See under Needs you a watch that has stayed quiet for six checks or notified you four times in a week, so you can change it or have it notify more quietly.
- Open Standing from the Bot's header to see everything it keeps in mind, edit or forget an entry without a model call, and see what each scheduled task used in the last 30 days. Only the preferences you gave it reach briefings and scheduled runs.
- Have the Bot set up a check the app runs by itself, such as a price moving more than 5% from one close to the next or crossing a level you name: it uses no AI until the check is met, then the Bot reads what happened and reports it, at most three times a day for one check.
- See a proposed check before it runs: what it checks, what it reads and how it did on recent days. Turn on runs exactly the code you approved, and a later change to it waits for your OK while the approved version keeps running.
- Get a met check's card in the Bot at once, filled in by the Bot's report as soon as it can run. A check whose code fails three times in a row stops and says so.
- Turn on price moves from an asset's page, and see each check's reads and results in Schedule and Usage.

## 0.1.6 - 2026-11-24

- Attach files and images to a message again: the composer shows each image's thumbnail and sends the files, where every signed build until now refused to send them ("could not read or send the attached message") and showed no preview.
- Paste an image with ⌘V: a screenshot, an image copied from a browser or Preview, several images at once, or files copied in Finder attach to the message under the same limits as the + menu, each pasted image under its own name ("Pasted image 2026-11-23 21.04.png"). Copying a spreadsheet range or a web page still pastes its text.
- See what ⌘A selected in the message box: the selection now shows in the system selection colour with the text readable, instead of greying the text out so it looked as if nothing was selected.
- Get a Korean company's statements from OpenDART: statements for every company and period OpenDART has are read again instead of failing as an invalid response, and a company that files only separate statements returns those, marked separate.
- See a Korean company's statement figures cited to OpenDART and checked: a statements read too large to show whole now gives the AI every line item to read and cite, and the number checks no longer miss rates printed bare in a DART results table, a tax benefit on an expense (income) row, or count a row number as a figure.
- Read a long OpenDART filing without paging through it: the first page gives its outline with the page of each section, a search finds a word or figure anywhere in the filing with its page, each page arrives whole, and a page you ask to keep is saved as a Source.
- Name a Korean company by its ticker, its Korean or English name or its corp code in any OpenDART read: Loupist downloads OpenDART's company list once, keeps it on your Mac and refreshes it in the background, so a lookup no longer waits for it on every question.
- See why an OpenDART read failed: no data for that period, a key OpenDART refused with its status, the daily limit, maintenance, a slow answer or a company list still downloading, each with what to change.
- Look up a Korean company by its ticker, corp code or exact name and get its overview from OpenDART too: CEO, market, industry code, address, homepage, founding date and fiscal year-end.
- Read a Korean company's treasury shares, auditor and audit opinion, employees and salaries, and share issues and reductions from its annual, half-year or quarterly report.
- Read a Korean company's lawsuits, split-mergers and overseas listing or delisting decisions from OpenDART, like its other capital decisions.
- Read an insider ownership report OpenDART dates a day after its receipt number, instead of losing the whole list.
- Keep using a saved key while several reads run at once: a read waits its turn instead of failing as "being changed right now", for OpenDART, other data connections, custom connections and the GitHub sharing account.
- See a US company's income statement, balance sheet and cash flow read from SEC filings reach the answer with every period and line item, instead of arriving empty.
- Get the latest rows of a long price or rate history in the answer: a result too large to show whole now gives the AI its last rows and date range as well as its first.
- Keep market data working after a large request: a long price history no longer ends the market-data session for the rest of the reply. FINRA short interest, the petroleum status report and the Svensson yield curve are not offered for now, because one request to them stopped every other market-data read.
- Read stablecoins pegged to currencies other than the dollar, get "no data" instead of an error for a World Bank year not published yet, and use a free Alpha Vantage key without being told it needs a paid plan.
- Trust a fall a news passage states in words: a negative figure no longer warns of a sign mismatch when the passage says the market fell by that much, including when the point change comes first with the percent in brackets or one word ends a list of stocks, in new and already saved answers. A rise stated in words still flags a negative figure.
- See a market-data read the source had nothing for as no result: the step says so, and the answer does not count it as a failed run.
- Open a chart or table shown in an answer in the side panel beside the chat.
- Keep price tables out of World: a saved price history, quote, exchange rate, index level or rate series, or a table of them, is kept and citable but spends no World requests and no longer delays the filings and articles behind it. The Library and the reader say "Price data · not added to World", and such Sources already waiting leave the queue.
- Continue every Source paused at the request limit in one step: World says how many stopped there and offers Continue all, which first asks how many Sources it continues and how many model requests it can use at most. Each Source gets the same fresh allowance as its own Try again, which stays in the Library.
- Keep World reading a Source while its model changes: when you pick another World model, or the recommended one follows an account you connect or disconnect, World reads the part it was on again with the new model instead of giving up, and Try again works on a Source that stopped after an earlier part failed.
- Have World add a relationship type to what it tracks only when it is a lasting relation between two names, not a reporting verb such as "announces" or "supports", and name it as a short noun in your language.
- Get answers in the language you ask in: a Loupist whose name or focus is in another language no longer pulls the answer or its progress notes into that language.
- Have a question that a saved Source answers read that Source first, before the web.
- See your Chat default by its own name while its AI account is disconnected: Settings and the Loupist's Chat defaults show the stored model with "Not available now", not the model Chat uses meanwhile.
- Choose Live or Demo when setting up a connection: the list opens on top of the setup dialog.
- Open Bot from the icon rail: hovering its icon keeps the icon, and the click opens Bot.
- Finish onboarding with one click on Create Loupist when no AI account is connected: the window no longer stays on the name step with Save and continue, and a notice that could not be saved is asked again in the app.
- Stop a reply without hearing about queued messages that do not exist: the line mentions them only while one is waiting.
- Read every market-data step in your interface language, and see under a failed one why it was not used.
- Read more World relations in your language, "files reports with" among them.
- See a cited web page that has no title of its own named by its address, instead of "N/A".
- See a price read in the reply that follows an asset credited to the market data it came from, not to Memory.
- See which research session a waiting save waits for, named from its question while its first answer still runs, instead of "Untitled research session".

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
