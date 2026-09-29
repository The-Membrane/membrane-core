# Bad Debt Handling After Liquidation

## Table of Contents
1. [Overview](#overview)
2. [Bad Debt Detection](#bad-debt-detection)
3. [Bad Debt Fulfillment Flow](#bad-debt-fulfillment-flow)
4. [Revenue-Based Recapitalization](#revenue-based-recapitalization)
5. [LTV Disco Bad Debt Distribution](#ltv-disco-bad-debt-distribution)
6. [Debt Auction Mechanism](#debt-auction-mechanism)
7. [Complete Flow Diagram](#complete-flow-diagram)
8. [Technical Details](#technical-details)

---

## Overview

When a CDP position is liquidated, the system attempts to repay all outstanding debt using the liquidated collateral. However, in some cases, the collateral value may be insufficient to cover the full debt amount, resulting in **bad debt**. This document explains how the system detects, handles, and recapitalizes bad debt after a liquidation.

### Key Concepts

- **Bad Debt**: Debt that cannot be repaid by the liquidated collateral value
- **Recapitalization**: The process of covering bad debt using protocol revenue or MBRN auctions
- **LTV Disco**: Protocol that can absorb bad debt for specific collateral assets
- **Debt Auction**: Mechanism to sell MBRN tokens at a discount to raise CDT for bad debt repayment

---

## Bad Debt Detection

### Trigger Point

After a liquidation completes, the CDP contract automatically triggers a bad debt check:

```rust
// After liquidation completes, check for bad debt
let msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
    contract_addr: env.contract.address.to_string(),
    msg: to_json_binary(&ExecuteMsg::Callback(CallbackMsg::BadDebtCheck {
        position_id,
        position_owner: valid_position_owner.clone(),
    }))?,
    funds: vec![],
});

// Reply on error only - don't halt liquidation if check fails
// Don't care about the success case so didn't use reply_always
let call_back = SubMsg::reply_on_error(msg, BAD_DEBT_REPLY_ID);
```

**Key Points:**
- Bad debt check is triggered **after** all liquidation attempts complete
- Uses `CallbackMsg::BadDebtCheck` which can only be called by the contract itself
- Uses `reply_on_error` so failures don't revert the liquidation
- The check happens asynchronously via a submessage
- The callback handler verifies `info.sender == env.contract.address` for security

### Detection Logic

The `check_and_fulfill_bad_debt()` function determines if bad debt exists:

```rust
fn check_and_fulfill_bad_debt(
    deps: DepsMut,
    env: Env,
    position_id: Uint128,
    position_owner: Addr,
) -> Result<Response, ContractError>
```

#### Step 1: Calculate Remaining Collateral Value

1. **Load Position State**
   - Retrieves the liquidated position
   - Gets remaining collateral assets (if any)

2. **Calculate Total Asset Value**
   - For each remaining collateral asset:
     - Gets current price from oracle
     - Calculates: `asset_value = price * asset_amount`
   - Sums all asset values: `total_asset_value = sum(all asset_values)`

3. **Get Remaining Debt**
   - Reads `target_position.credit_amount` (remaining CDT debt)

#### Step 2: Determine if Bad Debt Exists

Bad debt exists if **both** conditions are true:

1. **Remaining debt > 0**: `target_position.credit_amount > 0`
2. **Collateral value ≤ $1**: `total_asset_value ≤ Decimal::one()`

**Why $1 threshold?**
- Full liquidations leave small rounding errors
- Using $1 as a floor prevents false positives from rounding
- Positions with > $1 collateral value are considered solvent

#### Step 3: Handle Solvent vs Insolvent Positions

**If Position is Solvent:**
- Returns `ContractError::PositionSolvent`
- No bad debt exists
- Position remains open with remaining collateral

**If Position is Insolvent:**
- Proceeds to bad debt fulfillment flow
- Position debt is set to zero
- Remaining collateral is zeroed out
- Position is effectively closed

---

## Bad Debt Fulfillment Flow

Once bad debt is detected, the system attempts to fulfill it through multiple mechanisms in priority order:

### Priority Order

1. **Protocol Revenue** (if available)
2. **LTV Disco** (for specific collateral assets)
3. **Debt Auction** (MBRN sales for remaining bad debt)

### Flow Overview

```
Bad Debt Detected
    │
    ├─→ Step 1: Use Protocol Revenue
    │   │
    │   ├─→ Revenue Available?
    │   │   │
    │   │   ├─→ YES → Apply revenue to bad debt
    │   │   │   └─→ Reduce bad debt amount
    │   │   │
    │   │   └─→ NO → Continue to Step 2
    │   │
    │   └─→ Continue to Step 2
    │
    ├─→ Step 2: Distribute to LTV Disco (per asset)
    │   │
    │   ├─→ For each collateral asset:
    │   │   │
    │   │   ├─→ Calculate asset's share of bad debt
    │   │   ├─→ Query LTV Disco: CanHandleBadDebt?
    │   │   │
    │   │   ├─→ YES → Send bad debt to LTV Disco
    │   │   │   └─→ Reduce remaining bad debt
    │   │   │
    │   │   └─→ NO → Skip this asset
    │   │
    │   └─→ Continue to Step 3
    │
    └─→ Step 3: Send Remaining to Debt Auction
        │
        ├─→ Remaining bad debt > 0?
        │   │
        │   ├─→ YES → Start/Add to Debt Auction
        │   │   └─→ Auction sells MBRN for CDT
        │   │
        │   └─→ NO → Complete (all bad debt handled)
```

---

## Revenue-Based Recapitalization

### Overview

The first mechanism to fulfill bad debt is using protocol revenue stored in the basket's `pending_revenue`.

### Implementation

```rust
// If the basket has revenue, "mint" and repay the bad debt
if !basket.pending_revenue.total_pending.is_zero() {
    if bad_debt_amount >= basket.pending_revenue.total_pending {
        // Bad debt exceeds revenue - use all revenue
        bad_debt_amount -= basket.pending_revenue.total_pending;
        basket.pending_revenue.total_pending = Uint128::zero();
    } else {
        // Revenue exceeds bad debt - use all bad debt amount
        basket.pending_revenue.total_pending -= bad_debt_amount;
        bad_debt_amount = Uint128::zero();
    }
}
```

### How It Works

1. **Check Revenue Availability**
   - Queries `basket.pending_revenue.total_pending`
   - If zero, skips to next mechanism

2. **Apply Revenue to Bad Debt**
   - **Case 1**: Bad debt ≥ Revenue
     - Uses all available revenue
     - Reduces bad debt by revenue amount
     - Sets revenue to zero
   - **Case 2**: Revenue > Bad debt
     - Uses bad debt amount from revenue
     - Reduces revenue by bad debt amount
     - Sets bad debt to zero (fully covered)

3. **Update State**
   - Saves updated basket with reduced revenue
   - Continues to next mechanism if bad debt remains

### Example

**Scenario**: 
- Bad debt: 1,000 CDT
- Pending revenue: 600 CDT

**Result**:
- Revenue applied: 600 CDT
- Remaining bad debt: 400 CDT
- Revenue after: 0 CDT
- Proceeds to LTV Disco distribution

---

## LTV Disco Bad Debt Distribution

### Overview

LTV Disco (LTV Discovery Protocol) can absorb bad debt for specific collateral assets. The system distributes bad debt proportionally to each collateral asset that was liquidated, and sends each asset's share to LTV Disco if it can handle it.

### Distribution Logic

#### Step 1: Calculate Per-Asset Bad Debt

For each collateral asset that was liquidated:

```rust
// Calculate bad debt amount per asset using collateral ratios
let bad_debt_amount_for_asset = decimal_multiplication(
    Decimal::from_ratio(bad_debt_amount, Uint128::one()), 
    cAsset_ratios[num]  // Asset's proportion of total collateral
)?.to_uint_ceil();  // Round up to prevent rounding errors
```

**Key Points:**
- Uses `cAsset_ratios` from liquidation propagation
- Each asset's share = `bad_debt * asset_ratio`
- Uses `to_uint_ceil()` to round up (prevents under-allocation)

#### Step 2: Query LTV Disco Capacity

For each asset, queries LTV Disco to check if it can handle the bad debt:

```rust
let can_handle_bad_debt = deps.querier.query_wasm_smart::<bool>(
    config.ltv_disco.to_string(),
    &LTVDisco_QueryMsg::CanHandleBadDebt {
        asset: cAsset.asset.info.to_string(),
        amount: bad_debt_amount_for_asset,
    },
)?;
```

**LTV Disco Capacity Check:**
The LTV Disco checks if it has sufficient resources:
1. **Available Dispersals**: Checks pending and active dispersals
2. **Deposit Token Value**: Calculates CDT value of deposit tokens via oracle
3. **Total Capacity**: `dispersals + deposit_value >= requested_amount`

#### Step 3: Send Bad Debt to LTV Disco

If LTV Disco can handle the bad debt:

```rust
if can_handle_bad_debt {
    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.ltv_disco.to_string(),
        msg: to_json_binary(&LTVDisco_ExecuteMsg::AddBadDebt {
            asset: cAsset.asset.info.to_string(),
            amount: bad_debt_amount_for_asset,
        })?,
        funds: vec![],
    }));
    // Update remaining bad debt
    bad_debt_amount -= bad_debt_amount_for_asset;
}
```

**What Happens in LTV Disco:**
- Bad debt is added to the asset's bad debt tracking
- LTV Disco uses its dispersals and deposit tokens to cover the bad debt
- Depositors in LTV Disco absorb the loss proportionally

### Example

**Scenario**:
- Total bad debt: 1,000 CDT
- Liquidated assets:
  - marsUSDC: 60% of collateral (ratio = 0.6)
  - ATOM: 40% of collateral (ratio = 0.4)

**Distribution**:
- marsUSDC share: 1,000 * 0.6 = 600 CDT
- ATOM share: 1,000 * 0.4 = 400 CDT

**If LTV Disco can handle both**:
- marsUSDC: 600 CDT sent to LTV Disco
- ATOM: 400 CDT sent to LTV Disco
- Remaining bad debt: 0 CDT

**If LTV Disco can only handle marsUSDC**:
- marsUSDC: 600 CDT sent to LTV Disco
- ATOM: 400 CDT proceeds to auction
- Remaining bad debt: 400 CDT

---

## Debt Auction Mechanism

### Overview

If bad debt remains after revenue and LTV Disco distribution, it is sent to the Debt Auction contract. The auction sells MBRN tokens at a discount to raise CDT for bad debt repayment.

### Starting the Auction

```rust
if config.debt_auction.is_some() && !bad_debt_amount.is_zero() {
    let auction_msg = AuctionExecuteMsg::StartAuction {
        repayment_position_info: Some(UserInfo {
            position_id,
            position_owner: position_owner.to_string(),
        }),
        auction_asset: Asset {
            amount: bad_debt_amount,
            info: basket.clone().credit_asset.info,  // CDT
        },
        send_to: None,
    };

    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.debt_auction.unwrap_or_else(|| Addr::unchecked("")).to_string(),
        msg: to_json_binary(&auction_msg)?,
        funds: vec![],
    }));
} else {
    // Returns error if debt_auction is None OR if bad_debt_amount is zero
    // Note: If bad_debt_amount is zero, no auction is needed, but code still returns error
    return Err(ContractError::CustomError {
        val: String::from("Debt Auction contract not added to config"),
    });
}
```

### Auction Process

#### Phase 1: Auction Creation

1. **Auction Initialization**
   - Creates or adds to existing `DEBT_AUCTION`
   - Sets `remaining_recapitalization = bad_debt_amount`
   - Records `auction_start_time`

2. **Auction State**
   - Tracks how much CDT still needs to be raised
   - Auction remains active until `remaining_recapitalization = 0`

#### Phase 2: MBRN Swapping

Users can swap CDT for discounted MBRN:

```rust
fn swap_for_mbrn(deps: DepsMut, info: MessageInfo, env: Env) -> Result<Response, ContractError>
```

**Process**:

1. **User Sends CDT**
   - User calls `Auction::SwapForMBRN` with CDT tokens
   - CDT amount is `swap_amount`

2. **Calculate Discount**
   - Gets current MBRN price from oracle (TWAP)
   - Calculates discount based on time elapsed:
     ```rust
     discount_ratio = get_discount_ratio(env, auction.auction_start_time, config)
     ```
   - Discount increases over time to incentivize early participation

3. **Calculate MBRN to Mint**
   - Gets CDT price (usually $1 peg)
   - Calculates CDT value: `cdt_value = cdt_amount * cdt_price`
   - Applies discount: `discounted_mbrn_price = mbrn_price * discount_ratio`
   - Mints MBRN: `mbrn_amount = cdt_value / discounted_mbrn_price`

4. **Fulfill Bad Debt**
   - Determines fulfillment: `fulfill_amount = min(swap_amount, remaining_recapitalization)`
   - Sends fulfilled CDT to CDP: `CDP::FulfillBadDebt`
   - CDP burns the CDT, reducing `basket.pending_bad_debt`

5. **Return Overpayment**
   - If `swap_amount > remaining_recapitalization`:
     - Returns excess CDT to user
   - Updates `remaining_recapitalization -= fulfill_amount`

6. **Complete Auction**
   - If `remaining_recapitalization = 0`:
     - Removes `DEBT_AUCTION` from storage
     - Auction is complete

### Discount Mechanism

The discount increases over time to incentivize early recapitalization:

```rust
fn get_discount_ratio(env: Env, auction_start_time: u64, config: Config) -> StdResult<Decimal> {
    let time_elapsed = env.block.time.seconds() - auction_start_time;
    let discount_periods = time_elapsed / config.discount_increase_timeframe;
    let discount_increase = config.discount_increase * Decimal::from_ratio(discount_periods, 1u64);
    let discount_ratio = config.initial_discount + discount_increase;
    Ok(discount_ratio)
}
```

**Example Discount Schedule**:
- Initial discount: 5%
- Discount increase: 1% per hour
- After 1 hour: 6% discount
- After 5 hours: 10% discount
- After 10 hours: 15% discount

### Fulfilling Bad Debt

When CDT is sent to fulfill bad debt:

```rust
// In Auction contract
msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
    contract_addr: config.positions_contract.to_string(),
    msg: to_json_binary(&CDPExecuteMsg::FulfillBadDebt {})?,
    funds: vec![Coin { denom: coin.denom, amount: fulfill_amount }],
}));
```

**In CDP Contract**:
```rust
fn fulfill_bad_debt(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let mut basket = BASKET.load(deps.storage)?;
    
    // Find CDT coin in funds
    let cdt_amount = info.funds.iter()
        .find(|coin| coin.denom == basket.credit_asset.info.to_string())
        .map(|c| c.amount)
        .ok_or_else(|| ContractError::CustomError { 
            val: String::from("No CDT coin found in msg info") 
        })?;
    
    // Calculate fulfilled amount and excess
    let (fulfilled_bad_debt, excess) = if basket.pending_bad_debt >= cdt_amount {
        (cdt_amount, Uint128::zero())
    } else {
        (basket.pending_bad_debt, cdt_amount - basket.pending_bad_debt)
    };
    
    // Reduce pending bad debt
    basket.pending_bad_debt -= fulfilled_bad_debt;
    
    // Burn only the fulfilled amount via chain_proxy
    let burn_message = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.chain_proxy.unwrap().to_string(),
        msg: to_json_binary(&OsmoExecuteMsg::BurnTokens {
            denom: basket.credit_asset.info.to_string(),
            amount: fulfilled_bad_debt,
            burn_from_address: env.contract.address.to_string(),
        })?,
        funds: vec![],
    });
    msgs.push(burn_message);
    
    BASKET.save(deps.storage, &basket)?;
    
    // Send back excess CDT to sender (always pushed, even if zero)
    msgs.push(CosmosMsg::Bank(BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: vec![Coin {
            denom: basket.credit_asset.info.to_string(),
            amount: excess,
        }],
    }));
    
    Ok(Response::new()
        .add_messages(msgs)
        .add_attribute("bad_debt_fulfilled", fulfilled_bad_debt)
        .add_attribute("excess_returned", excess))
}
```

**Key Points:**
- Only burns the amount that fulfills bad debt (not the full sent amount)
- Returns excess CDT to sender if more was sent than needed
- Uses `basket.credit_asset.info` to identify CDT denom (not hardcoded)

---

## Complete Flow Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                    Liquidation Completes                     │
└──────────────────────────┬──────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│              Trigger Bad Debt Check (Async)                 │
│         SubMsg::reply_on_error(CheckBadDebt)               │
└──────────────────────────┬──────────────────────────────────┘
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│            check_and_fulfill_bad_debt()                     │
│                                                             │
│  1. Calculate remaining collateral value                   │
│  2. Get remaining debt amount                              │
│  3. Check: collateral_value ≤ $1 AND debt > 0?            │
└──────────────────────────┬──────────────────────────────────┘
                           │
        ┌──────────────────┴──────────────────┐
        │                                      │
        ▼                                      ▼
┌──────────────────┐              ┌──────────────────────┐
│  Position        │              │  Bad Debt Exists     │
│  Solvent         │              │  Proceed to          │
│  (No bad debt)   │              │  Fulfillment         │
└──────────────────┘              └──────────┬─────────────┘
                                             │
                                             ▼
                    ┌──────────────────────────────────────┐
                    │   Step 1: Use Protocol Revenue       │
                    │                                       │
                    │   if revenue > 0:                     │
                    │     bad_debt -= min(revenue, bad_debt)│
                    └──────────────┬───────────────────────┘
                                   │
                                   ▼
                    ┌──────────────────────────────────────┐
                    │   Step 2: Distribute to LTV Disco    │
                    │                                       │
                    │   For each liquidated asset:         │
                    │     1. Calculate asset's share       │
                    │     2. Query: CanHandleBadDebt?       │
                    │     3. If yes: AddBadDebt to LTV Disco│
                    │     4. Reduce remaining bad_debt      │
                    └──────────────┬───────────────────────┘
                                   │
                                   ▼
                    ┌──────────────────────────────────────┐
                    │   Step 3: Send to Debt Auction       │
                    │                                       │
                    │   if bad_debt > 0:                    │
                    │     StartAuction(remaining_bad_debt) │
                    └──────────────┬───────────────────────┘
                                   │
                                   ▼
                    ┌──────────────────────────────────────┐
                    │   Debt Auction Active                │
                    │                                       │
                    │   Users can:                         │
                    │     SwapForMBRN(CDT) → Get MBRN      │
                    │     (at increasing discount)         │
                    │                                       │
                    │   Auction:                           │
                    │     - Fulfills bad debt with CDT      │
                    │     - Mints discounted MBRN          │
                    │     - Returns overpayment             │
                    │     - Completes when remaining = 0    │
                    └──────────────────────────────────────┘
```

---

## Technical Details

### State Updates

#### Position State
```rust
// Position debt set to zero
target_position.credit_amount = Uint128::zero();

// Remaining collateral zeroed out
target_position.collateral_assets = vec![];

// Position effectively closed
update_position(deps.storage, position_owner, target_position)?;
```

#### Basket State
```rust
// Add to pending bad debt
basket.pending_bad_debt += bad_debt_amount;

// Reduce revenue if used
basket.pending_revenue.total_pending -= revenue_used;

// Save updated basket
BASKET.save(deps.storage, &basket)?;
```

#### Liquidation State
```rust
// Remove liquidation propagation after processing
LIQUIDATION.remove(deps.storage);
```

### Error Handling

**Bad Debt Check Errors:**
- Uses `reply_on_error` so errors don't revert liquidation
- Errors are logged but don't halt the process
- Bad debt may remain unfulfilled if check fails

**LTV Disco Errors:**
- If `CanHandleBadDebt` query fails: Skips that asset
- If `AddBadDebt` execution fails: Bad debt proceeds to auction
- Errors don't prevent auction from starting

**Auction Errors:**
- If auction contract not configured: Returns error
- If auction start fails: Bad debt remains in `pending_bad_debt`
- Users can still participate in existing auctions

### Reply IDs

- `BAD_DEBT_REPLY_ID`: Used for bad debt check callback
  - Currently only handles errors (doesn't process success case)
  - Handler: `BAD_DEBT_REPLY_ID => Ok(Response::new())`
  - Errors are logged but don't affect liquidation

### Callback Security

The callback mechanism ensures only the contract can trigger bad debt checks:

```rust
ExecuteMsg::Callback(msg) => {
    if info.sender == env.contract.address {
        callback_handler(deps, env, msg)
    } else {
        Err(ContractError::Unauthorized { owner: env.contract.address.to_string() })
    }
}
```

This prevents external actors from triggering bad debt checks directly.

### Query Functions

**LTV Disco Capacity Check:**
```rust
LTVDisco_QueryMsg::CanHandleBadDebt {
    asset: String,      // Asset identifier
    amount: Uint128,   // Bad debt amount to check
}
```

Returns: `bool` - true if LTV Disco can handle the amount

### Execute Functions

**Add Bad Debt to LTV Disco:**
```rust
LTVDisco_ExecuteMsg::AddBadDebt {
    asset: String,      // Asset identifier
    amount: Uint128,   // Bad debt amount
}
```

**Start Debt Auction:**
```rust
AuctionExecuteMsg::StartAuction {
    repayment_position_info: Option<UserInfo>,
    auction_asset: Asset,  // CDT with bad debt amount
    send_to: Option<String>,
}
```

**Fulfill Bad Debt:**
```rust
CDPExecuteMsg::FulfillBadDebt {}
// Sends CDT as funds to burn
```

---

## Summary

The bad debt handling system provides multiple layers of protection:

1. **Automatic Detection**: Bad debt is automatically detected after each liquidation
2. **Priority-Based Fulfillment**: Revenue → LTV Disco → Auction (in order)
3. **Proportional Distribution**: Bad debt is distributed proportionally to liquidated assets
4. **Incentivized Recapitalization**: Debt auctions offer increasing discounts to encourage early participation
5. **Graceful Degradation**: Errors in one mechanism don't prevent others from working

This multi-layered approach ensures that bad debt is handled efficiently while providing multiple pathways for recapitalization, protecting the protocol's solvency and maintaining system stability.

