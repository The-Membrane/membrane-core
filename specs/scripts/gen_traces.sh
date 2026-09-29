#!/usr/bin/env bash
# specs/scripts/gen_traces.sh
#
# Generate ITF (Informal Trace Format) traces from each Quint test scenario
# and drop them into the membrane-solidity differential-test fixtures
# directory. The Solidity port's `test/differential/*TraceReplay.t.sol` tests
# replay these traces against the ported contracts to assert per-step
# state-hash equivalence.
#
# Usage:
#   bash specs/scripts/gen_traces.sh
#
# Output:
#   ${SOLIDITY_ROOT:-/Users/EBmic/membrane-solidity}/test/differential/traces/<organ>_<scenario>.itf.json
#
# Requirements:
#   - quint  (https://github.com/informalsystems/quint), tested with v0.30+
#
# Implementation notes:
#   - Targets bash 3.2 (the macOS default) — no associative arrays, no
#     `mapfile`, no `<<<` heredocs in subshells.

set -eu

SPECS_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
QUINT_TESTS="${SPECS_ROOT}/quint/tests"
SOLIDITY_ROOT="${SOLIDITY_ROOT:-/Users/EBmic/membrane-solidity}"
OUT_DIR="${SOLIDITY_ROOT}/test/differential/traces"

mkdir -p "${OUT_DIR}"

if ! command -v quint >/dev/null 2>&1; then
  echo "error: quint not found on PATH" >&2
  exit 1
fi

# Pairs of "<test-file>:<organ-slug>". The slug is the prefix used in the
# resulting <organ>_<scenario>.itf.json fixture path; subclasses of
# TraceReplay.t.sol must agree on it.
PAIRS="
cdp_test.qnt:cdp
ltv_disco_test.qnt:ltv_disco
auction_test.qnt:auction
acquisition_test.qnt:acquisition
governance_test.qnt:governance
liquidation_engine_test.qnt:liquidation_engine
staking_test.qnt:staking
transmuter_test.qnt:transmuter
"

extract_runs() {
  # Pull each `run <name> =` declaration name from a Quint test file.
  grep -E "^[[:space:]]*run[[:space:]]+[a-zA-Z_][a-zA-Z0-9_]*[[:space:]]*=" "$1" \
    | sed -E "s/^[[:space:]]*run[[:space:]]+([a-zA-Z_][a-zA-Z0-9_]*)[[:space:]]*=.*$/\1/"
}

echo "Generating ITF traces -> ${OUT_DIR}"
echo

total=0
generated=0
failed=0

for entry in $PAIRS; do
  test_file="${entry%%:*}"
  organ="${entry##*:}"
  src="${QUINT_TESTS}/${test_file}"
  if [ ! -f "${src}" ]; then
    echo "skip: ${test_file} (not found)"
    continue
  fi

  for run_name in $(extract_runs "${src}"); do
    total=$((total + 1))
    out="${OUT_DIR}/${organ}_${run_name}.itf.json"
    echo "  [${organ}] ${run_name} -> $(basename "${out}")"

    # `--mbt` adds `mbt::actionTaken` and `mbt::nondetPicks` fields needed by
    # the Solidity replay harness.
    if quint run \
        --mbt \
        --max-steps=200 \
        --max-samples=1 \
        --step="${run_name}" \
        --out-itf="${out}" \
        "${src}" >/dev/null 2>&1; then
      generated=$((generated + 1))
    else
      echo "    !! quint run failed (continuing)"
      failed=$((failed + 1))
    fi
  done
done

echo
echo "done: ${generated}/${total} traces generated (${failed} failed)"
