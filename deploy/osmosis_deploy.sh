#!/bin/bash
set -euo pipefail

###############################################################################
# osmosis_deploy.sh — Deploy Membrane protocol contracts on Osmosis
#
# Usage:
#   ./osmosis_deploy.sh              # Full deploy (resume-safe)
#   ./osmosis_deploy.sh --reset      # Wipe state and start fresh
#   ./osmosis_deploy.sh --from-step N # Resume from step N
#
# State is saved to deploy_state.json after every step so the script can
# resume where it left off if it fails midway.
#
# Architecture: Modular CDP (collateral + debt + liquidation_engine)
# LTV Disco: Unstaking model (no dispersal)
###############################################################################

# ========================== CONFIGURATION ====================================

NODE="https://osmosis-rpc.polkachu.com:443"
CHAIN_ID="osmosis-1"
WALLET="wallet"
GAS_PRICES="0.025uosmo"
GAS_ADJUSTMENT="1.5"
ARTIFACTS_DIR="/Users/EBmic/membrane-core/artifacts"
STATE_FILE="$(cd "$(dirname "$0")" && pwd)/deploy_state.json"

# IBC denoms on Osmosis
NOBLE_USDC_DENOM="ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4"
ATOM_DENOM="ibc/27394FB092D2ECCD56123C74F36E4C1F926001CEADA9CA97EA622B25F41E5EB2"
USDC_AXL_DENOM="ibc/D189335C6E4A68B513C10AB227BF1C1D38C746766278BA3EEB4FB14124F1D858"
OSMO_DENOM="uosmo"
ALLBTC_DENOM="factory/osmo1z6r6qdknhgsc0zeracktgpcxf43j6sekq07nw8sxduc9lg0qjjlqfu25e3/alloyed/allBTC"

# WASM file mapping — contract_name -> wasm filename (without .wasm extension)
declare -A WASM_FILES=(
  [osmosis_proxy]="osmosis_proxy"
  [oracle]="oracle"
  [staking]="staking"
  [collateral]="collateral"
  [debt]="debt"
  [liquidation_engine]="liquidation_engine"
  [liq_queue]="liq_queue"
  [ltv_disco]="ltv_disco"
  [transmuter]="transmuter"
  [revenue_distributor]="revenue_distributor"
  [acquisition]="acquisition"
  [points_system]="points_system"
  [system_discounts]="system_discounts"
  [auction]="auction"
  [emissions_voting]="emissions_voting"
)

# Contracts to deploy (in order — used for the store phase)
CONTRACTS=(
  osmosis_proxy
  oracle
  staking
  collateral
  debt
  liquidation_engine
  liq_queue
  ltv_disco
  transmuter
  revenue_distributor
  acquisition
  points_system
  system_discounts
  auction
  emissions_voting
)

# ========================== HELPERS ==========================================

log()  { echo -e "\033[1;36m[DEPLOY]\033[0m $*"; }
warn() { echo -e "\033[1;33m[WARN]\033[0m $*"; }
err()  { echo -e "\033[1;31m[ERROR]\033[0m $*" >&2; }
ok()   { echo -e "\033[1;32m[OK]\033[0m $*"; }

# --------------- State management ---------------

init_state() {
  if [[ ! -f "$STATE_FILE" ]]; then
    echo '{"code_ids":{},"addresses":{},"denoms":{},"step":0}' > "$STATE_FILE"
    log "Initialized fresh state file: $STATE_FILE"
  fi
}

# Robust state getter
state_get() {
  python3 -c "
import json, sys
with open('$STATE_FILE') as f:
    d = json.load(f)
keys = '$1'.split('.')
v = d
for k in keys:
    if isinstance(v, dict) and k in v:
        v = v[k]
    else:
        sys.exit(0)
print(v)
" 2>/dev/null || echo ""
}

state_set() {
  # Usage: state_set 'code_ids.collateral' '1234'
  python3 -c "
import json
with open('$STATE_FILE') as f:
    d = json.load(f)
keys = '$1'.split('.')
ref = d
for k in keys[:-1]:
    ref = ref.setdefault(k, {})
ref[keys[-1]] = '$2'
with open('$STATE_FILE', 'w') as f:
    json.dump(d, f, indent=2)
"
}

state_set_int() {
  python3 -c "
import json
with open('$STATE_FILE') as f:
    d = json.load(f)
keys = '$1'.split('.')
ref = d
for k in keys[:-1]:
    ref = ref.setdefault(k, {})
ref[keys[-1]] = $2
with open('$STATE_FILE', 'w') as f:
    json.dump(d, f, indent=2)
"
}

get_step() {
  python3 -c "import json; print(json.load(open('$STATE_FILE'))['step'])" 2>/dev/null || echo "0"
}

set_step() {
  state_set_int "step" "$1"
}

# --------------- Transaction helpers ---------------

OWNER_ADDR=""
get_owner_addr() {
  if [[ -z "$OWNER_ADDR" ]]; then
    OWNER_ADDR=$(osmosisd keys show "$WALLET" -a 2>/dev/null)
    if [[ -z "$OWNER_ADDR" ]]; then
      err "Could not resolve wallet address for key '$WALLET'"
      exit 1
    fi
  fi
  echo "$OWNER_ADDR"
}

tx_flags() {
  echo "--node $NODE --chain-id $CHAIN_ID --from $WALLET --gas-prices $GAS_PRICES --gas auto --gas-adjustment $GAS_ADJUSTMENT -y --output json"
}

# Store a wasm binary and return the code_id
store_contract() {
  local name=$1
  local wasm_file="${ARTIFACTS_DIR}/${WASM_FILES[$name]}.wasm"

  # Check if already stored
  local existing
  existing=$(state_get "code_ids.$name")
  if [[ -n "$existing" && "$existing" != "None" ]]; then
    log "$name already stored (code_id=$existing), skipping"
    echo "$existing"
    return 0
  fi

  if [[ ! -f "$wasm_file" ]]; then
    err "WASM file not found: $wasm_file"
    err "Add the compiled .wasm to: $ARTIFACTS_DIR/"
    exit 1
  fi

  log "Storing $name from $wasm_file ..."
  local result
  result=$(osmosisd tx wasm store "$wasm_file" $(tx_flags) 2>&1)

  local txhash
  txhash=$(echo "$result" | python3 -c "import json,sys; print(json.load(sys.stdin)['txhash'])" 2>/dev/null)

  if [[ -z "$txhash" ]]; then
    err "Failed to broadcast store tx for $name"
    err "$result"
    exit 1
  fi

  log "Waiting for tx $txhash ..."
  sleep 10

  local tx_result
  tx_result=$(osmosisd query tx "$txhash" --node "$NODE" --output json 2>/dev/null)

  local code_id
  code_id=$(echo "$tx_result" | python3 -c "
import json, sys
d = json.load(sys.stdin)
for ev in d.get('events', d.get('logs', [{}])[0].get('events', [])):
    if ev.get('type') == 'store_code':
        for attr in ev['attributes']:
            if attr['key'] == 'code_id':
                print(attr['value'])
                sys.exit(0)
for log_entry in d.get('logs', []):
    for ev in log_entry.get('events', []):
        if ev.get('type') == 'store_code':
            for attr in ev['attributes']:
                if attr['key'] == 'code_id':
                    print(attr['value'])
                    sys.exit(0)
print('')
" 2>/dev/null)

  if [[ -z "$code_id" ]]; then
    err "Could not extract code_id for $name from tx $txhash"
    err "Check: osmosisd query tx $txhash --node $NODE"
    exit 1
  fi

  state_set "code_ids.$name" "$code_id"
  ok "$name stored: code_id=$code_id"
  echo "$code_id"
}

# Instantiate a contract and return the contract address
instantiate_contract() {
  local label=$1
  local code_id=$2
  local init_msg=$3
  local extra_flags=${4:-""}

  log "Instantiating $label (code_id=$code_id) ..."

  local owner
  owner=$(get_owner_addr)

  local result
  result=$(osmosisd tx wasm instantiate "$code_id" "$init_msg" \
    --label "$label" --admin "$owner" $extra_flags $(tx_flags) 2>&1)

  local txhash
  txhash=$(echo "$result" | python3 -c "import json,sys; print(json.load(sys.stdin)['txhash'])" 2>/dev/null)

  if [[ -z "$txhash" ]]; then
    err "Failed to broadcast instantiate tx for $label"
    err "$result"
    exit 1
  fi

  log "Waiting for tx $txhash ..."
  sleep 10

  local tx_result
  tx_result=$(osmosisd query tx "$txhash" --node "$NODE" --output json 2>/dev/null)

  local contract_addr
  contract_addr=$(echo "$tx_result" | python3 -c "
import json, sys
d = json.load(sys.stdin)
for ev in d.get('events', d.get('logs', [{}])[0].get('events', [])):
    if ev.get('type') == 'instantiate':
        for attr in ev['attributes']:
            if attr['key'] == '_contract_address':
                print(attr['value'])
                sys.exit(0)
for log_entry in d.get('logs', []):
    for ev in log_entry.get('events', []):
        if ev.get('type') == 'instantiate':
            for attr in ev['attributes']:
                if attr['key'] == '_contract_address':
                    print(attr['value'])
                    sys.exit(0)
print('')
" 2>/dev/null)

  if [[ -z "$contract_addr" ]]; then
    err "Could not extract contract address for $label from tx $txhash"
    exit 1
  fi

  state_set "addresses.$label" "$contract_addr"
  ok "$label instantiated: $contract_addr"
  echo "$contract_addr"
}

# Execute a contract message
execute_contract() {
  local contract_addr=$1
  local exec_msg=$2
  local description=${3:-"execute"}
  local funds=${4:-""}

  log "Executing on $(echo "$contract_addr" | cut -c1-20)... ($description)"

  local funds_flag=""
  if [[ -n "$funds" ]]; then
    funds_flag="--amount $funds"
  fi

  local result
  result=$(osmosisd tx wasm execute "$contract_addr" "$exec_msg" \
    $funds_flag $(tx_flags) 2>&1)

  local txhash
  txhash=$(echo "$result" | python3 -c "import json,sys; print(json.load(sys.stdin)['txhash'])" 2>/dev/null)

  if [[ -z "$txhash" ]]; then
    err "Failed to broadcast execute tx ($description)"
    err "$result"
    exit 1
  fi

  log "Waiting for tx $txhash ..."
  sleep 10

  # Verify success
  local tx_result
  tx_result=$(osmosisd query tx "$txhash" --node "$NODE" --output json 2>/dev/null)
  local code
  code=$(echo "$tx_result" | python3 -c "import json,sys; print(json.load(sys.stdin).get('code',0))" 2>/dev/null)

  if [[ "$code" != "0" ]]; then
    local raw_log
    raw_log=$(echo "$tx_result" | python3 -c "import json,sys; print(json.load(sys.stdin).get('raw_log',''))" 2>/dev/null)
    err "Execute failed ($description): $raw_log"
    exit 1
  fi

  ok "$description succeeded (tx: $txhash)"
}

# Query a contract
query_contract() {
  local contract_addr=$1
  local query_msg=$2
  osmosisd query wasm contract-state smart "$contract_addr" "$query_msg" \
    --node "$NODE" --output json 2>/dev/null
}

# ========================== STEP FUNCTIONS ===================================

# ---- STEP 1: Store all WASM binaries ----
step_1_store_contracts() {
  log "========== STEP 1: Store contract WASMs =========="
  for contract in "${CONTRACTS[@]}"; do
    store_contract "$contract"
  done
  set_step 1
  ok "All contracts stored"
}

# ---- STEP 2: Instantiate Osmosis Proxy ----
step_2_instantiate_proxy() {
  log "========== STEP 2: Instantiate Osmosis Proxy =========="

  local existing
  existing=$(state_get "addresses.osmosis_proxy")
  if [[ -n "$existing" && "$existing" != "None" ]]; then
    log "osmosis_proxy already deployed at $existing, skipping"
    set_step 2; return 0
  fi

  local code_id
  code_id=$(state_get "code_ids.osmosis_proxy")

  instantiate_contract "osmosis_proxy" "$code_id" '{}' > /dev/null
  set_step 2
}

# ---- STEP 3: Create CDT and MBRN denoms ----
step_3_create_denoms() {
  log "========== STEP 3: Create CDT & MBRN denoms =========="

  local proxy_addr
  proxy_addr=$(state_get "addresses.osmosis_proxy")

  # Check if denoms already exist
  local existing_cdt
  existing_cdt=$(state_get "denoms.cdt")
  if [[ -n "$existing_cdt" && "$existing_cdt" != "None" ]]; then
    log "Denoms already created, skipping"
    set_step 3; return 0
  fi

  # Create CDT
  execute_contract "$proxy_addr" \
    '{"create_denom":{"subdenom":"ucdt","max_supply":null}}' \
    "Create CDT denom"

  # Create MBRN (with max supply of 80B with 6 decimals)
  execute_contract "$proxy_addr" \
    '{"create_denom":{"subdenom":"umbrn","max_supply":"80000000000000"}}' \
    "Create MBRN denom"

  # Query denoms from proxy
  local denoms_result
  denoms_result=$(query_contract "$proxy_addr" '{"get_contract_denoms":{"limit":null}}')

  local cdt_denom mbrn_denom
  cdt_denom=$(echo "$denoms_result" | python3 -c "
import json, sys
d = json.load(sys.stdin)
denoms = d.get('data', d).get('denoms', [])
for denom in denoms:
    if 'ucdt' in denom:
        print(denom)
        sys.exit(0)
print('')
" 2>/dev/null)

  mbrn_denom=$(echo "$denoms_result" | python3 -c "
import json, sys
d = json.load(sys.stdin)
denoms = d.get('data', d).get('denoms', [])
for denom in denoms:
    if 'umbrn' in denom:
        print(denom)
        sys.exit(0)
print('')
" 2>/dev/null)

  if [[ -z "$cdt_denom" || -z "$mbrn_denom" ]]; then
    # Construct from pattern if query fails
    cdt_denom="factory/$proxy_addr/ucdt"
    mbrn_denom="factory/$proxy_addr/umbrn"
    warn "Could not query denoms, using constructed: $cdt_denom / $mbrn_denom"
  fi

  state_set "denoms.cdt" "$cdt_denom"
  state_set "denoms.mbrn" "$mbrn_denom"
  ok "CDT denom: $cdt_denom"
  ok "MBRN denom: $mbrn_denom"
  set_step 3
}

# ---- STEP 4: Instantiate Oracle ----
step_4_instantiate_oracle() {
  log "========== STEP 4: Instantiate Oracle =========="

  local existing
  existing=$(state_get "addresses.oracle")
  if [[ -n "$existing" && "$existing" != "None" ]]; then
    log "oracle already deployed at $existing, skipping"
    set_step 4; return 0
  fi

  local code_id
  code_id=$(state_get "code_ids.oracle")

  instantiate_contract "oracle" "$code_id" \
    '{"owner":null,"positions_contract":null}' > /dev/null
  set_step 4
}

# ---- STEP 5: Instantiate Staking ----
step_5_instantiate_staking() {
  log "========== STEP 5: Instantiate Staking =========="

  local existing
  existing=$(state_get "addresses.staking")
  if [[ -n "$existing" && "$existing" != "None" ]]; then
    log "staking already deployed at $existing, skipping"
    set_step 5; return 0
  fi

  local code_id proxy_addr mbrn_denom
  code_id=$(state_get "code_ids.staking")
  proxy_addr=$(state_get "addresses.osmosis_proxy")
  mbrn_denom=$(state_get "denoms.mbrn")

  local init_msg
  init_msg=$(cat <<EOF
{
  "owner": null,
  "positions_contract": null,
  "auction_contract": null,
  "vesting_contract": null,
  "governance_contract": null,
  "osmosis_proxy": "$proxy_addr",
  "emissions_voting_contract": null,
  "incentive_schedule": null,
  "unstaking_period": null,
  "mbrn_denom": "$mbrn_denom"
}
EOF
  )

  instantiate_contract "staking" "$code_id" "$init_msg" > /dev/null
  set_step 5
}

# ---- STEP 6: Instantiate Collateral ----
# NOTE: ltv_disco is required+validated at init. We use the deployer address as
# a valid placeholder, then update to real Disco addr after Disco is deployed.
step_6_instantiate_collateral() {
  log "========== STEP 6: Instantiate Collateral =========="

  local existing
  existing=$(state_get "addresses.collateral")
  if [[ -n "$existing" && "$existing" != "None" ]]; then
    log "collateral already deployed at $existing, skipping"
    set_step 6; return 0
  fi

  local code_id oracle_addr proxy_addr owner
  code_id=$(state_get "code_ids.collateral")
  oracle_addr=$(state_get "addresses.oracle")
  proxy_addr=$(state_get "addresses.osmosis_proxy")
  owner=$(get_owner_addr)

  local init_msg
  init_msg=$(cat <<EOF
{
  "owner": null,
  "oracle_contract": "$oracle_addr",
  "chain_proxy": "$proxy_addr",
  "ltv_disco": "$owner",
  "debt_contract": null,
  "liquidation_contract": null,
  "collateral_twap_timeframe": 60,
  "oracle_time_limit": 600,
  "create_basket": {
    "basket_id": "1",
    "collateral_types": [],
    "collateral_supply_caps": [],
    "multi_asset_supply_caps": []
  },
  "ltv_upward_kp": null,
  "ltv_downward_period": null,
  "ltv_downward_decay_rate": null,
  "ltv_borrow_distance": null
}
EOF
  )

  instantiate_contract "collateral" "$code_id" "$init_msg" > /dev/null
  set_step 6
}

# ---- STEP 7: Instantiate Debt ----
step_7_instantiate_debt() {
  log "========== STEP 7: Instantiate Debt =========="

  local existing
  existing=$(state_get "addresses.debt")
  if [[ -n "$existing" && "$existing" != "None" ]]; then
    log "debt already deployed at $existing, skipping"
    set_step 7; return 0
  fi

  local code_id collateral_addr staking_addr proxy_addr oracle_addr cdt_denom
  code_id=$(state_get "code_ids.debt")
  collateral_addr=$(state_get "addresses.collateral")
  staking_addr=$(state_get "addresses.staking")
  proxy_addr=$(state_get "addresses.osmosis_proxy")
  oracle_addr=$(state_get "addresses.oracle")
  cdt_denom=$(state_get "denoms.cdt")

  local init_msg
  init_msg=$(cat <<EOF
{
  "owner": null,
  "collateral_contract": "$collateral_addr",
  "liquidation_contract": null,
  "staking_contract": "$staking_addr",
  "chain_proxy": "$proxy_addr",
  "debt_auction": null,
  "oracle_contract": "$oracle_addr",
  "liquidity_contract": null,
  "discounts_contract": null,
  "revenue_distributor": null,
  "transmuter_addr": null,
  "points_contract": null,
  "liq_fee": "0",
  "credit_twap_timeframe": 480,
  "cpc_multiplier": "1",
  "debt_minimum": "20000000",
  "base_debt_cap_multiplier": "1000000000",
  "rate_slope_multiplier": "1.618",
  "affiliate_fee_max": "0.01",
  "irm_config": {
    "adjustment_speed": "50",
    "min_adaptive_rate": "0.001",
    "max_adaptive_rate": "2"
  },
  "acquisition_contract": null,
  "create_basket": {
    "basket_id": "1",
    "credit_asset": {
      "info": {"native_token": {"denom": "$cdt_denom"}},
      "amount": "0"
    },
    "credit_price": "1",
    "base_interest_rate": "0.03",
    "credit_pool_infos": [],
    "liq_queue": null
  }
}
EOF
  )

  instantiate_contract "debt" "$code_id" "$init_msg" > /dev/null
  set_step 7
}

# ---- STEP 8: Instantiate Liquidation Queue ----
step_8_instantiate_liq_queue() {
  log "========== STEP 8: Instantiate Liquidation Queue =========="

  local existing
  existing=$(state_get "addresses.liq_queue")
  if [[ -n "$existing" && "$existing" != "None" ]]; then
    log "liq_queue already deployed at $existing, skipping"
    set_step 8; return 0
  fi

  local code_id collateral_addr proxy_addr owner
  code_id=$(state_get "code_ids.liq_queue")
  collateral_addr=$(state_get "addresses.collateral")
  proxy_addr=$(state_get "addresses.osmosis_proxy")
  owner=$(get_owner_addr)

  local init_msg
  init_msg=$(cat <<EOF
{
  "owner": "$owner",
  "positions_contract": "$collateral_addr",
  "osmosis_proxy_contract": "$proxy_addr",
  "waiting_period": 60,
  "minimum_bid": "5000000",
  "maximum_waiting_bids": 5000
}
EOF
  )

  instantiate_contract "liq_queue" "$code_id" "$init_msg" > /dev/null
  set_step 8
}

# ---- STEP 9: Instantiate LTV Disco ----
# cdp_contract is required+validated at init. We pass the collateral contract address.
step_9_instantiate_ltv_disco() {
  log "========== STEP 9: Instantiate LTV Disco =========="

  local existing
  existing=$(state_get "addresses.ltv_disco")
  if [[ -n "$existing" && "$existing" != "None" ]]; then
    log "ltv_disco already deployed at $existing, skipping"
    set_step 9; return 0
  fi

  local code_id collateral_addr oracle_addr proxy_addr mbrn_denom cdt_denom
  code_id=$(state_get "code_ids.ltv_disco")
  collateral_addr=$(state_get "addresses.collateral")
  oracle_addr=$(state_get "addresses.oracle")
  proxy_addr=$(state_get "addresses.osmosis_proxy")
  mbrn_denom=$(state_get "denoms.mbrn")
  cdt_denom=$(state_get "denoms.cdt")

  local init_msg
  init_msg=$(cat <<EOF
{
  "owner": null,
  "cdp_contract": "$collateral_addr",
  "deposit_denom": {
    "denom": "$mbrn_denom",
    "vault_info": null
  },
  "cdt_denom": "$cdt_denom",
  "minimum_deposit": "5000000",
  "unstaking_period": null,
  "oracle_contract": "$oracle_addr",
  "chain_proxy_contract": "$proxy_addr",
  "emissions_voting_contract": null,
  "affiliate_fee": "0.01",
  "max_management_fee": "0.05",
  "points_system_contract": null,
  "revenue_distributor": null,
  "auction_contract": null,
  "mbrn_denom": "$mbrn_denom"
}
EOF
  )

  instantiate_contract "ltv_disco" "$code_id" "$init_msg" > /dev/null
  set_step 9
}

# ---- STEP 10: Wire Collateral — set ltv_disco to real Disco addr ----
step_10_wire_collateral_disco() {
  log "========== STEP 10: Wire Collateral — set ltv_disco =========="

  local collateral_addr ltv_disco_addr
  collateral_addr=$(state_get "addresses.collateral")
  ltv_disco_addr=$(state_get "addresses.ltv_disco")

  execute_contract "$collateral_addr" \
    "{\"update_config\":{\"ltv_disco\":\"$ltv_disco_addr\"}}" \
    "Collateral: set ltv_disco to real Disco address"

  set_step 10
}

# ---- STEP 11: Instantiate Liquidation Engine ----
step_11_instantiate_liquidation_engine() {
  log "========== STEP 11: Instantiate Liquidation Engine =========="

  local existing
  existing=$(state_get "addresses.liquidation_engine")
  if [[ -n "$existing" && "$existing" != "None" ]]; then
    log "liquidation_engine already deployed at $existing, skipping"
    set_step 11; return 0
  fi

  local code_id collateral_addr debt_addr proxy_addr staking_addr cdt_denom
  code_id=$(state_get "code_ids.liquidation_engine")
  collateral_addr=$(state_get "addresses.collateral")
  debt_addr=$(state_get "addresses.debt")
  proxy_addr=$(state_get "addresses.osmosis_proxy")
  staking_addr=$(state_get "addresses.staking")
  cdt_denom=$(state_get "denoms.cdt")

  local init_msg
  init_msg=$(cat <<EOF
{
  "owner": null,
  "collateral_contract": "$collateral_addr",
  "debt_contract": "$debt_addr",
  "liq_queue": null,
  "chain_proxy": "$proxy_addr",
  "debt_auction": null,
  "staking_contract": "$staking_addr",
  "liq_fee": "0",
  "credit_denom": "$cdt_denom",
  "liquidation_delay": null
}
EOF
  )

  instantiate_contract "liquidation_engine" "$code_id" "$init_msg" > /dev/null
  set_step 11
}

# ---- STEP 12: Instantiate Transmuter ----
step_12_instantiate_transmuter() {
  log "========== STEP 12: Instantiate Transmuter =========="

  local existing
  existing=$(state_get "addresses.transmuter")
  if [[ -n "$existing" && "$existing" != "None" ]]; then
    log "transmuter already deployed at $existing, skipping"
    set_step 12; return 0
  fi

  local code_id debt_addr cdt_denom
  code_id=$(state_get "code_ids.transmuter")
  debt_addr=$(state_get "addresses.debt")
  cdt_denom=$(state_get "denoms.cdt")

  local init_msg
  init_msg=$(cat <<EOF
{
  "owner": null,
  "tokenfactory_contract": null,
  "discounts_contract": "",
  "cdp_contract": "$debt_addr",
  "vault_subdenom": "usdc-transmuter",
  "deposit_pair": {
    "cdt": "$cdt_denom",
    "paired_asset": "$NOBLE_USDC_DENOM"
  },
  "composition_leeway": "0.05",
  "asset_a_to_b_rate": "1",
  "cdt_target_ratio": "0",
  "usage_fee": "0.0005",
  "usage_fee_utilization_threshold": "0.9",
  "swap_history_cap": 100,
  "volume_history_cap": 100,
  "rate_limit_window_secs": 28800,
  "rate_limit_threshold": "0.05",
  "revenue_distributor_addr": null,
  "revenue_distributions": null,
  "allowlist": null,
  "allowlist_rate_limit_threshold": null,
  "global_rate_limit_window_secs": 86400,
  "global_rate_limit_threshold": "0.2",
  "lock_ceiling": 1460,
  "affiliate_fee": "0.01",
  "send_swap_fee": true
}
EOF
  )

  instantiate_contract "transmuter" "$code_id" "$init_msg" > /dev/null

  # Save the vault token denom
  local transmuter_addr
  transmuter_addr=$(state_get "addresses.transmuter")
  local vt_denom="factory/$transmuter_addr/usdc-transmuter"
  state_set "denoms.transmuter_vt" "$vt_denom"
  ok "Transmuter vault token: $vt_denom"

  set_step 12
}

# ---- STEP 13: Instantiate Revenue Distributor ----
step_13_instantiate_revenue_distributor() {
  log "========== STEP 13: Instantiate Revenue Distributor =========="

  local existing
  existing=$(state_get "addresses.revenue_distributor")
  if [[ -n "$existing" && "$existing" != "None" ]]; then
    log "revenue_distributor already deployed at $existing, skipping"
    set_step 13; return 0
  fi

  local code_id cdt_denom ltv_disco_addr transmuter_addr vt_denom owner
  code_id=$(state_get "code_ids.revenue_distributor")
  cdt_denom=$(state_get "denoms.cdt")
  ltv_disco_addr=$(state_get "addresses.ltv_disco")
  transmuter_addr=$(state_get "addresses.transmuter")
  vt_denom=$(state_get "denoms.transmuter_vt")
  owner=$(get_owner_addr)

  local init_msg
  init_msg=$(cat <<EOF
{
  "owner": "$owner",
  "canonical_asset": {
    "info": {"native_token": {"denom": "$cdt_denom"}},
    "amount": "0"
  },
  "revenue_destinations": [
    {
      "destination": "$transmuter_addr",
      "distribution_ratio": "0.9"
    },
    {
      "destination": "$ltv_disco_addr",
      "distribution_ratio": "0.1"
    }
  ],
  "ltv_disco": "$ltv_disco_addr",
  "transmuter_vault": {
    "vault_addr": "$transmuter_addr",
    "deposit_token": "$cdt_denom",
    "vault_token": "$vt_denom"
  },
  "points_system_contract": null,
  "cdp_contract": null,
  "revenue_dispersal_window": null,
  "acquisition_contract": null,
  "ltv_disco_contract": null,
  "auction_contract": null
}
EOF
  )

  instantiate_contract "revenue_distributor" "$code_id" "$init_msg" > /dev/null
  set_step 13
}

# ---- STEP 14: Instantiate Acquisition ----
step_14_instantiate_acquisition() {
  log "========== STEP 14: Instantiate Acquisition =========="

  local existing
  existing=$(state_get "addresses.acquisition")
  if [[ -n "$existing" && "$existing" != "None" ]]; then
    log "acquisition already deployed at $existing, skipping"
    set_step 14; return 0
  fi

  local code_id transmuter_addr proxy_addr cdt_denom mbrn_denom staking_addr ltv_disco_addr owner
  code_id=$(state_get "code_ids.acquisition")
  transmuter_addr=$(state_get "addresses.transmuter")
  proxy_addr=$(state_get "addresses.osmosis_proxy")
  cdt_denom=$(state_get "denoms.cdt")
  mbrn_denom=$(state_get "denoms.mbrn")
  staking_addr=$(state_get "addresses.staking")
  ltv_disco_addr=$(state_get "addresses.ltv_disco")
  owner=$(get_owner_addr)

  local init_msg
  init_msg=$(cat <<EOF
{
  "owner": "$owner",
  "transmuter_contract": "$transmuter_addr",
  "neutron_proxy": "$proxy_addr",
  "lockdrop_incentive_size": "10000000000000",
  "deposit_period_days": 5,
  "withdrawal_period_days": 2,
  "deposit_token": "$cdt_denom",
  "minimum_deposit": "1000000",
  "mbrn_denom": "$mbrn_denom",
  "staking_contract": "$staking_addr",
  "mars_mirror_contract": null,
  "ltv_disco_contract": "$ltv_disco_addr",
  "discounts_contract": "",
  "maximum_boost": "2",
  "minimum_lock_days": 30,
  "emissions_voting_contract": null
}
EOF
  )

  instantiate_contract "acquisition" "$code_id" "$init_msg" > /dev/null
  set_step 14
}

# ---- STEP 15: Instantiate Points System ----
step_15_instantiate_points_system() {
  log "========== STEP 15: Instantiate Points System =========="

  local existing
  existing=$(state_get "addresses.points_system")
  if [[ -n "$existing" && "$existing" != "None" ]]; then
    log "points_system already deployed at $existing, skipping"
    set_step 15; return 0
  fi

  local code_id cdt_denom mbrn_denom oracle_addr collateral_addr lq_addr proxy_addr
  code_id=$(state_get "code_ids.points_system")
  cdt_denom=$(state_get "denoms.cdt")
  mbrn_denom=$(state_get "denoms.mbrn")
  oracle_addr=$(state_get "addresses.oracle")
  collateral_addr=$(state_get "addresses.collateral")
  lq_addr=$(state_get "addresses.liq_queue")
  proxy_addr=$(state_get "addresses.osmosis_proxy")

  local init_msg
  init_msg=$(cat <<EOF
{
  "cdt_denom": "$cdt_denom",
  "mbrn_denom": "$mbrn_denom",
  "oracle_contract": "$oracle_addr",
  "positions_contract": "$collateral_addr",
  "stability_pool_contract": "",
  "liq_queue_contract": "$lq_addr",
  "governance_contract": "",
  "osmosis_proxy_contract": "$proxy_addr",
  "emissions_voting_contract": null
}
EOF
  )

  instantiate_contract "points_system" "$code_id" "$init_msg" > /dev/null
  set_step 15
}

# ---- STEP 16: Instantiate System Discounts ----
step_16_instantiate_system_discounts() {
  log "========== STEP 16: Instantiate System Discounts =========="

  local existing
  existing=$(state_get "addresses.system_discounts")
  if [[ -n "$existing" && "$existing" != "None" ]]; then
    log "system_discounts already deployed at $existing, skipping"
    set_step 16; return 0
  fi

  local code_id oracle_addr collateral_addr staking_addr ltv_disco_addr owner
  code_id=$(state_get "code_ids.system_discounts")
  oracle_addr=$(state_get "addresses.oracle")
  collateral_addr=$(state_get "addresses.collateral")
  staking_addr=$(state_get "addresses.staking")
  ltv_disco_addr=$(state_get "addresses.ltv_disco")
  owner=$(get_owner_addr)

  local init_msg
  init_msg=$(cat <<EOF
{
  "owner": "$owner",
  "oracle_contract": "$oracle_addr",
  "positions_contract": "$collateral_addr",
  "staking_contract": "$staking_addr",
  "lockdrop_contract": null,
  "discount_vault_contract": null,
  "ltv_disco_contract": "$ltv_disco_addr",
  "minimum_time_in_network": 7,
  "max_discount": "0.75",
  "mbrn_at_max_discount": null,
  "max_boost": null
}
EOF
  )

  instantiate_contract "system_discounts" "$code_id" "$init_msg" > /dev/null
  set_step 16
}

# ---- STEP 17: Instantiate Emissions Voting ----
step_17_instantiate_emissions_voting() {
  log "========== STEP 17: Instantiate Emissions Voting =========="

  local existing
  existing=$(state_get "addresses.emissions_voting")
  if [[ -n "$existing" && "$existing" != "None" ]]; then
    log "emissions_voting already deployed at $existing, skipping"
    set_step 17; return 0
  fi

  local code_id ltv_disco_addr staking_addr owner
  code_id=$(state_get "code_ids.emissions_voting")
  ltv_disco_addr=$(state_get "addresses.ltv_disco")
  staking_addr=$(state_get "addresses.staking")
  owner=$(get_owner_addr)

  local init_msg
  init_msg=$(cat <<EOF
{
  "owner": "$owner",
  "ltv_disco_contract": "$ltv_disco_addr",
  "staking_contract": "$staking_addr"
}
EOF
  )

  instantiate_contract "emissions_voting" "$code_id" "$init_msg" > /dev/null
  set_step 17
}

# ---- STEP 18: Instantiate Auction (Debt Auction) ----
step_18_instantiate_auction() {
  log "========== STEP 18: Instantiate Auction =========="

  local existing
  existing=$(state_get "addresses.auction")
  if [[ -n "$existing" && "$existing" != "None" ]]; then
    log "auction already deployed at $existing, skipping"
    set_step 18; return 0
  fi

  local code_id oracle_addr proxy_addr debt_addr staking_addr mbrn_denom rev_dist_addr ltv_disco_addr
  code_id=$(state_get "code_ids.auction")
  oracle_addr=$(state_get "addresses.oracle")
  proxy_addr=$(state_get "addresses.osmosis_proxy")
  debt_addr=$(state_get "addresses.debt")
  staking_addr=$(state_get "addresses.staking")
  mbrn_denom=$(state_get "denoms.mbrn")
  rev_dist_addr=$(state_get "addresses.revenue_distributor")
  ltv_disco_addr=$(state_get "addresses.ltv_disco")

  local init_msg
  init_msg=$(cat <<EOF
{
  "owner": null,
  "oracle_contract": "$oracle_addr",
  "osmosis_proxy": "$proxy_addr",
  "positions_contract": "$debt_addr",
  "governance_contract": "$staking_addr",
  "staking_contract": "$staking_addr",
  "twap_timeframe": 60,
  "mbrn_denom": "$mbrn_denom",
  "initial_discount": "0.01",
  "discount_increase_timeframe": 36,
  "discount_increase": "0.01",
  "delay_window_minutes": 60,
  "revenue_distributor_contract": "$rev_dist_addr",
  "ltv_disco_contract": "$ltv_disco_addr"
}
EOF
  )

  instantiate_contract "auction" "$code_id" "$init_msg" > /dev/null
  set_step 18
}

# ========================== WIRING STEPS =====================================

# ---- STEP 19: Wire — Update Oracle config ----
step_19_wire_oracle() {
  log "========== STEP 19: Wire Oracle =========="

  local oracle_addr collateral_addr
  oracle_addr=$(state_get "addresses.oracle")
  collateral_addr=$(state_get "addresses.collateral")

  execute_contract "$oracle_addr" \
    "{\"update_config\":{\"owner\":null,\"positions_contract\":\"$collateral_addr\"}}" \
    "Oracle: set positions_contract to Collateral"

  set_step 19
}

# ---- STEP 20: Wire — Collateral config (debt, liquidation_engine) ----
step_20_wire_collateral() {
  log "========== STEP 20: Wire Collateral config =========="

  local collateral_addr debt_addr liq_engine_addr
  collateral_addr=$(state_get "addresses.collateral")
  debt_addr=$(state_get "addresses.debt")
  liq_engine_addr=$(state_get "addresses.liquidation_engine")

  local msg
  msg=$(cat <<EOF
{
  "update_config": {
    "debt_contract": "$debt_addr",
    "liquidation_contract": "$liq_engine_addr"
  }
}
EOF
  )

  execute_contract "$collateral_addr" "$msg" "Collateral: wire debt + liquidation engine"
  set_step 20
}

# ---- STEP 21: Wire — Debt config (auction, discounts, rev dist, transmuter, points, liq engine, acquisition) ----
step_21_wire_debt() {
  log "========== STEP 21: Wire Debt config =========="

  local debt_addr auction_addr discounts_addr rev_dist_addr transmuter_addr points_addr liq_engine_addr acquisition_addr
  debt_addr=$(state_get "addresses.debt")
  auction_addr=$(state_get "addresses.auction")
  discounts_addr=$(state_get "addresses.system_discounts")
  rev_dist_addr=$(state_get "addresses.revenue_distributor")
  transmuter_addr=$(state_get "addresses.transmuter")
  points_addr=$(state_get "addresses.points_system")
  liq_engine_addr=$(state_get "addresses.liquidation_engine")
  acquisition_addr=$(state_get "addresses.acquisition")

  local msg
  msg=$(cat <<EOF
{
  "update_config": {
    "debt_auction": "$auction_addr",
    "discounts_contract": "$discounts_addr",
    "revenue_distributor": "$rev_dist_addr",
    "transmuter_addr": "$transmuter_addr",
    "points_contract": "$points_addr",
    "liquidation_contract": "$liq_engine_addr",
    "acquisition_contract": "$acquisition_addr"
  }
}
EOF
  )

  execute_contract "$debt_addr" "$msg" "Debt: wire auction, discounts, rev dist, transmuter, points, liq engine, acquisition"
  set_step 21
}

# ---- STEP 22: Wire — Debt basket (add LQ, set negative_rates, cpc_margin_of_error) ----
step_22_wire_debt_basket() {
  log "========== STEP 22: Wire Debt basket =========="

  local debt_addr lq_addr
  debt_addr=$(state_get "addresses.debt")
  lq_addr=$(state_get "addresses.liq_queue")

  local msg
  msg=$(cat <<EOF
{
  "edit_basket": {
    "added_c_asset": null,
    "liq_queue": "$lq_addr",
    "collateral_supply_caps": null,
    "base_interest_rate": null,
    "credit_asset_twap_price_source": null,
    "negative_rates": false,
    "cpc_margin_of_error": "0.01",
    "frozen": null,
    "distribute_revenue": null,
    "multi_asset_supply_caps": null,
    "credit_pool_infos": [],
    "take_revenue": null
  }
}
EOF
  )

  execute_contract "$debt_addr" "$msg" "Debt: add LQ to basket, set negative_rates=false, cpc_margin=1%"
  set_step 22
}

# ---- STEP 23: Wire — Liquidation Engine (LQ, debt auction) ----
step_23_wire_liquidation_engine() {
  log "========== STEP 23: Wire Liquidation Engine =========="

  local liq_engine_addr lq_addr auction_addr
  liq_engine_addr=$(state_get "addresses.liquidation_engine")
  lq_addr=$(state_get "addresses.liq_queue")
  auction_addr=$(state_get "addresses.auction")

  local msg
  msg=$(cat <<EOF
{
  "update_config": {
    "liq_queue": "$lq_addr",
    "debt_auction": "$auction_addr"
  }
}
EOF
  )

  execute_contract "$liq_engine_addr" "$msg" "Liquidation Engine: wire LQ + auction"
  set_step 23
}

# ---- STEP 24: Wire — Staking config ----
step_24_wire_staking() {
  log "========== STEP 24: Wire Staking config =========="

  local staking_addr debt_addr auction_addr proxy_addr emissions_addr owner
  staking_addr=$(state_get "addresses.staking")
  debt_addr=$(state_get "addresses.debt")
  auction_addr=$(state_get "addresses.auction")
  proxy_addr=$(state_get "addresses.osmosis_proxy")
  emissions_addr=$(state_get "addresses.emissions_voting")
  owner=$(get_owner_addr)

  local msg
  msg=$(cat <<EOF
{
  "update_config": {
    "owner": "$owner",
    "positions_contract": "$debt_addr",
    "auction_contract": "$auction_addr",
    "vesting_contract": null,
    "governance_contract": null,
    "osmosis_proxy": "$proxy_addr",
    "emissions_voting_contract": "$emissions_addr",
    "mbrn_denom": null,
    "incentive_schedule": null,
    "unstaking_period": null,
    "max_commission_rate": null,
    "keep_raw_cdt": null,
    "vesting_rev_multiplier": null,
    "buyback_and_burn": null
  }
}
EOF
  )

  execute_contract "$staking_addr" "$msg" "Staking: wire contracts"
  set_step 24
}

# ---- STEP 25: Wire — Transmuter config ----
step_25_wire_transmuter() {
  log "========== STEP 25: Wire Transmuter config =========="

  local transmuter_addr discounts_addr rev_dist_addr emissions_addr points_addr collateral_addr debt_addr liq_engine_addr
  transmuter_addr=$(state_get "addresses.transmuter")
  discounts_addr=$(state_get "addresses.system_discounts")
  rev_dist_addr=$(state_get "addresses.revenue_distributor")
  emissions_addr=$(state_get "addresses.emissions_voting")
  points_addr=$(state_get "addresses.points_system")
  collateral_addr=$(state_get "addresses.collateral")
  debt_addr=$(state_get "addresses.debt")
  liq_engine_addr=$(state_get "addresses.liquidation_engine")

  local msg
  msg=$(cat <<EOF
{
  "update_config": {
    "owner": null,
    "deposit_pair": null,
    "composition_leeway": null,
    "cdt_target_ratio": null,
    "tokenfactory_contract": null,
    "discounts_contract": "$discounts_addr",
    "cdp_contract": null,
    "usage_fee": null,
    "usage_fee_utilization_threshold": null,
    "swap_history_cap": null,
    "volume_history_cap": null,
    "rate_limit_window_secs": null,
    "rate_limit_threshold": null,
    "allowlist": [
      {"entry": "$collateral_addr", "remove": false},
      {"entry": "$debt_addr", "remove": false},
      {"entry": "$liq_engine_addr", "remove": false}
    ],
    "allowlist_rate_limit_threshold": null,
    "global_rate_limit_window_secs": null,
    "global_rate_limit_threshold": null,
    "revenue_distributor_addr": "$rev_dist_addr",
    "revenue_distributions": null,
    "lock_ceiling": null,
    "affiliate_fee": "0.01",
    "send_swap_fee": true,
    "revenue_distributor_fee_percentage": null,
    "acquisition_contract": null,
    "points_system_contract": "$points_addr",
    "retention_multiplier": "0"
  }
}
EOF
  )

  execute_contract "$transmuter_addr" "$msg" "Transmuter: wire discounts, rev distributor, points"
  set_step 25
}

# ---- STEP 26: Wire — LTV Disco config ----
step_26_wire_ltv_disco() {
  log "========== STEP 26: Wire LTV Disco config =========="

  local ltv_disco_addr emissions_addr points_addr rev_dist_addr auction_addr mbrn_denom
  ltv_disco_addr=$(state_get "addresses.ltv_disco")
  emissions_addr=$(state_get "addresses.emissions_voting")
  points_addr=$(state_get "addresses.points_system")
  rev_dist_addr=$(state_get "addresses.revenue_distributor")
  auction_addr=$(state_get "addresses.auction")
  mbrn_denom=$(state_get "denoms.mbrn")

  local msg
  msg=$(cat <<EOF
{
  "update_config": {
    "owner": null,
    "cdp_contract": null,
    "deposit_denom": null,
    "cdt_denom": null,
    "minimum_deposit": null,
    "unstaking_period": null,
    "oracle_contract": null,
    "chain_proxy_contract": null,
    "emissions_voting_contract": "$emissions_addr",
    "affiliate_fee": null,
    "max_management_fee": null,
    "points_system_contract": "$points_addr",
    "revenue_distributor": "$rev_dist_addr",
    "auction_contract": "$auction_addr",
    "mbrn_denom": "$mbrn_denom"
  }
}
EOF
  )

  execute_contract "$ltv_disco_addr" "$msg" "LTV Disco: wire emissions, points, rev dist, auction"
  set_step 26
}

# ---- STEP 27: Wire — Acquisition config ----
step_27_wire_acquisition() {
  log "========== STEP 27: Wire Acquisition config =========="

  local acquisition_addr discounts_addr emissions_addr
  acquisition_addr=$(state_get "addresses.acquisition")
  discounts_addr=$(state_get "addresses.system_discounts")
  emissions_addr=$(state_get "addresses.emissions_voting")

  local msg
  msg=$(cat <<EOF
{
  "update_config": {
    "owner": null,
    "transmuter_contract": null,
    "neutron_proxy": null,
    "lockdrop_incentive_size": null,
    "deposit_period_days": null,
    "withdrawal_period_days": null,
    "deposit_token": null,
    "minimum_deposit": null,
    "mbrn_denom": null,
    "staking_contract": null,
    "mars_mirror_contract": null,
    "ltv_disco_contract": null,
    "discounts_contract": "$discounts_addr",
    "maximum_boost": null,
    "minimum_lock_days": null,
    "emissions_voting_contract": "$emissions_addr"
  }
}
EOF
  )

  execute_contract "$acquisition_addr" "$msg" "Acquisition: wire discounts, emissions voting"
  set_step 27
}

# ---- STEP 28: Wire — Points System config ----
step_28_wire_points_system() {
  log "========== STEP 28: Wire Points System config =========="

  local points_addr emissions_addr rev_dist_addr
  points_addr=$(state_get "addresses.points_system")
  emissions_addr=$(state_get "addresses.emissions_voting")
  rev_dist_addr=$(state_get "addresses.revenue_distributor")

  local msg
  msg=$(cat <<EOF
{
  "update_config": {
    "owner": null,
    "cdt_denom": null,
    "mbrn_denom": null,
    "oracle_contract": null,
    "positions_contract": null,
    "stability_pool_contract": null,
    "liq_queue_contract": null,
    "governance_contract": null,
    "osmosis_proxy_contract": null,
    "transmuter_contract": null,
    "ltv_disco_contract": null,
    "system_discounts_contract": null,
    "emissions_voting_contract": "$emissions_addr",
    "revenue_distributor_contract": "$rev_dist_addr",
    "mbrn_per_point": null,
    "max_mbrn_distribution": null,
    "points_per_dollar": null,
    "points_multipliers": null
  }
}
EOF
  )

  execute_contract "$points_addr" "$msg" "Points System: wire emissions, rev dist"
  set_step 28
}

# ---- STEP 29: Wire — Revenue Distributor config ----
step_29_wire_revenue_distributor() {
  log "========== STEP 29: Wire Revenue Distributor config =========="

  local rev_dist_addr points_addr debt_addr acquisition_addr ltv_disco_addr auction_addr
  rev_dist_addr=$(state_get "addresses.revenue_distributor")
  points_addr=$(state_get "addresses.points_system")
  debt_addr=$(state_get "addresses.debt")
  acquisition_addr=$(state_get "addresses.acquisition")
  ltv_disco_addr=$(state_get "addresses.ltv_disco")
  auction_addr=$(state_get "addresses.auction")

  local msg
  msg=$(cat <<EOF
{
  "update_config": {
    "revenue_destinations": null,
    "ltv_disco": null,
    "transmuter_vault": null,
    "points_system_contract": "$points_addr",
    "cdp_contract": "$debt_addr",
    "revenue_dispersal_window": 7,
    "acquisition_contract": "$acquisition_addr",
    "ltv_disco_contract": "$ltv_disco_addr",
    "auction_contract": "$auction_addr"
  }
}
EOF
  )

  execute_contract "$rev_dist_addr" "$msg" "Revenue Distributor: wire points, debt, acquisition, ltv disco, auction"
  set_step 29
}

# ---- STEP 30: Wire — Osmosis Proxy owners ----
step_30_wire_proxy() {
  log "========== STEP 30: Wire Osmosis Proxy owners =========="

  local proxy_addr debt_addr staking_addr lq_addr auction_addr acquisition_addr transmuter_addr
  proxy_addr=$(state_get "addresses.osmosis_proxy")
  debt_addr=$(state_get "addresses.debt")
  staking_addr=$(state_get "addresses.staking")
  lq_addr=$(state_get "addresses.liq_queue")
  auction_addr=$(state_get "addresses.auction")
  acquisition_addr=$(state_get "addresses.acquisition")
  transmuter_addr=$(state_get "addresses.transmuter")

  local msg
  msg=$(cat <<EOF
{
  "update_config": {
    "owners": [
      {"owner": {"owner": "$debt_addr", "non_token_contract_auth": false}, "remove": false},
      {"owner": {"owner": "$staking_addr", "non_token_contract_auth": false}, "remove": false},
      {"owner": {"owner": "$lq_addr", "non_token_contract_auth": false}, "remove": false},
      {"owner": {"owner": "$auction_addr", "non_token_contract_auth": false}, "remove": false},
      {"owner": {"owner": "$acquisition_addr", "non_token_contract_auth": false}, "remove": false}
    ],
    "transmutation_pairs": null,
    "debt_auction": "$auction_addr",
    "transmuter_contract": "$transmuter_addr",
    "vaults": null,
    "astroport_factory": null,
    "astroport_router": null,
    "enable_dynamic_routing": null,
    "transmute_supply_thresholds": null,
    "vesting_contract": null,
    "vesting_period": null
  }
}
EOF
  )

  execute_contract "$proxy_addr" "$msg" "Proxy: add contract owners (Debt, Staking, LQ, Auction, Acquisition)"
  set_step 30
}

# ---- STEP 31: Create Emissions Voting graphs ----
step_31_create_emissions_graphs() {
  log "========== STEP 31: Create Emissions Voting graphs =========="

  local emissions_addr acquisition_addr
  emissions_addr=$(state_get "addresses.emissions_voting")
  acquisition_addr=$(state_get "addresses.acquisition")

  # Acquisition graph (Uint128)
  execute_contract "$emissions_addr" \
    "{\"create_graph\":{\"label\":\"acquisition\",\"graph_type\":\"uint128\",\"range_min\":\"0\",\"range_max\":\"1000000000000\",\"period_days\":7,\"callback_contract\":\"$acquisition_addr\",\"persistent_voting\":null}}" \
    "Create graph: acquisition"

  # Transmuter retention emissions derived from acquisition rate * retention_multiplier (0.5)
  # No separate voting graphs needed.

  set_step 31
}

# ---- STEP 32: Add oracle price feeds ----
step_32_add_oracle_feeds() {
  log "========== STEP 32: Add oracle price feeds =========="

  local oracle_addr
  oracle_addr=$(state_get "addresses.oracle")

  # OSMO
  execute_contract "$oracle_addr" \
    "{\"add_asset\":{\"asset_info\":\"uosmo\",\"oracle_info\":{\"basket_id\":\"1\",\"pyth_price_feed_id\":\"5867f5683c757393a0670ef0f701490950fe93fdb006d181c8265a831ac0c5c6\",\"pools_for_osmo_twap\":[{\"pool_id\":678,\"base_asset_denom\":\"uosmo\",\"quote_asset_denom\":\"$USDC_AXL_DENOM\"}],\"is_usd_par\":false,\"lp_pool_info\":null,\"vault_info\":null,\"decimals\":6}}}" \
    "Oracle: add OSMO feed"

  # Noble USDC
  execute_contract "$oracle_addr" \
    "{\"add_asset\":{\"asset_info\":\"$NOBLE_USDC_DENOM\",\"oracle_info\":{\"basket_id\":\"1\",\"pyth_price_feed_id\":\"eaa020c61cc479712813461ce153894a96a6c00b21ed0cfc2798d1f9a9e9c94a\",\"pools_for_osmo_twap\":[{\"pool_id\":1464,\"base_asset_denom\":\"ibc/D189335C6E4A68B513C10AB227BF1C1D38C746766278BA3EEB4FB14124F1D858\",\"quote_asset_denom\":\"uosmo\"}],\"is_usd_par\":true,\"lp_pool_info\":null,\"vault_info\":null,\"decimals\":6}}}" \
    "Oracle: add USDC feed"

  # allBTC
  execute_contract "$oracle_addr" \
    "{\"add_asset\":{\"asset_info\":\"$ALLBTC_DENOM\",\"oracle_info\":{\"basket_id\":\"1\",\"pyth_price_feed_id\":null,\"pools_for_osmo_twap\":[{\"pool_id\":1943,\"base_asset_denom\":\"$ALLBTC_DENOM\",\"quote_asset_denom\":\"$NOBLE_USDC_DENOM\"}],\"is_usd_par\":false,\"lp_pool_info\":null,\"vault_info\":null,\"decimals\":8}}}" \
    "Oracle: add allBTC feed"

  # ATOM
  execute_contract "$oracle_addr" \
    "{\"add_asset\":{\"asset_info\":\"$ATOM_DENOM\",\"oracle_info\":{\"basket_id\":\"1\",\"pyth_price_feed_id\":\"b00b60f88b03a6a625a8d1c048c3f66653edf217439983d037e7222c4e612819\",\"pools_for_osmo_twap\":[{\"pool_id\":1,\"base_asset_denom\":\"$ATOM_DENOM\",\"quote_asset_denom\":\"$OSMO_DENOM\"}],\"is_usd_par\":false,\"lp_pool_info\":null,\"vault_info\":null,\"decimals\":6}}}" \
    "Oracle: add ATOM feed"

  set_step 32
}

# ---- STEP 33: Add collateral assets to Collateral contract ----
step_33_add_collateral() {
  log "========== STEP 33: Add collateral assets =========="

  local collateral_addr
  collateral_addr=$(state_get "addresses.collateral")

  # OSMO — max_LTV 50% (max_borrow_LTV auto-derived = max_LTV - ltv_borrow_distance)
  local msg
  msg=$(cat <<EOF
{
  "edit_basket": {
    "added_cAsset": {
      "asset": {"info": {"native_token": {"denom": "uosmo"}}, "amount": "0"},
      "max_borrow_LTV": "0",
      "max_LTV": "0.50",
      "rate_index": "1",
      "pool_info": null,
      "force_redemptions": null
    },
    "collateral_supply_caps": [
      {"asset_info": {"native_token": {"denom": "uosmo"}}, "current_supply": "0", "debt_total": "0", "supply_cap_ratio": "1", "lp": false, "stability_pool_ratio_for_debt_cap": null}
    ],
    "multi_asset_supply_caps": null
  }
}
EOF
  )
  execute_contract "$collateral_addr" "$msg" "Collateral: add OSMO (50% LTV)"

  # ATOM — max_LTV 55%
  msg=$(cat <<EOF
{
  "edit_basket": {
    "added_cAsset": {
      "asset": {"info": {"native_token": {"denom": "$ATOM_DENOM"}}, "amount": "0"},
      "max_borrow_LTV": "0",
      "max_LTV": "0.55",
      "rate_index": "1",
      "pool_info": null,
      "force_redemptions": null
    },
    "collateral_supply_caps": [
      {"asset_info": {"native_token": {"denom": "$ATOM_DENOM"}}, "current_supply": "0", "debt_total": "0", "supply_cap_ratio": "1", "lp": false, "stability_pool_ratio_for_debt_cap": null}
    ],
    "multi_asset_supply_caps": null
  }
}
EOF
  )
  execute_contract "$collateral_addr" "$msg" "Collateral: add ATOM (55% LTV)"

  # allBTC — max_LTV 70%
  msg=$(cat <<EOF
{
  "edit_basket": {
    "added_cAsset": {
      "asset": {"info": {"native_token": {"denom": "$ALLBTC_DENOM"}}, "amount": "0"},
      "max_borrow_LTV": "0",
      "max_LTV": "0.70",
      "rate_index": "1",
      "pool_info": null,
      "force_redemptions": null
    },
    "collateral_supply_caps": [
      {"asset_info": {"native_token": {"denom": "$ALLBTC_DENOM"}}, "current_supply": "0", "debt_total": "0", "supply_cap_ratio": "1", "lp": false, "stability_pool_ratio_for_debt_cap": null}
    ],
    "multi_asset_supply_caps": null
  }
}
EOF
  )
  execute_contract "$collateral_addr" "$msg" "Collateral: add allBTC (70% LTV)"

  # Noble USDC — max_LTV 96%
  msg=$(cat <<EOF
{
  "edit_basket": {
    "added_cAsset": {
      "asset": {"info": {"native_token": {"denom": "$NOBLE_USDC_DENOM"}}, "amount": "0"},
      "max_borrow_LTV": "0",
      "max_LTV": "0.96",
      "rate_index": "1",
      "pool_info": null,
      "force_redemptions": true
    },
    "collateral_supply_caps": [
      {"asset_info": {"native_token": {"denom": "$NOBLE_USDC_DENOM"}}, "current_supply": "0", "debt_total": "0", "supply_cap_ratio": "1", "lp": false, "stability_pool_ratio_for_debt_cap": null}
    ],
    "multi_asset_supply_caps": null
  }
}
EOF
  )
  execute_contract "$collateral_addr" "$msg" "Collateral: add Noble USDC (96% LTV)"

  set_step 33
}

# ---- STEP 34: Set per-asset max_threshold_to_delay via EditcAsset ----
# See docs/liquidation_delay_window.md for full analysis.
# 4% max 1hr window optimizes breach rate vs recovery rate for BTC, ATOM, OSMO.
step_34_set_liquidation_thresholds() {
  log "========== STEP 34: Set per-asset max_threshold_to_delay =========="

  local collateral_addr
  collateral_addr=$(state_get "addresses.collateral")

  # allBTC — 4%
  execute_contract "$collateral_addr" \
    "{\"editc_asset\":{\"asset\":{\"native_token\":{\"denom\":\"$ALLBTC_DENOM\"}},\"max_borrow_LTV\":null,\"max_LTV\":null,\"max_threshold_to_delay\":\"0.04\"}}" \
    "Collateral: set allBTC max_threshold_to_delay (4%)"

  # ATOM — 4%
  execute_contract "$collateral_addr" \
    "{\"editc_asset\":{\"asset\":{\"native_token\":{\"denom\":\"$ATOM_DENOM\"}},\"max_borrow_LTV\":null,\"max_LTV\":null,\"max_threshold_to_delay\":\"0.04\"}}" \
    "Collateral: set ATOM max_threshold_to_delay (4%)"

  # OSMO — 4%
  execute_contract "$collateral_addr" \
    "{\"editc_asset\":{\"asset\":{\"native_token\":{\"denom\":\"uosmo\"}},\"max_borrow_LTV\":null,\"max_LTV\":null,\"max_threshold_to_delay\":\"0.04\"}}" \
    "Collateral: set OSMO max_threshold_to_delay (4%)"

  # Noble USDC — 0% (immediate liquidation always)
  execute_contract "$collateral_addr" \
    "{\"editc_asset\":{\"asset\":{\"native_token\":{\"denom\":\"$NOBLE_USDC_DENOM\"}},\"max_borrow_LTV\":null,\"max_LTV\":null,\"max_threshold_to_delay\":\"0\"}}" \
    "Collateral: set USDC max_threshold_to_delay (0%)"

  set_step 34
}

# ---- STEP 35: Add liquidation queues for each collateral ----
step_35_add_liq_queues() {
  log "========== STEP 35: Add liquidation queues =========="

  local lq_addr
  lq_addr=$(state_get "addresses.liq_queue")

  # OSMO — 30 premium slots, 1T threshold
  execute_contract "$lq_addr" \
    "{\"add_queue\":{\"bid_for\":{\"native_token\":{\"denom\":\"uosmo\"}},\"max_premium\":\"30\",\"bid_threshold\":\"1000000000000\"}}" \
    "LQ: add OSMO queue (30 slots)"

  # ATOM — 30 premium slots, 1T threshold
  execute_contract "$lq_addr" \
    "{\"add_queue\":{\"bid_for\":{\"native_token\":{\"denom\":\"$ATOM_DENOM\"}},\"max_premium\":\"30\",\"bid_threshold\":\"1000000000000\"}}" \
    "LQ: add ATOM queue (30 slots)"

  # allBTC — 30 premium slots, 1T threshold
  execute_contract "$lq_addr" \
    "{\"add_queue\":{\"bid_for\":{\"native_token\":{\"denom\":\"$ALLBTC_DENOM\"}},\"max_premium\":\"30\",\"bid_threshold\":\"1000000000000\"}}" \
    "LQ: add allBTC queue (30 slots)"

  # Noble USDC — 4 premium slots, 1T threshold
  execute_contract "$lq_addr" \
    "{\"add_queue\":{\"bid_for\":{\"native_token\":{\"denom\":\"$NOBLE_USDC_DENOM\"}},\"max_premium\":\"4\",\"bid_threshold\":\"1000000000000\"}}" \
    "LQ: add USDC queue (4 slots)"

  set_step 35
}

# ---- STEP 36: Create LTV Disco queues ----
step_36_create_disco_queues() {
  log "========== STEP 36: Create LTV Disco queues =========="

  local ltv_disco_addr
  ltv_disco_addr=$(state_get "addresses.ltv_disco")

  # OSMO — min_ltv 40%, max_ltv 65%
  execute_contract "$ltv_disco_addr" \
    "{\"create_queue\":{\"asset\":\"uosmo\",\"min_ltv\":\"0.40\",\"max_ltv\":\"0.65\"}}" \
    "Disco: create OSMO queue (40-65% LTV)"

  # ATOM — min_ltv 45%, max_ltv 65%
  execute_contract "$ltv_disco_addr" \
    "{\"create_queue\":{\"asset\":\"$ATOM_DENOM\",\"min_ltv\":\"0.45\",\"max_ltv\":\"0.65\"}}" \
    "Disco: create ATOM queue (45-65% LTV)"

  # allBTC — min_ltv 60%, max_ltv 80%
  execute_contract "$ltv_disco_addr" \
    "{\"create_queue\":{\"asset\":\"$ALLBTC_DENOM\",\"min_ltv\":\"0.60\",\"max_ltv\":\"0.80\"}}" \
    "Disco: create allBTC queue (60-80% LTV)"

  # Noble USDC — min_ltv 88%, max_ltv 98%
  execute_contract "$ltv_disco_addr" \
    "{\"create_queue\":{\"asset\":\"$NOBLE_USDC_DENOM\",\"min_ltv\":\"0.88\",\"max_ltv\":\"0.98\"}}" \
    "Disco: create USDC queue (88-98% LTV)"

  set_step 36
}

# ---- STEP 37: Final summary ----
step_37_summary() {
  log "========== DEPLOYMENT COMPLETE =========="
  echo ""
  echo "=== Contract Addresses ==="
  python3 -c "
import json
with open('$STATE_FILE') as f:
    d = json.load(f)
for name, addr in sorted(d.get('addresses', {}).items()):
    print(f'  {name:25s} {addr}')
print()
print('=== Denoms ===')
for name, denom in sorted(d.get('denoms', {}).items()):
    print(f'  {name:25s} {denom}')
print()
print('=== Code IDs ===')
for name, cid in sorted(d.get('code_ids', {}).items()):
    print(f'  {name:25s} {cid}')
"
  echo ""
  ok "All contracts deployed and wired. State saved to: $STATE_FILE"
  set_step 37
}

# ========================== MAIN =============================================

main() {
  # Parse args
  local from_step=0
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --reset)
        rm -f "$STATE_FILE"
        log "State reset"
        shift
        ;;
      --from-step)
        from_step=$2
        shift 2
        ;;
      *)
        err "Unknown arg: $1"
        exit 1
        ;;
    esac
  done

  init_state

  # Determine starting step
  local current_step
  current_step=$(get_step)
  if [[ $from_step -gt 0 ]]; then
    current_step=$((from_step - 1))
    set_step "$current_step"
  fi

  log "Starting from step $((current_step + 1))"
  log "State file: $STATE_FILE"
  log "Owner: $(get_owner_addr)"
  echo ""

  # Pre-flight: check WASM files exist
  local missing=0
  for contract in "${CONTRACTS[@]}"; do
    local wasm="${ARTIFACTS_DIR}/${WASM_FILES[$contract]}.wasm"
    if [[ ! -f "$wasm" ]]; then
      # Only warn if we haven't stored this one yet
      local stored
      stored=$(state_get "code_ids.$contract")
      if [[ -z "$stored" || "$stored" == "None" ]]; then
        warn "Missing WASM: $wasm"
        missing=1
      fi
    fi
  done
  if [[ $missing -eq 1 ]]; then
    warn "Some WASM files are missing from $ARTIFACTS_DIR/"
    warn "Add the compiled .wasm files before running."
    warn "Continuing — will fail at store step for missing files."
    echo ""
  fi

  # Execute steps
  [[ $current_step -lt 1  ]] && step_1_store_contracts
  [[ $current_step -lt 2  ]] && step_2_instantiate_proxy
  [[ $current_step -lt 3  ]] && step_3_create_denoms
  [[ $current_step -lt 4  ]] && step_4_instantiate_oracle
  [[ $current_step -lt 5  ]] && step_5_instantiate_staking
  [[ $current_step -lt 6  ]] && step_6_instantiate_collateral
  [[ $current_step -lt 7  ]] && step_7_instantiate_debt
  [[ $current_step -lt 8  ]] && step_8_instantiate_liq_queue
  [[ $current_step -lt 9  ]] && step_9_instantiate_ltv_disco
  [[ $current_step -lt 10 ]] && step_10_wire_collateral_disco
  [[ $current_step -lt 11 ]] && step_11_instantiate_liquidation_engine
  [[ $current_step -lt 12 ]] && step_12_instantiate_transmuter
  [[ $current_step -lt 13 ]] && step_13_instantiate_revenue_distributor
  [[ $current_step -lt 14 ]] && step_14_instantiate_acquisition
  [[ $current_step -lt 15 ]] && step_15_instantiate_points_system
  [[ $current_step -lt 16 ]] && step_16_instantiate_system_discounts
  [[ $current_step -lt 17 ]] && step_17_instantiate_emissions_voting
  [[ $current_step -lt 18 ]] && step_18_instantiate_auction
  [[ $current_step -lt 19 ]] && step_19_wire_oracle
  [[ $current_step -lt 20 ]] && step_20_wire_collateral
  [[ $current_step -lt 21 ]] && step_21_wire_debt
  [[ $current_step -lt 22 ]] && step_22_wire_debt_basket
  [[ $current_step -lt 23 ]] && step_23_wire_liquidation_engine
  [[ $current_step -lt 24 ]] && step_24_wire_staking
  [[ $current_step -lt 25 ]] && step_25_wire_transmuter
  [[ $current_step -lt 26 ]] && step_26_wire_ltv_disco
  [[ $current_step -lt 27 ]] && step_27_wire_acquisition
  [[ $current_step -lt 28 ]] && step_28_wire_points_system
  [[ $current_step -lt 29 ]] && step_29_wire_revenue_distributor
  [[ $current_step -lt 30 ]] && step_30_wire_proxy
  [[ $current_step -lt 31 ]] && step_31_create_emissions_graphs
  [[ $current_step -lt 32 ]] && step_32_add_oracle_feeds
  [[ $current_step -lt 33 ]] && step_33_add_collateral
  [[ $current_step -lt 34 ]] && step_34_set_liquidation_thresholds
  [[ $current_step -lt 35 ]] && step_35_add_liq_queues
  [[ $current_step -lt 36 ]] && step_36_create_disco_queues
  [[ $current_step -lt 37 ]] && step_37_summary
}

main "$@"
