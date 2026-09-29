#!/usr/bin/env bash
# specs/scripts/run_quint_all.sh
#
# Type-check every Quint module. Fast first-pass sanity.

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
QUINT_DIR="${REPO_ROOT}/specs/quint"

if ! command -v quint >/dev/null 2>&1; then
  echo "quint not installed. Install with: npm i -g @informalsystems/quint" >&2
  exit 127
fi

fail=0
while IFS= read -r -d '' f; do
  echo "== typecheck: ${f#$REPO_ROOT/}"
  if ! quint typecheck "${f}"; then
    fail=$((fail + 1))
  fi
done < <(find "${QUINT_DIR}" -name '*.qnt' -print0)

if (( fail > 0 )); then
  echo "${fail} file(s) failed typecheck" >&2
  exit 1
fi
echo "All Quint modules type-check clean."
