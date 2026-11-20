# Onboarding

Creating or importing a Loupist finishes in Bot. Failed creation leaves the name step open; a failed notice save leaves the shell notice gate available to retry. Onboarding opens with no Loupist, or for a new profile with neither an AI account nor an acknowledged notice. Removing the last AI account leaves saved Chats and research readable; new answers need an account.

Welcome requires acknowledgment of the AI notice and offers trusted folder import.
Connect offers an AI account or Look around first. Back returns to the previous
step, skipping Connect when already connected. Each step, forward or back, puts
focus on its heading. Import opens Import a Loupist, named like Settings › Loupists'
Import Loupist button. `onboarding/ConnectStep.tsx` shows
Continue with ChatGPT first, then Continue with SuperGrok. Other ways to connect
scrolls inside the step and offers Anthropic (Claude requires an API key), OpenAI,
Gemini, xAI, Upstage Solar and OpenRouter keys, a model on this Mac and a custom
endpoint. Sign-in shows Open again and Cancel (also Escape), plus Copy the sign-in
link if the browser could not open. When Pi awaits manual entry, "Browser didn't
come back?" offers a paste field after 20 s, immediately if opening failed.
Rust validates it (`providers/support.rs`): ChatGPT accepts only
`http://127.0.0.1:1455/auth/callback` with one `code` and `state`; the debug Codex
client accepts its localhost callback or `code#state`. SuperGrok shows a device
code. Typed failures offer Retry or Use an API key for an ineligible plan.

Create shows My portfolio preselected in the template grid (`LoupistTemplateFields`)
and Continue. Custom also asks for Focus. Crypto, Macro and Commodities offer follow
chips; Real assets is absent. A tapped chip is picked and a second tap drops it; another
template drops the picks. The picks are followed in the Loupist once it is created, here
and in New Loupist, never in the Loupist that is open. The selected template, starter
and tone wear the accent.
Name follows (`LoupistIdentityFields`): the face at 64 px beside the name, the Name
field filled with the template's suggestion ("Chip Loupist"; Custom suggests none) and
editable, the ten face colours, and Create Loupist. Preset focus and tone use defaults
and can be changed later in Loupist settings. The notice explains that background
source reading builds the Map and learns methods using the AI account, and can be
turned off in Loupist settings. In New Loupist the notice spans the fixed footer
above the Cancel and Create actions while the form scrolls. An existing AI account skips Connect. An existing
Loupist skips Create but still gets Name, with Save and continue: its stored name is
kept, unless it equals its folder's name (what the host stores for a folder registered
without one, such as the development seed), when the template's suggestion replaces it.
No screen shows a folder name as a Loupist's name. Onboarding creates no schedule.

Until the user sends a message in Bot, its greeting offers setup cards alongside
suggested questions (`BotGreeting`). Without an AI account, **Connect an AI account** comes first.
**Add assets to follow** opens Following when the list is empty. **Get a morning
briefing** drafts a request when the briefing is off. After the first follow,
the greeting suggests questions about the followed assets. If the Loupist has
no active principle, a separate prompt below the cards drafts one for the user
to complete. **Hide this suggestion** dismisses the prompt for that Loupist on
this Mac. Questions and drafted requests wait for the user to send them.

## Code

- Loupist creation / import / profile: `src/components/onboarding/`, `src/app/AppDialogs.tsx`, `src/app/useLoupistDirectory.ts`, `src-tauri/src/commands/loupist.rs`
- Native file chooser: `src/bridge/tauriBridge.ts`, `src-tauri/src/commands/dialog.rs`
