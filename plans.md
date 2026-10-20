# LoL Tracker: architecture and plan

A standalone Rust rewrite of a private Python League of Legends Discord tracker. It is a new
implementation, not a port: no legacy source, assets, private identifiers or credentials are included. The
technical audit, feature-parity table and issue list are in [REVIEW.md](REVIEW.md).

## Scope

**Kept from the original:** Riot ID resolution; Match-V5 history polling with per-account baselines and
bounded catch-up; optional Spectator start messages edited into results; full 10-player results; Solo/Duo
and Flex rank tracking with cautious per-match LP changes; persistent result buttons (damage chart, gold
chart, events, scoreboard, loadout); `/lastgame`, `/rank`, `/players`, `/config channel | rankchannel |
addplayer | removeplayer | rankreport`; daily reports and the Sunday combined weekly report;
deterministic role-aware verdict; Vietnamese/English output.

**Changed on purpose:** one SQLite database replaces JSON state files; results reach Discord through a
durable at-least-once outbox; commands are registered only on request and never clear other commands;
all mentions are disabled; LLM commentary uses a plain OpenAI-compatible API key instead of Hermes/ChatGPT
session credentials.

**Dropped on purpose:** Hermes integration, credential subprocesses, image report cards, wrapped
videos/TTS, tier lists, screenshot analysis, custom emoji catalogue, private lore and insult pools,
ranks for non-tracked scoreboard rows, activity-adaptive polling, and import of legacy history.

## Architecture

One binary, one Tokio runtime, one SQLite file. No plugin system, agent framework or external runtime.

| Module | Responsibility |
| --- | --- |
| `main` | CLI modes (`check`, `probe`, `render-scoreboard`, `run`), `App` wiring, worker supervision, SIGINT/SIGTERM shutdown. |
| `config` | TOML settings and env/dotenv secrets, validated without printing values. |
| `model` | Typed match/participant/rank/notice records and Match-V5 normalisation. |
| `riot` | reqwest client with one shared pacing gate (`request_interval_ms`), Retry-After and auth backoff, bounded retries and bodies, sanitised errors. |
| `store` | rusqlite behind a mutex that is never held across `.await`: roster, baselines, seen IDs, pending games, rank snapshots, outbox, message references, timeline cache, retention. |
| `tracker` | Loop with independently failing steps: rank refresh → roster resolution and history → Spectator → pending results → pruning. |
| `i18n` | Vietnamese/English catalog: every user-facing string, queue/role/objective names, date formats, Discord locales. |
| `presentation` | Text summaries, role-aware verdict, CSV/text detail views. |
| `render`, `render_assets` | Rust-built SVG rasterised by resvg (scoreboard, damage chart, gold chart); font DB built once; Data Dragon artwork cache. |
| `reports` | Rank refresh; one `Report` model for daily/weekly/monthly periods (stats, ranks, honours, pairs) rendered as text; timezone-aware scheduler with a durable once-per-day marker. |
| `report_card` | Report card PNG (hero, group LP, honours, player table, pairs). |
| `discord` | Serenity gateway (GUILDS intent), commands, autocomplete, buttons, permission checks, outbox delivery, optional commentary worker. |
| `llm` | Optional OpenAI-compatible Chat Completions adapter. |

### Data flow

1. Load config and secrets, open or create the versioned database, seed the roster once.
2. Refresh ranks, resolve unresolved accounts, and baseline each new account's existing history silently.
3. Poll recent match IDs. Global IDs deduplicate shared games. Unseen recent matches are fetched, normalised,
   rank-enriched and stored together with an outbox row in one transaction. Too-old and unparseable
   matches are marked seen.
4. Optional Spectator records pending games and pre-game ranks. Disappearance triggers result lookup,
   with a 15-minute timeout.
5. The delivery worker sends or edits Discord messages and acknowledges outbox rows only after success.
6. The optional commentary worker appends AI text afterwards. Failure leaves the factual message untouched.
7. The scheduler queues one report per local day (weekly on Sunday). Retention trims old delivered data.

### LP attribution

The change is `after − before`. `before` is the Spectator pre-game rank or, in history mode, the newest
snapshot at or before the game start that was taken by the current process. `after` is fetched within 20
minutes of the game ending and retried while League-V4 lags. Anything else is reported as unknown.

## Dependencies

Tokio, Serenity, reqwest (rustls), serde/serde_json, toml, dotenvy, rusqlite (bundled SQLite),
chrono/chrono-tz, anyhow, tracing, clap, resvg, base64. `Cargo.lock` is committed.

## Status

Done (see REVIEW.md for details):

- Initial rewrite: storage, tracking, Discord, reports, PNG scoreboard, optional LLM.
- Audit pass (2026-10-20):
  - LP changes for history-mode games
  - failure isolation in the tracker loop
  - no refetch loops for unparseable matches
  - robust and local-capable LLM adapter
  - role-aware verdict
  - damage and gold charts
  - public `/lastgame` and `/rank`
  - `removeplayer` autocomplete
  - channel permission checks
  - cached font database
  - configurable Riot pacing
  - sorted report rows
- Localization pass: typed `Lang`, compile-checked VI/EN catalog (`src/i18n.rs`), localized command errors and slash-command descriptions, `render-scoreboard --language all --charts`.
- Report cards: daily/weekly/monthly PNG cards with text fallback, monthly schedule (`reports.monthly`), `/config rankreport period:monthly`, `render-report` preview.

Verified: `cargo build --locked`, an offline `check`, a real `render-scoreboard` (scoreboard, both charts
and the verdict inspected visually), and a `run --no-discord --once` sweep against a scratch copy of the
database.

Not verified live: the Discord gateway, slash commands, autocomplete, buttons, message edits, permission
checks against a real guild, and real LLM providers.

## Constraints

No tests, CI/CD, benchmarks or deployment changes in this repository. The legacy VPS deployment is never
modified. Nothing is pushed or published.
