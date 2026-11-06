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

## Real-model walk (GPT-6.1-Sol, medium)

After the user connected ChatGPT, the live preset moved to GPT-6.1-Sol at medium (`agent-driver.mjs live-preset`).

- Research session: "TC bonder supply chain by customer". The agent searched Lens and earlier chats, read 11 web sources, saved 9 Sources and filed a new subject page "HBM TC본더 고객사별 공급망" with four dated sections. Receipt: "자료 11개 읽음 · 새로 배운 것 4개 · 페이지 1개 사용 · 자료 9개 저장".
- Bot hand-off: a deep-research request in the Bot called `research_open`; the Bot showed "조사 중 … 진행 중 · 열기", the session appeared in the sidebar, and its run read the earlier page first. The run finished (`chat_handoffs.status = done`), saved 16 Sources, revised the earlier page and created another ("새로 배운 것 6개 · 페이지 2개 사용").
- World: the map grew to 18 names and 14 relationships while processing continued.

| ID | Kind | Finding | Fix |
| --- | --- | --- | --- |
| J9 | Bug | Right after connecting ChatGPT, the AI account card shows "ChatGPT 모델을 불러오지 못했어요" with the raw English error "another provider support operation is already active" (a model reload raced the connect). | Do not start a second model load while one is active; show localized text, never a raw backend error. |
| J10 | Bug | A session whose run the host started (a hand-off) shows no answer until the app reloads, although the store has it. | Refresh chats changed by host runs. |
| J11 | Bug | The Bot's hand-off card stays "진행 중" after the run finished until the app reloads (the store already says done with a learned digest). | Same refresh path for the Bot thread. |
| J12 | UX | The hand-off card says "배운 것 2" (pages) while the session receipt says "새로 배운 것 6개" (sections). | Count and word it the same way as the receipt. |
| J13 | UX | The hand-off card summary is the run's first line, a Markdown heading shown with its "#". | Use the first body paragraph as plain text. |
| J14 | Bug | One uncertain World extraction call (150 s job, no usage reported) quarantined the World. The map stopped at 18 names and kept saying "업데이트 중", nine queued jobs waited, and the 16 Sources the hand-off run saved were never queued, until someone found World processing recovery in Loupist settings. | The graph worker queues the no-model rebuild itself; a failed rebuild still waits for the settings action. On the live profile two rebuilds ran unattended, 27 jobs were queued again and the map grew to 35 names. |
| J15 | UX | The Pages list drew no marker for subject and idea pages, which the map draws as filled and hollow squares. | Same markers in the list. |
| J16 | Bug | The used-pages receipt under an answer and the Learned card showed the host's English "Outlook" title in the Korean interface. | Both use the localized page title. |
| J17 | Build | The onboarding and Memory UX merge repeated one import in `MapToolbar.tsx`; the renderer failed to compile and the dev window showed Vite's error overlay. The unit's web gate stopped at a flaky test before the type check. | Removed; `tsc` passes. |
| J18 | Bug | Lens selects its first page by itself once its data loads, and the reader then focused that page's heading. Data that arrived late took focus from wherever the user was, closing an open header menu so the pick in it was lost. This was the intermittent `chat-workspace.test.tsx` failure. | Only a page someone opened takes focus. The full renderer suite then passed under load (1,603 tests). |
| J19 | Copy | The Bot's catch-up named a visit earlier the same day by weekday: "토요일 이후 한 가지예요" on a Saturday. | A visit earlier today reads as its time ("오전 2:32 이후"). |
| J20 | Copy | Opening a built-in tool group showed its internal English name, "Opened Source retention and journals", in the Korean chat. | Built-in groups use the translated catalog wording; connection names stay (follow-up). |
| J21 | Bug | `loupist_setup` and Loupist settings changed the focus, but the perspective page kept the text seeded at creation. | Every focus change rewrites only the page's `focus` section. |
| J22 | UX | After J13, the re-walk's hand-off card summarized its run with the answer's bold dateline, "조사 기준일: 2026년 11월 6일". | A short paragraph that is only bold text is a label; the summary starts at the prose after it (scheduled results too). |

## Re-walk after the fixes (GPT-6.1-Sol, medium)

- New Loupist "배터리 루피스트" from the dialog's custom preset, with no focus. Its Bot asked what it researches. The reply to "나는 2차전지 소재 공급망을 리서치해. LG에너지솔루션이랑 에코프로비엠 팔로우해 줘." called `loupist_setup`; the receipt read "LG에너지솔루션 관심 자산에 추가 · 에코프로비엠 관심 자산에 추가 · 리서치 주제 바꿈", and Lens showed the perspective page "관점" with "관심 분야: 2차전지 소재 공급망".
- The Pages tab, the used-pages receipt and the reader show "관점" and "신념" in Korean; subject pages have their marker.
- The semiconductor World, stalled at 18 names, rebuilt itself after the fix and reached 61 names and 73 relationships.
- A deep-research request in the new Bot opened one research session, which appeared in the sidebar at once with the card "조사 중 … 진행 중 · 열기". When its run finished, the card turned into "조사 완료 … 완료 · 새로 배운 것 4개 · 열기" in the open window, without a reload (J10, J11, J12). The run found no direct contract between the two companies in public sources and said so.
