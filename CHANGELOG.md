# Changelog

## Unreleased

- Introduce Loupist, a local-first financial research desktop app for macOS 13 or newer on Apple Silicon. Windows, Intel Macs and Linux are not supported in this release.
- Connect an AI account with a supported provider API key or sign-in; no Loupist account or cloud sync is required.
- Read filings, market data and economic series through read-only Data connections, including OpenBB, OpenDART, KRX and Korea Investment.
- Keep each Loupist’s research sessions, retained Sources, Map, Lens, research notes and Journal in its own local folder, which can be copied and reopened.
- Find Bot, Following, Map, Lens and Schedule in the sidebar; Following brings followed assets, holdings import, Watch-for choices and recent alerts together.
- Follow assets without starting automatic checks; Watch all turns on class defaults, and Watching offers Pause/Resume and Check now without AI usage.
- Teach principles and develop reusable methods in Lens, with editable pages, dated revision comparisons and evidence; open Sources, Journal and Notes separately in the Library.
- Look around before connecting an AI account, go Back during setup, and choose a template and name, then change avatar and tone later in settings; Bot keeps a three-step checklist for following, teaching a principle and turning on a briefing.
- Run numerical analysis in a pinned offline Python worker with no shell, network, process spawning or host environment; read and edit research files in the selected folder and open results beside Chat.
- Open Private chat from the composer’s + menu to skip Memory reads and saves together; chat history is still saved and messages still go to the AI provider.
- Run scheduled research and briefings while the app is open or in the menu bar; inspect runs, pause or resume tasks, Run now, and delete custom tasks from Schedule.
- Retry recurring briefings at their next occurrence before pausing after three consecutive failures; a successful run clears the streak, and missing AI credentials pause immediately.
- Read bundled Skills and their references again without allowing changes to them; built-in tools need no loading step, while Data connections keep their grants.
- Update the local SQLite database engine with its security fix, refuse sharing when a required saved-credential scan fails, and save AI API keys through private process input instead of a process environment.
- Beta. Deliver a Mac installer signed with Loupist's own certificate and not notarized by Apple, so the first launch needs a one-time approval in System Settings; updates are signed with the Loupist updater key, install only with consent, and are refused unless the package names the version in its feed. Compatible database upgrades make a private backup and leave unsupported formats unchanged.
- License the app under AGPL-3.0-only; Loupist never trades, offers no trading execution, and is not a licensed adviser.
