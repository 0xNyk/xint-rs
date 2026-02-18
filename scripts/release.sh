#!/usr/bin/env bash
# Wrapper: use xint's canonical release script.
set -euo pipefail

if [[ -n "${XINT_RELEASE_SCRIPT:-}" ]]; then
  CANDIDATE="$XINT_RELEASE_SCRIPT"
else
  SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  CANDIDATE="$SCRIPT_DIR/../../xint/scripts/release.sh"
fi

if [[ ! -x "$CANDIDATE" ]]; then
  echo "[release][error] Canonical script not found/executable: $CANDIDATE" >&2
  echo "Set XINT_RELEASE_SCRIPT to the path of xint/scripts/release.sh" >&2
  exit 1
fi

exec "$CANDIDATE" "$@"
