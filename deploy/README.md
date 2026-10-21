# Updating a self-hosted rift-recap

Servers **pull** releases; nothing in this repository logs in to a server and no deployment secret
lives on GitHub.

```
git tag v0.3.0 && git push origin v0.3.0
        │
        ▼  .github/workflows/release.yml
test → static musl build → SHA256SUMS → build-provenance attestation → GitHub Release
        │
        ▼  on your server, every few minutes
rift-recap-update  →  verify checksum → pre-flight → back up → swap binary → restart → watch → (roll back)
```

## Releasing

1. Set `version` in `Cargo.toml` (and `Cargo.lock`) to the new version and merge it.
2. Push a matching tag, `vMAJOR.MINOR.PATCH`. The workflow refuses a tag that differs from `Cargo.toml`.
3. The release holds `rift-recap-linux-x86_64` (a static binary that runs on any x86_64 Linux),
   `SHA256SUMS`, and an attestation proving the binary was built by that workflow.

## Installing the updater on a server

Needs Python 3.11+, `systemctl`, and permission to write the install directory (usually root).

```bash
install -m 755 deploy/rift-recap-update /usr/local/sbin/rift-recap-update
install -d /etc/rift-recap
install -m 644 deploy/rift-recap-update.example.toml /etc/rift-recap/update.toml   # then edit it
install -m 644 deploy/rift-recap-update.service deploy/rift-recap-update.timer /etc/systemd/system/

rift-recap-update check                      # what is installed, what is the latest release
rift-recap-update update --dry-run           # download + verify + pre-flight, change nothing
systemctl enable --now rift-recap-update.timer
```

Every path and name comes from `update.toml`; `rift-recap-update.example.toml` lists all settings. Any
scheduler works instead of the timer, for example an agent that runs the command on an interval.

The first update needs a marker for the version you already run. Either let the first run install the
latest release (it backs up what is there), or write one by hand:
`echo '{"tag": "v0.1.0"}' > /opt/rift-recap/.deployed-release.json`.

## What an update does

1. Asks GitHub for the latest release (never a draft or pre-release) and downloads the binary.
2. Checks it against `SHA256SUMS`, and against its attestation when `require_attestation = true`.
3. The binary must report exactly the release version and pass `check` against a **copy** of your
   config and database with **dummy secrets**, so a binary you have not run yet never sees your keys.
4. Backs up the previous binary and the SQLite database (the backup is restored into a scratch file to
   prove it is readable).
5. Replaces the binary, restarts the service and waits until it runs the new binary under one PID for
   several seconds.
6. On any failure it puts the old binary back. The database snapshot is restored only if the new
   binary changed the schema; otherwise data written in the meantime is kept.
7. Optionally runs `notify_command` with the result.

`rift-recap-update update --tag v0.1.0` installs a specific (also older) release, which is how you roll
back by hand. `rift-recap-update status` prints health as JSON and exits 1 when the service is not
running the binary the updater installed.

## Tests

`python3 deploy/test_update.py` runs the whole flow against a fake GitHub, a fake `systemctl` and tiny
fake binaries: good update, repeat, bad checksum, wrong version, pre-release, dry run, crash rollback and
schema rollback. CI runs it on every push.
