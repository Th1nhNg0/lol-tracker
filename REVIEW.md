# Technical review: Rust LoL Tracker

Independent audit of the Rust rewrite (state on 2026-10-20) against the private legacy Python tracker
(`../legacy/lol-tracker`, read locally; the running VPS was not contacted). Every finding below was
verified by reading the code; items marked *unverified* could not be exercised without live Discord/Riot
traffic. The "Status" column records what this pass changed.

## 1. Architecture summary

One binary, one Tokio runtime, one SQLite file.

| Module | Responsibility |
| --- | --- |
| `main.rs` | CLI (`check`, `probe`, `render-scoreboard`, `run`), `App` (config + store + Riot client + optional LLM), worker supervision and signal shutdown. |
| `config.rs` | TOML settings, explicit dotenv/env secrets, validation. Secrets type has no `Debug`. |
| `model.rs` | Typed `Match`/`Participant`/`Rank`/`Pending`/`Notice`, Match-V5 normalisation. |
| `riot.rs` | reqwest client: Account-V1, Match-V5, League-V4, Spectator-V5; a shared pacing gate with Retry-After handling; bounded bodies. |
| `store.rs` | rusqlite behind a `std::sync::Mutex` (never held across `.await`): roster, baselines, seen IDs, pending games, rank snapshots, transactional outbox, message references, timeline cache, retention. |
| `tracker.rs` | Sequential loop: roster resolution → Match-V5 history sweep → optional Spectator → pending results → rank refresh → pruning. |
| `presentation.rs` | Text summaries, commentary, CSV/text detail views. |
| `render.rs`, `render_assets.rs` | SVG built in Rust and rasterised with `resvg`; Data Dragon artwork cache. |
| `reports.rs` | Rank refresh, daily/weekly report text, timezone-aware scheduler with a durable "last report" marker. |
| `discord.rs` | Serenity gateway (GUILDS intent only), slash commands, result buttons, outbox delivery worker, optional commentary worker. |
| `llm.rs` | Optional OpenAI-compatible Chat Completions commentary. |

Data flow: tracker writes match + outbox row in one transaction → delivery worker sends/edits Discord
messages and acknowledges → optional commentary worker edits message content afterwards. Discord delivery
is at-least-once.

The overall structure is sound and appropriately small: no plugin system, no agent framework, no Hermes
code, secrets never logged, SQLite transactions used correctly, mentions disabled everywhere. The problems
are concentrated in tracking correctness, failure isolation and a few dropped legacy features.

## 2. Feature parity with the legacy tracker

| Legacy feature | Rust before this pass | Status now |
| --- | --- | --- |
| Riot ID → PUUID, platform/region routing | Present | — |
| Match-V5 history polling, first-sight baseline, 3 h catch-up | Present (per-account baselines, better than legacy) | — |
| Optional Spectator start message edited into result, 15 min timeout | Present | — |
| Full 10-player result with items/spells/runes/objectives | Present | — |
| **Per-match LP change for history-detected games** (legacy used a timestamped rank cache) | **Missing** – only Spectator games got a "before" rank; the default config disables Spectator | Fixed (P0-1) |
| League-V4 retry while rank lags | Present (3×5 s; legacy 6×5 s) | — |
| Rank shown for all 10 scoreboard rows | Missing (tracked players only) | Not restored – costs ~9 extra League-V4 calls per game; see §6 |
| PNG scoreboard | Present (new resvg renderer) | Font DB cached |
| Damage / gold **charts** on buttons | Replaced by CSV attachments | Restored as PNG charts, CSV fallback |
| Events button inline when short | Always a file | Inline when ≤1900 chars |
| `/lastgame`, `/rank` public replies | Changed to ephemeral | Restored public; config/roster replies stay private |
| `/players`, `/config channel/rankchannel/addplayer/removeplayer/rankreport` | Present | — |
| `removeplayer` Riot-ID autocomplete | Missing | Restored |
| Rank snapshot when a player is added | Missing | Restored |
| Startup check of bot permissions in target channels | Missing (type/guild only) | Restored |
| Deterministic role-aware "carry / weakest" verdict | Missing – commentary only restated K/D/A | Restored as a factual, role-weighted standout/least-impact verdict (no insult pools) |
| Daily report (best→worst LP) and Sunday combined weekly report | Present, but rank rows unordered | Sorted by LP change |
| LLM roast | Hermes/ChatGPT-session based | Replaced by optional OpenAI-compatible API (was already the case); made robust |
| Activity-based adaptive history interval | Missing | Not restored – unnecessary at 10 accounts; see §6 |
| Weekly/daily image cards, wrapped videos, TTS, tier lists, screenshot analysis, guild emoji | Intentionally dropped | Still dropped (out of scope; documented) |

## 3. Critical bugs and design weaknesses

### P0 — Critical

**P0-1 LP change never computed in the default configuration.** `tracker.rs::Tracker::finish` set
`rank_before` only from `Pending.before`, which exists only for Spectator-observed games. With
`spectator = false` (the default, and the legacy deployment's setting) every ranked result showed
"ΔLP unknown". Legacy derived "before" from a rank cache whose timestamp predated the game.
*Fix:* use the latest stored snapshot taken at or before the game start **during the current process
run** (`Store::rank_at` + `Tracker::since`). Ranks are refreshed before the first history sweep, so a
restart can't pair a pre-downtime snapshot with a post-downtime rank. The attribution window was widened
from 10 to 20 minutes after game end, which still can't contain a second complete game.

**P0-2 One failing step stopped the whole tracker loop.** `Tracker::tick` used `?` between steps. A single
pending game whose Match-V5 payload failed normalisation (`Match::from_riot(..)?` in `pending_results`)
returned an error on every tick, so rank refresh and pruning **never ran again** and the game was never
cleared. *Fix:* every step runs independently and logs its own failure. Per-game failures are isolated.
Unparseable pending games time out, and unparseable history matches are marked seen.

**P0-3 Unparseable history matches were fetched again forever.** `Tracker::history` logged normalisation
failures but never marked the ID seen. Each sweep re-downloaded the same match through the 1.25 s
rate gate until it fell out of the 20-ID window. This wasted Riot quota and delayed other players. Fixed
by the same change.

### P1 — Important

**P1-1 LLM output handling broke with most models.** `llm.rs` required the raw message to be a bare JSON
array, `finish_reason == "stop"` exactly, numeric IDs, and rejected the **whole** response if any comment
had a `<`, `@`, newline or >120 chars. Typical models wrap JSON in fences or quote IDs. *Fix:* strip
fences, extract the array, accept string or numeric IDs, sanitise and truncate each comment, and reject
only on `length`/`content_filter` or links.

**P1-2 Local OpenAI-compatible servers were impossible.** The base URL had to be HTTPS and a key was
mandatory. *Fix:* plain HTTP is allowed only for loopback hosts (Ollama, llama.cpp, LM Studio), and the
key is optional there. Remote endpoints still require HTTPS and a key.

**P1-3 AI commentary replaced the factual summary.** The commentary worker overwrote the deterministic
lines. *Fix:* the AI block is appended when both fit in one Discord message, and replaces the summary only
when they don't.

**P1-4 Deterministic commentary was not commentary.** `presentation::commentary` repeated K/D/A and
CS/min. Legacy `verdict.py` ranked the tracked team with role-weighted metrics compared with the lobby
and the lane opponent. *Fix:* reimplemented in Rust (`presentation::verdict`) with factual reasons,
bilingual and without the legacy insult pools. This needed three extra participant fields (objective
damage, CC time, heals+shields on teammates), which are `serde(default)` and backward-compatible.

**P1-5 Charts were dropped.** Damage and gold buttons produced CSV, which Discord users can't read inline.
The resvg pipeline already existed. *Fix:* `render::damage_chart` and `render::gold_chart` produce PNGs,
with CSV as the fallback.

**P1-6 Fonts were rescanned on every render.** `render::scoreboard` called `load_system_fonts()` and
copied both bundled fonts on every image. *Fix:* the font database is built once (`OnceLock<Arc<_>>`)
and shared by all renders.

**P1-7 Discord UX regressions.** `/lastgame` and `/rank` were made private, removing their purpose in a
group channel. `removeplayer` had no autocomplete. Missing channel permissions only showed up as repeated
delivery failures. *Fix:* restored public replies and autocomplete. Startup and `/config` now verify View
Channel, Send Messages, Embed Links and Attach Files for the bot.

**P1-8 Spectator champion names were never retried.** `Tracker::run` fetched the Data Dragon champion map
once. A transient failure meant "Champion 157" in every start message until restart. *Fix:* retried at
most every 10 minutes while empty.

**P1-9 Fixed Riot pacing.** The hard-coded 1250 ms gate suits a development key but needlessly slows a
production key. *Fix:* `riot.request_interval_ms` (default unchanged).

### P2 — Improvements

- `Tracker::finish` cleared an untracked pending game by queuing a `Timeout` notice that the delivery worker
  then discarded. Replaced with `Store::discard`.
- Artwork cache pruning walked the directory twice per render on the async runtime. It now runs once, in
  `spawn_blocking`.
- Daily/weekly rank rows were ordered by PUUID. They are now sorted by LP change (legacy order).
- `addplayer` didn't snapshot ranks, so a new player's first report baseline came up to 30 min late.
- Clippy warnings (collapsible `if`s etc.) cleaned up where code was touched.
- `plans.md` mixed a forward-looking plan with stale execution logs. Rewritten to describe the actual
  architecture.

## 4. Performance and reliability notes

- Riot calls are strictly sequential behind one gate (default 0.8 req/s). With 10 accounts a history
  sweep costs 10 calls/min plus new matches, well within a development key's 100 calls / 2 min. Enabling
  Spectator adds 10 calls per 15 s, which **exceeds** a development key's long window. Use a production
  key or raise `spectator_interval_secs`. This is documented now.
- The SQLite mutex is never held across `.await`. All queries are indexed by primary key or `ended_at`
  except `pending()`, which is tiny.
- Outbox: failures back off up to 15 min per notice and never block other notices. A notice that can
  never be delivered (deleted channel) keeps retrying until fixed. That is deliberate, since nothing is
  silently lost.
- Graceful shutdown: SIGINT/SIGTERM stop the gateway and abort workers. All durable state is in SQLite, so
  aborting mid-request at worst repeats a Riot fetch or a Discord send (at-least-once).
- Memory: bounded HTTP bodies, timeline cache capped at 50 rows, artwork cache capped at 64 MiB/30 days.

## 5. Security and publication readiness

- No credentials, private IDs, roster names or Hermes code in tracked files (checked with grep for
  17–20-digit snowflakes, `hermes`, `codex`, `/root`).
- `.gitignore` covers `.env*`, `config.toml`, `players.json`, `data/`, `*.db*`, `legacy/`, `private/`.
  The local `.env`, `config.toml`, `players.json` and `data/` in the working tree are ignored and have
  mode 0600/0700.
- Logs never include URLs (which contain PUUIDs/Riot IDs), response bodies, tokens or TOML parse errors.
  Dependency logs are filtered out.
- The LLM receives only anonymised numeric participant IDs, roles and final statistics.
- Bundled fonts are OFL with provenance. Riot artwork is fetched at runtime and never committed.

## 6. Remaining limitations (not implemented, by decision)

- Ranks for non-tracked scoreboard rows (legacy did this; it costs ~9 extra rate-limited calls per game).
- Activity-based adaptive polling intervals (legacy optimisation; unnecessary for small rosters).
- Image report cards, wrapped videos, TTS, tier lists, screenshot analysis, custom emoji, private lore.
- No automatic import of legacy history (by design; new schema).
## 7. Verification performed in this pass

- `cargo build --locked` and `cargo clippy` are clean, except one cosmetic `too_many_arguments` lint on an
  SVG helper.
- `check` (offline) passes with the private local config.
- On a real recent Flex match: the scoreboard, damage chart and gold chart were rendered and inspected, and
  the VI/EN commentary with verdict was printed. This exposed and fixed a font fallback where `★`/`−` are
  missing from the bundled font.
- `run --no-discord --once` was run against a **scratch copy** of the database: ranks refreshed first,
  history ran, and two real matches were stored. Their tracked players correctly got *no* LP attribution,
  because the games started before the process did. The scratch copy was deleted.
- **Not verified:** the live Discord gateway (commands, autocomplete, buttons, message edits, the new
  permission check against a real guild) and real LLM providers. The LLM parser changes are reasoned,
  not exercised.
