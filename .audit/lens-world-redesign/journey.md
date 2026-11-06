# Native user journey review (2026-11-07)

Driven through WebDriver (`apps/loupist/e2e/agent-driver.mjs`) on the development window built from this branch, Korean UI, macOS. The profile had no AI account; the walk used the local Ollama model `llama3.2:1b` (no credentials). That model cannot use tools, so agent behaviour (filing, hand-off, discovery) was not observable; screens, flows and empty states were. Screenshots: scratchpad `journey/`.

Steps: notice → connect (local model) → automatic Portfolio Loupist and Bot → Bot request → New Loupist (Semiconductors preset) → Bot vocabulary card → Loupist settings → Lens panel and the default perspective page → World panel empty state → new research session draft.

## Findings

| ID | Kind | Finding | Fix |
| --- | --- | --- | --- |
| J1 | Gap | The agent cannot set up a Loupist from chat: no tool follows or unfollows assets, sets the research focus or applies a preset. The Bot asks "What are you researching?" but cannot act on the answer. | `loupist_setup` tool, staged and committed with the reply, interactive turns only; SYSTEM.md rule. |
| J2 | Gap | Onboarding creates a Portfolio Loupist without offering the presets the New Loupist dialog shows. | Show the preset picker in onboarding. |
| J3 | Bug | A new Loupist's Bot loses its first-visit guidance once a host post (the preset vocabulary card) exists. | Count only user messages when deciding the Bot is new. |
| J4 | Bug | The default perspective page reads "Outlook" with a "Beliefs" section holding the focus text, in English in the Korean UI. | Seed a `focus` section; show host-default title and headings in the interface language. |
| J5 | Bug | Pages group label and count run together ("관점1"). | Separate them. |
| J6 | Bug (pre-existing) | World processing status shows an English sentence in the Korean UI ("Waiting for a supported model provider…"). | Localize by status code. |
| J7 | UX | "What the map tracks" sits inside the Learning disclosure; users look for it under World. | Move it to the World section. |
| J8 | Copy | The chat context card calls methods "way of working" (작업 방식); the Pages tab says Methods (분석 방법). | One term. |
