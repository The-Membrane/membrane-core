#!/usr/bin/env bash
# specs/scripts/run_lean.sh
#
# Build the Lean arithmetic carve-out. First run downloads Mathlib; expect a
# few minutes.

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
LEAN_DIR="${REPO_ROOT}/specs/lean"

if ! command -v lake >/dev/null 2>&1; then
  echo "Lean 4 / Lake not installed. Install elan from https://github.com/leanprover/elan" >&2
  exit 127
fi

cd "${LEAN_DIR}"
lake update
lake build
