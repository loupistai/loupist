# Five-session growth scenario

A scripted product journey with fictional companies and dated fictional reviews — not
an investment benchmark. It creates a fresh Semiconductors Loupist watching Aster
Silicon, Brindle Devices and Cobalt Wafer, and never imports a handwritten method or
a prebuilt World.

The opt-in Rust test drives the production Chat and broker, Source retention, World
supervisor, automatic learning review and scheduled briefing dispatch. Every model
call uses `openai-codex/gpt-6.1-sol` at `medium` with provider retries disabled. The
existing Pi directory is used in place; app, Loupist and World index state
are disposable. No window opens and no external data connection is enabled.

With staged Pi and embedding resources, from the repository root:

```sh
LOUPIST_GROWTH_PI_AGENT_DIR=/absolute/existing/pi-directory \
LOUPIST_GROWTH_OUT=/absolute/new/growth-sessions.jsonl \
cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml \
  --locked --offline --features eval-harness live_growth_scenario_v1 \
  -- --ignored --nocapture
```

The output path must be new. Paid calls: one Chat in sessions 1, 2 and 5, one real
briefing in session 4, and bounded World/helper calls. Each session writes a complete
JSONL record (Map nodes and edges, counts by kind, growth events, learning reviews,
principles, methods, Journal entries, inbox items, answers and usage) before the next.

| Session | Inputs and observable result |
| --- | --- |
| 1 | Retain two Q1 reviews, process World, ask what explains margin pressure. |
| 2 | Teach the capacity-cost principle through Chat and request its checklist. |
| 3 | Enable method learning, retain three Q3 reviews, process World and drain learning review work. |
| 4 | Dispatch the actual briefing task and inspect its persisted inbox result. |
| 5 | Open a new Chat with saved-context reads only; compare all three companies. |

Method learning is off for sessions 1–2 so an early review does not consume the
six-hour cooldown; no timestamps, thresholds or outcomes are rewritten. Each Q3
review shares one pattern (rising utilization and margins with worsening inventory
days and operating cash flow), and Aster's continued capacity costs contradict its
earlier expectation. Zero Journal entries is the expected restraint. A learning
claim needs both the saved review and the method's use in the new Chat; report an
absent `lens_revised` event honestly.

To render session tables and a sanitized renderer-only preview fixture:

```sh
python3 apps/loupist/evals/world/growth-scenario-v1/report.py \
  /absolute/growth-sessions.jsonl \
  --report output/reports/growth-scenario.md \
  --preview apps/loupist/src/preview/growth-replay.json
```

The `growth-replay` preview shows the final Map with replay controls. IDs are
replaced and acquisition timestamps shifted together to fit the preview clock (the
report records the shift). It is a renderer review aid, not evidence of native
persistence or model behavior.
