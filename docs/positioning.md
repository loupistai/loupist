# Positioning and voice

This page owns audience, product language and copy standards. Implemented behavior
belongs to [Architecture](architecture.md) and [Knowledge and Memory](knowledge-model.md).

## Product promise

**Watch. Remember. Explain.** A Loupist watches what you follow, remembers how you
think, and explains what changed, with the evidence attached.

A loupe is the small lens an appraiser holds to examine something closely; a Loupist
is the one who examines. The name is the agent noun ("a Loupist", "my semiconductor
Loupist"; ko 루피스트). It claims care, not authority: an examiner, not an adviser.

Loupist is for individual investors investigating companies, industries and
markets. It combines provider-backed Chat, read-only data connections, local
calculation and retained Memory. Each Loupist is a local folder with multiple Chats.
Source retains material, Map connects claims, Lens holds principles and editable research pages,
and the Journal holds the user's own judgments.

[Loupist creation and import](runtime/storage-authority.md#where-data-lives) keep research in local
folders. AI accounts are separate from the saved local data. The desktop does not require a Loupist
account or provide cloud/mobile synchronization. Local schedules depend on the
app process; they are not an always-on cloud service.

## Voice

Lead with the user's question, result or next action. Use concrete words and show
failures with a recovery action. Distinguish retained evidence, processing status,
a proposed change and a committed save. Do not call a retrieval hit, record count
or saved method proof of better analysis; evaluation must test subsequent work.

Call the agent "your Loupist" or its name, a research companion (ko 리서치 동료).
Home is a place. The sidebar shows Home · Following · Map · Lens · Schedule.
Map and Lens open beside Chat; the header also opens Chat materials. Map contains
the [Library](ui/map-lens.md#map-and-library), with Sources · Journal · Notes tabs.
Lens holds principles, perspectives, methods, topics and ideas. Use World only in
developer descriptions of the model, storage or code; identifiers stay unchanged.
Do not expose storage IDs, hashes, worker names, raw envelopes
or byte offsets as product concepts. Put source limitations, model usage, commands,
file access and actionable diagnostics in the appropriate details.

## First-run terms

Onboarding, the Map and Briefings explain the product words in place. New copy
uses the same meanings.

| Term | What the interface says |
| --- | --- |
| Loupist | A research companion for one topic. It remembers what you read together and learns how you like to research. |
| Home / 홈; Chat / 대화 | The standing conversation; a separate conversation for one question. |
| Following / 관심 자산; Holdings / 보유 자산 | What you follow, shared across Loupists; what you hold. Following is not a trade. |
| Watching / 지켜보기 | Automatic checks while the app is running; their results are alerts (알림). |
| Map / 지도 | Connections from Sources; background reading is Map building (지도 구성). |
| Library / 자료함; Source / 자료 | Saved material and its retained evidence. |
| Lens / 렌즈 | Your principles and research pages, with evidence and history; Perspective / 관점 is a starting view. |
| Principle / 원칙 | A criterion taught by the user; it outranks methods. |
| Method / 분석 방법 | A check the Loupist learned, or one saved from Chat. |
| Journal / 저널 | The user's own judgment. Research notes / 리서치 노트 are the Loupist's saved analysis. |
| Schedule / 예약; briefing / 브리핑 | Timed work; a briefing reports what changed for followed assets and can share a reasoned view. |
| Memory / 기억; Private chat / 비공개 대화 | Everything it keeps; Private chat skips Memory use and saves, but still saves the chat and sends it to the AI provider. |
| AI account / AI 계정; Data connection / 데이터 연결 | Model sign-in or key; a source of data. |
| this Mac / 이 Mac | The user's machine, wherever copy says what stays local; never "this device" or "this computer". |

The welcome screen also says the app reads sources, builds a map of what the
user follows, and leaves the choice with the user.

Loupist is an AI research companion that can share its view; it is not a licensed adviser
and never trades. Journal entries
record the user's judgment, not an assistant recommendation. Avoid promises of
returns or improved prediction. Learning means retained evidence/methods, not
model-weight training. [Investment judgment](agent/prompt.md#investment-judgment)
owns the detailed model behavior boundary.

## Visual language

Use neutral reading surfaces, clear source access and burnt amber `#a4530c` for
focus/identity. Reserve price-direction colors for data. Avoid decorative AI brains,
circuits and opaque intelligence scores. Preserve keyboard focus, long-form reading,
light/dark themes and the minimum desktop size. The
[design rubric](dev/ui-qa.md#design-rubric) owns measurable review standards.
