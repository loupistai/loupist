# Onboarding

Creating or importing a Loupist finishes in Bot. Failed creation stays on Create; a failed notice save leaves the shell notice gate available to retry. Onboarding opens with no Loupist, or for a new profile with neither an AI account nor an acknowledged notice. Removing the last AI account leaves saved Chats and research readable; new answers need an account.

Welcome requires acknowledgment of the AI notice and offers trusted folder import.
Connect offers an AI account or Look around first. Back returns to the previous
step, skipping Connect when already connected. `onboarding/ConnectStep.tsx` shows
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

Create shows My portfolio preselected in the template grid (`LoupistTemplateFields`),
Name and Create Loupist. Custom also asks for Focus. Avatar, preset focus and tone
use defaults and can be changed later in Loupist settings. Crypto, Macro and
Commodities offer follow chips; Real assets is absent. The notice explains that
background source reading builds the Map and learns methods using the AI account,
and can be turned off in Loupist settings. An existing Loupist skips Create;
an existing AI account skips Connect. Onboarding creates no schedule.

Bot shows a checklist: follow something, teach a principle, get a morning
briefing, plus Import holdings (CSV). Steps complete independently; it stays until
all three are done or hidden. Buttons fill the composer without sending. Without
an AI account, Connect leads, Follow opens Following, and the other steps wait.

## Code

- Loupist creation / import / profile: `src/components/onboarding/`, `src/app/AppDialogs.tsx`, `src/app/useLoupistDirectory.ts`, `src-tauri/src/commands/loupist.rs`
- Native file chooser: `src/bridge/tauriBridge.ts`, `src-tauri/src/commands/dialog.rs`
