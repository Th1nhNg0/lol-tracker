#!/usr/bin/env bash
# Backpressure: the recap must stay shippable and honest, and only prompt.md may change.
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0

if ! git diff --quiet -- src Cargo.toml Cargo.lock; then
  echo "FAIL: source files changed; only prompt.md is in scope for this session" >&2
  git diff --stat -- src Cargo.toml Cargo.lock >&2 || true
  fail=1
fi

while read -r want path; do
  [ -n "$path" ] || continue
  got=$(sha256sum "$path" | cut -d' ' -f1)
  if [ "$got" != "$want" ]; then
    echo "FAIL: benchmark file changed: $path" >&2
    fail=1
  fi
done < .auto/pin.txt

python3 .auto/invariants.py || fail=1

exit "$fail"
