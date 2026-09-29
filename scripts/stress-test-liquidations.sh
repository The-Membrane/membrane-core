#!/usr/bin/env bash
#
# Liquidation Engine Stress Test — Production State Simulation
#
# Queries live Osmosis chain state (positions, LQ depth, oracle prices, pool
# liquidity) and simulates price-drop cascades to estimate:
#   - Which positions become liquidatable at each drop level
#   - Whether LQ bids can absorb the liquidation volume
#   - How much spills to sell wall and whether pools have liquidity
#   - Estimated bad debt at extreme drops
#
# Usage:
#   chmod +x scripts/stress-test-liquidations.sh
#   RPC="https://rpc.osmosis.zone:443" ./scripts/stress-test-liquidations.sh
#
# Requirements: osmosisd, jq, bc
# ---------------------------------------------------------------------------

set -euo pipefail

# ==========================================================================
# Configuration — fill in RPC before running
# ==========================================================================
RPC="${RPC:-https://YOUR_RPC_HERE:443}"
NODE_FLAG="--node $RPC"
OUTPUT_JSON="json"

# Contract addresses (Osmosis mainnet)
CDP="osmo1gy5gpqqlth0jpm9ydxlmff6g5mpnfvrfxd3mfc8dhyt03waumtzqt8exxr"
ORACLE="osmo16sgcpe0hcs42qk5vumk06jzmstkpka9gjda9tfdelwn65ksu3l7s7d4ggs"
LIQ_QUEUE="osmo1ycmtfa7h0efexjxuaw7yh3h3qayy5lspt9q4n4e3stn06cdcgm8s50zmjl"
STABILITY_POOL="osmo1326cxlzftxklgf92vdep2nvmqffrme0knh8dvugcn9w308ya9wpqv03vk8"
OSMOSIS_PROXY="osmo1s794h9rxggytja3a4pmwul53u98k06zy2qtrdvjnfuxruh7s8yjs6cyxgd"

# Price drop levels to simulate (percentages)
PRICE_DROPS=(5 10 15 20 25 30 40 50 75)

# TWAP timeframe and oracle time limit for price queries
TWAP_TIMEFRAME=10    # minutes
ORACLE_TIME_LIMIT=60 # seconds

# Temp directory for intermediate data
TMPDIR=$(mktemp -d /tmp/liq-stress.XXXXXX)
trap 'rm -rf "$TMPDIR"' EXIT

echo "============================================================"
echo "  Liquidation Engine — Production Stress Test"
echo "============================================================"
echo "  RPC:       $RPC"
echo "  CDP:       $CDP"
echo "  Oracle:    $ORACLE"
echo "  Liq Queue: $LIQ_QUEUE"
echo "  SP:        $STABILITY_POOL"
echo "  Temp:      $TMPDIR"
echo "============================================================"
echo ""

# ==========================================================================
# Helper: query a wasm contract
# ==========================================================================
query_wasm() {
    local contract="$1"
    local query_msg="$2"
    osmosisd query wasm contract-state smart "$contract" "$query_msg" \
        $NODE_FLAG --output "$OUTPUT_JSON" 2>/dev/null
}

# ==========================================================================
# Helper: safe math with bc
# ==========================================================================
div() {
    echo "scale=18; $1 / $2" | bc -l 2>/dev/null || echo "0"
}

mul() {
    echo "scale=18; $1 * $2" | bc -l 2>/dev/null || echo "0"
}

sub() {
    echo "scale=18; $1 - $2" | bc -l 2>/dev/null || echo "0"
}

add() {
    echo "scale=18; $1 + $2" | bc -l 2>/dev/null || echo "0"
}

gt() {
    # returns 0 (true) if $1 > $2
    echo "$1 > $2" | bc -l 2>/dev/null | grep -q '^1$'
}

fmt_usd() {
    printf '%0.2f' "$1" 2>/dev/null || echo "$1"
}

# ==========================================================================
# Step 1: Fetch CDP Config & Basket (includes per-asset LTV params)
# ==========================================================================
echo "[1/7] Fetching CDP config and basket..."

CDP_CONFIG=$(query_wasm "$CDP" '{"config":{}}')
echo "$CDP_CONFIG" > "$TMPDIR/cdp_config.json"

BASKET=$(query_wasm "$CDP" '{"get_basket":{}}')
echo "$BASKET" > "$TMPDIR/basket.json"

# Extract collateral asset denoms from basket
COLLATERAL_DENOMS=$(echo "$BASKET" | jq -r '.data.collateral_types[]?.asset.info.native_token.denom // empty' 2>/dev/null || true)

# Build per-denom LTV lookup from basket.collateral_types[].
# Each entry has: .asset.info.native_token.denom, .max_LTV, .max_borrow_LTV
# We write a JSON map: { "denom": { "max_ltv": "0.6", "borrow_ltv": "0.5" }, ... }
echo "$BASKET" | jq '
    [.data.collateral_types[] |
        {
            key: .asset.info.native_token.denom,
            value: {
                max_ltv: .max_LTV,
                borrow_ltv: .max_borrow_LTV
            }
        }
    ] | from_entries
' > "$TMPDIR/ltv_lookup.json"

echo "  Collateral denoms and LTV params:"
echo "$BASKET" | jq -r '.data.collateral_types[] | "    \(.asset.info.native_token.denom): max_LTV=\(.max_LTV) borrow_LTV=\(.max_borrow_LTV)"'
echo ""

# ==========================================================================
# Step 2: Fetch all positions (paginated)
# ==========================================================================
echo "[2/7] Fetching all positions..."

ALL_POSITIONS="[]"
START_AFTER=""
PAGE=0

while true; do
    PAGE=$((PAGE + 1))
    if [ -z "$START_AFTER" ]; then
        QUERY='{"get_basket_positions":{"limit":30}}'
    else
        QUERY="{\"get_basket_positions\":{\"start_after\":\"$START_AFTER\",\"limit\":30}}"
    fi

    RESULT=$(query_wasm "$CDP" "$QUERY")
    POSITIONS=$(echo "$RESULT" | jq '.data // []')
    COUNT=$(echo "$POSITIONS" | jq 'length')

    if [ "$COUNT" -eq 0 ] || [ "$COUNT" = "null" ]; then
        break
    fi

    ALL_POSITIONS=$(echo "$ALL_POSITIONS $POSITIONS" | jq -s '.[0] + .[1]')

    # Get last user for pagination
    START_AFTER=$(echo "$POSITIONS" | jq -r '.[-1].user // empty')
    if [ -z "$START_AFTER" ]; then
        break
    fi

    echo "  Page $PAGE: fetched $COUNT positions (total: $(echo "$ALL_POSITIONS" | jq 'length'))"

    # Safety: avoid infinite loop
    if [ "$PAGE" -ge 100 ]; then
        echo "  Warning: hit 100-page limit, stopping pagination"
        break
    fi
done

TOTAL_POSITIONS=$(echo "$ALL_POSITIONS" | jq 'length')
echo "$ALL_POSITIONS" > "$TMPDIR/positions.json"
echo "  Total positions: $TOTAL_POSITIONS"
echo ""

# ==========================================================================
# Step 3: Fetch current oracle prices for all collateral assets
# ==========================================================================
echo "[3/7] Fetching oracle prices..."

declare -A ASSET_PRICES
declare -A ASSET_DECIMALS

for DENOM in $COLLATERAL_DENOMS; do
    PRICE_QUERY="{\"price\":{\"asset_info\":{\"native_token\":{\"denom\":\"$DENOM\"}},\"twap_timeframe\":$TWAP_TIMEFRAME,\"oracle_time_limit\":$ORACLE_TIME_LIMIT}}"

    PRICE_RESULT=$(query_wasm "$ORACLE" "$PRICE_QUERY" 2>/dev/null || echo '{}')
    echo "$PRICE_RESULT" > "$TMPDIR/price_${DENOM}.json"

    PRICE=$(echo "$PRICE_RESULT" | jq -r '.data.price // "0"')
    DECIMALS=$(echo "$PRICE_RESULT" | jq -r '.data.decimals // "6"')

    ASSET_PRICES[$DENOM]="$PRICE"
    ASSET_DECIMALS[$DENOM]="$DECIMALS"

    echo "  $DENOM: \$$PRICE (${DECIMALS} decimals)"
done
echo ""

# ==========================================================================
# Step 4: Fetch oracle asset info (pool routes for sell wall estimation)
# ==========================================================================
echo "[4/7] Fetching oracle asset info (pool routes)..."

# Build asset_infos array for the Assets query
ASSET_INFOS_JSON="["
FIRST=true
for DENOM in $COLLATERAL_DENOMS; do
    if [ "$FIRST" = true ]; then
        FIRST=false
    else
        ASSET_INFOS_JSON="$ASSET_INFOS_JSON,"
    fi
    ASSET_INFOS_JSON="$ASSET_INFOS_JSON{\"native_token\":{\"denom\":\"$DENOM\"}}"
done
ASSET_INFOS_JSON="$ASSET_INFOS_JSON]"

ASSETS_QUERY="{\"assets\":{\"asset_infos\":$ASSET_INFOS_JSON}}"
ASSETS_RESULT=$(query_wasm "$ORACLE" "$ASSETS_QUERY" 2>/dev/null || echo '{"data":[]}')
echo "$ASSETS_RESULT" > "$TMPDIR/oracle_assets.json"

# Print pool routes discovered from oracle asset info
echo "$ASSETS_RESULT" | jq -r '
    .data[]? //empty |
    .asset_info.native_token.denom as $d |
    .oracle_info[]? //empty |
    .pools_for_osmo_twap[]? //empty |
    "  \($d) → pool \(.pool_id) (\(.base_asset_denom)/\(.quote_asset_denom))"
' 2>/dev/null || true
echo ""

# ==========================================================================
# Step 5: Fetch LQ queue depth per asset
# ==========================================================================
echo "[5/7] Fetching liquidation queue depth..."

declare -A LQ_DEPTH

LQ_QUEUES=$(query_wasm "$LIQ_QUEUE" '{"queues":{"limit":30}}')
echo "$LQ_QUEUES" > "$TMPDIR/lq_queues.json"

for DENOM in $COLLATERAL_DENOMS; do
    # Query premium slots for this asset to get total bid depth
    SLOTS_QUERY="{\"premium_slots\":{\"bid_for\":{\"native_token\":{\"denom\":\"$DENOM\"}},\"limit\":30}}"
    SLOTS_RESULT=$(query_wasm "$LIQ_QUEUE" "$SLOTS_QUERY" 2>/dev/null || echo '{"data":[]}')
    echo "$SLOTS_RESULT" > "$TMPDIR/lq_slots_${DENOM}.json"

    # Sum total_bid_amount across all premium slots
    TOTAL_BIDS=$(echo "$SLOTS_RESULT" | jq -r '[.data[]?.total_bid_amount // "0" | tonumber] | add // 0')
    LQ_DEPTH[$DENOM]="$TOTAL_BIDS"

    # Format for display
    if [ "$TOTAL_BIDS" -gt 0 ] 2>/dev/null; then
        BIDS_CDT=$(div "$TOTAL_BIDS" "1000000")
        echo "  $DENOM: ${BIDS_CDT} CDT in bids"
    else
        echo "  $DENOM: no active bids"
    fi
done
echo ""

# ==========================================================================
# Step 6: Fetch Stability Pool capacity
# ==========================================================================
echo "[6/7] Fetching stability pool capacity..."

SP_RESULT=$(query_wasm "$STABILITY_POOL" '{"asset_pool":{"deposit_limit":1}}')
echo "$SP_RESULT" > "$TMPDIR/sp_pool.json"

SP_TOTAL=$(echo "$SP_RESULT" | jq -r '.data.credit_asset.amount // "0"')
SP_CDT=$(div "${SP_TOTAL:-0}" "1000000")
echo "  Stability Pool: ${SP_CDT} CDT available"
echo ""

# ==========================================================================
# Step 7: Fetch pool liquidity for sell wall estimation
# ==========================================================================
echo "[7/7] Fetching pool liquidity..."

# Get swap routes from osmosis proxy
ROUTES_RESULT=$(query_wasm "$OSMOSIS_PROXY" '{"get_swap_routes":{}}' 2>/dev/null || echo '{"data":[]}')
echo "$ROUTES_RESULT" > "$TMPDIR/swap_routes.json"

# Query pool state for each pool referenced by oracle asset info
POOL_IDS=$(echo "$ASSETS_RESULT" | jq -r '[.data[]?.oracle_info[]?.pools_for_osmo_twap[]?.pool_id // empty] | unique | .[]' 2>/dev/null || true)

for POOL_ID in $POOL_IDS; do
    if [ -n "$POOL_ID" ] && [ "$POOL_ID" != "null" ]; then
        POOL_STATE=$(query_wasm "$OSMOSIS_PROXY" "{\"pool_state\":{\"id\":$POOL_ID}}" 2>/dev/null || echo '{}')
        echo "$POOL_STATE" > "$TMPDIR/pool_${POOL_ID}.json"
        echo "  Pool $POOL_ID: $(echo "$POOL_STATE" | jq -c '.data.assets // "unavailable"' 2>/dev/null)"
    fi
done
echo ""

# ==========================================================================
# Flatten positions for simulation
#
# PositionResponse structure:
#   .position_id          (Uint128)
#   .collateral_assets[]  (Vec<cAsset>)
#     .asset.info.native_token.denom
#     .asset.amount
#     .max_LTV             (per-asset liquidation threshold)
#     .max_borrow_LTV      (per-asset borrow threshold)
#   .credit_amount        (Uint128, total debt in micro-CDT)
#   .avg_max_LTV          (weighted average across collateral)
#   .avg_borrow_LTV       (weighted average across collateral)
#
# We flatten each position into one entry per collateral asset, carrying
# the position-level avg LTVs and debt alongside per-asset data.
# ==========================================================================
echo "============================================================"
echo "  SIMULATION: Price Drop Cascades"
echo "============================================================"
echo ""

# Flatten: one row per (user, position, collateral_asset)
# Each row carries both the per-asset LTVs and the position-level avg LTVs.
jq -c '
    [.[] |
        .user as $user |
        .positions[]? |
        {
            user: $user,
            position_id: .position_id,
            debt: (.credit_amount // "0"),
            avg_max_ltv: (.avg_max_LTV // "0.6"),
            avg_borrow_ltv: (.avg_borrow_LTV // "0.5"),
            collateral: [
                .collateral_assets[]? | {
                    denom: .asset.info.native_token.denom,
                    amount: .asset.amount,
                    max_ltv: .max_LTV,
                    borrow_ltv: .max_borrow_LTV
                }
            ]
        }
    ]
' "$TMPDIR/positions.json" > "$TMPDIR/flat_positions.json" 2>/dev/null || echo "[]" > "$TMPDIR/flat_positions.json"

POS_COUNT=$(jq 'length' "$TMPDIR/flat_positions.json")
echo "Parsed $POS_COUNT positions"
echo ""

# ==========================================================================
# For each price drop level, compute liquidation impact
# ==========================================================================
for DROP in "${PRICE_DROPS[@]}"; do
    DROP_MULT=$(sub "1" "$(div "$DROP" "100")")

    echo "--- Price Drop: ${DROP}% ---"

    LIQUIDATED_POSITIONS=0
    TOTAL_DEBT_LIQUIDATED="0"
    TOTAL_COLLATERAL_SOLD="0"
    TOTAL_LQ_COVERED="0"
    TOTAL_SELL_WALL="0"
    TOTAL_BAD_DEBT="0"

    # Process each position (multi-collateral aware)
    while read -r POS; do
        DEBT_AMOUNT=$(echo "$POS" | jq -r '.debt')
        AVG_MAX_LTV=$(echo "$POS" | jq -r '.avg_max_ltv')
        AVG_BORROW_LTV=$(echo "$POS" | jq -r '.avg_borrow_ltv')
        COLLATERAL_JSON=$(echo "$POS" | jq -c '.collateral')

        # Compute total collateral value at dropped prices
        TOTAL_COLL_VALUE="0"
        while read -r COLL; do
            DENOM=$(echo "$COLL" | jq -r '.denom')
            AMOUNT=$(echo "$COLL" | jq -r '.amount')

            CURRENT_PRICE="${ASSET_PRICES[$DENOM]:-0}"
            DECIMALS="${ASSET_DECIMALS[$DENOM]:-6}"
            if [ "$CURRENT_PRICE" = "0" ]; then
                continue
            fi

            DROPPED_PRICE=$(mul "$CURRENT_PRICE" "$DROP_MULT")
            DIVISOR=$(echo "10 ^ $DECIMALS" | bc)
            ASSET_VALUE=$(mul "$(div "$AMOUNT" "$DIVISOR")" "$DROPPED_PRICE")
            TOTAL_COLL_VALUE=$(add "$TOTAL_COLL_VALUE" "$ASSET_VALUE")
        done < <(echo "$COLLATERAL_JSON" | jq -c '.[]' 2>/dev/null)

        # Debt value (CDT ≈ $1, 6 decimals)
        DEBT_VALUE=$(div "$DEBT_AMOUNT" "1000000")

        # Skip zero positions
        if ! gt "$DEBT_VALUE" "0" || ! gt "$TOTAL_COLL_VALUE" "0"; then
            continue
        fi

        # Current LTV at dropped price
        LTV=$(div "$DEBT_VALUE" "$TOTAL_COLL_VALUE")

        # Use position's avg_max_LTV (already weighted by the contract)
        if gt "$LTV" "$AVG_MAX_LTV"; then
            LIQUIDATED_POSITIONS=$((LIQUIDATED_POSITIONS + 1))
            TOTAL_DEBT_LIQUIDATED=$(add "$TOTAL_DEBT_LIQUIDATED" "$DEBT_VALUE")

            # Repay formula: ((current_ltv - borrow_ltv) / current_ltv) * debt_value
            EXCESS=$(sub "$LTV" "$AVG_BORROW_LTV")
            FRACTION=$(div "$EXCESS" "$LTV")
            REPAY_VALUE=$(mul "$DEBT_VALUE" "$FRACTION")

            # Cap at collateral value for >100% LTV (bad debt)
            if gt "$LTV" "1.0"; then
                REPAY_VALUE="$TOTAL_COLL_VALUE"
                BAD_DEBT=$(sub "$DEBT_VALUE" "$TOTAL_COLL_VALUE")
                TOTAL_BAD_DEBT=$(add "$TOTAL_BAD_DEBT" "$BAD_DEBT")
            fi

            TOTAL_COLLATERAL_SOLD=$(add "$TOTAL_COLLATERAL_SOLD" "$REPAY_VALUE")

            # Check LQ coverage per-asset (proportional to collateral ratio)
            # Each collateral asset's share of liquidation goes to its own LQ queue
            POS_LQ_COVERED="0"
            POS_SELL_WALL="0"
            while read -r COLL; do
                DENOM=$(echo "$COLL" | jq -r '.denom')
                AMOUNT=$(echo "$COLL" | jq -r '.amount')

                CURRENT_PRICE="${ASSET_PRICES[$DENOM]:-0}"
                DECIMALS="${ASSET_DECIMALS[$DENOM]:-6}"
                if [ "$CURRENT_PRICE" = "0" ]; then
                    continue
                fi

                DROPPED_PRICE=$(mul "$CURRENT_PRICE" "$DROP_MULT")
                DIVISOR=$(echo "10 ^ $DECIMALS" | bc)
                ASSET_VALUE=$(mul "$(div "$AMOUNT" "$DIVISOR")" "$DROPPED_PRICE")

                # This asset's ratio of total collateral
                if gt "$TOTAL_COLL_VALUE" "0"; then
                    RATIO=$(div "$ASSET_VALUE" "$TOTAL_COLL_VALUE")
                else
                    RATIO="0"
                fi

                # This asset's share of the repayment
                ASSET_REPAY=$(mul "$REPAY_VALUE" "$RATIO")

                # LQ depth for this denom (in CDT, 6 decimals)
                LQ_AVAILABLE="${LQ_DEPTH[$DENOM]:-0}"
                LQ_CDT=$(div "$LQ_AVAILABLE" "1000000")

                if gt "$LQ_CDT" "$ASSET_REPAY"; then
                    POS_LQ_COVERED=$(add "$POS_LQ_COVERED" "$ASSET_REPAY")
                else
                    POS_LQ_COVERED=$(add "$POS_LQ_COVERED" "$LQ_CDT")
                    POS_SELL_WALL=$(add "$POS_SELL_WALL" "$(sub "$ASSET_REPAY" "$LQ_CDT")")
                fi
            done < <(echo "$COLLATERAL_JSON" | jq -c '.[]' 2>/dev/null)

            TOTAL_LQ_COVERED=$(add "$TOTAL_LQ_COVERED" "$POS_LQ_COVERED")
            TOTAL_SELL_WALL=$(add "$TOTAL_SELL_WALL" "$POS_SELL_WALL")
        fi

    done < <(jq -c '.[]' "$TMPDIR/flat_positions.json" 2>/dev/null)

    # Print results
    printf "  Positions liquidated:    %d / %d\n" "$LIQUIDATED_POSITIONS" "$POS_COUNT"
    printf "  Total debt at risk:      \$%s\n" "$(fmt_usd "$TOTAL_DEBT_LIQUIDATED")"
    printf "  Collateral to sell:      \$%s\n" "$(fmt_usd "$TOTAL_COLLATERAL_SOLD")"
    printf "  LQ coverage:             \$%s\n" "$(fmt_usd "$TOTAL_LQ_COVERED")"
    printf "  Sell wall spillover:     \$%s\n" "$(fmt_usd "$TOTAL_SELL_WALL")"
    printf "  Estimated bad debt:      \$%s\n" "$(fmt_usd "$TOTAL_BAD_DEBT")"

    if gt "$TOTAL_COLLATERAL_SOLD" "0"; then
        LQ_RATIO=$(mul "$(div "$TOTAL_LQ_COVERED" "$TOTAL_COLLATERAL_SOLD")" "100")
        printf "  LQ coverage ratio:       %s%%\n" "$(fmt_usd "$LQ_RATIO")"
    fi
    echo ""

done

# ==========================================================================
# Summary: Pool liquidity vs sell wall demand
# ==========================================================================
echo "============================================================"
echo "  Pool Liquidity Summary"
echo "============================================================"
echo ""

for POOL_FILE in "$TMPDIR"/pool_*.json; do
    if [ -f "$POOL_FILE" ]; then
        POOL_ID=$(basename "$POOL_FILE" .json | sed 's/pool_//')
        echo "Pool $POOL_ID:"
        jq -r '.data.assets[]? | "  \(.token.denom // "unknown"): \(.token.amount // "0")"' "$POOL_FILE" 2>/dev/null || echo "  (no data)"
        echo ""
    fi
done

# ==========================================================================
# Per-asset LTV summary from basket
# ==========================================================================
echo "============================================================"
echo "  Per-Asset LTV Configuration (from basket)"
echo "============================================================"
echo ""
jq -r 'to_entries[] | "  \(.key): max_LTV=\(.value.max_ltv) borrow_LTV=\(.value.borrow_ltv)"' "$TMPDIR/ltv_lookup.json" 2>/dev/null
echo ""

# ==========================================================================
# Save full report
# ==========================================================================
REPORT_FILE="$TMPDIR/report.json"
jq -n \
    --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --arg rpc "$RPC" \
    --argjson total_positions "$TOTAL_POSITIONS" \
    --argjson pos_count "$POS_COUNT" \
    --arg sp_cdt "$SP_CDT" \
    --argjson ltv_lookup "$(cat "$TMPDIR/ltv_lookup.json")" \
    '{
        timestamp: $ts,
        rpc: $rpc,
        total_positions: $total_positions,
        position_entries: $pos_count,
        stability_pool_cdt: $sp_cdt,
        price_drops_simulated: [5,10,15,20,25,30,40,50,75],
        ltv_configuration: $ltv_lookup
    }' > "$REPORT_FILE"

echo "============================================================"
echo "  Full data saved to: $TMPDIR/"
echo "  Report: $REPORT_FILE"
echo "============================================================"
echo ""
echo "Files:"
ls -la "$TMPDIR/"
