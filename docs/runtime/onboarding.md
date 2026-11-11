# Onboarding

Creating or importing a Loupist finishes on that Loupist's Chat. A save failure is reported and does not block Chat. Onboarding opens when there is no Loupist, or a new profile has no AI account and has not finished onboarding. Once the notice has been acknowledged, losing or removing the last AI account keeps saved Chats and research readable. The composer offers account connection for new answers.

A new user acknowledges the notice and connects an AI provider. The welcome title uses the display size. The connect step (`onboarding/ConnectStep.tsx`)
shows Continue with ChatGPT as the primary button and Continue with SuperGrok as a
secondary button under it. On a short window the other ways scroll inside the step so those buttons stay above the fold, and focusing a way below that list scrolls it into the list. Other ways to connect lists API keys
for Anthropic (the only way to connect Claude), OpenAI, Gemini, xAI, Upstage Solar and
OpenRouter; a model on this Mac; and a custom endpoint. A browser sign-in shows a
waiting card with Open again and Cancel (Escape also cancels), plus Copy the
sign-in link when the browser could not open. Once Pi waits for manual entry,
"Browser didn't come back?" offers a paste field 20 s after the browser opened (at
once if it did not). Rust validates the pasted value (`providers/support.rs`)
before Pi sees it: for ChatGPT only the exact `http://127.0.0.1:1455/auth/callback`
address with one `code` and `state`; for the debug Codex client its
`http://localhost` or `http://127.0.0.1` callback address, or `code#state`.
SuperGrok shows a device code to copy. Failures are typed (declined plan
permission, an ineligible plan with Use an API key, a busy port, a timeout,
offline) and offer Retry. An existing connection skips the step. After connecting, onboarding shows the New Loupist preset picker (`LoupistTemplateFields`) with My portfolio preselected and one Create Loupist button, so the first Loupist starts from the preset the user chose. With a Loupist already present, onboarding skips the picker and opens it. A profile with a connected provider skips sign-in. Assets and tone can be configured later. The welcome screen also offers the existing trusted folder import. Onboarding creates no schedule.

## Code

- Loupist creation / import / profile: `src/components/onboarding/`, `src/app/AppDialogs.tsx`, `src/app/useLoupistDirectory.ts`, `src-tauri/src/commands/loupist.rs`
- Native file chooser: `src/bridge/tauriBridge.ts`, `src-tauri/src/commands/dialog.rs`
