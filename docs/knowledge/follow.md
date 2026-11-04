# What you follow

Followed assets are an explicit list the host reads onto the Map. Following is not a holding and not a trade.

### What you follow

What the user follows (the watchlist, then holdings; one entry per symbol and market)
reaches each Loupist's World as Sources the host writes, never as a host-written World
snapshot (`knowledge/followed.rs`, `commands/knowledge/followed.rs`). Each is a
`user_statement` Source with provider `loupist:followed` and the stored English title
"What you follow". Its first line says each line is one asset with its ticker in
parentheses, written `TICKER:MARKET` when the market is known; then one
`- {name} ({symbol}:{market})` line per asset, with no market when the user gave none.
The market stays inside the identifier, as it does in anchors. The reader also accepts
the earlier `- {name} ({symbol}, {market})` form, so existing Sources do not cause
the same assets to be listed again. Holdings appear by name only, never quantities or
values. Sources are immutable, so a Loupist gets a new Source for the assets none of its
earlier "What you follow" Sources lists, at most 24 per Source (the
per-episode anchor limit). Each new asset becomes an anchor (its symbol, plus
`symbol:market` for a known market), and each Source gets one ordinary World job.
Unfollowing or selling removes nothing; following again adds nothing.

Rust brings a Loupist up to date when it is created, when it is opened, and for every
Loupist about three seconds after the last follow, holdings import or wallet read, so a
burst makes one Source and one job per Loupist. A Loupist that is unavailable or under
maintenance catches up when next opened. Without a connected model the Source waits for
the backfill; with processing off it is read once processing is turned on. The
interface shows the title and origin in its own language through the display key
`followed` (`titleKey` on catalog records, the Source reader and `source_retained`
growth events). Briefings leave these Sources out of the World growth they summarize,
and the morning message never names one as where a value came from.

## Code

- What you follow (followed assets to the Map): `src-tauri/src/knowledge/followed.rs`, `src-tauri/src/commands/knowledge/followed.rs`, `src-tauri/src/commands/user_context.rs`, `src/components/knowledge/hostSource.ts`
