#!/usr/bin/env bash
# specs/scripts/check_drift.sh
#
# Drift check: for every row in specs/SPEC_MAP.md, verify that the Rust
# source file referenced has not changed since the `Last synced commit`
# unless the spec was also updated in the same commit.
#
# Fails only when (a) the Rust source has drifted AND (b) the currently
# staged/PR'd changes touch the referenced lines. This keeps CI cheap
# and deterministic.
#
# Exit codes: 0 clean, 1 drift detected, 2 malformed row.

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
SPEC_MAP="${REPO_ROOT}/specs/SPEC_MAP.md"

if [[ ! -f "${SPEC_MAP}" ]]; then
  echo "SPEC_MAP.md not found at ${SPEC_MAP}" >&2
  exit 2
fi

# Base ref for PR diff detection. Override with DRIFT_BASE=<ref>.
BASE_REF="${DRIFT_BASE:-origin/main}"

drift_count=0

# SPEC_MAP rows look like:
# | TR.inv_foo | organs/transmuter.qnt:inv_foo | contracts/.../file.rs:10-20 | <sha> | notes |
# Extract "file:lines" and the recorded sha.
grep -E '^\| [A-Z]+\.' "${SPEC_MAP}" | while IFS='|' read -r _ _id _qloc rust sha _notes _; do
  rust="$(echo "${rust}" | xargs)"
  sha="$(echo "${sha}" | xargs)"

  if [[ -z "${rust}" || "${rust}" == "—" || "${rust}" == "-" ]]; then
    continue
  fi
  if [[ "${sha}" == "<pending-init-sync>" || "${sha}" == "—" ]]; then
    # Allow unsynced rows before first review pass; CI warns but does not fail.
    continue
  fi

  # rust may be semicolon-separated list of file:lines — check each.
  IFS=';' read -ra refs <<< "${rust}"
  for ref in "${refs[@]}"; do
    ref="$(echo "${ref}" | xargs)"
    file="$(echo "${ref}" | awk -F: '{print $1}')"
    if [[ -z "${file}" ]]; then continue; fi
    if [[ ! -f "${REPO_ROOT}/${file}" ]]; then
      echo "SPEC_MAP row '${_id}' references missing file: ${file}" >&2
      drift_count=$((drift_count + 1))
      continue
    fi
    # Get the last-modifying commit for the file.
    last_commit="$(git -C "${REPO_ROOT}" log -1 --format=%H -- "${file}" 2>/dev/null || true)"
    if [[ -z "${last_commit}" ]]; then continue; fi
    if [[ "${last_commit}" == "${sha}" ]]; then continue; fi

    # Drift detected. Only report if this PR touched the referenced file.
    if git -C "${REPO_ROOT}" diff --name-only "${BASE_REF}"...HEAD | grep -Fxq "${file}"; then
      echo "DRIFT: ${_id} — ${file} last changed in ${last_commit}, SPEC_MAP records ${sha}"
      drift_count=$((drift_count + 1))
    fi
  done
done

if (( drift_count > 0 )); then
  echo ""
  echo "Found ${drift_count} drift row(s). Update SPEC_MAP.md and the"
  echo "corresponding .qnt/.lean files in this PR before merging." >&2
  exit 1
fi

echo "check_drift: clean"
exit 0
