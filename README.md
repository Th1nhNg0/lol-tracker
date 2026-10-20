# LoL Tracker

A standalone Rust League of Legends tracker for one Discord server. Riot supplies the facts; deterministic summaries work without an AI account, an agent, or any external runtime.

## Features

- Match-V5 history polling, restart catch-up and persistent duplicate suppression.
- Optional Spectator start announcements, edited into final results when available.
- Full-team scoreboards, individual outcomes, remakes, K/D/A, CS, damage, gold, vision, objectives and item/spell/rune IDs.
- Native Rust PNG scoreboards with champion splash art, portraits, spell/rune/item icons, compact statistics and text fallback; no browser.
- Solo/Duo and Flex ranks, observed rank history, per-match LP changes (Spectator or history mode), with unknown shown instead of guessed values.
- Last-game, rank and roster commands; administrator player management with Riot ID autocomplete.
- Deterministic, role-aware match verdict (MVP and worst player) that needs no LLM.
- Daily, weekly (Sunday) and monthly (last day of the month) report cards: group LP, honours, per-player statistics, rank and LP change per queue, and same-team pairs. Text fallback.
- Persistent result buttons: damage chart, gold-difference chart, event log, full scoreboard text and loadout CSV (CSV fallback when a chart cannot be drawn).
- Full Vietnamese/English localization (`language = "vi"` or `"en"`): messages, images, charts, reports, events and errors, plus localized slash-command descriptions that follow each member's Discord client language. No privileged message-content intent.
- SQLite state and a retryable Discord outbox. Optional, anonymized AI commentary using OpenAI-compatible APIs.

This is a fresh implementation, not a distribution of the reference application. See [plans.md](plans.md) for the architecture and decisions, and [REVIEW.md](REVIEW.md) for the technical audit and feature-parity record. Report image cards, videos, narration, screenshot analysis, custom guild emoji and private inside jokes are intentionally **not** included. No legacy history or source is bundled.

## Requirements

- Current stable Rust (edition 2024), Cargo and a C compiler for bundled SQLite.
- A Riot developer API key with access to Account-V1, Match-V5 and League-V4; Spectator-V5 access only if enabled. Development keys normally expire after 24 hours. Obtain/renew keys at <https://developer.riotgames.com/>.
- For Discord mode: a bot token, one guild and target text channels. Invite with `bot` and `applications.commands` scopes. Grant **View Channel**, **Send Messages**, **Embed Links** and **Attach Files** in the target channels; startup and `/config` verify these and refuse a channel that lacks them. No administrator privilege is needed by the bot.
- An LLM key is **not** required. No Python, Node, Hermes, browser, FFmpeg or graphics service is needed at runtime.
- PNG rendering bundles fresh Be Vietnam Pro Regular/Bold fonts under SIL OFL 1.1; provenance and license are in `assets/fonts/`. Riot artwork is fetched from public Data Dragon endpoints without sending the Riot API key. Missing artwork degrades to placeholders.

## Setup

Keep actual credentials, player identities and configuration outside this repository, preferably in an owner-only directory. Example using a generic private directory:

```sh
umask 077
mkdir -p "$HOME/.config/lol-tracker"
cp config.example.toml "$HOME/.config/lol-tracker/config.toml"
cp players.example.json "$HOME/.config/lol-tracker/players.json"
cp .env.example "$HOME/.config/lol-tracker/.env"
```

Edit these private files:

1. `.env`: set `RIOT_API_KEY` and `DISCORD_TOKEN` (omit Discord token for tracking-only mode).
2. `config.toml`: set Riot platform, guild ID, games channel and optionally rank channel. Leave `llm.enabled = false` to use deterministic commentary only. Spectator is off by default, matching the reference deployment's active behavior.
3. `players.json`: use real Riot IDs, optionally with display nicknames:

```json
[
  { "game_name": "ExamplePlayer", "tag_line": "EXAMPLE", "nickname": "Player" }
]
```

Build and validate locally:

```sh
cargo build --locked
./target/debug/lol-tracker \
  --config "$HOME/.config/lol-tracker/config.toml" \
  --env-file "$HOME/.config/lol-tracker/.env" check
```

`check` is the default when no subcommand is provided. It validates configuration and creates/seeds **local** storage without network access. It never prints credential values. Environment variables override the explicitly supplied dotenv file; no implicit search through parent directories or agent stores occurs.

Optional read-only credential check:

```sh
./target/debug/lol-tracker \
  --config "$HOME/.config/lol-tracker/config.toml" \
  --env-file "$HOME/.config/lol-tracker/.env" probe
```

`probe` resolves only the first configured Riot account and checks the Discord current-bot REST endpoint if a token exists. It does not start a gateway session, install commands, read chat or send messages. Success verifies credentials/routing, not every endpoint permission or future availability.

## Run

**Do not run two bot instances against the same Discord application or database.** For safe local development while another deployment is running, use tracking-only mode. Never use `--register-commands` on the live application's token merely to verify a build.

```sh
# Collect locally; no Discord gateway, command writes, messages or LLM calls.
./target/debug/lol-tracker --config /path/to/private/config.toml \
  --env-file /path/to/private/.env run --no-discord

# One local collection sweep, then exit. This still updates local SQLite state.
./target/debug/lol-tracker --config /path/to/private/config.toml \
  --env-file /path/to/private/.env run --no-discord --once

# Intentional Discord operation, only when ready to use this bot deployment.
# First run: explicitly install this tracker's guild slash commands.
./target/debug/lol-tracker --config /path/to/private/config.toml \
  --env-file /path/to/private/.env run --register-commands

# Subsequent starts: no command registration writes.
./target/debug/lol-tracker --config /path/to/private/config.toml \
  --env-file /path/to/private/.env run
```

Ctrl-C or SIGTERM shuts down workers; pending results and notifications remain in SQLite. For an optimized binary, use `cargo build --release --locked` and `target/release/lol-tracker` with the same arguments. No deployment scripts or service modifications are included.

**Tracking-only mode retains notifications in the outbox.** Starting Discord mode later will deliver those queued results/reports. Use a separate database when collecting disposable development data rather than later pointing it at your real Discord server.

### Render a scoreboard preview

```sh
# Local, Git-ignored config.toml/.env. Save a private PNG without sending.
./target/debug/lol-tracker --env-file .env render-scoreboard \
  --output data/scoreboard-preview.png

# Explicitly post ONE image. No gateway or command registration.
./target/debug/lol-tracker --env-file .env render-scoreboard \
  --output data/scoreboard-preview.png --post
```

```sh
# Every image (scoreboard, damage chart, gold chart) in both languages, for review.
./target/debug/lol-tracker --env-file .env render-scoreboard \
  --output data/preview/scoreboard.png --language all --charts
```

`--language vi|en|all` overrides the configured language; `all` writes one file per language (`-vi`/`-en` suffixes) and, with `--post`, posts one preview per language. `--charts` also writes the damage and gold charts (`-damage`/`-gold` suffixes). Use `--match-id PLATFORM_matchnumber` for an exact real match. Otherwise use the latest locally stored match or, if none is stored yet, the newest published match among one recent ID per roster member. Preview fetching does not import old matches, change baselines, drain notifications or call an LLM (it may cache the match timeline locally). Images are labeled as previews and use the configured report timezone. Champion portraits, splash art and spell/rune/item icons come from Riot Data Dragon. The keyless artwork cache sits beside the database in `artwork/`, with 24-hour metadata caching and a 64 MiB/30-day file budget enforced during rendering. No Riot artwork is committed or bundled with source. An external private output path is also supported.

### Localization

All user-facing text lives in [`src/i18n.rs`](src/i18n.rs): one `Msg` catalog in which every entry has exactly one string per language, plus queue, role, objective, monster and building names, and date formats. A missing translation is a compile error, so adding a language means adding a `Lang` variant and one column. Logs, CSV headers and the LLM prompt stay English; KDA, CS, KP, LP and champion names are not translated. Command errors meant for members are localized; unexpected internal errors show a localized generic message and are logged in English. Slash-command descriptions are registered in English (Discord's default) with Vietnamese localizations, so re-run `--register-commands` after upgrading.

Normal result notifications and `/lastgame` reuse the renderer with text fallback. Damage and gold buttons render PNG charts with the same renderer. Full scoreboards, events and loadout data remain accessible through the buttons.

### Render a report card preview

```sh
# Weekly card and text in both languages. Never posts.
./target/debug/lol-tracker --env-file .env render-report \
  --period weekly --language all --output data/preview/report.png
```

`--period daily|weekly|monthly`. Uses stored matches for the window; if none are stored yet (a fresh database), it fetches up to `--max-matches` (default 15) recent real matches read-only and does not track them (raw match data is cached under `data/preview-matches/` so repeated previews are instant). LP changes need rank snapshots spanning the window, so a new database shows them as unknown until the bot has been observing ranks for a full period.

### Discord commands

All commands are guild-restricted. `/lastgame` and `/rank` reply in the channel (as in the original bot); `/players` and every `/config` reply are private to the invoking member, as are button responses. Result/report notifications go to the configured channels. Mentions are disabled.

| Command | Behavior | Permission |
| --- | --- | --- |
| `/lastgame` | Latest scoreboard and detail buttons | Any member |
| `/rank` | Fetch current tracked Solo/Duo and Flex ranks | Any member |
| `/players` | Current roster | Any member |
| `/config channel channel:…` | Change game notification channel | Manage Server |
| `/config rankchannel [enabled:…] [channel:…]` | Configure scheduled reports | Manage Server |
| `/config addplayer riot_id:Name#Tag [nickname:…]` | Resolve, baseline and persist an account | Administrator |
| `/config removeplayer riot_id:Name#Tag` | Stop tracking an account (Riot ID autocompletes) | Administrator |
| `/config rankreport period:daily\|weekly\|monthly` | Queue an immediate report card | Manage Server |

Permissions are checked in the handler, not just the command picker. At most two slow interactions run at once. Registration updates only this project's four named guild commands; it does not clear other guild commands or any global commands. Previously registered global commands from another implementation must be managed separately by an operator. After upgrading from a build without `removeplayer` autocomplete, start once with `--register-commands` to update the command definitions.

### Configuration and state

`config.example.toml` lists all settings. Paths are relative to the config file, not the working directory. A process handles one platform and one guild; use separate configuration/databases for others. The database records these bindings and rejects accidental reuse with a different platform/guild.

- **Roster:** `players_file` seeds a new database once. After that, Discord administrator commands update SQLite; editing the seed file does not overwrite those changes. No live config reload; restart after TOML/secret changes. To reset a disposable database, stop the process and move/remove its database plus associated WAL/SHM files; this also discards history and queued work.
- **Channels:** persisted `/config` channel overrides take precedence over TOML. A zero rank channel suppresses automatic scheduling until set. Report enablement can also be overridden in SQLite.
- **Polling:** history defaults to 60 seconds, 20 recent IDs per player; optional Spectator defaults to 15 seconds. All Riot requests share one pacing gate (`riot.request_interval_ms`, default 1250 ms, which fits a development key's 100 requests / 2 min) plus Retry-After backoff. Spectator with a large roster needs a production key or a longer interval. Rank refreshes run every 30 minutes (first, before any history sweep) and after ranked results. A failing step (e.g. Riot outage) is logged and retried without stopping the others.
- **First launch/new player:** baseline existing history silently. Subsequent starts recover unseen results within `catch_up_hours` (default three) and the configured ID-count window. This is bounded catch-up, not a complete historical backfill. Baselines are per-account so a newly added account cannot suppress another member's shared result.
- **LP:** the per-match change compares a "before" rank with the rank fetched just after the game. "Before" is the Spectator pre-game rank or, in history mode, the newest snapshot taken at or before the game start **by the current process**, so a restart never folds downtime games into one delta. The post-game lookup must happen within 20 minutes of the game ending; League-V4 lag is retried briefly. An unchanged rank shows unknown rather than zero. Reports compare observed snapshots and label partial baselines. Cross-tier LP uses a normalized tier/division value; it is not MMR.
- **Reports:** posted at the configured local time (default 23:59 Asia/Ho_Chi_Minh) as a PNG card with a text fallback. Daily and weekly reports use rolling 24-hour/seven-day windows; the monthly report covers the calendar month so far. Sunday's weekly report replaces the daily one; on the last day of the month the monthly report replaces the daily one, and if that day is a Sunday both the weekly and monthly reports are posted (`reports.monthly = false` disables monthly reports). Weekly and monthly cards add honours (most kills, most deaths, top damage per game, CS/min, vision/min, best win rate with minimum game counts) and same-team pairs. Remakes do not count. LP change per queue compares the rank before the window with the latest one; a single observation shows as unknown, never as zero. Report dates and queued payloads persist atomically. Startup after today's scheduled time can catch up today's report, but not every missed calendar day.
- **Retention:** old delivered match data, snapshots and deduplication rows are trimmed after `retention_days`; latest rank snapshots, pending games and unsent result data survive. Timeline cache retains at most 50 matches. SQLite reuses freed pages; this is age retention, not a hard disk quota. Back up SQLite consistently (stop this local instance or use SQLite's backup facility), including uncheckpointed WAL data.
- **Delivery:** each notification retries independently with bounded backoff, so one failing channel does not block other notifications. Stored results and delivery intent are atomic, but Discord acknowledgement is not: a crash after a successful send can still cause a duplicate. Sending is **at-least-once**, not exactly-once. Edits only fall back to a new message on a definite missing-message response.
- **Unavailable data:** Riot custom/private games and unavailable timelines cannot be reconstructed from history. A failed API call never implies a player is out of game. If publication remains unavailable, a timeout message leaves history polling enabled.

Charts and CSV fallbacks are bounded; CSV names are quoted and protected against spreadsheet formula injection. Item/spell/rune IDs in the loadout CSV are raw Riot identifiers. Result embeds are bounded; the scoreboard button always supplies full stored detail.

## Optional LLM

The bot is fully functional with this section untouched:

```toml
[llm]
enabled = false
provider = "openrouter"
model = ""
base_url = "https://openrouter.ai/api/v1"
timeout_secs = 15
```

To enable, set `enabled = true`, choose a `model`, and set the provider's **API root** as `base_url` (the application appends `/chat/completions`): `https://openrouter.ai/api/v1` for OpenRouter, `https://api.openai.com/v1` for OpenAI, or any other OpenAI-compatible HTTPS endpoint. Set `LLM_API_KEY` privately. A server on the same machine (Ollama, llama.cpp, LM Studio) may use `http://localhost:PORT/v1` and needs no key. `provider` is a descriptive label; there is one provider-independent HTTP contract and no provider-specific extensions. Replies may be a bare JSON array or one wrapped in a Markdown fence. Comments are sanitized and truncated; links, missing/duplicate players, `length`/`content_filter` stops or timeouts discard the AI text.

### Persona and style (optional)

Set `prompt_file = "prompt.md"` under `[llm]` to control the commentary voice. Copy [`prompt.example.md`](prompt.example.md), edit it, and keep your copy private (`prompt.md` is git-ignored; relative paths resolve against the config file). Without it a short built-in style is used. The file only sets tone and style: the output contract (a JSON array of `player`, `title`, `comment`, `emoji`) is appended by the bot. Each reply line renders as `emoji Name — title: comment`. The model also receives the last few comments so it avoids repeating jokes, and all ten participants' statistics (the other five are context only). `send_names = true` additionally sends tracked players' display names so the prompt can use inside jokes; it is off by default. `/lastgame` generates and stores commentary on first use for games that have none, such as imported history.

Core result delivery happens first. A separate worker then appends the AI block below the deterministic summary when both fit in one message (otherwise the AI block replaces it), keeping the factual scoreboard image and buttons. Requests carry numeric participant labels, champions, roles, teams and final statistics, never Riot IDs, PUUIDs, Discord IDs, secrets or chat history; display names only with `send_names = true`. That data still goes to your chosen provider, so review its terms and privacy policy. Missing credentials, unsupported models, malformed replies, provider failures or edit failures leave the deterministic summary intact. No LLM is used by commands, collection, reports, permissions or state transitions. AI commentary is explicitly labeled and may still be inaccurate.

### ChatGPT account sign-in investigation

Current first-party documentation describes official **Sign in with ChatGPT** for eligible open-source/local apps, including explicit ChatGPT-plan inference authorization. Identity sign-in alone is insufficient, and ChatGPT Plus does **not** give general API-key access. That integration needs dynamic client registration, PKCE, validated ID tokens, dedicated scopes, token refresh and account/usage management; availability and limitations may change.

This release deliberately uses the simpler API-key integration and does not implement OAuth. It never reads browser cookies, ChatGPT sessions, another application's OAuth credentials or agent credential stores. This is a design choice, **not** a claim that no official sign-in flow exists. See the [official open-source overview](https://developers.openai.com/siwc/token-sharing-open-source) and [sign-in contract](https://developers.openai.com/siwc/token-sharing-open-source/sign-in).

## Repository and licensing

`src/` contains focused configuration, Riot, storage, tracking, presentation, reporting, Discord and optional LLM modules. Cargo.lock pins dependencies. No tests, CI/CD, benchmarks or extra framework/tooling are included.

The new code is MIT-licensed; see [LICENSE](LICENSE). No license file was found for the reference application, so its implementation/assets were not reused or relicensed. Dependencies retain their own licenses. Riot Games, League of Legends, OpenAI and Discord marks belong to their owners; this is an unofficial community tool, not endorsed by them. Follow Riot's developer policies and register/obtain an appropriate key before distributing a hosted service.

Bundled Be Vietnam Pro fonts retain their SIL Open Font License; see [`assets/fonts/OFL.txt`](assets/fonts/OFL.txt) and the pinned upstream provenance in [`assets/fonts/README.md`](assets/fonts/README.md). Runtime Riot artwork is not covered by this repository's MIT license. Review [Riot's fan-project policy](https://www.riotgames.com/en/legal) and [Data Dragon documentation](https://developer.riotgames.com/docs/lol) before distributing or hosting.

> LoL Tracker was created under Riot Games' "Legal Jibber Jabber" policy using assets owned by Riot Games. Riot Games does not endorse or sponsor this project.

Before publishing your own copy, inspect the file list and Git history. Never add `.env`, actual configuration/rosters, SQLite data, the private reference snapshot, generated user data or credentials—even if you later delete them. This project does not publish or push anything automatically.
