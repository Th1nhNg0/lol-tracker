#!/usr/bin/env bash
# Prompt benchmark: generates the real commentary request and blind-judges two recaps.
set -euo pipefail
cd "$(dirname "$0")/.."

bytes=$(wc -c < prompt.md | tr -d ' ')
if [ "$bytes" -gt 16384 ]; then
  echo "prompt.md is $bytes bytes; src/llm.rs rejects anything over 16384 and drops the whole style" >&2
  exit 1
fi
if [ "$bytes" -lt 4000 ]; then
  echo "prompt.md is only $bytes bytes; the emoji catalog and persona must still be there" >&2
  exit 1
fi

exec python3 .auto/harness.py --fixtures 8 --modern 1 --workers 8 "$@"
