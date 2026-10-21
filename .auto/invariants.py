#!/usr/bin/env python3
"""Shippability and anti-cheat invariants for prompt.md, plus the faithfulness gate.

Runs after the benchmark. Anything here is a reason not to keep a candidate: the bot
must still work (emoji ids, lore, size limit) and the recap must not get funnier by
lying, dropping players or reading field names out loud.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
AUTO = ROOT / ".auto"
EMOJI_RE = re.compile(r"<a?:[A-Za-z0-9_]+:\d+>")
# The friends whose inside jokes live in the prompt; the wording may change, the people may not vanish.
LORE = ["Mục Sư", "Crasky", "Y Đức Thầy Thuốc", "Ma đạo vibecode", "Đồ Sát Ricon",
        "Ranarok Arrival", "Ăm Chã Hơm", "Bị coin chơi", "I am on the case", "ErikPui"]
META = ["benchmark", "autoresearch", "harness", "judge", "fixture", "METRIC", "BẢN A", "BẢN B", "bản A", "bản B"]
problems: list[str] = []


def fail(message: str) -> None:
    problems.append(message)


def main() -> int:
    candidate = (ROOT / "prompt.md").read_text(encoding="utf-8")
    baseline = (AUTO / "baseline.md").read_text(encoding="utf-8")

    size = len(candidate.encode())
    if size > 16384:
        fail(f"prompt.md is {size} bytes; src/llm.rs rejects more than 16384")
    known = set(EMOJI_RE.findall(baseline))
    now = set(EMOJI_RE.findall(candidate))
    if known - now:
        fail(f"{len(known - now)} custom emoji codes were dropped, e.g. {sorted(known - now)[:2]}")
    for name in LORE:
        if name not in candidate:
            fail(f"inside joke for {name!r} disappeared")
    for word in META:
        if word in candidate:
            fail(f"prompt.md mentions {word!r}: the style notes must not talk about how it is graded")

    run = json.loads((AUTO / "last_run.json").read_text(encoding="utf-8"))
    if run["format_fail"]:
        fail(f"{run['format_fail']} fixtures got no usable JSON")
    if run["missing_players"]:
        fail(f"{run['missing_players']} tracked players were missing or duplicated")
    if run["emoji_invalid"] > run.get("base_emoji_invalid", 0) + 2:
        fail(f"{run['emoji_invalid']} lines used an emoji the catalog does not contain "
             f"(baseline {run.get('base_emoji_invalid', 0)}); the bot posts those without one")
    if run["field_leak"]:
        fail(f"{run['field_leak']} lines read a field name out loud")
    # The judge's faithful score is a 0-10 human-ish rating of a handful of lines: across logged
    # runs its per-pair delta has sd 3.1, so at 12 fixtures the standard error is ~0.9. A -2.5 gate
    # is ~2.8 sigma, i.e. it fires on a real accuracy drop, not on a lucky baseline draw. Actual
    # fabrication is caught exactly by the unmatched-number check below.
    if run["delta_faithful"] < -2.5:
        fail(f"faithfulness dropped by {abs(run['delta_faithful'])} points vs baseline")
    if run["ungrounded_pct"] > run["base_ungrounded_pct"] + 5.0:
        fail(f"unmatched numbers rose to {run['ungrounded_pct']}% (baseline {run['base_ungrounded_pct']}%)")
    if run["over_140"] > run.get("base_over_140", 0) + 2:
        fail(f"{run['over_140']} comments exceeded 140 characters (baseline {run.get('base_over_140', 0)})")
    if run["prompt_bytes"] != size:
        fail("last_run.json measured a different prompt.md than the one on disk")

    if problems:
        for problem in problems:
            print(f"FAIL: {problem}", file=sys.stderr)
        return 1
    print("invariants ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
