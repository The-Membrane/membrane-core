#!/usr/bin/env bash

set -euo pipefail

# Q-Racing deployment script for CosmWasm chains (e.g., wasmd/osmosisd)
# Requires: bash, jq, a running node, and compiled wasm artifacts

# Configurable environment (override via env or .env file)
: "${CHAIN_BIN:=osmosisd}"
: "${CHAIN_ID:=localosmosis}"
: "${FROM:=wallet}"
: "${FEES:=5000uosmo}"
: "${GAS:=auto}"
: "${GAS_ADJUSTMENT:=1.5}"
: "${BROKER:=http://localhost:26657}"
: "${DENOM:=uosmo}"
: "${ADMIN:=$( $CHAIN_BIN keys show $FROM -a 2>/dev/null || echo "" )}"

# Artifacts (adjust if different build output path)
ROOT_DIR="$(cd "$(dirname "$0")"/../.. && pwd)"
ARTIFACTS_DIR="$ROOT_DIR/artifacts"

CAR_WASM="$ARTIFACTS_DIR/car.wasm"
TRACK_WASM="$ARTIFACTS_DIR/track_manager.wasm"
RACE_WASM="$ARTIFACTS_DIR/race_engine.wasm"
TOURNAMENT_WASM="$ARTIFACTS_DIR/tournament.wasm"
BYTE_MINTER_WASM="$ARTIFACTS_DIR/byte_minter.wasm"

OUT_DIR="$(cd "$(dirname "$0")" && pwd)"
OUT_JSON="$OUT_DIR/deploy_out.json"

usage() {
  cat <<EOF
Usage: $(basename "$0") [--no-byte-minter]

Environment variables:
  CHAIN_BIN, CHAIN_ID, FROM, FEES, GAS, GAS_ADJUSTMENT, BROKER, DENOM, ADMIN
  ARTIFACT paths default to ../../artifacts/*.wasm relative to this script.

Writes addresses and code IDs to: $OUT_JSON
EOF
}

NO_BYTE_MINTER=0
for arg in "$@"; do
  case "$arg" in
    --no-byte-minter) NO_BYTE_MINTER=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "Unknown arg: $arg"; usage; exit 1 ;;
  esac
done

require() {
  local f="$1"; if [ ! -f "$f" ]; then echo "Missing file: $f" >&2; exit 1; fi
}

require "$CAR_WASM"
require "$TRACK_WASM"
require "$RACE_WASM"
require "$TOURNAMENT_WASM"
if [ $NO_BYTE_MINTER -eq 0 ] && [ -f "$BYTE_MINTER_WASM" ]; then
  HAS_BYTE_MINTER=1
else
  HAS_BYTE_MINTER=0
fi

echo "Using binary: $CHAIN_BIN (chain-id=$CHAIN_ID)"
if [ -z "$ADMIN" ]; then
  echo "ADMIN is empty and could not be resolved from key '$FROM'. Set ADMIN explicitly." >&2
  exit 1
fi

tx() {
  $CHAIN_BIN tx wasm "$@" \
    --from "$FROM" \
    --chain-id "$CHAIN_ID" \
    --fees "$FEES" \
    --gas "$GAS" \
    --gas-adjustment "$GAS_ADJUSTMENT" \
    --node "$BROKER" \
    -y -o json | jq -r .
}

q_tx() {
  $CHAIN_BIN query tx "$1" --node "$BROKER" -o json | jq -r .
}

parse_code_id() {
  jq -r '.. | objects | select(has("code_id")) | .code_id' | head -n1
}

store() {
  local wasm="$1"
  echo "Storing $(basename "$wasm")..." >&2
  local res; res=$(tx store "$wasm")
  local txhash; txhash=$(echo "$res" | jq -r '.txhash // ."txhash" // empty')
  echo "Waiting for tx $txhash..." >&2
  sleep 2
  local q; q=$(q_tx "$txhash") || true
  # Fallback: parse from immediate response if available
  local code_id; code_id=$(echo "$q" | parse_code_id)
  if [ -z "$code_id" ] || [ "$code_id" = "null" ]; then
    code_id=$(echo "$res" | parse_code_id)
  fi
  if [ -z "$code_id" ] || [ "$code_id" = "null" ]; then
    echo "$res" | jq . >&2
    echo "$q" | jq . >&2
    echo "Failed to parse code_id" >&2; exit 1
  fi
  echo "$code_id"
}

instantiate() {
  local code_id="$1"; shift
  local label="$1"; shift
  local init_msg="$1"; shift
  local funds="$1"; shift || true
  [ -z "$funds" ] && funds=""
  echo "Instantiating $label (code_id=$code_id)..." >&2
  local cmd=(instantiate "$code_id" "$init_msg" --label "$label" --admin "$ADMIN")
  if [ -n "$funds" ]; then
    cmd+=(--amount "$funds")
  fi
  local res; res=$(tx "${cmd[@]}")
  local addr; addr=$(echo "$res" | jq -r '.. | objects | select(has("_contract_address")) | ._contract_address' | head -n1)
  if [ -z "$addr" ] || [ "$addr" = "null" ]; then
    echo "$res" | jq . >&2
    echo "Failed to parse instantiated address" >&2; exit 1
  fi
  echo "$addr"
}

write_out() {
  local key="$1"; local value="$2"
  if [ ! -f "$OUT_JSON" ]; then echo '{}' > "$OUT_JSON"; fi
  tmp=$(mktemp)
  jq --arg k "$key" --arg v "$value" '. as $o | $o + {($k): $v}' "$OUT_JSON" > "$tmp" && mv "$tmp" "$OUT_JSON"
}

# 1) Store wasm
CAR_CODE_ID=$(store "$CAR_WASM")
TRACK_CODE_ID=$(store "$TRACK_WASM")
RACE_CODE_ID=$(store "$RACE_WASM")
TOURNAMENT_CODE_ID=$(store "$TOURNAMENT_WASM")
if [ $HAS_BYTE_MINTER -eq 1 ]; then
  BYTE_MINTER_CODE_ID=$(store "$BYTE_MINTER_WASM")
fi

write_out CAR_CODE_ID "$CAR_CODE_ID"
write_out TRACK_CODE_ID "$TRACK_CODE_ID"
write_out RACE_CODE_ID "$RACE_CODE_ID"
write_out TOURNAMENT_CODE_ID "$TOURNAMENT_CODE_ID"
if [ $HAS_BYTE_MINTER -eq 1 ]; then
  write_out BYTE_MINTER_CODE_ID "$BYTE_MINTER_CODE_ID"
fi

# 2) Instantiate contracts
# car InstantiateMsg { name, symbol, payment_options? }
CAR_INIT=$(jq -n --arg name "QRacing Car" --arg symbol "QCAR" '{name:$name, symbol:$symbol, payment_options:null}')
CAR_ADDR=$(instantiate "$CAR_CODE_ID" "qcar" "$CAR_INIT")
write_out CAR_ADDR "$CAR_ADDR"

# track-manager InstantiateMsg { admin }
TRACK_INIT=$(jq -n --arg admin "$ADMIN" '{admin:$admin}')
TRACK_ADDR=$(instantiate "$TRACK_CODE_ID" "track-manager" "$TRACK_INIT")
write_out TRACK_ADDR "$TRACK_ADDR"

# race-engine InstantiateMsg { admin, track_contract, car_contract }
RACE_INIT=$(jq -n --arg admin "$ADMIN" --arg track "$TRACK_ADDR" --arg car "$CAR_ADDR" '{admin:$admin, track_contract:$track, car_contract:$car}')
RACE_ADDR=$(instantiate "$RACE_CODE_ID" "race-engine" "$RACE_INIT")
write_out RACE_ADDR "$RACE_ADDR"

# tournament InstantiateMsg { admin, race_engine }
TOURNAMENT_INIT=$(jq -n --arg admin "$ADMIN" --arg race "$RACE_ADDR" '{admin:$admin, race_engine:$race}')
TOURNAMENT_ADDR=$(instantiate "$TOURNAMENT_CODE_ID" "tournament" "$TOURNAMENT_INIT")
write_out TOURNAMENT_ADDR "$TOURNAMENT_ADDR"

# optional byte-minter InstantiateMsg
if [ $HAS_BYTE_MINTER -eq 1 ]; then
  # Defaults for testing; adjust as needed via env later
  : "${SUBDENOM:=byte}"
  : "${MINT_AMOUNT:=1000}"
  : "${MAZE_DEFAULT_DIFFICULTY:=2}"
  : "${MAZE_WIDTH:=16}"
  : "${MAZE_HEIGHT:=16}"
  : "${MAZE_CADENCE:=600}"
  : "${MAZE_WINDOW:=300}"
  : "${PVP_CADENCE:=600}"
  : "${PVP_WINDOW:=300}"
  BYTE_INIT=$(jq -n \
    --arg admin "$ADMIN" \
    --arg tm "$TRACK_ADDR" \
    --arg race "$RACE_ADDR" \
    --arg car "$CAR_ADDR" \
    --arg sub "$SUBDENOM" \
    --argjson mint "$MINT_AMOUNT" \
    --argjson maze_d "$MAZE_DEFAULT_DIFFICULTY" \
    --argjson maze_w "$MAZE_WIDTH" \
    --argjson maze_h "$MAZE_HEIGHT" \
    --argjson maze_c "$MAZE_CADENCE" \
    --argjson maze_win "$MAZE_WINDOW" \
    --argjson pvp_c "$PVP_CADENCE" \
    --argjson pvp_win "$PVP_WINDOW" \
    '{admin:$admin, track_manager_contract:$tm, race_engine_contract:$race, car_contract:$car, subdenom:$sub, tokenfactory_contract:null, mint_amount:$mint, maze_default_difficulty:$maze_d, maze_width:$maze_w, maze_height:$maze_h, maze_event_cadence_seconds:$maze_c, maze_event_window_seconds:$maze_win, pvp_event_cadence_seconds:$pvp_c, pvp_event_window_seconds:$pvp_win, min_progress_to_finish_per_start_tile:null, min_start_tile_progress_threshold:null, max_start_tile_progress_diff:null, revenue_contract:null, create_denom:false}')
  BYTE_ADDR=$(instantiate "$BYTE_MINTER_CODE_ID" "byte-minter" "$BYTE_INIT")
  write_out BYTE_MINTER_ADDR "$BYTE_ADDR"
fi

echo "\nDeployment complete. Results written to $OUT_JSON"


