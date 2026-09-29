#!/usr/bin/env bash
# specs/scripts/run_apalache_all.sh
#
# Bounded model check every organ's `safety` invariant via Apalache.
# Start at --length=10; raise per invariant once green.
#
# Usage: LENGTH=15 INVARIANT=safety ./specs/scripts/run_apalache_all.sh

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
QUINT_DIR="${REPO_ROOT}/specs/quint"
LENGTH="${LENGTH:-10}"
INVARIANT="${INVARIANT:-safety}"

if ! command -v quint >/dev/null 2>&1; then
  echo "quint not installed." >&2
  exit 127
fi

fail=0
for f in "${QUINT_DIR}"/organs/*.qnt "${QUINT_DIR}"/compositions/*.qnt; do
  echo "== verify --length=${LENGTH} --invariant=${INVARIANT}: ${f#$REPO_ROOT/}"
  if ! quint verify --length="${LENGTH}" --invariant="${INVARIANT}" "${f}"; then
    fail=$((fail + 1))
  fi
done

if (( fail > 0 )); then
  echo "${fail} module(s) failed at length=${LENGTH}" >&2
  exit 1
fi
echo "All organs pass Apalache at length=${LENGTH}."
