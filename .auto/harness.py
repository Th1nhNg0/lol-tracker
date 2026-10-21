#!/usr/bin/env python3
"""Prompt-quality benchmark for prompt.md.

Generates the exact commentary request the bot would send (system = prompt.md + the
fixed contract from src/llm.rs, user = the stored `context()` payload) with pi as the
model, then has a *different* model judge two blind recaps of the same match.

Only real, stored matches are used, sampled fresh every run, so tuning jokes to a
fixture cannot help. Nothing here looks at the rules a candidate prompt claims.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import random
import re
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
AUTO = ROOT / ".auto"
FIXTURES = AUTO / "fixtures"
EMOJI_RE = re.compile(r"<a?:[A-Za-z0-9_]+:\d+>")
NUM_RE = re.compile(r"\d+(?:[.,]\d+)?")
FIELD_RE = re.compile(r"\b(kp_pct|gold_share_pct|damage_share_pct|cs_per_min|cs_diff|gold_diff|damage_diff|level_diff|deaths_diff|dead_seconds|objective_damage|cc_seconds)\b")
HEX_RE = re.compile(r"^#?[0-9a-f]{6,}$", re.I)


# ---------------------------------------------------------------- model calls
def run_pi(system: str, user: str, model: str, timeout: int) -> str:
    cmd = [
        "pi", "--no-session", "--no-tools", "--no-skills", "--no-extensions",
        "--no-context-files", "--no-mcp", "--no-prompt-templates", "--no-approve",
        "--thinking", "off", "--model", model,
        "--system-prompt", system, "-p", user,
    ]
    done = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, cwd="/tmp")
    if done.returncode != 0:
        raise RuntimeError(f"pi exit {done.returncode}: {done.stderr.strip()[:400]}")
    return done.stdout.strip()


def call_json(system: str, user: str, model: str, timeout: int = 240, retries: int = 2,
              require_players: bool = True) -> dict | None:
    for attempt in range(retries + 1):
        try:
            text = run_pi(system, user, model, timeout)
            parsed = extract_json(text, require_players)
            if parsed is not None:
                return parsed
        except Exception as exc:  # noqa: BLE001 - provider flakiness is not a result
            if attempt == retries:
                print(f"  ! call failed: {exc}", file=sys.stderr)
        time.sleep(2 + attempt * 4)
    return None


def extract_json(text: str, require_players: bool = True) -> dict | None:
    """Production accepts an object with a `players` array, or a bare array."""
    start, end = text.find("{"), text.rfind("}")
    if start != -1 and end > start:
        try:
            value = json.loads(text[start : end + 1])
            if isinstance(value, dict) and (not require_players or isinstance(value.get("players"), list)):
                return value
        except json.JSONDecodeError:
            pass
    if not require_players:
        return None
    start, end = text.find("["), text.rfind("]")
    if start != -1 and end > start:
        try:
            rows = json.loads(text[start : end + 1])
            if isinstance(rows, list):
                return {"players": rows, "key_player": None}
        except json.JSONDecodeError:
            pass
    return None


# ------------------------------------------------------------------- prompt
def style_of(path: Path) -> str:
    return path.read_text(encoding="utf-8").strip()


def system_prompt(style: str, group: bool) -> str:
    suffix = (FIXTURES / ("suffix_group.txt" if group else "suffix_single.txt")).read_text(encoding="utf-8")
    return style + suffix


# ------------------------------------------------------------------ payload
def numbers_in(payload) -> set[str]:
    out: set[str] = set()

    def walk(value):
        if isinstance(value, dict):
            for v in value.values():
                walk(v)
        elif isinstance(value, list):
            for v in value:
                walk(v)
        elif isinstance(value, bool):
            return
        elif isinstance(value, (int, float)):
            out.add(fmt_num(value))
        elif isinstance(value, str) and value and NUM_RE.fullmatch(value):
            out.add(fmt_num(value))

    walk(payload)
    return out


def fmt_num(value) -> str:
    if isinstance(value, str):
        # Vietnamese writes 38541 as "38.541"; a real decimal has at most three leading digits
        # and never groups digits in threes twice.
        if re.fullmatch(r"\d{4,}", value):
            pass
        elif re.fullmatch(r"\d{1,3}([.,]\d{3})+", value):
            value = re.sub(r"[.,]", "", value)
        else:
            value = value.replace(",", ".")
        try:
            value = float(value)
        except ValueError:
            return ""
    if float(value).is_integer():
        return str(int(value))
    return f"{value:.2f}".rstrip("0").rstrip(".")


def emoji_catalog(prompt_text: str) -> set[str]:
    return set(EMOJI_RE.findall(prompt_text))


# ------------------------------------------------------------------- scoring
def analyze(text: dict, payload: dict, style: str, players: list[dict]) -> dict:
    """Deterministic faithfulness/format checks on one rendered candidate recap."""
    catalog = emoji_catalog(style)
    allowed = numbers_in(payload)
    flags = {"missing": 0, "emoji_invalid": 0, "over_140": 0, "ungrounded": 0,
             "field_leak": 0, "dup_title": 0, "lines": 0, "len_sum": 0, "tokens": 0}
    ids = {int(p["player"]) for p in players}
    seen: set[int] = set()
    titles: dict[str, int] = {}
    for row in text.get("players", []):
        try:
            pid = int(row.get("player"))
        except (TypeError, ValueError):
            flags["missing"] += 1
            continue
        if pid not in ids or pid in seen:
            flags["missing"] += 1
            continue
        seen.add(pid)
        comment = str(row.get("comment") or "")
        title = str(row.get("title") or "")
        emoji = str(row.get("emoji") or "")
        flags["lines"] += 1
        flags["len_sum"] += len(comment)
        if len(comment) > 140:
            flags["over_140"] += 1
        if FIELD_RE.search(comment):
            flags["field_leak"] += 1
        if emoji.startswith("<") and emoji not in catalog:
            flags["emoji_invalid"] += 1
        elif not emoji:
            flags["emoji_invalid"] += 1
        if title:
            key = title.lower().strip()
            titles[key] = titles.get(key, 0) + 1
        for token in NUM_RE.findall(comment + " " + title):
            value = fmt_num(token)
            if not value or HEX_RE.match(token):
                continue
            flags["tokens"] += 1
            if value not in allowed:
                flags["ungrounded"] += 1
    flags["missing"] += max(0, len(ids) - len(seen))
    flags["dup_title"] = sum(count - 1 for count in titles.values() if count > 1)
    return flags


def render(payload: dict, parsed: dict) -> str:
    """The exact text the bot posts, so the judge reads what Discord would show."""
    by_id = {int(p["player"]): p for p in payload["players"]}
    lines = []
    for row in parsed.get("players", []):
        try:
            player = by_id[int(row.get("player"))]
        except (TypeError, ValueError, KeyError):
            continue
        emoji = str(row.get("emoji") or "").strip()
        title = str(row.get("title") or "").strip()
        comment = str(row.get("comment") or "").strip()
        head = f"{emoji} " if emoji else ""
        tail = f" — {title}" if title else ""
        lines.append(f"{head}{player.get('name') or ('Player ' + str(player['player']))}{tail}: {comment}")
    key = parsed.get("key_player")
    if isinstance(key, dict) and key.get("reason"):
        try:
            player = by_id[int(key.get("player"))]
            lines.append(f"Nhân vật chính: {player.get('name')} — {key['reason']}")
        except (TypeError, ValueError, KeyError):
            pass
    return "\n".join(lines)


JUDGE_SYSTEM = """Bạn là một người trong nhóm bạn chơi Liên Minh, đang đọc recap trận vừa xong trong kênh Discord của nhóm. Bạn KHÔNG viết recap, bạn chỉ chọn bản nào bạn thích đọc hơn.

Bạn nhận được: số liệu trận đấu (JSON thật, do bot cung cấp cho cả hai bản) và hai bản recap A, B do hai cấu hình khác nhau viết. Hai bản được xáo trộn ngẫu nhiên, không phải bản nào "mới".

Chấm từng bản trên thang 0-10:
- funny: đọc có buồn cười thật không (bật cười, nhếch mép, muốn chụp màn hình gửi tiếp). Câu đùa sắc, bất ngờ, đúng chỗ. Đùa gượng, cố tỏ ra nguy hiểm, hoặc chửi thề rải khắp câu để gọi là hài thì điểm thấp.
- natural: có giống một người bạn nhắn thật không — nhịp câu tự nhiên, hóm hỉnh, không giống bot đang đọc bảng số hay đang chấm điểm. Câu nào cũng mở đầu giống nhau, mô-típ lặp, hay giọng văn hành chính thì điểm thấp.
- faithful: mọi con số trong recap có khớp đúng số liệu trận không. Sai số, bịa thêm diễn biến (phút thứ mấy, call team, ăn baron...) không có trong số liệu, hoặc gán số của người này cho người khác thì điểm rất thấp.

Đừng cộng điểm chỉ vì bản đó chửi nhiều hơn, dài hơn, nhiều số hơn. Đừng trừ điểm chỉ vì bản đó ngắn hơn. Đánh giá như người đọc thật: bản nào bạn muốn đọc trong group hơn.

Chỉ trả về JSON đúng định dạng, không thêm chữ nào khác:
{"A":{"funny":0,"natural":0,"faithful":0},"B":{"funny":0,"natural":0,"faithful":0},"winner":"A","note":"lý do ngắn"}"""


def judge(payload: dict, arm_a: str, arm_b: str, model: str) -> dict | None:
    facts = json.dumps(payload, ensure_ascii=False)
    user = f"""SỐ LIỆU TRẬN (nguồn duy nhất để kiểm tra số):
{facts}

BẢN A:
{arm_a}

BẢN B:
{arm_b}"""
    return call_json(JUDGE_SYSTEM, user, model, timeout=300, require_players=False)


# ------------------------------------------------------------------- fixtures
def load_fixtures(args: argparse.Namespace, rng: random.Random) -> list[dict]:
    manifest = json.loads((FIXTURES / "manifest.json").read_text(encoding="utf-8"))
    detailed, group, single = [], [], []
    for row in manifest:
        path = FIXTURES / row["file"]
        payload = json.loads(path.read_text(encoding="utf-8"))
        row = dict(row)
        row["payload"] = payload
        row["modern"] = any("wards_killed" in p for p in payload["players"])
        if row["tracked"] >= 2 and row["modern"]:
            detailed.append(row)
        elif row["tracked"] >= 2:
            group.append(row)
        elif row["tracked"] == 1:
            single.append(row)
    if args.fixture_ids:
        wanted = {i.strip() for i in args.fixture_ids.split(",")}
        chosen = [r for r in detailed + group + single if r["id"] in wanted]
    else:
        n_modern = min(args.modern, len(detailed))
        n_group = max(0, args.fixtures - n_modern)
        chosen = rng.sample(detailed, n_modern) + rng.sample(group, min(n_group, len(group)))
        if len(chosen) < args.fixtures:
            chosen += rng.sample(single, min(args.fixtures - len(chosen), len(single)))
    if not chosen:
        raise SystemExit("no fixtures selected")
    return chosen


def recent_comments(fixture_id: str, limit: int = 2) -> list[str]:
    """Previous games' lines, picked by fixture id so both arms and every rerun see the same ones."""
    path = AUTO / "recent_pool.jsonl"
    if not path.exists():
        return []
    rows = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
    rows = [r for r in rows if r.get("id") != fixture_id and r.get("text")]
    if not rows:
        return []
    rng = random.Random(int(re.sub(r"\D", "", fixture_id) or 0))
    return [r["text"] for r in rng.sample(rows, min(limit, len(rows)))]


def remember_recent(fixture_id: str, text: str) -> None:
    path = AUTO / "recent_pool.jsonl"
    with path.open("a", encoding="utf-8") as handle:
        handle.write(json.dumps({"id": fixture_id, "text": text}, ensure_ascii=False) + "\n")


# ----------------------------------------------------------------------- main
def generate_arm(style: str, fixture: dict, model: str, cache_dir: Path | None, fresh: bool) -> dict | None:
    payload = fixture["payload"]
    system = system_prompt(style, fixture["group"])
    key = hashlib.sha256((system + "\x00" + json.dumps(payload, ensure_ascii=False, sort_keys=True)).encode()).hexdigest()[:20]
    cached = cache_dir / f"{key}.json" if cache_dir else None
    if cached and cached.exists() and not fresh:
        try:
            return json.loads(cached.read_text(encoding="utf-8"))
        except json.JSONDecodeError:
            cached.unlink(missing_ok=True)
    parsed = call_json(system, json.dumps(payload, ensure_ascii=False), model)
    if parsed is not None and cached:
        cached.parent.mkdir(parents=True, exist_ok=True)
        cached.write_text(json.dumps(parsed, ensure_ascii=False), encoding="utf-8")
    return parsed


def mean(values: list[float]) -> float:
    return sum(values) / len(values) if values else 0.0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--baseline", default=str(AUTO / "baseline.md"))
    parser.add_argument("--candidate", default=str(ROOT / "prompt.md"))
    parser.add_argument("--fixtures", type=int, default=8)
    parser.add_argument("--modern", type=int, default=1, help="how many fixtures must use the current payload shape")
    parser.add_argument("--fixture-ids", default="", help="comma separated fixture ids instead of random sampling")
    parser.add_argument("--model", default="openai-codex/gpt-6-luna")
    parser.add_argument("--judge-model", default="openai-codex/gpt-6.1-sol")
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--fresh", action="store_true", help="ignore the candidate generation cache")
    parser.add_argument("--no-base-cache", action="store_true")
    parser.add_argument("--tag", default="")
    parser.add_argument("--record-recent", action="store_true", help="append the candidate lines to recent_pool.jsonl")
    args = parser.parse_args()

    seed = args.seed or random.randrange(1, 2**31)
    rng = random.Random(seed)
    base_style = style_of(Path(args.baseline))
    cand_style = style_of(Path(args.candidate))
    fixtures = load_fixtures(args, rng)
    run_dir = AUTO / "runs" / f"{time.strftime('%Y%m%d-%H%M%S')}-{seed}{('-' + args.tag) if args.tag else ''}"
    run_dir.mkdir(parents=True, exist_ok=True)

    for fixture in fixtures:
        fixture["payload"]["recent_comments"] = recent_comments(fixture["id"])
        fixture["order"] = "base_first" if rng.random() < 0.5 else "cand_first"
        fixture["swap"] = rng.random() < 0.5

    base_cache = None if args.no_base_cache else AUTO / "cache" / "base"
    cand_cache = AUTO / "cache" / "cand"

    def work(fixture: dict) -> dict:
        def run_base():
            return generate_arm(base_style, fixture, args.model, base_cache, fresh=False)

        def run_cand():
            return generate_arm(cand_style, fixture, args.model, cand_cache, fresh=args.fresh)

        if fixture["order"] == "base_first":
            base = run_base()
            cand = run_cand()
        else:
            cand = run_cand()
            base = run_base()
        swap = fixture["swap"]
        base_text = render(fixture["payload"], base) if base else "(không tạo được)"
        cand_text = render(fixture["payload"], cand) if cand else "(không tạo được)"
        arm_a, arm_b = (cand_text, base_text) if swap else (base_text, cand_text)
        verdict = judge(fixture["payload"], arm_a, arm_b, args.judge_model)
        row = {
            "id": fixture["id"], "group": fixture["group"], "tracked": fixture["tracked"],
            "modern": fixture["modern"], "swapped": swap,
            "candidate_raw": cand, "baseline_raw": base,
            "candidate_text": cand_text, "baseline_text": base_text,
            "verdict": verdict,
        }
        if verdict:
            top = verdict.get("A") or {}
            bot = verdict.get("B") or {}
            if swap:
                row["cand_score"] = {k: float(top.get(k) or 0) for k in ("funny", "natural", "faithful")}
                row["base_score"] = {k: float(bot.get(k) or 0) for k in ("funny", "natural", "faithful")}
            else:
                row["cand_score"] = {k: float(bot.get(k) or 0) for k in ("funny", "natural", "faithful")}
                row["base_score"] = {k: float(top.get(k) or 0) for k in ("funny", "natural", "faithful")}
            winner = str(verdict.get("winner") or "").upper()[:1]
            cand_arm = "A" if swap else "B"
            row["judge_pick"] = 0.5 if winner not in ("A", "B") else float(winner == cand_arm)
            cs = (row["cand_score"]["funny"] + row["cand_score"]["natural"]) / 2
            bs = (row["base_score"]["funny"] + row["base_score"]["natural"]) / 2
            row["win"] = 0.5 if cs == bs else float(cs > bs)
        row["cand_flags"] = analyze(cand or {"players": []}, fixture["payload"], cand_style,
                                    [p for p in fixture["payload"]["players"] if p.get("tracked")])
        row["base_flags"] = analyze(base or {"players": []}, fixture["payload"], base_style,
                                    [p for p in fixture["payload"]["players"] if p.get("tracked")])
        return row

    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        rows = list(pool.map(work, fixtures))

    deltas, wins, faithful, funny, natural = [], [], [], [], []
    for row in rows:
        (run_dir / f"{row['id']}.json").write_text(json.dumps(row, ensure_ascii=False, indent=1), encoding="utf-8")
        if args.record_recent and row.get("candidate_text"):
            remember_recent(row["id"], row["candidate_text"])
        if "cand_score" not in row:
            deltas.append(-10.0)
            wins.append(0.0)
            faithful.append(-10.0)
            funny.append(0.0)
            natural.append(0.0)
            continue
        c, b = row["cand_score"], row["base_score"]
        deltas.append((c["funny"] + c["natural"]) / 2 - (b["funny"] + b["natural"]) / 2)
        faithful.append(c["faithful"] - b["faithful"])
        funny.append(c["funny"])
        natural.append(c["natural"])
        wins.append(row.get("win", 0.0))

    players = sum(len([p for p in f["payload"]["players"] if p.get("tracked")]) for f in fixtures)
    cand_flags = [row["cand_flags"] for row in rows]
    base_flags = [row["base_flags"] for row in rows]
    fail = sum(1 for row in rows if not row.get("candidate_raw"))
    ungrounded = sum(f["ungrounded"] for f in cand_flags) / max(1, sum(f["tokens"] for f in cand_flags)) * 100
    summary = {
        "seed": seed, "run_dir": str(run_dir),
        "judge_delta": round(mean(deltas), 3),
        "delta_faithful": round(mean(faithful), 3),
        "win_pct": round(mean(wins) * 100, 1),
        "judge_pick_pct": round(mean([row.get("judge_pick", 0.0) for row in rows]) * 100, 1),
        "cand_funny": round(mean(funny), 2),
        "cand_natural": round(mean(natural), 2),
        "ungrounded_pct": round(ungrounded, 1),
        "base_ungrounded_pct": round(sum(f["ungrounded"] for f in base_flags) / max(1, sum(f["tokens"] for f in base_flags)) * 100, 1),
        "format_fail": fail,
        "missing_players": sum(f["missing"] for f in cand_flags),
        "base_missing_players": sum(f["missing"] for f in base_flags),
        "emoji_invalid": sum(f["emoji_invalid"] for f in cand_flags),
        "base_emoji_invalid": sum(f["emoji_invalid"] for f in base_flags),
        "over_140": sum(f["over_140"] for f in cand_flags),
        "base_over_140": sum(f["over_140"] for f in base_flags),
        "field_leak": sum(f["field_leak"] for f in cand_flags),
        "dup_title": sum(f["dup_title"] for f in cand_flags),
        "comment_len_avg": round(sum(f["len_sum"] for f in cand_flags) / max(1, players), 1),
        "fixtures": [row["id"] for row in rows],
        "prompt_bytes": Path(args.candidate).stat().st_size,
    }
    (AUTO / "last_run.json").write_text(json.dumps(summary, ensure_ascii=False, indent=1), encoding="utf-8")
    for key, value in summary.items():
        if isinstance(value, (int, float)):
            print(f"METRIC {key}={value}")
    print(f"run: {run_dir}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
