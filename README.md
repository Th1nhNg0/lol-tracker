<div align="center">

# LoL Tracker

**A self-hosted Discord bot for friend groups who play League of Legends together.**
It watches your ranked games, posts a clean scoreboard the moment each one ends,
adds optional AI commentary, and sums up the day, week and month in LP.

[![Rust](https://img.shields.io/badge/Rust-2024-orange?logo=rust)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![Languages](https://img.shields.io/badge/bot%20language-English%20%7C%20Ti%E1%BA%BFng%20Vi%E1%BB%87t-green)](#configuration)

<img src="docs/images/scoreboard-en.webp" alt="Match scoreboard" width="860">

</div>

## Features

- **Post-game scoreboard** – one image with all ten players in tinted team blocks: KDA with kill participation, damage dealt and taken, vision, CS, items, summoner spells and runes. A strip between the teams compares objectives (dragons, Baron, Herald, towers, inhibitors), total kills and total gold.
- **LP and rank per game** – Solo/Duo and Flex, with the LP gained or lost shown next to each tracked player. `MVP` and `WORST` labels sit on the portraits.
- **Daily, weekly and monthly reports** – one LP bar per player, the biggest climber and the biggest drop framed, and six honours (kills, deaths, damage, farm, vision, win rate) on weekly and monthly cards.
- **Optional AI commentary** – a short, funny line per tracked player with a title and emoji, written by any OpenAI-compatible model in the voice *you* define in a `prompt.md`. Custom Discord emojis are supported.
- **Slash commands** – `/lastgame`, `/rank`, `/players` and admin `/config` for channels, roster and on-demand reports.
- **English and Vietnamese** – messages, images and slash-command descriptions.
- **Small and self-contained** – one Rust binary. SQLite is bundled, fonts are embedded, images are drawn with `resvg`. No browser, Python or Node. Around 20 MB of RAM.

## Screenshots

<table>
<tr>
<td width="50%"><b>Daily report</b><br><img src="docs/images/report-daily-en.webp" alt="Daily report"></td>
<td width="50%"><b>Weekly report</b><br><img src="docs/images/report-weekly-en.webp" alt="Weekly report"></td>
</tr>
<tr>
<td colspan="2"><b>Monthly report</b> (same layout as weekly, over the calendar month)<br><img src="docs/images/report-monthly-en.webp" alt="Monthly report" width="50%"></td>
</tr>
</table>

Everything above is rendered from anonymised sample data. Set `language = "vi"` for Vietnamese output.

### What a result looks like

The scoreboard is posted first, then the AI commentary is edited into the message text a few seconds later:

> 🛡️ **Atlas** — *Stone Wall*: Ornn soaked 48k damage and still showed up for 18 assists, but Malphite out-traded him all game.
> 🔥 **Pixel** — *Jungle Furnace*: 20 takedowns, 13 deaths. The fire was real, the exit wasn't.
> 🌙 **Zephyr** — *Midnight Shift*: Even farm with Yasuo, three extra deaths. Quiet game, loud ending.
>
> **Key player:** Juniper — 43k damage from support, the only reason it was close.

*(Illustrative text. The tone is entirely controlled by your `prompt.md`.)*

## Quick start

### Requirements

- A recent stable **Rust** toolchain (edition 2024) and a C compiler (for the bundled SQLite).
- A **Riot developer API key** from <https://developer.riotgames.com/> with Account-V1, Match-V5 and League-V4. Development keys expire after 24 hours; apply for a personal or production key for a long-running bot.
- A **Discord bot** invited with the `bot` and `applications.commands` scopes. In the channels it posts to it needs **View Channel**, **Send Messages** and **Attach Files**. No administrator rights and no privileged intents; only the `GUILDS` intent is used.

### Install and configure

```sh
git clone https://github.com/Th1nhNg0/lol-tracker.git
cd lol-tracker
cargo build --release --locked

# Keep secrets and player names outside the repository.
umask 077
mkdir -p ~/.config/lol-tracker
cp config.example.toml ~/.config/lol-tracker/config.toml
cp players.example.json ~/.config/lol-tracker/players.json
cp .env.example ~/.config/lol-tracker/.env
```

1. `.env` – set `RIOT_API_KEY` and `DISCORD_TOKEN`.
2. `config.toml` – set `riot.platform` (for example `vn2`, `na1`, `euw1`), `discord.guild_id`, `games_channel_id` and `rank_channel_id`.
3. `players.json` – your friends' Riot IDs, with an optional display nickname:

   ```json
   [
     { "game_name": "Nova", "tag_line": "VN2", "nickname": "Nova" }
   ]
   ```

   This file only seeds a *new* database. After that, use `/config addplayer` and `/config removeplayer`.

### Check, then run

```sh
bin=./target/release/lol-tracker
args="--config $HOME/.config/lol-tracker/config.toml --env-file $HOME/.config/lol-tracker/.env"

$bin $args check                    # validate config and storage, no network
$bin $args probe                    # read-only check of the Riot key and Discord token
$bin $args run --register-commands  # first run: also installs the slash commands
$bin $args run                      # every later start
```

New players are baselined silently the first time they are seen, so the bot never replays old games. Only games that ended within `catch_up_hours` (default 3) of a sweep are announced.

> **Run only one instance per Discord application and database.** For experiments, use a separate database and `run --no-discord`, which tracks without touching Discord and keeps notifications queued.

## Slash commands

| Command | What it does | Who |
|---|---|---|
| `/lastgame` | Posts the latest scoreboard, with AI commentary when enabled | everyone |
| `/rank` | Current Solo/Duo and Flex rank of everyone | everyone |
| `/players` | The tracked roster | everyone (private reply) |
| `/config channel` | Set the channel for match results | Manage Server |
| `/config rankchannel` | Enable, disable or move the report channel | Manage Server |
| `/config addplayer` | Track an account by `Name#Tag` | Administrator |
| `/config removeplayer` | Stop tracking an account (with autocomplete) | Administrator |
| `/config rankreport` | Post a daily, weekly or monthly report now | Manage Server |

Reports are posted automatically once a day at `reports.hour:minute` (default 23:59, `Asia/Ho_Chi_Minh`): the daily report on most days, the weekly one on Sundays, the monthly one on the last day of the month.

## Configuration

Relative paths are resolved against the config file's directory. Unknown keys are rejected so typos fail loudly. See [`config.example.toml`](config.example.toml) for the annotated file.

| Key | Default | Meaning |
|---|---|---|
| `language` | `vi` | `vi` or `en` for all bot output |
| `players_file` | `players.json` | Seed roster for a new database |
| `database` | `data/tracker.db` | SQLite file |
| `retention_days` | `180` | How long old matches and snapshots are kept |
| `riot.platform` | `vn2` | Riot platform routing value |
| `riot.history_interval_secs` | `60` | Seconds between match-history sweeps |
| `riot.catch_up_hours` | `3` | Oldest game that is still announced after downtime |
| `riot.history_count` | `20` | Recent matches fetched per player per sweep |
| `riot.request_interval_ms` | `1250` | Minimum gap between Riot requests (fits a development key) |
| `discord.guild_id` | – | The one server the bot serves |
| `discord.games_channel_id` | – | Where results are posted |
| `discord.rank_channel_id` | `0` | Where reports are posted (`0` disables reports) |
| `reports.enabled`, `monthly`, `timezone`, `hour`, `minute` | on, on, `Asia/Ho_Chi_Minh`, `23`, `59` | Report schedule |
| `llm.*` | disabled | See below |

Secrets are read only from the environment or the dotenv file you pass with `--env-file`: `RIOT_API_KEY`, `DISCORD_TOKEN` and, optionally, `LLM_API_KEY`.

## AI commentary

Off by default; the bot works fully without it. To enable it, point it at any OpenAI-compatible chat-completions endpoint:

```toml
[llm]
enabled = true
model = "gpt-4o-mini"
base_url = "https://api.openai.com/v1"      # OpenRouter, Ollama (http://localhost:11434/v1), a local gateway...
prompt_file = "prompt.md"                   # optional persona and style
send_names = false                          # true lets the prompt use inside jokes about your friends
timeout_secs = 90
```

- Put `LLM_API_KEY` in `.env` (not needed for a server on `localhost`).
- Copy [`prompt.example.md`](prompt.example.md) to `prompt.md` and make it yours. It controls tone and style only; the JSON reply format is enforced by the bot.
- **Custom emojis:** list Discord emoji codes such as `<:name:123456789012345678>` in the prompt file with a short description of each. The bot only accepts codes it finds there, so the model cannot invent one. Emojis uploaded as *application emojis* work everywhere.
- The model receives final statistics of all ten players (the other five are context), the last few comments so it avoids repeating itself, and display names only if `send_names = true`. Never Riot IDs, PUUIDs or Discord IDs. That data goes to the provider you choose.
- Games without commentary, such as imported history, get theirs the first time someone runs `/lastgame`.

## Run it as a service

```ini
# /etc/systemd/system/lol-tracker.service
[Unit]
Description=LoL tracker Discord bot
After=network-online.target
Wants=network-online.target

[Service]
WorkingDirectory=/opt/lol-tracker
ExecStart=/opt/lol-tracker/lol-tracker --config /opt/lol-tracker/config.toml --env-file /opt/lol-tracker/.env run
Restart=always
RestartSec=10
MemoryMax=300M

[Install]
WantedBy=multi-user.target
```

State lives in one SQLite file in WAL mode. To back it up, stop the service or use SQLite's `.backup`. Pending Discord posts are kept in the database and retried, so a restart or an outage loses nothing.

## Development

```sh
cargo build --locked
./target/debug/lol-tracker --env-file .env render-scoreboard --language all --output data/board.png
./target/debug/lol-tracker --env-file .env render-report --period weekly --language all --output data/report.png
```

`render-scoreboard` draws a stored or recent real match to a PNG (add `--post` to send one image to the games channel). `render-report` does the same for a report and prints its text form. Neither imports matches, touches baselines or calls an LLM.

| Path | Role |
|---|---|
| `src/tracker.rs` | History sweeps, rank snapshots, LP attribution |
| `src/riot.rs` | Rate-limited Riot client |
| `src/store.rs` | SQLite storage and the delivery outbox |
| `src/discord.rs` | Gateway, slash commands, delivery and commentary workers |
| `src/render.rs`, `src/report_card.rs`, `src/render_assets.rs` | SVG images rasterised by `resvg`; Data Dragon and objective artwork cache |
| `src/reports.rs`, `src/presentation.rs` | Report aggregation and the role-aware MVP / worst-player score |
| `src/llm.rs` | Optional commentary client |
| `src/i18n.rs` | Compile-checked English and Vietnamese catalogue |

Artwork (champions, items, objective icons) is fetched from public Data Dragon and Community Dragon URLs at render time and cached next to the database; nothing from Riot is committed. If it cannot be fetched, images fall back to placeholders. No Riot API key is ever sent to those hosts.

## Legal

LoL Tracker is not endorsed by Riot Games and does not reflect the views or opinions of Riot Games or anyone officially involved in producing or managing Riot Games properties. Riot Games and all associated properties are trademarks or registered trademarks of Riot Games, Inc.

The bundled Be Vietnam Pro fonts are licensed under the SIL Open Font License 1.1 ([`assets/fonts`](assets/fonts)). The code is released under the [MIT License](LICENSE).
