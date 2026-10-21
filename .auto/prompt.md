# Autoresearch: prompt.md làm recap hài hơn và tự nhiên hơn

## Objective
`prompt.md` is the optional style/persona file the bot appends above the fixed contract in
`src/llm.rs`; the model writes one line per tracked friend (emoji + title + comment) plus a key
player line, posted into the group's Discord channel. The goal is recaps that read like a friend
joking about the game — funnier, with a more natural voice — without losing accuracy or the
things the bot needs to work (emoji ids, inside jokes, the 16 KiB limit).

Only `prompt.md` may change. Everything else is fixed.

## Metrics
- **Primary**: `judge_delta` (points, higher better) — mean, over the sampled matches, of
  `((funny + natural)/2 candidate − (funny + natural)/2 baseline)`, from a blind pairwise judge
  that sees the real match payload and both recaps in random order.
- **Secondary**: `win_pct` (share of matches where the judge preferred the candidate),
  `delta_faithful` (accuracy score delta), `ungrounded_pct` (share of numbers in the recap that do
  not appear in the payload), `cand_funny`, `cand_natural` (absolute judge scores, drift-prone,
  for monitoring only), `over_140`, `dup_title`, `emoji_invalid`, `missing_players`,
  `comment_len_avg`, `prompt_bytes`.

## How to Run
`./.auto/measure.sh` → prints `METRIC name=number` lines and writes `.auto/last_run.json`.
`./.auto/checks.sh` (runs automatically after a passing benchmark) → shippability + faithfulness +
anti-cheat gates. Both are pinned in `.auto/pin.txt`; changing them is out of scope.

What the benchmark does, per run:
1. Samples 6 **real stored matches** at random from `.auto/fixtures` (387 games, 1 with the current
   payload shape, the rest older shapes without the newer counters) — fresh fixtures every run, so
   jokes tuned to one particular game do not help.
2. Builds the exact production system prompt (`prompt.md` + the contract and key-player clause
   copied from `src/llm.rs`) and the exact `context()` payload, including 0-3 `recent_comments`
   taken from previous generations.
3. Generates both arms with `openai-codex/gpt-6-luna` through `pi` (`--system-prompt` = the style
   file, user message = the payload), exactly like the provider call the bot makes.
4. Judges with `openai-codex/gpt-6.1-sol`, blind: arm order is shuffled, the judge only knows
   "A" and "B" plus the match numbers. It scores `funny`, `natural`, `faithful` 0-10 and picks a
   winner. Nothing in the judge prompt mentions the rules the candidate prompt claims to follow.
5. Computes the deterministic checks (faithfulness, emoji, length, duplicates) in Python.

The baseline arm (`./.auto/baseline.md`, the prompt as it was when the session started) is cached
per fixture, so only the candidate is regenerated each run; the frozen baseline draw is the
reference point of the paired comparison.

To inspect what a specific candidate actually wrote: `.auto/runs/<timestamp>-<seed>/<match>.json`
has both recaps, both score sets and the judge's note.

## Files in Scope
- `prompt.md` (root, git-ignored) — the only file to edit. `./.auto/best.md` keeps the last kept
  version so a discard can restore it.

## Off Limits
- `src/**`, `Cargo.toml`, `Cargo.lock` — `checks.sh` fails if they differ from HEAD.
- `.auto/measure.sh`, `.auto/harness.py`, `.auto/checks.sh`, `.auto/invariants.py`,
  `.auto/baseline.md`, `.auto/fixtures/**` — pinned by hash in `.auto/pin.txt`.
- The custom emoji codes (`<:name:id>`), the 10 friends' inside jokes by name, the 16384-byte
  limit, the language (Vietnamese) and the JSON contract: the bot appends that contract itself, so
  prompt.md must not restate output schema rules as if it owned them.

## Constraints
- `prompt.md` ≤ 16384 bytes (hard limit in `src/llm.rs`; over it the bot silently drops the style).
- `checks.sh` must pass: no dropped emoji ids, no missing inside-joke names, no meta-talk about
  benchmarks/judges, no missing/duplicated players, no invalid emoji, no field names in the text,
  faithfulness not worse than baseline, ungrounded numbers not worse than baseline +3pp.
- Do not chase the judge: no keyword stuffing, no gaming the rubric, no writing about the harness.
  A prompt that only wins this judge but reads worse for a human is a failed experiment.
- Keep it a *style guide*: persona, voice, what to notice in the numbers, how to be funny. Not a
  checklist that produces bot prose.

## What's Been Tried
- Setup: fixture pool dumped from 395 stored matches (`src/llm.rs::context()` output, 387 usable
  after dropping remakes and games with no tracked player). The baseline prompt is the version that
  was live on the server (14849 bytes, ~90% of the limit already).
- Baseline is heavily over-constrained: ~14 negative rules ("do not…", "never…", "max one…"),
  a ban list of metaphors, an explicit swearing quota, per-line length and structure limits, and a
  60-entry emoji catalog. Hypothesis: the density of prohibitions flattens the voice and makes every
  line read like it is dodging a checklist.
