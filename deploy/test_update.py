#!/usr/bin/env python3
"""End-to-end test of deploy/rift-recap-update against a fake GitHub, a fake systemd and fake binaries.

Needs a C compiler (the fake rift-recap is a tiny ELF program) and Python 3.11+. Run: python3 deploy/test_update.py
"""
import hashlib
import http.server
import json
import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path

UPDATER = Path(__file__).with_name("rift-recap-update")
C_SOURCE = r"""
#include <stdio.h>
#include <string.h>
#include <unistd.h>
static volatile char padding[200000] = {1};
int main(int argc, char **argv) {
    (void)padding;
    for (int i = 1; i < argc; i++) {
        if (!strcmp(argv[i], "--version")) { printf("rift-recap %s\n", VERSION); return 0; }
        if (!strcmp(argv[i], "check")) { printf("config ok\n"); return 0; }
    }
    if (CRASH) return 3;
    for (;;) sleep(60);
}
"""
FAKE_SYSTEMCTL = r"""#!/bin/bash
alive() { [ -f "$FAKE_STATE/pid" ] && kill -0 "$(cat "$FAKE_STATE/pid")" 2>/dev/null; }
start() {
  if [ -f "$FAKE_STATE/mutate_schema" ]; then
    python3 -c "import sqlite3; c=sqlite3.connect('$FAKE_DB'); c.execute('ALTER TABLE matches ADD COLUMN extra TEXT'); c.commit()"
    rm -f "$FAKE_STATE/mutate_schema"
  fi
  nohup "$FAKE_INSTALL/rift-recap" run >/dev/null 2>&1 &
  echo $! > "$FAKE_STATE/pid"
}
stop() { if alive; then kill "$(cat "$FAKE_STATE/pid")"; sleep 0.2; fi; rm -f "$FAKE_STATE/pid"; }
case "$1" in
  is-active) if alive; then echo active; else echo inactive; fi ;;
  show) case "$*" in *MainPID*) if alive; then cat "$FAKE_STATE/pid"; else echo 0; fi ;; *NRestarts*) echo 0 ;; esac ;;
  restart) stop; start ;;
  stop) stop ;;
  start) start ;;
esac
"""


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


class FakeGitHub(http.server.BaseHTTPRequestHandler):
    routes: dict = {}

    def do_GET(self):
        body = self.routes.get(self.path)
        if body is None:
            self.send_response(404)
            self.end_headers()
            return
        self.send_response(200)
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


class UpdaterTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.dir = Path(tempfile.mkdtemp(prefix="updater-test-"))
        cls.bins = {}
        for name, version, crash in [("v1", "0.1.0", 0), ("v2", "0.2.0", 0), ("wrong", "9.9.9", 0), ("crash", "0.4.0", 1), ("crash2", "0.4.1", 1)]:
            source = cls.dir / f"{name}.c"
            source.write_text(C_SOURCE)
            out = cls.dir / name
            subprocess.run(["cc", f'-DVERSION="{version}"', f"-DCRASH={crash}", "-o", str(out), str(source)], check=True)
            cls.bins[name] = out.read_bytes()
        cls.install = cls.dir / "install"
        (cls.install / "data").mkdir(parents=True)
        (cls.install / "config.toml").write_text('database = "data/tracker.db"\nplayers_file = "players.json"\n')
        (cls.install / ".env").write_text("RIOT_API_KEY=real-secret\n")
        cls.db = cls.install / "data/tracker.db"
        connection = sqlite3.connect(cls.db)
        connection.execute("CREATE TABLE matches (id TEXT PRIMARY KEY, data TEXT)")
        connection.execute("INSERT INTO matches VALUES ('VN2_1', '{}')")
        connection.commit()
        connection.close()
        cls.state = cls.dir / "state"
        cls.state.mkdir()
        systemctl = cls.dir / "systemctl"
        systemctl.write_text(FAKE_SYSTEMCTL)
        systemctl.chmod(0o755)
        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), FakeGitHub)
        threading.Thread(target=cls.server.serve_forever, daemon=True).start()
        base = f"http://127.0.0.1:{cls.server.server_address[1]}"
        cls.base = base
        (cls.dir / "update.toml").write_text(
            f'repo = "o/r"\nservice = "fake.service"\ninstall_dir = "{cls.install}"\nbackup_dir = "{cls.dir}/backups"\nscratch_dir = "{cls.dir}"\n'
            f'api_base = "{base}"\nsystemctl = ["{systemctl}"]\nhealth_timeout_secs = 8\nnotify_command = ["tee", "{cls.dir}/notifications"]\n')
        os.environ.update(FAKE_STATE=str(cls.state), FAKE_DB=str(cls.db), FAKE_INSTALL=str(cls.install))
        # The installed v0.1.0, already running.
        shutil.copy(cls.dir / "v1", cls.install / "rift-recap")
        (cls.install / ".deployed-release.json").write_text(json.dumps({"tag": "v0.1.0", "binary_sha256": sha256(cls.bins["v1"])}))
        subprocess.run([str(systemctl), "start"], check=True)

    @classmethod
    def tearDownClass(cls):
        subprocess.run([str(cls.dir / "systemctl"), "stop"], check=False)
        cls.server.shutdown()
        shutil.rmtree(cls.dir, ignore_errors=True)

    def publish(self, tag, binary, checksum=None, latest=True, prerelease=False):
        asset = "rift-recap-linux-x86_64"
        data = self.bins[binary]
        prefix = f"/o/r/releases/download/{tag}"
        info = {"tag_name": tag, "draft": False, "prerelease": prerelease, "html_url": f"{self.base}/{tag}", "assets": [
            {"name": asset, "browser_download_url": f"{self.base}{prefix}/{asset}"},
            {"name": "SHA256SUMS", "browser_download_url": f"{self.base}{prefix}/SHA256SUMS"}]}
        FakeGitHub.routes[f"{prefix}/{asset}"] = data
        FakeGitHub.routes[f"{prefix}/SHA256SUMS"] = f"{checksum or sha256(data)}  {asset}\n".encode()
        FakeGitHub.routes[f"/repos/o/r/releases/tags/{tag}"] = json.dumps(info).encode()
        if latest:
            FakeGitHub.routes["/repos/o/r/releases/latest"] = json.dumps(info).encode()

    def updater(self, *args):
        return subprocess.run([sys.executable, str(UPDATER), "--config", str(self.dir / "update.toml"), *args], text=True, capture_output=True, timeout=120)

    def installed_sha(self):
        return sha256((self.install / "rift-recap").read_bytes())

    def marker_tag(self):
        return json.loads((self.install / ".deployed-release.json").read_text())["tag"]

    def assert_healthy(self, tag):
        result = self.updater("status")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(json.loads(result.stdout)["tag"], tag)

    def columns(self):
        connection = sqlite3.connect(self.db)
        try:
            return [row[1] for row in connection.execute("PRAGMA table_info(matches)")]
        finally:
            connection.close()

    def test_flow(self):
        self.assert_healthy("v0.1.0")

        # 1. A good release is installed, the service runs it, and a database backup exists.
        self.publish("v0.2.0", "v2")
        result = self.updater("update")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["status"], "updated")
        self.assertEqual(self.installed_sha(), sha256(self.bins["v2"]))
        self.assert_healthy("v0.2.0")
        self.assertTrue(list((self.dir / "backups/database").glob("tracker-before-v0.2.0-*.db")))
        self.assertIn("v0.1.0 → v0.2.0", (self.dir / "notifications").read_text())

        # 2. Nothing to do the second time.
        self.assertEqual(json.loads(self.updater("update").stdout)["status"], "up-to-date")

        # 3. A wrong checksum is rejected before anything changes.
        before = self.installed_sha()
        self.publish("v0.3.0", "v2", checksum="0" * 64)
        result = self.updater("update")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SHA256SUMS", result.stderr)
        self.assertEqual(self.installed_sha(), before)

        # 4. A binary that reports the wrong version is rejected before it is installed.
        self.publish("v0.3.1", "wrong")
        result = self.updater("update")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("expected", result.stderr)
        self.assertEqual(self.installed_sha(), before)

        # 5. Pre-releases are never installed.
        self.publish("v0.3.2", "v2", prerelease=True)
        self.assertIn("pre-release", self.updater("update").stderr)

        # 6. A dry run verifies the binary and changes nothing.
        self.publish("v0.2.0", "v2")
        result = self.updater("update", "--dry-run")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["status"], "dry-run ok")
        self.assertEqual(self.installed_sha(), before)

        # 7. A release that dies on start is rolled back; the data written meanwhile is kept.
        self.publish("v0.4.0", "crash")
        result = self.updater("update")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("old binary restored", result.stderr)
        self.assertNotIn("snapshot", result.stderr)
        self.assertEqual(self.installed_sha(), before)
        self.assertEqual(self.marker_tag(), "v0.2.0")
        self.assert_healthy("v0.2.0")
        self.assertIn("failed", (self.dir / "notifications").read_text())

        # 8. If the failed release changed the schema, the database snapshot is restored too.
        (self.state / "mutate_schema").write_text("")
        self.publish("v0.4.1", "crash2")
        result = self.updater("update")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("database snapshot", result.stderr)
        self.assertEqual(self.columns(), ["id", "data"])
        self.assert_healthy("v0.2.0")


if __name__ == "__main__":
    unittest.main(verbosity=2)
