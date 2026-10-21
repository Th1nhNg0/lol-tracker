#!/usr/bin/python3
"""Restricted root-side deploy endpoint for the GitHub Actions SSH key.

The key is forced to this program by authorized_keys. It accepts only:
  deploy <40-char commit SHA>          (binary on stdin)
  status
  notify <overall> <build> <deploy> <sha> <Actions run URL>
"""
from __future__ import annotations

import datetime as dt
import hashlib
import json
import os
import platform
import re
import shutil
import sqlite3
import struct
import subprocess
import sys
import tempfile
import time
import tomllib
from pathlib import Path

SERVICE = "lol-tracker.service"
SERVICE_DIR = Path("/root/code/lol-tracker-rs")
BINARY = SERVICE_DIR / "lol-tracker"
CONFIG = SERVICE_DIR / "config.toml"
ENV_FILE = SERVICE_DIR / ".env"
DATABASE = SERVICE_DIR / "data/tracker.db"
MARKER = SERVICE_DIR / ".deployed-commit.json"
BACKUP_ROOT = Path("/root/backups/rift-recap")
HERMES = Path("/usr/local/lib/hermes-agent/.hermes/bin/hermes")
DISCORD_TARGET = "discord:1562348018609557534:1562348018609557534"
MAX_BINARY_BYTES = 128 * 1024 * 1024


def fail(message: str, code: int = 1) -> None:
    print(message, file=sys.stderr)
    raise SystemExit(code)


def run(argv: list[str], timeout: int = 30, check: bool = True) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(argv, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)
    if check and result.returncode != 0:
        raise RuntimeError(f"command failed: {Path(argv[0]).name} (exit {result.returncode})")
    return result


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def sha256_stream(stream) -> str:
    digest = hashlib.sha256()
    for chunk in iter(lambda: stream.read(1024 * 1024), b""):
        digest.update(chunk)
    return digest.hexdigest()


def sqlite_uri(path: Path) -> str:
    return path.resolve().as_uri() + "?mode=ro"


def db_facts(path: Path) -> dict:
    connection = sqlite3.connect(sqlite_uri(path), uri=True, timeout=30)
    try:
        integrity = connection.execute("PRAGMA integrity_check").fetchone()[0]
        if integrity != "ok":
            raise RuntimeError(f"SQLite integrity check failed: {integrity}")
        version = connection.execute("PRAGMA user_version").fetchone()[0]
        tables: dict[str, dict] = {}
        names = [row[0] for row in connection.execute(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
        )]
        for name in names:
            quoted = '"' + name.replace('"', '""') + '"'
            columns = [row[1] for row in connection.execute(f"PRAGMA table_info({quoted})")]
            rows = connection.execute(f"SELECT count(*) FROM {quoted}").fetchone()[0]
            tables[name] = {"columns": columns, "rows": rows}
        return {"integrity": integrity, "user_version": version, "tables": tables}
    finally:
        connection.close()


def sqlite_backup(source_path: Path, destination_path: Path) -> None:
    source = sqlite3.connect(sqlite_uri(source_path), uri=True, timeout=60)
    destination = sqlite3.connect(str(destination_path), timeout=60)
    try:
        source.backup(destination, pages=256, sleep=0.05)
        destination.commit()
    finally:
        destination.close()
        source.close()


def replace_config_path(text: str, key: str, value: str) -> str:
    pattern = re.compile(rf"(?m)^(\s*{re.escape(key)}\s*=\s*)[^\r\n]*$")
    updated, count = pattern.subn(lambda match: match.group(1) + json.dumps(value), text, count=1)
    if count != 1:
        raise RuntimeError(f"could not set config field {key} for isolated preflight")
    return updated


def candidate_check(candidate: Path) -> str:
    if not CONFIG.is_file() or not ENV_FILE.is_file() or not DATABASE.is_file():
        raise RuntimeError("live config, env file, or database is missing")
    parsed = tomllib.loads(CONFIG.read_text(encoding="utf-8"))
    players = Path(parsed.get("players_file", "players.json"))
    if not players.is_absolute():
        players = CONFIG.parent / players
    prompt = parsed.get("llm", {}).get("prompt_file")
    prompt_path = None
    if prompt:
        prompt_path = Path(prompt)
        if not prompt_path.is_absolute():
            prompt_path = CONFIG.parent / prompt_path
    with tempfile.TemporaryDirectory(prefix="rift-recap-preflight-", dir="/root/.hermes/cache/scratch") as directory:
        temp = Path(directory)
        test_db = temp / "tracker.db"
        sqlite_backup(DATABASE, test_db)
        config_text = CONFIG.read_text(encoding="utf-8")
        config_text = replace_config_path(config_text, "database", str(test_db))
        config_text = replace_config_path(config_text, "players_file", str(players))
        if prompt:
            config_text = replace_config_path(config_text, "prompt_file", str(prompt_path))
        test_config = temp / "config.toml"
        test_config.write_text(config_text, encoding="utf-8")
        result = subprocess.run(
            [str(candidate), "--config", str(test_config), "--env-file", str(ENV_FILE), "check"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            timeout=60,
        )
        if result.returncode != 0:
            raise RuntimeError(f"candidate config/storage check failed (exit {result.returncode})")
        facts = db_facts(test_db)
        return f"{result.stdout.strip()} tables={len(facts['tables'])}"


def verified_backup(sha: str) -> tuple[Path, str, dict]:
    if not DATABASE.is_file():
        raise RuntimeError("live SQLite database is missing; refusing deployment")
    backup_dir = BACKUP_ROOT / "database"
    backup_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(BACKUP_ROOT, 0o700)
    os.chmod(backup_dir, 0o700)
    stamp = dt.datetime.now(dt.timezone.utc).strftime("%Y%m%dT%H%M%S.%fZ")
    fd, temp_name = tempfile.mkstemp(prefix=".tracker-", suffix=".tmp", dir=backup_dir)
    os.close(fd)
    temp_path = Path(temp_name)
    try:
        sqlite_backup(DATABASE, temp_path)
        facts = db_facts(temp_path)
        with tempfile.TemporaryDirectory(prefix="rift-recap-restore-test-", dir="/root/.hermes/cache/scratch") as directory:
            restored = Path(directory) / "restored.db"
            sqlite_backup(temp_path, restored)
            restored_facts = db_facts(restored)
            if facts != restored_facts:
                raise RuntimeError("SQLite restore verification did not match the snapshot")
        digest = sha256_file(temp_path)
        previous = sorted(backup_dir.glob("tracker-before-*.db"), key=lambda item: item.stat().st_mtime_ns, reverse=True)
        if previous and sha256_file(previous[0]) == digest:
            temp_path.unlink(missing_ok=True)
            return previous[0], digest, facts
        backup_path = backup_dir / f"tracker-before-{sha[:12]}-{stamp}.db"
        os.chmod(temp_path, 0o600)
        os.replace(temp_path, backup_path)
        for old in sorted(backup_dir.glob("tracker-before-*.db"), key=lambda item: item.stat().st_mtime_ns, reverse=True)[10:]:
            old.unlink(missing_ok=True)
        return backup_path, digest, facts
    finally:
        temp_path.unlink(missing_ok=True)


def save_binary_backup() -> tuple[Path, str]:
    if not BINARY.is_file():
        raise RuntimeError("current service binary is missing; refusing deployment")
    backup_dir = BACKUP_ROOT / "binary"
    backup_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    os.chmod(BACKUP_ROOT, 0o700)
    os.chmod(backup_dir, 0o700)
    old_sha = sha256_file(BINARY)
    backup_path = backup_dir / f"lol-tracker-{old_sha}.bin"
    if not backup_path.exists():
        shutil.copy2(BINARY, backup_path)
        os.chmod(backup_path, 0o600)
    if sha256_file(backup_path) != old_sha:
        raise RuntimeError("current binary backup hash mismatch")
    for old in sorted(backup_dir.glob("lol-tracker-*.bin"), key=lambda item: item.stat().st_mtime_ns, reverse=True)[5:]:
        old.unlink(missing_ok=True)
    return backup_path, old_sha


def validate_binary(path: Path, data_size: int) -> str:
    if data_size < 100_000 or data_size > MAX_BINARY_BYTES:
        raise RuntimeError(f"binary size outside accepted range: {data_size}")
    with path.open("rb") as stream:
        header = stream.read(20)
    if len(header) < 20 or header[:4] != b"\x7fELF" or header[4] != 2 or header[5] != 1:
        raise RuntimeError("artifact is not a 64-bit little-endian ELF binary")
    machine = struct.unpack("<H", header[18:20])[0]
    if platform.machine() != "x86_64" or machine != 62:
        raise RuntimeError("artifact architecture does not match this x86_64 host")
    os.chmod(path, 0o755)
    version = subprocess.run([str(path), "--version"], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=10)
    if version.returncode != 0 or not version.stdout.startswith("rift-recap "):
        raise RuntimeError("artifact did not identify itself as a working rift-recap binary")
    return version.stdout.strip()


def service_value(field: str) -> str:
    return run(["systemctl", "show", SERVICE, f"-p{field}", "--value"], timeout=10).stdout.strip()


def process_hash(pid: int) -> str:
    return sha256_file(Path(f"/proc/{pid}/exe"))


def wait_for_service(expected_hash: str, baseline_restarts: int | None, timeout: int = 30) -> int:
    deadline = time.monotonic() + timeout
    stable = 0
    last_pid = 0
    while time.monotonic() < deadline:
        active = run(["systemctl", "is-active", SERVICE], timeout=10, check=False).stdout.strip()
        try:
            pid = int(service_value("MainPID"))
            restarts = int(service_value("NRestarts"))
        except (ValueError, RuntimeError):
            pid, restarts = 0, -1
        ok = active == "active" and pid > 0
        if ok:
            try:
                ok = process_hash(pid) == expected_hash
            except (FileNotFoundError, ProcessLookupError, PermissionError):
                ok = False
        if baseline_restarts is not None and restarts != baseline_restarts:
            ok = False
        if ok and pid == last_pid:
            stable += 1
        elif ok:
            stable = 1
        else:
            stable = 0
        last_pid = pid if ok else 0
        if stable >= 4:
            return pid
        time.sleep(1)
    raise RuntimeError("service did not stay active on the expected binary")


def atomic_copy(source: Path, destination: Path, mode: int = 0o755) -> None:
    fd, name = tempfile.mkstemp(prefix=f".{destination.name}.", dir=destination.parent)
    temp = Path(name)
    try:
        with os.fdopen(fd, "wb") as output, source.open("rb") as input_stream:
            shutil.copyfileobj(input_stream, output)
            output.flush()
            os.fsync(output.fileno())
        os.chmod(temp, mode)
        os.chown(temp, 0, 0)
        os.replace(temp, destination)
    finally:
        temp.unlink(missing_ok=True)


def restore_database(snapshot: Path) -> None:
    if run(["systemctl", "is-active", SERVICE], timeout=10, check=False).stdout.strip() == "active":
        raise RuntimeError("refusing to restore the database while the service is active")
    fd, name = tempfile.mkstemp(prefix=".tracker-rollback-", suffix=".db", dir=DATABASE.parent)
    os.close(fd)
    restored = Path(name)
    try:
        sqlite_backup(snapshot, restored)
        db_facts(restored)
        for suffix in ("-wal", "-shm", "-journal"):
            Path(str(DATABASE) + suffix).unlink(missing_ok=True)
        old_mode = DATABASE.stat().st_mode & 0o777 if DATABASE.exists() else 0o600
        os.chmod(restored, old_mode)
        os.chown(restored, 0, 0)
        os.replace(restored, DATABASE)
    finally:
        restored.unlink(missing_ok=True)


def write_marker(commit: str, binary_hash: str, version: str, database_backup: Path, database_hash: str) -> None:
    document = {
        "commit": commit,
        "deployed_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "binary_sha256": binary_hash,
        "version": version,
        "database_backup": str(database_backup),
        "database_sha256": database_hash,
    }
    fd, name = tempfile.mkstemp(prefix=".deployed-commit.", dir=SERVICE_DIR)
    temp = Path(name)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as stream:
            json.dump(document, stream, sort_keys=True)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temp, 0o600)
        os.chown(temp, 0, 0)
        os.replace(temp, MARKER)
    finally:
        temp.unlink(missing_ok=True)


def restore_marker(previous: bytes | None) -> None:
    if previous is None:
        MARKER.unlink(missing_ok=True)
        return
    fd, name = tempfile.mkstemp(prefix=".deployed-commit-rollback.", dir=SERVICE_DIR)
    temp = Path(name)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(previous)
            stream.flush()
            os.fsync(stream.fileno())
        os.chmod(temp, 0o600)
        os.chown(temp, 0, 0)
        os.replace(temp, MARKER)
    finally:
        temp.unlink(missing_ok=True)


def deploy(commit: str) -> None:
    payload = sys.stdin.buffer.read(MAX_BINARY_BYTES + 1)
    if len(payload) > MAX_BINARY_BYTES:
        raise RuntimeError("artifact exceeds maximum accepted size")
    SERVICE_DIR.mkdir(parents=True, exist_ok=True)
    fd, name = tempfile.mkstemp(prefix=".lol-tracker-new.", dir=SERVICE_DIR)
    candidate = Path(name)
    replaced = False
    snapshot: Path | None = None
    previous_marker = MARKER.read_bytes() if MARKER.exists() else None
    old_binary_backup: Path | None = None
    old_hash: str | None = None
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        version = validate_binary(candidate, len(payload))
        preflight = candidate_check(candidate)
        old_binary_backup, old_hash = save_binary_backup()
        snapshot, database_hash, facts = verified_backup(commit)
        new_hash = sha256_file(candidate)
        os.chown(candidate, 0, 0)
        os.chmod(candidate, 0o755)
        os.replace(candidate, BINARY)
        replaced = True
        baseline_restarts = int(service_value("NRestarts"))
        run(["systemctl", "restart", SERVICE], timeout=45)
        pid = wait_for_service(new_hash, baseline_restarts)
        write_marker(commit, new_hash, version, snapshot, database_hash)
        print(json.dumps({
            "status": "deployed",
            "commit": commit,
            "version": version,
            "binary_sha256": new_hash,
            "previous_binary_sha256": old_hash,
            "pid": pid,
            "database_backup": str(snapshot),
            "database_sha256": database_hash,
            "database_tables": {name: table["rows"] for name, table in facts["tables"].items()},
            "preflight": preflight,
        }, sort_keys=True))
    except Exception as error:
        if replaced:
            try:
                run(["systemctl", "stop", SERVICE], timeout=30, check=False)
                deadline = time.monotonic() + 15
                while time.monotonic() < deadline:
                    state = run(["systemctl", "is-active", SERVICE], timeout=10, check=False).stdout.strip()
                    if state in ("inactive", "failed"):
                        break
                    time.sleep(1)
                if run(["systemctl", "is-active", SERVICE], timeout=10, check=False).stdout.strip() == "active":
                    raise RuntimeError("could not stop service for rollback")
                if old_binary_backup is None or snapshot is None:
                    raise RuntimeError("rollback material is missing")
                atomic_copy(old_binary_backup, BINARY)
                restore_database(snapshot)
                restore_marker(previous_marker)
                run(["systemctl", "start", SERVICE], timeout=30)
                wait_for_service(old_hash or sha256_file(BINARY), None)
                print("automatic rollback: old binary, database snapshot, and marker restored", file=sys.stderr)
            except Exception as rollback_error:
                print(f"automatic rollback failed: {rollback_error}", file=sys.stderr)
        raise RuntimeError(f"deployment failed: {error}") from error
    finally:
        candidate.unlink(missing_ok=True)


def status() -> int:
    active = run(["systemctl", "is-active", SERVICE], timeout=10, check=False).stdout.strip()
    try:
        pid = int(service_value("MainPID"))
    except (ValueError, RuntimeError):
        pid = 0
    binary_hash = sha256_file(BINARY) if BINARY.is_file() else None
    live_hash = None
    if pid > 0:
        try:
            live_hash = process_hash(pid)
        except (FileNotFoundError, ProcessLookupError, PermissionError):
            pass
    marker = json.loads(MARKER.read_text(encoding="utf-8")) if MARKER.is_file() else {}
    healthy = bool(active == "active" and pid > 0 and binary_hash and live_hash == binary_hash
                   and marker.get("binary_sha256") == binary_hash and marker.get("commit"))
    print(json.dumps({
        "healthy": healthy,
        "service": SERVICE,
        "active": active,
        "pid": pid,
        "binary_sha256": binary_hash,
        "process_sha256": live_hash,
        "commit": marker.get("commit"),
        "version": marker.get("version"),
        "deployed_at": marker.get("deployed_at"),
    }, sort_keys=True))
    return 0 if healthy else 1


def notify(overall: str, build: str, deployed: str, commit: str, run_url: str) -> None:
    allowed = {"success", "failure", "cancelled", "skipped"}
    if overall not in {"success", "failure", "cancelled"} or build not in allowed or deployed not in allowed:
        raise RuntimeError("invalid workflow result")
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise RuntimeError("invalid commit SHA")
    if not re.fullmatch(r"https://github\.com/Th1nhNg0/rift-recap/actions/runs/[0-9]+", run_url):
        raise RuntimeError("invalid GitHub Actions URL")
    if overall == "success" and (build != "success" or deployed != "success"):
        raise RuntimeError("success notification does not match job results")
    icon = {"success": "✅", "failure": "❌", "cancelled": "⚠️"}[overall]
    message = (
        f"{icon} Rift Recap deployment **{overall}**.\n"
        f"Build: `{build}` · deploy: `{deployed}` · commit: `{commit[:12]}`\n"
        f"<{run_url}>"
    )
    result = subprocess.run(
        [str(HERMES), "send", "--to", DISCORD_TARGET, "--quiet"],
        input=message,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=60,
    )
    if result.returncode != 0:
        raise RuntimeError(f"Discord notification failed (exit {result.returncode})")
    print(json.dumps({"status": "notified", "target": DISCORD_TARGET, "commit": commit[:12]}, sort_keys=True))


def main() -> int:
    original = os.environ.get("SSH_ORIGINAL_COMMAND", "")
    match = re.fullmatch(r"deploy ([0-9a-f]{40})", original)
    if match:
        deploy(match.group(1))
        return 0
    if original == "status":
        return status()
    match = re.fullmatch(
        r"notify (success|failure|cancelled) (success|failure|cancelled|skipped) "
        r"(success|failure|cancelled|skipped) ([0-9a-f]{40}) "
        r"(https://github\.com/Th1nhNg0/rift-recap/actions/runs/[0-9]+)",
        original,
    )
    if match:
        notify(*match.groups())
        return 0
    fail("command rejected", 64)
    return 64


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except SystemExit:
        raise
    except Exception as error:
        fail(str(error))
