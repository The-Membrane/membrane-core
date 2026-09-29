#!/usr/bin/env bash

set -euo pipefail

# Q-Racing deployment script for Neutron (neutrond)
# - Uploads and instantiates: car, track_manager, race_engine, byte_minter, racing_revenue
# - Wires car -> race_engine (update_config)
# - Wires race_engine -> byte_minter (update_config)
# - Sets racing_revenue as revenue contract for byte_minter
# - Emits addresses to a JSON file and shell exports

# Requirements: neutrond, jq

command -v neutrond >/dev/null 2>&1 || { echo "neutrond not found in PATH" >&2; exit 1; }
command -v jq >/dev/null 2>&1 || { echo "jq not found in PATH" >&2; exit 1; }

# Configurable inputs (env vars override defaults)
KEY_NAME=${KEY_NAME:-"wallet"}
CHAIN_ID=${CHAIN_ID:-"neutron-1"}
NODE=${NODE:-'https://rpc-celatone.neutron-1.neutron.org'}
GAS_PRICES=${GAS_PRICES:-"0.025untrn"}
GAS_ADJ=${GAS_ADJ:-"1.7"}
BROADCAST_MODE=${BROADCAST_MODE:-"sync"}

# Admin address (defaults to key address)
FROM_ADDR=${FROM_ADDR:-$(neutrond keys show "$KEY_NAME" -a 2>/dev/null)}
if [ -z "${FROM_ADDR}" ]; then
  echo "Unable to resolve FROM_ADDR from KEY_NAME='$KEY_NAME'. Ensure the key exists (neutrond keys add ...)" >&2
  exit 1
fi
ADMIN_ADDR=${ADMIN_ADDR:-"$FROM_ADDR"}

# WASM artifacts location (override with WASM_DIR if needed)
DEFAULT_WASM_DIR="/Users/EBmic/membrane-core/artifacts"
WASM_DIR=${WASM_DIR:-"$DEFAULT_WASM_DIR"}

CAR_WASM=${CAR_WASM:-"$WASM_DIR/car.wasm"}
TRACK_MGR_WASM=${TRACK_MGR_WASM:-"$WASM_DIR/track_manager.wasm"}
RACE_ENGINE_WASM=${RACE_ENGINE_WASM:-"$WASM_DIR/race_engine.wasm"}
BYTE_MINTER_WASM=${BYTE_MINTER_WASM:-"$WASM_DIR/byte_minter.wasm"}
RACING_REVENUE_WASM=${RACING_REVENUE_WASM:-"$WASM_DIR/racing_revenue.wasm"}

for f in "$CAR_WASM" "$TRACK_MGR_WASM" "$RACE_ENGINE_WASM" "$BYTE_MINTER_WASM"; do
  if [ ! -f "$f" ]; then
    echo "WASM file not found: $f" >&2
    exit 1
  fi
done

# Racing-revenue is optional - check if it exists
if [ ! -f "$RACING_REVENUE_WASM" ]; then
  echo "Warning: Racing-revenue WASM not found: $RACING_REVENUE_WASM" >&2
  echo "Deploying without racing-revenue contract..." >&2
  DEPLOY_RACING_REVENUE=false
else
  DEPLOY_RACING_REVENUE=true
fi

# Labels
CAR_LABEL=${CAR_LABEL:-"qracing-car"}
TRACK_MGR_LABEL=${TRACK_MGR_LABEL:-"qracing-track-manager"}
RACE_ENGINE_LABEL=${RACE_ENGINE_LABEL:-"qracing-race-engine"}
BYTE_MINTER_LABEL=${BYTE_MINTER_LABEL:-"qracing-byte-minter"}
RACING_REVENUE_LABEL=${RACING_REVENUE_LABEL:-"qracing-revenue"}

# Car instantiate config
CAR_NAME=${CAR_NAME:-"Q-Racer Car"}
CAR_SYMBOL=${CAR_SYMBOL:-"QRACER"}
# Provide empty payment options by default

# Byte-minter instantiate config (reasonable defaults; adjust as desired)
SUBDENOM=${SUBDENOM:-"byte"}
MINT_AMOUNT=${MINT_AMOUNT:-"16180339"} # Uint128 string
MAZE_DEFAULT_DIFFICULTY=${MAZE_DEFAULT_DIFFICULTY:-"2"}
MAZE_WIDTH=${MAZE_WIDTH:-"20"}
MAZE_HEIGHT=${MAZE_HEIGHT:-"20"}
MAZE_EVENT_CADENCE_SECONDS=${MAZE_EVENT_CADENCE_SECONDS:-"1"}
MAZE_EVENT_WINDOW_SECONDS=${MAZE_EVENT_WINDOW_SECONDS:-"28800"}
PVP_EVENT_CADENCE_SECONDS=${PVP_EVENT_CADENCE_SECONDS:-"1"}
PVP_EVENT_WINDOW_SECONDS=${PVP_EVENT_WINDOW_SECONDS:-"28800"}
MIN_START_TILE_PROGRESS_THRESHOLD=${MIN_START_TILE_PROGRESS_THRESHOLD:-"1"}
MAX_START_TILE_PROGRESS_DIFF=${MAX_START_TILE_PROGRESS_DIFF:-"3"}
REVENUE_CONTRACT=${REVENUE_CONTRACT:-""} # optional
TOKENFACTORY_CONTRACT=${TOKENFACTORY_CONTRACT:-""} # optional
CREATE_DENOM=${CREATE_DENOM:-"true"} # keep false by default for testing

OUT_DIR=${OUT_DIR:-"$(cd "$(dirname "$0")" && pwd)"}
OUT_FILE=${OUT_FILE:-"$OUT_DIR/q_racing_addresses.${CHAIN_ID}.json"}

echo "Using settings:" >&2
echo "  KEY_NAME=$KEY_NAME" >&2
echo "  FROM_ADDR=$FROM_ADDR" >&2
echo "  ADMIN_ADDR=$ADMIN_ADDR" >&2
echo "  CHAIN_ID=$CHAIN_ID" >&2
echo "  NODE=$NODE" >&2
echo "  GAS_PRICES=$GAS_PRICES GAS_ADJ=$GAS_ADJ" >&2
echo "  WASM_DIR=$WASM_DIR" >&2

tx_submit() {
  neutrond tx "$@" \
    --from "$KEY_NAME" \
    --chain-id "$CHAIN_ID" \
    --node "$NODE" \
    --gas auto \
    --gas-adjustment "$GAS_ADJ" \
    --gas-prices "$GAS_PRICES" \
    --broadcast-mode "$BROADCAST_MODE" \
    -y --output json
}

query_tx_attr() {
  local txhash="$1" type="$2" key="$3"
  neutrond query tx "$txhash" --node "$NODE" --output json | \
    jq -r --arg t "$type" --arg k "$key" '
      if .logs and .logs[0] and .logs[0].events then
        .logs[0].events[] | select(.type==$t) | .attributes[] | select(.key==$k) | .value
      else
        empty
      end' || true
}

store_wasm() {
  local wasm_path="$1"
  echo "Storing WASM: $wasm_path" >&2
  local res txhash code_id
  
  # Get the current highest code ID before storing
  local before_code_id
  before_code_id=$(neutrond query wasm list-code --node "$NODE" --output json | jq -r '.code_infos[-1].code_id // "0"')
  echo "Current highest code ID before store: $before_code_id" >&2
  
  res=$(tx_submit wasm store "$wasm_path")
  echo "$res" | jq '.' >/dev/null # validate JSON
  txhash=$(echo "$res" | jq -r '.txhash')
  if [ -z "$txhash" ] || [ "$txhash" = "null" ]; then
    echo "Failed to get txhash for store code" >&2
    echo "$res" >&2
    exit 1
  fi
  
  # Extract code_id directly from the transaction response (skip the failing indexer)
  echo "Extracting code_id from transaction response..." >&2
  local tx_response code_id
  for i in {1..20}; do
    sleep 2
    tx_response=$(neutrond query tx "$txhash" --node "$NODE" --output json 2>/dev/null)
    if [ -n "$tx_response" ]; then
      code_id=$(echo "$tx_response" | jq -r '.events[] | select(.type=="store_code") | .attributes[] | select(.key=="code_id") | .value' 2>/dev/null)
      if [ -n "$code_id" ] && [ "$code_id" != "null" ]; then
        echo "Got code_id from transaction response: $code_id" >&2
        break
      fi
    fi
    echo "Waiting for transaction to be available... attempt $i/20" >&2
  done
  
  if [ -z "$code_id" ] || [ "$code_id" = "null" ]; then
    echo "Failed to extract code_id from transaction response after 20 attempts" >&2
    if [ -n "$tx_response" ]; then
      echo "$tx_response" | jq '.' >&2
    fi
    exit 1
  fi
  echo "$code_id"
}

instantiate() {
  local code_id="$1" init_json="$2" label="$3" admin_addr="$4"
  echo "Instantiating label=$label code_id=$code_id" >&2
  local res txhash contract_addr
  res=$(tx_submit wasm instantiate "$code_id" "$init_json" --label "$label" --admin "$admin_addr")
  echo "$res" | jq '.' >/dev/null
  txhash=$(echo "$res" | jq -r '.txhash')
  if [ -z "$txhash" ] || [ "$txhash" = "null" ]; then
    echo "Failed to get txhash for instantiate $label" >&2
    echo "$res" >&2
    exit 1
  fi
  # Extract contract address directly from the transaction response (skip the failing indexer)
  echo "Extracting contract address from transaction response..." >&2
  local tx_response contract_addr
  for i in {1..20}; do
    sleep 2
    tx_response=$(neutrond query tx "$txhash" --node "$NODE" --output json 2>/dev/null)
    if [ -n "$tx_response" ]; then
      # try different event types and keys
      contract_addr=$(echo "$tx_response" | jq -r '.events[] | select(.type=="instantiate") | .attributes[] | select(.key=="_contract_address") | .value' 2>/dev/null)
      if [ -z "$contract_addr" ] || [ "$contract_addr" = "null" ]; then
        contract_addr=$(echo "$tx_response" | jq -r '.events[] | select(.type=="instantiate_contract") | .attributes[] | select(.key=="_contract_address") | .value' 2>/dev/null)
      fi
      if [ -z "$contract_addr" ] || [ "$contract_addr" = "null" ]; then
        contract_addr=$(echo "$tx_response" | jq -r '.events[] | select(.type=="instantiate") | .attributes[] | select(.key=="contract_address") | .value' 2>/dev/null)
      fi
      if [ -n "$contract_addr" ] && [ "$contract_addr" != "null" ]; then
        echo "Got contract address from transaction response: $contract_addr" >&2
        break
      fi
    fi
    echo "Waiting for instantiate transaction to be available... attempt $i/20" >&2
  done
  
  if [ -z "$contract_addr" ] || [ "$contract_addr" = "null" ]; then
    echo "Failed to extract contract address for $label (tx $txhash) after 20 attempts" >&2
    if [ -n "$tx_response" ]; then
      echo "$tx_response" | jq '.' >&2
    fi
    exit 1
  fi
  echo "$contract_addr"
}

execute() {
  local contract_addr="$1" exec_json="$2" note="$3"
  echo "Executing on $contract_addr: $note" >&2
  local res txhash
  res=$(tx_submit wasm execute "$contract_addr" "$exec_json")
  echo "$res" | jq '.' >/dev/null
  txhash=$(echo "$res" | jq -r '.txhash')
  if [ -z "$txhash" ] || [ "$txhash" = "null" ]; then
    echo "Failed to get txhash for execute ($note)" >&2
    echo "$res" >&2
    exit 1
  fi
  echo "$txhash"
}

# Optional: use pre-provided code IDs to skip store
CAR_CODE_ID=${CAR_CODE_ID:-""}
TRACK_MGR_CODE_ID=${TRACK_MGR_CODE_ID:-""}
RACE_ENGINE_CODE_ID=${RACE_ENGINE_CODE_ID:-""}
BYTE_MINTER_CODE_ID=${BYTE_MINTER_CODE_ID:-""}
RACING_REVENUE_CODE_ID=${RACING_REVENUE_CODE_ID:-""}

if [ -z "$CAR_CODE_ID" ]; then CAR_CODE_ID=$(store_wasm "$CAR_WASM"); fi
if [ -z "$TRACK_MGR_CODE_ID" ]; then TRACK_MGR_CODE_ID=$(store_wasm "$TRACK_MGR_WASM"); fi
if [ -z "$RACE_ENGINE_CODE_ID" ]; then RACE_ENGINE_CODE_ID=$(store_wasm "$RACE_ENGINE_WASM"); fi
if [ -z "$BYTE_MINTER_CODE_ID" ]; then BYTE_MINTER_CODE_ID=$(store_wasm "$BYTE_MINTER_WASM"); fi
if [ "$DEPLOY_RACING_REVENUE" = "true" ] && [ -z "$RACING_REVENUE_CODE_ID" ]; then 
  RACING_REVENUE_CODE_ID=$(store_wasm "$RACING_REVENUE_WASM")
fi

echo "Code IDs:" >&2
echo "  CAR_CODE_ID=$CAR_CODE_ID" >&2
echo "  TRACK_MGR_CODE_ID=$TRACK_MGR_CODE_ID" >&2
echo "  RACE_ENGINE_CODE_ID=$RACE_ENGINE_CODE_ID" >&2
echo "  BYTE_MINTER_CODE_ID=$BYTE_MINTER_CODE_ID" >&2
echo "  RACING_REVENUE_CODE_ID=$RACING_REVENUE_CODE_ID" >&2

# Build instantiate messages
CAR_INIT=$(jq -n \
  --arg name "$CAR_NAME" \
  --arg symbol "$CAR_SYMBOL" \
  '{name:$name, symbol:$symbol, payment_options: []}')

TRACK_MGR_INIT=$(jq -n --arg admin "$ADMIN_ADDR" '{admin:$admin}')

# race_engine requires addresses of track manager and car (built after we know addresses)

# Instantiate car and track manager first
CAR_ADDR=$(instantiate "$CAR_CODE_ID" "$CAR_INIT" "$CAR_LABEL" "$ADMIN_ADDR")
TRACK_MGR_ADDR=$(instantiate "$TRACK_MGR_CODE_ID" "$TRACK_MGR_INIT" "$TRACK_MGR_LABEL" "$ADMIN_ADDR")

# Instantiate racing-revenue contract (simple contract that does nothing) - if available
if [ "$DEPLOY_RACING_REVENUE" = "true" ]; then
  RACING_REVENUE_INIT=$(jq -n '{}')
  RACING_REVENUE_ADDR=$(instantiate "$RACING_REVENUE_CODE_ID" "$RACING_REVENUE_INIT" "$RACING_REVENUE_LABEL" "$ADMIN_ADDR")
else
  RACING_REVENUE_ADDR=""
fi

# Now build race engine init with actual addresses (without byte_minter for now)
RACE_ENGINE_INIT=$(jq -n \
  --arg admin "$ADMIN_ADDR" \
  --arg track "$TRACK_MGR_ADDR" \
  --arg car "$CAR_ADDR" \
  '{admin:$admin, track_contract:$track, car_contract:$car}')

RACE_ENGINE_ADDR=$(instantiate "$RACE_ENGINE_CODE_ID" "$RACE_ENGINE_INIT" "$RACE_ENGINE_LABEL" "$ADMIN_ADDR")

# Wire car -> race engine config (set race_engine_contract)
UPDATE_CAR_CFG=$(jq -n --arg re "$RACE_ENGINE_ADDR" '{update_config:{race_engine_contract:$re}}')
execute "$CAR_ADDR" "$UPDATE_CAR_CFG" "update_config: race_engine_contract"

# Build byte-minter init with addresses and parameters
BYTE_MINTER_INIT=$(jq -n \
  --arg admin "$ADMIN_ADDR" \
  --arg track "$TRACK_MGR_ADDR" \
  --arg race "$RACE_ENGINE_ADDR" \
  --arg car "$CAR_ADDR" \
  --arg subdenom "$SUBDENOM" \
  --arg mint "$MINT_AMOUNT" \
  --arg revenue "${RACING_REVENUE_ADDR:-}" \
  --arg tfc "$TOKENFACTORY_CONTRACT" \
  --argjson maze_diff "$MAZE_DEFAULT_DIFFICULTY" \
  --argjson maze_w "$MAZE_WIDTH" \
  --argjson maze_h "$MAZE_HEIGHT" \
  --argjson maze_c "$MAZE_EVENT_CADENCE_SECONDS" \
  --argjson maze_wnd "$MAZE_EVENT_WINDOW_SECONDS" \
  --argjson pvp_c "$PVP_EVENT_CADENCE_SECONDS" \
  --argjson pvp_wnd "$PVP_EVENT_WINDOW_SECONDS" \
  --argjson min_thr "$MIN_START_TILE_PROGRESS_THRESHOLD" \
  --argjson max_diff "$MAX_START_TILE_PROGRESS_DIFF" \
  --argjson create_denom "$CREATE_DENOM" \
  '{
    admin:$admin,
    track_manager_contract:$track,
    race_engine_contract:$race,
    car_contract:$car,
    subdenom:$subdenom,
    tokenfactory_contract: (if ($tfc|length>0) then $tfc else null end),
    mint_amount:$mint,
    maze_default_difficulty: $maze_diff,
    maze_width: $maze_w,
    maze_height: $maze_h,
    maze_event_cadence_seconds: $maze_c,
    maze_event_window_seconds: $maze_wnd,
    pvp_event_cadence_seconds: $pvp_c,
    pvp_event_window_seconds: $pvp_wnd,
    min_start_tile_progress_threshold: $min_thr,
    max_start_tile_progress_diff: $max_diff,
    revenue_contract: (if ($revenue|length>0) then $revenue else null end),
    create_denom: $create_denom
  }')

BYTE_MINTER_ADDR=$(instantiate "$BYTE_MINTER_CODE_ID" "$BYTE_MINTER_INIT" "$BYTE_MINTER_LABEL" "$ADMIN_ADDR")

# Update race engine config to include byte_minter contract
UPDATE_RACE_ENGINE_CFG=$(jq -n --arg bm "$BYTE_MINTER_ADDR" '{update_config:{byte_minter_contract:$bm}}')
execute "$RACE_ENGINE_ADDR" "$UPDATE_RACE_ENGINE_CFG" "update_config: byte_minter_contract"

# Emit outputs
jq -n \
  --arg car "$CAR_ADDR" \
  --arg trackManager "$TRACK_MGR_ADDR" \
  --arg raceEngine "$RACE_ENGINE_ADDR" \
  --arg byteMinter "$BYTE_MINTER_ADDR" \
  --arg racingRevenue "${RACING_REVENUE_ADDR:-}" \
  '{car:$car, trackManager:$trackManager, raceEngine:$raceEngine, byteMinter:$byteMinter, racingRevenue:$racingRevenue}' | tee "$OUT_FILE"

echo >&2
echo "Deployed addresses:" >&2
echo "  CAR_ADDR=$CAR_ADDR" >&2
echo "  TRACK_MGR_ADDR=$TRACK_MGR_ADDR" >&2
echo "  RACE_ENGINE_ADDR=$RACE_ENGINE_ADDR" >&2
echo "  BYTE_MINTER_ADDR=$BYTE_MINTER_ADDR" >&2
if [ "$DEPLOY_RACING_REVENUE" = "true" ]; then
  echo "  RACING_REVENUE_ADDR=$RACING_REVENUE_ADDR" >&2
else
  echo "  RACING_REVENUE_ADDR=NOT_DEPLOYED" >&2
fi
echo >&2
echo "Shell exports:" >&2
echo "export CAR_ADDR=$CAR_ADDR" >&2
echo "export TRACK_MGR_ADDR=$TRACK_MGR_ADDR" >&2
echo "export RACE_ENGINE_ADDR=$RACE_ENGINE_ADDR" >&2
echo "export BYTE_MINTER_ADDR=$BYTE_MINTER_ADDR" >&2
if [ "$DEPLOY_RACING_REVENUE" = "true" ]; then
  echo "export RACING_REVENUE_ADDR=$RACING_REVENUE_ADDR" >&2
else
  echo "export RACING_REVENUE_ADDR=NOT_DEPLOYED" >&2
fi
echo >&2
echo "Addresses written to: $OUT_FILE" >&2


