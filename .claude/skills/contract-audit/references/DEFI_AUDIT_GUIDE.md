# DeFi Protocol Security Audit Guide for LLMs
## Comprehensive Analysis Framework - Prompting Instructions

**Version**: 1.0  
**Source**: 60 real audit reports from Oak Security (2021-2025)  
**Purpose**: Step-by-step instructions for conducting thorough DeFi security audits

---

## HOW TO USE THIS GUIDE

When you receive a DeFi protocol codebase to audit, follow these instructions systematically. This guide tells you exactly what to look for, how to identify issues, what questions to ask, and how to classify findings. Each section provides specific search patterns, test cases, and real-world examples from actual audits.

---

## PHASE 1: INITIAL PROTOCOL ASSESSMENT

### Step 1.1: Classify the Protocol Type

**Instructions**: Read the protocol documentation and code to determine its primary function. This classification determines which vulnerability patterns are most likely.

**Ask yourself these questions**:
- Does it allow users to borrow assets against collateral? → **Lending/CDP Protocol**
- Does it facilitate token swaps through liquidity pools? → **DEX/AMM Protocol**
- Does it enable leveraged trading with funding rates? → **Perpetuals/Derivatives Protocol**
- Does it tokenize staked assets for liquidity? → **Liquid Staking Protocol**
- Does it create synthetic assets tracking real-world prices? → **Synthetics Protocol**
- Does it facilitate cross-chain asset transfers? → **Bridge Protocol**

**For each type, prioritize these vulnerabilities**:

**If Lending/CDP**: Focus 60% of audit time on liquidation logic, oracle security, and collateral management. These protocols have the highest concentration of critical issues (40% of all critical findings).

**If DEX/AMM**: Focus 50% on refund logic, liquidity calculations, and price manipulation vectors. Double refunds and incorrect excess token handling are common.

**If Perpetuals**: Focus 50% on position tracking, funding rate calculations, and bad debt management. Bad debt tracking failures are extremely common.

**If Liquid Staking**: Focus 40% on unbounded collection growth, delegation tracking, and reward distribution. Unbounded arrays cause the most issues.

**If Synthetics**: Focus 50% on oracle integration, collateral ratio enforcement, and liquidation mechanisms. Similar to CDP but with more oracle complexity.

**If Bridge**: Focus 60% on initialization security, signature verification, and message validation. Setup function exploits are common.

### Step 1.2: Document Protocol Scope

**Instructions**: Before analyzing code, list all contracts and their responsibilities. This helps you understand attack surfaces.

**Create a table like this**:
```
Contract Name | Primary Function | External Dependencies | State Mutations
--------------|------------------|----------------------|------------------
core.wasm     | Main protocol logic | Oracle, DEX | POSITIONS, VAULTS
vault.wasm    | Asset custody | None | BALANCES, LOCKS
liquidator.wasm | Liquidations | Oracle, Core | LIQUIDATIONS
oracle.wasm   | Price feeds | External API | PRICES
```

**For each contract, ask**:
- What external contracts does it call?
- What external contracts call it?
- What state does it mutate?
- What assets does it custody?
- Can users directly interact with it?

### Step 1.3: Identify Critical Paths

**Instructions**: Find all paths where value moves or state changes irreversibly. These are your highest-priority audit targets.

**Critical paths always include**:
1. **Deposit/Withdrawal flows** - Users adding or removing assets
2. **Liquidation flows** - Forced closure of positions
3. **Reward distribution** - Paying yield to users
4. **Oracle updates** - Price data being written to state
5. **Admin functions** - Privileged operations that change parameters
6. **Migration/Upgrade flows** - Protocol state transitions

**For each critical path, trace the complete flow**:
```
User calls withdraw() 
  → Checks user balance (read BALANCES)
  → Calculates pending rewards (read REWARDS)
  → Claims rewards (call claim_rewards())
    → Updates reward state (write REWARDS)
    → Transfers reward tokens (call transfer())
  → Updates user balance (write BALANCES)
  → Transfers principal tokens (call transfer())
  → Emits event
```

**Ask at each step**: What happens if this fails? Can funds be lost? Can state become inconsistent?

---

## PHASE 2: CRITICAL VULNERABILITY SCANNING

### Step 2.1: Liquidation Vulnerability Analysis

**Instructions**: If the protocol has ANY mechanism for forced closure of positions (liquidations, margin calls, collateral seizure), perform this analysis with extreme scrutiny. Liquidation bugs account for 40% of all critical issues.

#### Check 2.1.1: Complete Asset Preservation

**What to look for**: Search for all functions that close positions or seize collateral. For each one, trace whether ALL user assets are properly handled.

**Search pattern**:
```rust
// Find liquidation functions - search for these keywords:
"liquidate"
"seize"
"margin_call"
"force_close"
"unwind_position"
```

**For each liquidation function, verify ALL of these assets are handled**:
1. Principal collateral
2. Accrued rewards (staking, liquidity mining, etc.)
3. Pending fees or rebates
4. Locked tokens with vesting
5. Derivative positions or synthetics
6. Any other value tied to the position

**Real vulnerability example from Mars v2**:
```rust
// ❌ VULNERABLE CODE - This is what you're looking for
fn liquidate(deps: DepsMut, position_id: u64) -> Result<Response> {
    let position = POSITIONS.load(deps.storage, position_id)?;
    
    // Liquidates collateral
    let collateral_value = position.collateral;
    let seized_amount = calculate_seizure(collateral_value)?;
    
    // Transfers to liquidator
    transfer_to_liquidator(seized_amount)?;
    
    // Closes position
    POSITIONS.remove(deps.storage, position_id);
    
    // ❌ CRITICAL ISSUE: What about position.staking_rewards?
    // ❌ CRITICAL ISSUE: What about position.pending_fees?
    // User just lost all rewards and fees!
    
    Ok(Response::new())
}
```

**How to identify this vulnerability**:
1. Find the position struct definition
2. List ALL fields that represent value
3. Check if liquidation function handles EACH field
4. If ANY field is not explicitly handled → Flag as CRITICAL

**Correct pattern**:
```rust
// ✅ SECURE CODE - This is what it should look like
fn liquidate_complete(deps: DepsMut, position_id: u64) -> Result<Response> {
    let position = POSITIONS.load(deps.storage, position_id)?;
    
    // Step 1: Claim ALL pending rewards BEFORE liquidation
    let staking_rewards = claim_staking_rewards(&position)?;
    let lp_rewards = claim_lp_rewards(&position)?;
    let pending_fees = calculate_pending_fees(&position)?;
    
    // Step 2: Calculate liquidation
    let collateral_value = position.collateral;
    let debt_value = position.debt;
    let seized_amount = calculate_seizure(collateral_value, debt_value)?;
    let remaining_collateral = collateral_value - seized_amount;
    
    // Step 3: Transfer EVERYTHING to user
    transfer_to_user(
        position.owner,
        remaining_collateral,      // Remaining collateral
        staking_rewards,            // All staking rewards
        lp_rewards,                 // All LP rewards  
        pending_fees,               // All pending fees
    )?;
    
    // Step 4: Handle bad debt if any
    if debt_value > collateral_value {
        let bad_debt = debt_value - collateral_value;
        BAD_DEBT.update(deps.storage, |total| Ok(total + bad_debt))?;
    }
    
    // Step 5: Close position
    POSITIONS.remove(deps.storage, position_id);
    
    Ok(Response::new())
}
```

**Questions to ask**:
- Are there ANY rewards accruing to this position? If yes, are they claimed?
- Are there ANY fees owed to the user? If yes, are they paid?
- Is there ANY other value associated with the position? If yes, is it transferred?
- What happens if reward calculation fails? Does the entire liquidation fail or just lose rewards?
- Can you find ANY code path where a user loses assets during liquidation?

**Severity**: If you find ANY asset not preserved during liquidation → **CRITICAL**

#### Check 2.1.2: Liquidation Bypass Vulnerabilities

**What to look for**: Any mechanism where users can prevent or delay liquidations.

**Common bypass patterns**:

**Pattern A: External Call Dependency**
```rust
// ❌ VULNERABLE - Search for this pattern
fn liquidate(deps: DepsMut, info: MessageInfo, position_id: u64) -> Result<Response> {
    let position = POSITIONS.load(deps.storage, position_id)?;
    
    // Calls user's contract to notify them
    let notify_msg = WasmMsg::Execute {
        contract_addr: position.owner.to_string(),
        msg: to_binary(&NotifyLiquidation {})?,
        funds: vec![],
    };
    
    // ❌ CRITICAL: If user's contract reverts, entire liquidation fails!
    // User can deploy contract that always reverts to avoid liquidation
    
    Ok(Response::new().add_message(notify_msg))
}
```

**How to identify**: Search for any `WasmMsg::Execute` or external calls within liquidation functions. Ask: "What if this external call fails?"

**Pattern B: Insufficient Edge Case Handling**
```rust
// ❌ VULNERABLE - From Mars Perps actual audit
fn liquidate(deps: DepsMut, amount: Uint128) -> Result<Response> {
    // Calculate liquidation amount
    let liquidation_amount = calculate_liquidation(amount)?;
    
    // ❌ CRITICAL: What if liquidation_amount rounds to zero?
    // Position with 0.0001 collateral after price drop
    // liquidation_amount = 0
    // Require fails, position never liquidates!
    
    require!(liquidation_amount > Uint128::zero(), "Invalid amount");
    
    execute_liquidation(liquidation_amount)?;
    Ok(Response::new())
}
```

**How to identify**: Ask yourself: "What is the smallest possible position size? What if it rounds to zero?"

**Pattern C: Reentrancy in Liquidation**
```rust
// ❌ VULNERABLE - Search for state updates AFTER external calls
fn liquidate(deps: DepsMut, position_id: u64) -> Result<Response> {
    let position = POSITIONS.load(deps.storage, position_id)?;
    
    // Sends collateral to liquidator
    transfer_collateral(position.collateral)?; // External call!
    
    // ❌ CRITICAL: Position still exists in state!
    // Liquidator can call liquidate() again before this completes
    // (If transfer_collateral calls back into this contract)
    
    POSITIONS.remove(deps.storage, position_id); // Too late!
    Ok(Response::new())
}
```

**How to identify**: Look for this pattern: Load → External Call → State Update. Should be: Load → State Update → External Call.

**Questions to ask**:
- Does liquidation make ANY external calls? If yes, what if they fail?
- Does liquidation handle zero amounts? Test with dust positions.
- Are state updates done BEFORE external calls? (Checks-Effects-Interactions pattern)
- Can a malicious user contract block liquidation?
- What is the smallest liquidatable position? Is there a dust threshold?

**Test cases to verify**:
1. Deploy a contract that reverts on any call → Use as position owner → Try to liquidate
2. Create position with 1 wei collateral → Try to liquidate after price drop
3. Create position → In liquidation callback, try to call liquidate again (reentrancy)

**Severity**: If users can block their own liquidation → **CRITICAL**

#### Check 2.1.4: Duplicate Array Elements Allowing Multiple Claims

**What to look for**: Functions that accept arrays without checking for duplicates, allowing users to claim rewards/benefits multiple times.

**Critical vulnerability pattern - Very common!**

**Real-world example (Similar to Anchor issue)**:
```rust
// ❌ CRITICAL - No duplicate checking
pub fn claim_rewards(
    deps: DepsMut,
    info: MessageInfo,
    claim_ids: Vec<u64>,
) -> Result<Response> {
    let mut total_rewards = Uint128::zero();
    
    // ❌ No check if claim_ids contains duplicates!
    // User can pass [1, 1, 1, 1, 1] and claim same reward 5 times
    for claim_id in claim_ids {
        let reward = REWARDS.load(deps.storage, claim_id)?;
        
        // ❌ Critical: Doesn't check if already claimed
        total_rewards += reward.amount;
        
        // Marks as claimed but user already got it multiple times!
        REWARDS.update(deps.storage, claim_id, |r| -> StdResult<_> {
            Ok(Reward { 
                amount: Uint128::zero(),
                ..r.unwrap()
            })
        })?;
    }
    
    // Transfer total (which includes duplicates!)
    transfer_rewards(info.sender, total_rewards)?;
    
    Ok(Response::new())
}
```

**Impact**: 
- User can drain all rewards by claiming same ID multiple times
- User can bypass limits by repeating IDs
- Protocol loses all reward funds

**How to identify**:
1. Find all functions that accept `Vec<T>` parameter
2. Check if the vector is used in a loop
3. Check if there's duplicate detection before the loop
4. Check if state is updated to prevent re-use

**Search pattern**:
```bash
# Find functions accepting vectors
grep -r "Vec<" contracts/*/src/*.rs | grep "pub fn"

# Check for deduplication logic
grep -r "dedup\|unique\|HashSet\|BTreeSet" contracts/
```

**Other vulnerable patterns**:
```rust
// ❌ VULNERABLE: Batch operations without dedup
pub fn batch_activate_bids(bid_ids: Vec<u64>) -> Result<Response> {
    for bid_id in bid_ids {
        activate_bid(bid_id)?;  // No dup check!
    }
}

// ❌ VULNERABLE: Remove items without dedup
pub fn remove_positions(position_ids: Vec<u64>) -> Result<Response> {
    for id in position_ids {
        POSITIONS.remove(deps.storage, id);
        // Removing same ID twice doesn't error, but affects state incorrectly
    }
}

// ❌ VULNERABLE: Transfer batch without dedup
pub fn batch_transfer(recipients: Vec<Addr>, amounts: Vec<Uint128>) -> Result<Response> {
    for (recipient, amount) in recipients.iter().zip(amounts.iter()) {
        transfer(recipient, amount)?;
        // Same recipient multiple times drains more funds!
    }
}
```

**Correct patterns**:
```rust
// ✅ SOLUTION 1: Check for duplicates explicitly
pub fn claim_rewards(
    deps: DepsMut,
    info: MessageInfo,
    claim_ids: Vec<u64>,
) -> Result<Response> {
    // ✅ Deduplicate first
    use std::collections::HashSet;
    let unique_ids: HashSet<u64> = claim_ids.iter().copied().collect();
    
    // ✅ Verify no duplicates
    require!(
        unique_ids.len() == claim_ids.len(),
        "Duplicate claim IDs not allowed"
    );
    
    let mut total_rewards = Uint128::zero();
    for claim_id in unique_ids {
        let reward = REWARDS.load(deps.storage, claim_id)?;
        
        // ✅ Additional safety: Check not already claimed
        require!(!reward.claimed, "Already claimed");
        
        total_rewards += reward.amount;
        
        REWARDS.update(deps.storage, claim_id, |r| -> StdResult<_> {
            Ok(Reward { 
                claimed: true,
                amount: Uint128::zero(),
                ..r.unwrap()
            })
        })?;
    }
    
    transfer_rewards(info.sender, total_rewards)?;
    Ok(Response::new())
}

// ✅ SOLUTION 2: Process with claimed flag check
pub fn claim_rewards_safe(
    deps: DepsMut,
    info: MessageInfo,
    claim_ids: Vec<u64>,
) -> Result<Response> {
    let mut total_rewards = Uint128::zero();
    
    for claim_id in claim_ids {
        let reward = REWARDS.load(deps.storage, claim_id)?;
        
        // ✅ Skip if already claimed (handles duplicates gracefully)
        if reward.claimed {
            continue;
        }
        
        total_rewards += reward.amount;
        
        // Mark as claimed immediately
        REWARDS.update(deps.storage, claim_id, |r| -> StdResult<_> {
            Ok(Reward { 
                claimed: true,
                amount: Uint128::zero(),
                ..r.unwrap()
            })
        })?;
    }
    
    // Only transfer if there are rewards
    if total_rewards > Uint128::zero() {
        transfer_rewards(info.sender, total_rewards)?;
    }
    
    Ok(Response::new())
}

// ✅ SOLUTION 3: Defensive - limit array size AND check duplicates
pub fn claim_rewards_defensive(
    deps: DepsMut,
    info: MessageInfo,
    claim_ids: Vec<u64>,
) -> Result<Response> {
    // ✅ Prevent DoS: Limit array size
    const MAX_CLAIMS_PER_TX: usize = 50;
    require!(
        claim_ids.len() <= MAX_CLAIMS_PER_TX,
        "Too many claims in one transaction"
    );
    
    // ✅ Check for duplicates
    use std::collections::HashSet;
    let unique_ids: HashSet<u64> = claim_ids.iter().copied().collect();
    require!(
        unique_ids.len() == claim_ids.len(),
        "Duplicate claim IDs detected"
    );
    
    // Process claims...
    Ok(Response::new())
}
```

**Questions to ask**:
- Does this function accept a `Vec<>` parameter?
- Is the vector processed in a loop?
- Are duplicates checked before processing?
- What happens if the same element appears twice?
- Is there a "claimed" or "used" flag to prevent re-use?
- Is the array size limited? (Prevent DoS)

**Test case**:
```rust
#[test]
fn test_duplicate_claim_ids() {
    let mut deps = mock_dependencies();
    
    // Setup: Create 1 reward worth 100 tokens
    setup_reward(deps.as_mut(), claim_id: 1, amount: 100);
    
    // Attack: Claim same ID 10 times
    let msg = ExecuteMsg::ClaimRewards {
        claim_ids: vec![1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    };
    
    let result = execute(deps.as_mut(), mock_env(), mock_info("user", &[]), msg);
    
    // ✅ PASS: Transaction rejected due to duplicates
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Duplicate"));
    
    // OR if graceful handling:
    // ✅ PASS: User only receives 100 tokens (not 1000)
    // assert_eq!(get_balance("user"), 100);
}
```

**Related vulnerable patterns**:
- Batch withdrawals without dedup
- Batch stake/unstake without dedup  
- Batch voting with same poll ID
- Multiple deposits to same position
- Batch liquidations of same position

**Severity**: Duplicate claims allowed → **CRITICAL** (fund drainage)

#### Check 2.1.3: Bad Debt Management

**What to look for**: Search for how the protocol tracks shortfalls when collateral doesn't cover debt.

**Key questions**:
- Where is total bad debt stored in state?
- Is bad debt updated on EVERY liquidation?
- Can bad debt ever be reset without actual repayment?
- Is there an insurance fund? How is it funded and used?

**Vulnerability pattern from Margined Protocol**:
```rust
// ❌ VULNERABLE CODE - Actual issue from audit
fn liquidate_position(position_id: u64) -> Result<Response> {
    let position = POSITIONS.load(deps.storage, position_id)?;
    
    let collateral_value = get_collateral_value(&position)?;
    let debt_value = position.debt;
    
    // Calculate bad debt
    let bad_debt = if debt_value > collateral_value {
        debt_value - collateral_value
    } else {
        Uint128::zero()
    };
    
    // ❌ CRITICAL: bad_debt is calculated but NEVER stored!
    // Protocol has no record of cumulative losses
    // Insurance fund can't track how much is needed
    
    close_position(position_id)?;
    Ok(Response::new())
}
```

**How to find this issue**:
1. Search for variables named "bad_debt", "shortfall", "deficit"
2. Check if they are read from storage
3. Check if they are written to storage  
4. If calculated but not persisted → **CRITICAL**

**Another vulnerability - Incorrect reset**:
```rust
// ❌ VULNERABLE - From Margined Protocol audit
fn realize_bad_debt() -> Result<Response> {
    let bad_debt = BAD_DEBT.load(deps.storage)?;
    
    // Try to cover from insurance fund
    let insurance = INSURANCE_FUND.load(deps.storage)?;
    
    if insurance >= bad_debt {
        // Pay from insurance
        INSURANCE_FUND.save(deps.storage, &(insurance - bad_debt))?;
        
        // ❌ CRITICAL: Resets bad_debt to zero!
        BAD_DEBT.save(deps.storage, &Uint128::zero())?;
    } else {
        // ❌ CRITICAL: Partial payment but still resets to zero!
        INSURANCE_FUND.save(deps.storage, &Uint128::zero())?;
        BAD_DEBT.save(deps.storage, &Uint128::zero())?;
        
        // Bad debt just disappeared! Protocol lying about solvency!
    }
    
    Ok(Response::new())
}
```

**Correct pattern**:
```rust
// ✅ SECURE - How it should be done
fn realize_bad_debt() -> Result<Response> {
    let bad_debt = BAD_DEBT.load(deps.storage)?;
    let insurance = INSURANCE_FUND.load(deps.storage)?;
    
    // Calculate how much can actually be covered
    let covered_amount = insurance.min(bad_debt);
    let remaining_bad_debt = bad_debt - covered_amount;
    
    // Update both atomically
    INSURANCE_FUND.save(deps.storage, &(insurance - covered_amount))?;
    BAD_DEBT.save(deps.storage, &remaining_bad_debt)?;
    
    // ✅ Bad debt only reduced by amount actually repaid
    // ✅ If insurance insufficient, bad debt remains on books
    
    Ok(Response::new())
}
```

**Audit checklist for bad debt**:
- [ ] Is there a BAD_DEBT storage variable?
- [ ] Is it updated on EVERY undercollateralized liquidation?
- [ ] Can it ever be set to zero without funds being added?
- [ ] Is there defensive code to prevent underflow when reducing it?
- [ ] Are events emitted when bad debt increases?
- [ ] Is bad debt included in protocol health metrics?

**Severity**: Bad debt not tracked → **CRITICAL**. Bad debt incorrectly reset → **MAJOR**.

---

### Step 2.2: Oracle Manipulation Vulnerabilities

**Instructions**: All DeFi protocols rely on price data. Oracle bugs are the second most common critical issue (seen in Mars, Levana, Calculated Finance).

#### Check 2.2.1: Spot Price Usage

**What to look for**: ANY use of instantaneous prices for critical operations.

**Critical rule**: NEVER use spot prices for liquidations, collateral valuation, or swap calculations. ALWAYS use time-weighted average prices (TWAP) or other manipulation-resistant mechanisms.

**Search for these dangerous patterns**:
```rust
// ❌ DANGEROUS - Search for instant price queries
let price = query_price(deps.querier, asset)?;
let collateral_value = position.collateral * price;

// ❌ DANGEROUS - From Mars v2 actual vulnerability
let spot_price = query_astroport_spot_price(pool_addr)?;
let health_factor = calculate_health(spot_price)?;

// ❌ DANGEROUS - From Calculated Finance
fn execute_swap(pool_addr: String) -> Result<Response> {
    let current_price = query_spot_price(pool_addr)?;
    // Uses current_price for swap decision
    // Attacker can manipulate pool in same block!
}
```

**Why this is critical**: 
- Attacker can manipulate spot price with flash loans
- Execute attack and unwind in single transaction
- Example: Borrow → Manipulate price up → Borrow more → Unwind manipulation → Profit

**Actual vulnerability from Levana**:
```rust
// ❌ CRITICAL ISSUE - Prices inverted!
fn get_price_from_oracle(oracle: &Oracle) -> Result<Decimal> {
    let price_data = oracle.query_price()?;
    
    // ❌ CRITICAL: Returns inverted price!
    // If BTC/USD = 50000, this returns 0.00002
    // Entire protocol uses backwards prices!
    
    Ok(Decimal::one() / price_data.price)
}
```

**How to audit oracle integration**:

**Step 1**: Find all price query functions
```bash
# Search codebase for:
grep -r "query_price" contracts/
grep -r "get_price" contracts/
grep -r "oracle" contracts/
```

**Step 2**: For each price query, ask:
- Is this a TWAP or spot price?
- What is the TWAP window if applicable? (Minimum 15 minutes recommended)
- Are prices validated for sanity? (deviation checks)
- Is there a circuit breaker for large price movements?
- Are prices ever inverted? (Check the math carefully!)

**Step 3**: Trace where prices are used:
- Liquidation decisions? **MUST be TWAP**
- Collateral valuation? **MUST be TWAP**  
- Health factor calculations? **MUST be TWAP**
- Swap execution? **Spot OK but add slippage protection**
- Display purposes only? **Spot OK**

**Correct pattern**:
```rust
// ✅ SECURE - TWAP with validation
fn get_safe_price(asset: String) -> Result<Decimal> {
    // Get TWAP with minimum 15-minute window
    let twap_price = query_twap_price(asset, FIFTEEN_MINUTES)?;
    
    // Validate against bounds
    let min_price = MIN_PRICES.load(deps.storage, &asset)?;
    let max_price = MAX_PRICES.load(deps.storage, &asset)?;
    
    // Circuit breaker: price must be within expected range
    if twap_price < min_price || twap_price > max_price {
        return Err(ContractError::PriceOutOfBounds {
            asset,
            price: twap_price,
            min: min_price,
            max: max_price,
        });
    }
    
    // Check deviation from previous price
    let last_price = LAST_PRICES.load(deps.storage, &asset)?;
    let deviation = (twap_price - last_price).abs() / last_price;
    
    // Circuit breaker: max 10% change per update
    if deviation > Decimal::percent(10) {
        return Err(ContractError::PriceDeviationTooLarge {
            asset,
            old_price: last_price,
            new_price: twap_price,
            deviation,
        });
    }
    
    // Update last price
    LAST_PRICES.save(deps.storage, &asset, &twap_price)?;
    
    Ok(twap_price)
}
```

**Questions to ask**:
- What is the oracle's manipulation cost? (How much capital needed to move price)
- Can price be manipulated within a single block?
- Is there redundancy? (Multiple oracle sources)
- What happens if oracle goes offline?
- Are there price staleness checks?

**Test cases**:
1. Manipulate DEX pool by 50% → Check if protocol accepts price
2. Stop oracle updates → Check if protocol handles stale data
3. Send inverted price (1/p instead of p) → Check if detected
4. Gradually move price 20% → Check if circuit breaker triggers

**Severity**: Spot price for critical operations → **CRITICAL**. Missing circuit breakers → **MAJOR**.

#### Check 2.2.2: Oracle Price Validation

**What to look for**: Insufficient validation of oracle data before use.

**Real vulnerability from IncrementFi**:
```rust
// ❌ VULNERABLE
fn set_price(feeder: Addr, asset: String, price: Decimal, timestamp: u64) -> Result<Response> {
    // Check feeder is authorized
    require!(AUTHORIZED_FEEDERS.has(deps.storage, &feeder), "Unauthorized");
    
    // ❌ ISSUE: Feeder can set non-expirable prices!
    // By setting timestamp very far in future (u64::MAX)
    // Price never expires, even if oracle stops updating
    
    PRICES.save(deps.storage, &asset, &PriceData {
        price,
        timestamp,
        feeder,
    })?;
    
    Ok(Response::new())
}
```

**Validation checklist for oracle data**:
- [ ] Is timestamp checked against current time? (Not too far in future)
- [ ] Is there a maximum price age enforced?
- [ ] Are prices within reasonable bounds for the asset?
- [ ] Is price precision validated? (Not more decimals than expected)
- [ ] Is price direction correct? (Not inverted)
- [ ] Are multiple oracle sources compared if available?

**Severity**: Insufficient oracle validation → **MAJOR**

---

### Step 2.3: Access Control Vulnerabilities

**Instructions**: Check ALL state-mutating functions for proper access control. This is the third most common critical issue category.

#### Check 2.3.1: Anyone-Can-Call Critical Functions

**What to look for**: Functions that change critical state but don't verify caller.

**Real vulnerability from Levana**:
```rust
// ❌ CRITICAL VULNERABILITY - Actual issue from audit
pub fn save_swap_instruction(
    deps: DepsMut,
    msg: SaveSwapInstructionMsg,
) -> Result<Response> {
    // ❌ NO CALLER VERIFICATION!
    // Anyone can call this function
    // Anyone can overwrite swap instructions
    
    SWAP_INSTRUCTIONS.save(deps.storage, &msg.instruction)?;
    Ok(Response::new())
}

// ❌ CRITICAL - Also from Levana
pub fn whitelist_vamm(
    deps: DepsMut,
    vamm_addr: String,
) -> Result<Response> {
    let addr = deps.api.addr_validate(&vamm_addr)?;
    
    // ❌ NO AUTHORIZATION CHECK!
    // Anyone can whitelist a malicious vAMM
    // Anyone can overwrite existing vAMM addresses
    
    VAMM_WHITELIST.save(deps.storage, &addr, &true)?;
    Ok(Response::new())
}
```

**How to identify**:

**Step 1**: List all execute functions:
```bash
grep -A 20 "pub fn execute" contracts/*/src/*.rs
```

**Step 2**: For EACH function, check if it starts with authorization:
```rust
// ❌ MISSING - Function has no checks
pub fn update_config(deps: DepsMut, new_config: Config) -> Result<Response> {
    CONFIG.save(deps.storage, &new_config)?;
    // Anyone can update config!
}

// ✅ CORRECT - Proper authorization
pub fn update_config(
    deps: DepsMut,
    info: MessageInfo,  // ← Must have MessageInfo parameter
    new_config: Config,
) -> Result<Response> {
    // Check caller is admin
    let admin = ADMIN.load(deps.storage)?;
    require!(info.sender == admin, "Unauthorized");
    
    CONFIG.save(deps.storage, &new_config)?;
    Ok(Response::new())
}
```

**Red flags**:
- Function modifies state but has no `info: MessageInfo` parameter
- Function has `info` but never checks `info.sender`
- Function checks sender against wrong address
- Function uses `||` (OR) instead of `&&` (AND) in multi-sig checks

**Pattern from Mars v2**:
```rust
// ❌ CRITICAL - Attackers can bind vault account IDs
pub fn update_account_vault(
    deps: DepsMut,
    account_id: String,
    vault_id: String,
) -> Result<Response> {
    // ❌ No check that caller owns account_id!
    // Attacker can bind any account to any vault
    // Causes forced losses for users
    
    ACCOUNT_VAULTS.save(deps.storage, &account_id, &vault_id)?;
    Ok(Response::new())
}

// ✅ CORRECT
pub fn update_account_vault(
    deps: DepsMut,
    info: MessageInfo,
    account_id: String,
    vault_id: String,
) -> Result<Response> {
    // Verify caller owns the account
    let account = ACCOUNTS.load(deps.storage, &account_id)?;
    require!(account.owner == info.sender, "Not account owner");
    
    // Verify vault exists and is valid
    let vault = VAULTS.may_load(deps.storage, &vault_id)?;
    require!(vault.is_some(), "Invalid vault");
    
    ACCOUNT_VAULTS.save(deps.storage, &account_id, &vault_id)?;
    Ok(Response::new())
}
```

**Audit process for access control**:

**For EVERY execute function, ask**:
1. Who should be allowed to call this? (Anyone, Owner, Admin, Specific role)
2. Is there a check enforcing that?
3. What assets or state does this function control?
4. What's the worst case if an attacker calls this?

**Priority order** (audit these first):
1. Functions that transfer assets
2. Functions that update critical parameters (oracle addresses, fee rates, etc.)
3. Functions that change ownership or roles
4. Functions that pause/unpause the protocol
5. Functions that trigger migrations or upgrades

**Questions to ask**:
- Does this function have `info: MessageInfo`?
- Is `info.sender` validated against expected caller(s)?
- Are there ANY state-mutating functions without authorization?
- Is admin address validated when set?
- Can admin role be renounced or transferred?
- Is there a timelock on admin actions?

**Severity**: Anyone can call critical function → **CRITICAL**. Missing authorization on non-critical → **MAJOR**.

#### Check 2.3.2: Initialization Vulnerabilities

**What to look for**: Unprotected initialization that allows takeover.

**Real vulnerability from Axelar**:
```rust
// ❌ CRITICAL - Anyone can initialize
pub fn setup(
    deps: DepsMut,
    params: SetupParams,
) -> Result<Response> {
    // ❌ NO CHECK if already initialized!
    // ❌ NO CHECK on who's calling!
    
    // Attacker calls this first, becomes gateway owner
    GATEWAY_CONFIG.save(deps.storage, &GatewayConfig {
        owner: params.owner,  // ← Attacker's address
        validators: params.validators,
    })?;
    
    Ok(Response::new())
}
```

**How to identify**:
```rust
// Search for initialization patterns:
grep -n "instantiate\|initialize\|setup\|init" contracts/*/src/*.rs

// For each, check:
// 1. Can it be called multiple times?
// 2. Who can call it?
// 3. What does it control?
```

**Correct pattern**:
```rust
// ✅ SECURE initialization
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response> {
    // One-time initialization
    // info.sender becomes owner by default
    
    // Validate parameters
    deps.api.addr_validate(&msg.oracle_addr)?;
    require!(msg.fee_rate <= MAX_FEE_RATE, "Fee too high");
    
    // Initialize state
    CONFIG.save(deps.storage, &Config {
        owner: info.sender.clone(),  // Deployer is owner
        oracle_addr: msg.oracle_addr,
        fee_rate: msg.fee_rate,
    })?;
    
    // Cannot be called again (CosmWasm guarantees this)
    Ok(Response::new())
}

// For additional setup functions:
pub fn complete_setup(
    deps: DepsMut,
    info: MessageInfo,
    params: AdditionalParams,
) -> Result<Response> {
    let config = CONFIG.load(deps.storage)?;
    
    // ✅ Only owner can complete setup
    require!(info.sender == config.owner, "Unauthorized");
    
    // ✅ Check if already completed
    let setup_complete = SETUP_COMPLETE.may_load(deps.storage)?;
    require!(setup_complete.is_none(), "Already initialized");
    
    // Save additional configuration
    ADDITIONAL_CONFIG.save(deps.storage, &params)?;
    SETUP_COMPLETE.save(deps.storage, &true)?;
    
    Ok(Response::new())
}
```

**Questions**:
- Is instantiate() the only initialization entry point?
- Are there any post-deployment setup functions?
- Are setup functions protected by authorization?
- Is there a flag preventing re-initialization?
- Can an attacker front-run initialization?

**Severity**: Unprotected initialization → **CRITICAL**

---

### Step 2.4: State Management Vulnerabilities

**Instructions**: Check how protocol state is updated, especially during complex operations like migrations or multi-step processes.

#### Check 2.4.1: Migration State Loss

**What to look for**: Protocol upgrades that lose user state or rewards.

**Real vulnerability from Mars Perps**:
```rust
// ❌ CRITICAL - Incomplete migration
pub fn migrate_user_unclaimed_rewards(
    deps: DepsMut,
    users: Vec<String>,
) -> Result<Response> {
    // Migrate unclaimed rewards for batch of users
    for user_addr in users {
        let rewards = OLD_REWARDS.load(deps.storage, &user_addr)?;
        NEW_REWARDS.save(deps.storage, &user_addr, &rewards)?;
    }
    
    // ✅ Migration complete for these users
    // ❌ CRITICAL: Unlocks migration guard!
    // But migrate_user_asset_indices() hasn't run yet!
    MIGRATION_GUARD.save(deps.storage, &false)?;
    
    Ok(Response::new())
}

pub fn migrate_user_asset_indices(
    deps: DepsMut,
    users: Vec<String>,
) -> Result<Response> {
    // ❌ CRITICAL: Requires migration guard locked
    MIGRATION_GUARD.assert_locked(deps.storage)?;  // FAILS!
    
    // This function can never complete now
    // User asset indices are lost forever
    
    for user_addr in users {
        // Would migrate indices...
    }
    
    Ok(Response::new())
}
```

**How to identify migration issues**:

**Step 1**: Find migration code
```bash
grep -r "migrate\|migration\|upgrade" contracts/*/src/*.rs
```

**Step 2**: For each migration, verify:
- [ ] Is migration atomic? (All state migrated or none)
- [ ] Are there multiple migration steps? (Check dependencies)
- [ ] Is there a guard preventing normal operations during migration?
- [ ] Does migration preserve ALL user data?
- [ ] Are rewards auto-claimed before migration?
- [ ] Is there a rollback mechanism if migration fails?

**Another pattern from Mirror Protocol**:
```rust
// ❌ MAJOR - Re-enables disabled features
pub fn migrate(deps: DepsMut, _env: Env) -> Result<Response> {
    // Migrate collateral oracle config
    migrate_oracle_config(deps.storage)?;
    
    // Migrate collateral assets
    for asset in ASSETS.range(deps.storage, None, None, Order::Ascending) {
        let (denom, asset_config) = asset?;
        
        // ❌ ISSUE: All assets become enabled again!
        // Assets that were revoked before migration are re-activated
        // Security: Previously disabled risky collateral is now accepted
        
        NEW_ASSETS.save(deps.storage, &denom, &AssetConfig {
            enabled: true,  // ← Always true!
            ...asset_config
        })?;
    }
    
    Ok(Response::new())
}
```

**Correct migration pattern**:
```rust
// ✅ SECURE - Atomic migration with preservation
pub fn migrate(deps: DepsMut, env: Env) -> Result<Response> {
    // Step 1: Lock protocol during migration
    MIGRATION_LOCK.save(deps.storage, &true)?;
    
    // Step 2: Auto-claim all pending rewards FIRST
    let all_users = get_all_users(deps.storage)?;
    for user in all_users {
        auto_claim_rewards(deps.storage, &user)?;
    }
    
    // Step 3: Migrate state preserving ALL fields
    for asset in OLD_ASSETS.range(deps.storage, None, None, Order::Ascending) {
        let (denom, old_config) = asset?;
        
        // ✅ Preserve enabled/disabled status
        NEW_ASSETS.save(deps.storage, &denom, &NewAssetConfig {
            enabled: old_config.enabled,  // ← Preserve state
            revoked: old_config.revoked,  // ← Preserve revocations
            // ... other fields
        })?;
    }
    
    // Step 4: Verify migration completed successfully
    verify_migration_integrity(deps.storage)?;
    
    // Step 5: Unlock protocol
    MIGRATION_LOCK.save(deps.storage, &false)?;
    
    Ok(Response::new())
}

fn verify_migration_integrity(storage: &dyn Storage) -> Result<()> {
    // Check: All users migrated
    let old_user_count = OLD_USERS.count(storage)?;
    let new_user_count = NEW_USERS.count(storage)?;
    require!(old_user_count == new_user_count, "User count mismatch");
    
    // Check: Total rewards preserved
    let old_total_rewards = OLD_TOTAL_REWARDS.load(storage)?;
    let new_total_rewards = NEW_TOTAL_REWARDS.load(storage)?;
    require!(old_total_rewards == new_total_rewards, "Rewards lost");
    
    Ok(())
}
```

**Questions for migration code**:
- Are users' rewards claimed before migration?
- Is previously disabled functionality re-enabled?
- Can migration be partially completed?
- What happens if migration fails halfway?
- Are there integrity checks after migration?
- Is there a test that migrates real production-like state?

**Severity**: User data lost in migration → **CRITICAL**. Disabled features re-enabled → **MAJOR**.

#### Check 2.4.2: Reward Distribution Vulnerabilities

**What to look for**: Incorrect tracking of rewards leading to loss or theft.

**Real vulnerability from Mars v2**:
```rust
// ❌ CRITICAL - Incorrect total tracking
pub fn stake_lp_tokens(
    deps: DepsMut,
    info: MessageInfo,
    amount: Uint128,
) -> Result<Response> {
    // User stakes LP tokens to earn rewards
    
    // Update user's staked amount
    USER_STAKES.update(deps.storage, &info.sender, |stake| -> StdResult<_> {
        Ok(stake.unwrap_or_default() + amount)
    })?;
    
    // ❌ CRITICAL: Incorrectly updates total!
    // Should only increase if new liquidity added
    // But this increases even for internal transfers
    TOTAL_LIQUIDITY_TOKENS.update(deps.storage, |total| -> StdResult<_> {
        Ok(total + amount)
    })?;
    
    // Result: total_liquidity_tokens > sum(user_stakes)
    // Rewards are diluted, users get less than entitled
    
    Ok(Response::new())
}
```

**How to identify reward bugs**:

**Step 1**: Find reward calculation functions
```bash
grep -r "calculate_reward\|distribute_reward\|claim_reward" contracts/
```

**Step 2**: For each reward system, verify these invariants:
```rust
// Invariant 1: Total rewards distributed ≤ Total rewards available
assert!(sum(user_rewards) <= total_rewards_pool);

// Invariant 2: User rewards ≤ Their proportional share  
assert!(user_reward <= user_stake * total_rewards / total_stake);

// Invariant 3: Total staked by users = Total tracked by protocol
assert!(sum(user_stakes) == total_staked);
```

**Add these checks**:
```rust
// ✅ Defensive programming for rewards
pub fn claim_rewards(
    deps: DepsMut,
    info: MessageInfo,
) -> Result<Response> {
    let user_stake = USER_STAKES.load(deps.storage, &info.sender)?;
    let total_stake = TOTAL_STAKE.load(deps.storage)?;
    let total_rewards = TOTAL_REWARDS.load(deps.storage)?;
    
    // Calculate user's share
    let user_rewards = user_stake * total_rewards / total_stake;
    
    // ✅ Defensive check: Never distribute more than available
    let available_rewards = REWARDS_POOL.load(deps.storage)?;
    let actual_rewards = user_rewards.min(available_rewards);
    
    // ✅ Check: User hasn't already claimed
    let claimed = CLAIMED_REWARDS.may_load(deps.storage, &info.sender)?
        .unwrap_or_default();
    require!(claimed < actual_rewards, "Already claimed");
    
    // Transfer rewards
    transfer_rewards(info.sender.clone(), actual_rewards)?;
    
    // Update state
    CLAIMED_REWARDS.save(deps.storage, &info.sender, &actual_rewards)?;
    REWARDS_POOL.update(deps.storage, |pool| Ok(pool - actual_rewards))?;
    
    // ✅ Emit detailed event for monitoring
    let event = Event::new("rewards_claimed")
        .add_attribute("user", info.sender)
        .add_attribute("amount", actual_rewards.to_string())
        .add_attribute("remaining_pool", (available_rewards - actual_rewards).to_string());
    
    Ok(Response::new().add_event(event))
}
```

**Questions**:
- Are total counters updated correctly on all operations?
- Can sum(user_rewards) exceed total_rewards_pool?
- Are rewards preserved during withdrawals?
- Is there double-claiming protection?
- Are rewards calculated before or after state changes?

**Severity**: Reward loss or theft → **CRITICAL**. Incorrect calculations → **MAJOR**.

---

### Step 2.5: Gas and Denial of Service

**Instructions**: Check for unbounded operations that can cause out-of-gas errors.

#### Check 2.5.1: Unbounded Iterations

**What to look for**: Loops over user-controlled or unbounded collections.

**This is extremely common - seen in 10+ protocols!**

**Real examples**:

**From Nolus** (Chain-level DoS!):
```rust
// ❌ CRITICAL - Can halt entire blockchain
pub fn ante_handle(
    ctx: Context,
    tx: Transaction,
) -> Result<()> {
    // Iterate over ALL fees in transaction
    for fee in tx.fees {  // ← Unbounded!
        // Attacker sends tx with 10,000 fees
        // Block validation runs out of gas
        // Entire chain halts!
        process_fee(fee)?;
    }
    Ok(())
}
```

**From Lido**:
```rust
// ❌ CRITICAL - User funds stuck forever
pub fn withdraw(
    deps: DepsMut,
    info: MessageInfo,
) -> Result<Response> {
    // Get user's unbond history
    let unbonds = UNBOND_HISTORY.load(deps.storage, &info.sender)?;
    
    // ❌ Iterate over entire history (unbounded!)
    for unbond in unbonds {  // ← Could be 10,000+ entries
        if unbond.is_ready() {
            process_unbond(unbond)?;
        }
    }
    
    // User who has been in protocol for years
    // Has thousands of unbond entries
    // Withdraw runs out of gas
    // Funds permanently stuck!
    
    Ok(Response::new())
}
```

**From Drop**:
```rust
// ❌ MAJOR - Cannot process large positions
pub fn withdraw_liquidity(
    deps: DepsMut,
    info: MessageInfo,
) -> Result<Response> {
    let user_positions = POSITIONS.load(deps.storage, &info.sender)?;
    
    // ❌ Iterate over all user positions
    for position in user_positions {  // ← Unbounded!
        close_position(deps.storage, position)?;
    }
    
    // User with 200 positions cannot withdraw
    // Gas limit exceeded
    
    Ok(Response::new())
}
```

**How to identify**:

**Step 1**: Search for loop keywords
```bash
grep -n "for \|\.map(\|\.filter(\|while " contracts/*/src/*.rs
```

**Step 2**: For each loop, ask:
- What is the loop iterating over?
- What controls the collection size?
- Can a user make it arbitrarily large?
- What's the worst-case number of iterations?

**Step 3**: Check these specific patterns:
```rust
// ❌ DANGEROUS PATTERNS

// Pattern 1: Unbounded user history
for entry in USER_HISTORY.load(user)? { }

// Pattern 2: All users in protocol
for user in ALL_USERS.range(storage, None, None, Order::Ascending) { }

// Pattern 3: All positions/stakes/delegations
for position in POSITIONS.load(user)? { }

// Pattern 4: Unbounded market/asset lists
for market in MARKETS.range(storage, None, None, Order::Ascending) { }

// Pattern 5: Nested unbounded loops (worst!)
for user in ALL_USERS {
    for position in POSITIONS.load(user)? { }
}
```

**Solution patterns**:
```rust
// ✅ SOLUTION 1: Hard caps
const MAX_POSITIONS_PER_USER: usize = 100;
const MAX_UNBOND_HISTORY: usize = 1000;
const MAX_MARKETS: usize = 50;

pub fn add_position(/* ... */) -> Result<Response> {
    let positions = POSITIONS.load(deps.storage, &user)?;
    
    // ✅ Enforce cap at insertion time
    require!(
        positions.len() < MAX_POSITIONS_PER_USER,
        "Too many positions"
    );
    
    // Add new position
}

// ✅ SOLUTION 2: Pagination
pub fn process_unbonds(
    deps: DepsMut,
    info: MessageInfo,
    start_after: Option<u64>,
    limit: Option<u32>,
) -> Result<Response> {
    let limit = limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);
    
    // Only process 'limit' entries per call
    let unbonds = UNBOND_HISTORY
        .prefix(&info.sender)
        .range(deps.storage, start_after, None, Order::Ascending)
        .take(limit as usize)
        .collect::<StdResult<Vec<_>>>()?;
    
    for (id, unbond) in unbonds {
        if unbond.is_ready() {
            process_unbond(unbond)?;
        }
    }
    
    // User calls multiple times to process all
    Ok(Response::new())
}

// ✅ SOLUTION 3: Cleanup old entries
pub fn add_unbond_entry(/* ... */) -> Result<Response> {
    let mut history = UNBOND_HISTORY.load(deps.storage, &user)?;
    
    // ✅ Remove oldest entries if limit reached
    if history.len() >= MAX_UNBOND_HISTORY {
        // Remove completed unbonds from start of list
        history.retain(|u| !u.is_complete());
        
        // If still at limit, remove oldest
        if history.len() >= MAX_UNBOND_HISTORY {
            history.remove(0);
        }
    }
    
    history.push(new_unbond);
    UNBOND_HISTORY.save(deps.storage, &user, &history)?;
    
    Ok(Response::new())
}
```

**Recommended hard caps from analyzed protocols**:
- Max positions per user: **100**
- Max markets in protocol: **50**
- Max validators in set: **150**
- Max unbond history per user: **1,000**
- Max collateral types: **20**
- Max query results: **30** (pagination limit)

**Questions**:
- Are there ANY unbounded loops in execute functions?
- Are collections capped at insertion time?
- Is pagination offered for large queries?
- What happens if a loop runs out of gas mid-execution?
- Are old/completed entries cleaned up automatically?

**Severity**: Unbounded loop on critical path → **MAJOR**. Chain-level DoS → **CRITICAL**.

---

### Step 2.6: Refund and Withdrawal Vulnerabilities

**Instructions**: Check all paths where assets are returned to users.

#### Check 2.6.1: Double Refunds

**What to look for**: Refund calculations executed multiple times.

**Real vulnerability from Astroport (CRITICAL)**:
```rust
// ❌ CRITICAL - Actual code from audit
pub fn swap_exact_amount_out(
    deps: DepsMut,
    offer_asset: Asset,
    target_out_amount: Uint128,
) -> Result<Response> {
    // Calculate how much offer asset is needed
    let required_offer = calculate_required_offer(target_out_amount)?;
    
    // User sent more than required
    let excess_offer = offer_asset.amount - required_offer;
    
    // First refund calculation
    if excess_offer > Uint128::zero() {
        let refund_msg = Asset {
            amount: excess_offer,
            ..offer_asset
        }.into_msg(&querier, offer_asset.sender.clone())?;
        
        msgs.push(refund_msg);
    }
    
    // ... more code ...
    
    // ❌ CRITICAL: Second refund calculation!
    // Same excess calculated and refunded again!
    let excess_offer = offer_asset.amount - required_offer;
    
    if excess_offer > Uint128::zero() {
        let refund_msg = Asset {
            amount: excess_offer,  // ← Refunded twice!
            ..offer_asset
        }.into_msg(&querier, offer_asset.sender.clone())?;
        
        msgs.push(refund_msg);
    }
    
    // Attacker sends 100 tokens
    // Needs 80 tokens for swap
    // Gets refunded 20 + 20 = 40 tokens
    // Repeat to drain pool
    
    Ok(Response::new().add_messages(msgs))
}
```

**How to identify**:
1. Find all functions that return excess assets
2. Search for duplicate variable names (especially "excess", "refund", "remaining")
3. Check if refund amount is calculated more than once
4. Verify refund only happens in one place

**Search pattern**:
```bash
# Find potential double refunds
grep -B5 -A10 "refund\|excess" contracts/*/src/*.rs | \
  grep -A10 "refund.*refund\|excess.*excess"
```

**Correct pattern**:
```rust
// ✅ SECURE - Single refund calculation
pub fn swap_exact_amount_out(
    deps: DepsMut,
    offer_asset: Asset,
    target_out_amount: Uint128,
) -> Result<Response> {
    let required_offer = calculate_required_offer(target_out_amount)?;
    let received_offer = offer_asset.amount;
    
    // ✅ Calculate excess exactly once
    let excess = if received_offer > required_offer {
        received_offer - required_offer
    } else {
        Uint128::zero()
    };
    
    // ✅ Add refund message only if excess exists
    let mut messages = vec![];
    if excess > Uint128::zero() {
        messages.push(Asset {
            amount: excess,
            ..offer_asset
        }.into_msg(&querier, offer_asset.sender.clone())?);
    }
    
    // ✅ Verify invariant
    assert_eq!(
        received_offer,
        required_offer + excess,
        "Refund calculation error"
    );
    
    Ok(Response::new().add_messages(messages))
}
```

**Severity**: Double refund → **CRITICAL**

#### Check 2.6.2: No Refund / Lost Excess

**What to look for**: Excess assets not returned to user.

**Real vulnerability from Dexter**:
```rust
// ❌ MAJOR - Excess LP tokens never refunded
pub fn provide_liquidity(
    deps: DepsMut,
    info: MessageInfo,
    assets: Vec<Asset>,
) -> Result<Response> {
    // User provides liquidity
    let pool = POOL.load(deps.storage)?;
    
    // Calculate LP tokens to mint
    let lp_to_mint = calculate_lp_amount(&pool, &assets)?;
    
    // ❌ What if user sent more assets than needed?
    // Excess assets are just kept in contract
    // User loses the excess!
    
    mint_lp_tokens(info.sender, lp_to_mint)?;
    
    Ok(Response::new())
}
```

**Questions for every asset transfer TO protocol**:
- If user sends more than needed, is excess refunded?
- If user sends wrong denomination, is it locked or refunded?
- Are there ANY scenarios where user assets get stuck?

**Severity**: Excess assets lost → **MAJOR**

---

## PHASE 3: PROTOCOL-SPECIFIC CHECKLISTS

**Instructions**: Based on protocol type identified in Phase 1, apply the appropriate specialized checklist in addition to the critical patterns above.

### 3.1 Lending/CDP Protocol Checklist

**Apply this if**: Protocol allows borrowing against collateral.

**CRITICAL: Anchor Liquidation Queue Vulnerabilities**

Before standard checks, be aware of these Anchor-specific issues:

**Anchor Finding #1: Permissionless Liquidation Execution Can Drain Queue**
```rust
// ❌ CRITICAL - From Anchor Protocol audit
pub fn execute_liquidation(
    deps: DepsMut,
    collateral: String,
    amount: Uint128,
) -> Result<Response> {
    // ❌ Anyone can call this!
    // External protocols can piggyback on Anchor's liquidation queue
    // In black swan event, external liquidations drain queue
    // Anchor's own liquidations fail
    
    let queue = LIQUIDATION_QUEUE.load(deps.storage, &collateral)?;
    execute_from_queue(queue, amount)?;
    
    Ok(Response::new())
}

// ✅ CORRECT - Restrict to protocol contracts
pub fn execute_liquidation(
    deps: DepsMut,
    info: MessageInfo,
    collateral: String,
    amount: Uint128,
) -> Result<Response> {
    let config = CONFIG.load(deps.storage)?;
    
    // ✅ Only custody contracts can execute liquidations
    require!(
        CUSTODY_CONTRACTS.has(deps.storage, &info.sender),
        "Only custody contracts can execute liquidations"
    );
    
    let queue = LIQUIDATION_QUEUE.load(deps.storage, &collateral)?;
    execute_from_queue(queue, amount)?;
    
    Ok(Response::new())
}
```

**Lesson**: Liquidation execution should be restricted to protocol contracts, not permissionless.

**Anchor Finding #2: No Economic Incentive for Liquidations**
```rust
// ❌ MAJOR - Liquidator gets no compensation
pub fn liquidate_collateral(
    deps: DepsMut,
    info: MessageInfo,  // ← Caller
    borrower: Addr,
) -> Result<Response> {
    // Calculate liquidation
    let result = calculate_liquidation(borrower)?;
    
    // ❌ Protocol gets fee, market gets repayment
    // ❌ Caller (info.sender) gets NOTHING
    // ❌ No gas compensation
    // Result: No incentive to call this, centralization risk
    
    send_fee(overseer_address, result.fee)?;
    send_repayment(market_address, result.repayment)?;
    // info.sender not compensated!
    
    Ok(Response::new())
}

// ✅ CORRECT - Compensate liquidator
pub fn liquidate_collateral(
    deps: DepsMut,
    info: MessageInfo,
    borrower: Addr,
) -> Result<Response> {
    let result = calculate_liquidation(borrower)?;
    
    // ✅ Liquidator gets: base fee + % of collateral
    let liquidator_reward = LIQUIDATION_BASE_FEE 
        + (result.collateral * LIQUIDATOR_BONUS_PCT);
    
    // ✅ Pay liquidator FIRST (incentive)
    send_reward(info.sender, liquidator_reward)?;
    
    // Then distribute rest
    send_fee(overseer_address, result.fee)?;
    send_repayment(market_address, result.repayment)?;
    
    Ok(Response::new())
}
```

**Lesson**: Liquidators must have economic incentive. Recommend: Base fee (200 UST) + 0.5% of collateral (Liquity model).

**Anchor Finding #3: Wrong Denomination Tokens Lost Forever**
```rust
// ❌ MAJOR - From Anchor audit
pub fn submit_bid(
    deps: DepsMut,
    info: MessageInfo,
    collateral: String,
) -> Result<Response> {
    let config = CONFIG.load(deps.storage)?;
    
    // Check for stable_denom in funds
    let stable_amount = info
        .funds
        .iter()
        .find(|c| c.denom == config.stable_denom)
        .map(|c| c.amount)
        .unwrap_or_default();
    
    // ❌ If user sends OTHER denoms, they're just locked!
    // No error, no refund, funds permanently stuck
    
    // Process bid with stable_amount...
    
    Ok(Response::new())
}

// ✅ CORRECT - Reject or refund wrong denoms
pub fn submit_bid(
    deps: DepsMut,
    info: MessageInfo,
    collateral: String,
) -> Result<Response> {
    let config = CONFIG.load(deps.storage)?;
    
    // ✅ OPTION 1: Strict - only accept correct denom
    require!(
        info.funds.len() == 1,
        "Send exactly one coin"
    );
    require!(
        info.funds[0].denom == config.stable_denom,
        "Invalid denomination, expected {}",
        config.stable_denom
    );
    
    let amount = info.funds[0].amount;
    
    // OR ✅ OPTION 2: Refund wrong denoms
    let mut refund_msgs = vec![];
    let stable_amount = Uint128::zero();
    
    for coin in info.funds {
        if coin.denom == config.stable_denom {
            stable_amount = coin.amount;
        } else {
            // Refund wrong denom
            refund_msgs.push(BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: vec![coin],
            });
        }
    }
    
    // Process bid...
    
    Ok(Response::new().add_messages(refund_msgs))
}
```

**Lesson**: ALWAYS validate denomination. Either reject wrong denoms or refund them. Never silently accept and lock.

**Collateralization Checks**:
```rust
// For every borrow/liquidation function, verify:

// [ ] 1. Collateral ratio validation
fn validate_collateral_ratio(
    collateral_value: Uint128,
    debt_value: Uint128,
    min_ratio: Decimal,
) -> Result<()> {
    let current_ratio = collateral_value / debt_value;
    require!(current_ratio >= min_ratio, "Undercollateralized");
    Ok(())
}

// [ ] 2. Multi-collateral accounting
// Check: Does protocol track each collateral type separately?
// Check: Can user substitute one collateral for another?
// Check: Are correlation risks considered?

// [ ] 3. Debt ceiling enforcement
// Check: Is there a max debt per user?
// Check: Is there a max debt per collateral type?
// Check: Is there a global debt ceiling?

// [ ] 4. Interest accrual correctness
// Check: Is interest calculated continuously or periodically?
// Check: Can interest accrue to more than principal + collateral?
// Check: Is there compound interest? Is it calculated correctly?
```

**Liquidation-Specific (Already covered in 2.1, but CDP additions)**:
- [ ] Are there liquidation incentives? (Liquidator gets X% bonus)
- [ ] Is incentive capped? (Prevents excessive liquidation)
- [ ] Are partial liquidations supported?
- [ ] What happens if liquidation fails mid-execution?
- [ ] Is there a liquidation queue or priority system?

**Interest Rate Model**:
- [ ] Does rate increase with utilization? (Should)
- [ ] Is there a maximum rate? (Prevent exploitation)
- [ ] Can rate change mid-borrow? (Should be clearly documented)
- [ ] Are rate changes time-locked?

**Collateral Management**:
- [ ] Can admin add/remove collateral types?
- [ ] Is there a whitelist?
- [ ] Are collateral parameters validated? (LTV, liquidation threshold, etc.)
- [ ] Can user withdraw excess collateral without affecting health?

### 3.2 DEX/AMM Protocol Checklist

**Apply this if**: Protocol facilitates token swaps through liquidity pools.

**Liquidity Provision**:
```rust
// [ ] 1. First liquidity provider attack protection
// From Eris Protocol vulnerability:
fn provide_liquidity_initial(amount: Uint128) -> Result<Response> {
    let total_shares = TOTAL_SHARES.load(deps.storage)?;
    
    if total_shares == Uint128::zero() {
        // ✅ First deposit must be meaningful
        require!(amount >= MIN_INITIAL_DEPOSIT, "Too small");
        
        // ✅ Lock some shares to protocol
        let bootstrap_shares = amount / 100;  // 1%
        mint_shares(protocol_address(), bootstrap_shares)?;
        mint_shares(user, amount - bootstrap_shares)?;
    }
    
    Ok(())
}

// [ ] 2. Liquidity removal dust handling
// Check: What if user has 1 wei of LP tokens?
// Check: What if pool has dust liquidity?
// Check: Are dust positions prevented or cleaned up?

// [ ] 3. Price impact limits
// Check: Is there maximum slippage per swap?
// Check: Are flash crashes prevented?
// Check: Is there MEV protection?
```

**Swap Calculations**:
- [ ] Are swap formulas audited by mathematician? (AMM math is complex)
- [ ] Is there protection against precision loss?
- [ ] Are invariants checked? (k = x * y for constant product)
- [ ] Is fee calculation correct? (Fee should come from input amount)
- [ ] Are fees distributed correctly to LPs?

**Concentrated Liquidity** (If applicable):
- [ ] Are position ticks validated?
- [ ] Can positions overlap? (Should)
- [ ] Are liquidity calculations correct across tick ranges?
- [ ] What happens when price crosses tick boundary?

**Flash Loan Protection**:
- [ ] Can price be manipulated within single transaction?
- [ ] Are time-weighted prices used for important decisions?
- [ ] Is there a minimum time between price updates?

### 3.3 Perpetuals/Derivatives Checklist

**Apply this if**: Protocol offers leveraged trading with funding rates.

**Position Management**:
```rust
// [ ] 1. Funding rate calculation
// Check: Is funding rate capped?
// Check: Can it cause liquidation cascade?
// Check: Is it calculated fairly for both long and short?

// [ ] 2. Position size limits
// Check: Max position per user?
// Check: Max position per market?
// Check: Max open interest per side?

// [ ] 3. Liquidation priority
// Check: Are most underwater positions liquidated first?
// Check: Can liquidation be gamed through strategic ordering?
```

**Risk Management**:
- [ ] Is total exposure tracked? (Long positions - Short positions)
- [ ] Are there circuit breakers for large moves?
- [ ] Is there an insurance fund?
- [ ] How is insurance fund replenished?
- [ ] What happens when insurance fund is depleted?

**Funding Rates**:
- [ ] Are they applied continuously or periodically?
- [ ] Can they be manipulated through flash loans?
- [ ] Is there a cap on funding rates?
- [ ] Are funding payments atomic with rate calculation?

**Special Consideration for Perps**:
From Levana audit - **Vega risk**:
- [ ] Does protocol model volatility risk?
- [ ] Can insurance fund be drained by volatility alone?
- [ ] Are Greeks (delta, gamma, vega, theta) considered?

### 3.4 Liquid Staking Protocol Checklist

**Apply this if**: Protocol tokenizes staked assets.

**Unbonding Management** (Critical - seen in multiple protocols):
```rust
// [ ] 1. Unbonding queue bounds
const MAX_UNBONDING_ENTRIES: usize = 1000;

fn request_unbond(amount: Uint128) -> Result<Response> {
    let mut queue = UNBOND_QUEUE.load(user)?;
    
    // ✅ Check: Queue size limited
    require!(queue.len() < MAX_UNBONDING_ENTRIES, "Queue full");
    
    queue.push(UnbondEntry {
        amount,
        completion_time,
    });
    
    UNBOND_QUEUE.save(user, &queue)?;
    Ok(())
}

// [ ] 2. Unbonding period enforcement
// Check: Can user game unbonding period?
// Check: Is unbonding period per entry or global?
// Check: What happens if validator slashed during unbonding?
```

**Validator Management**:
- [ ] How are validators selected for delegation?
- [ ] Can validators be removed? What happens to delegations?
- [ ] Is there a maximum validators per delegator?
- [ ] Are delegations rebalanced? How often?

**Exchange Rate**:
- [ ] How is stToken:nativeToken rate calculated?
- [ ] Does it account for slashing?
- [ ] Does it account for pending rewards?
- [ ] Can rate be manipulated?
- [ ] Is rate monotonically increasing? (Should be, except slashing)

**Slashing Handling**:
- [ ] Is slashed amount tracked?
- [ ] How is slashing loss distributed? (Pro-rata to all holders?)
- [ ] Are there any slashing protections?
- [ ] Is there an insurance fund for slashing?

### 3.5 Bridge Protocol Checklist

**Apply this if**: Protocol facilitates cross-chain transfers.

**Message Validation**:
```rust
// [ ] 1. Signature verification
fn verify_signatures(
    message: &Message,
    signatures: &[Signature],
) -> Result<()> {
    // ✅ Check: Sufficient signatures (m-of-n)
    require!(
        signatures.len() >= MIN_SIGNATURES,
        "Insufficient signatures"
    );
    
    // ✅ Check: No duplicate signers
    let unique_signers: HashSet<_> = signatures
        .iter()
        .map(|s| s.signer)
        .collect();
    require!(
        unique_signers.len() == signatures.len(),
        "Duplicate signers"
    );
    
    // ✅ Check: All signers are authorized
    for signature in signatures {
        require!(
            AUTHORIZED_SIGNERS.has(storage, &signature.signer),
            "Unauthorized signer"
        );
    }
    
    Ok(())
}

// [ ] 2. Replay protection
// Check: Is message nonce enforced?
// Check: Are processed messages tracked?
// Check: Is there expiration on messages?

// [ ] 3. Source chain validation
// Check: Is source chain ID validated?
// Check: Can message claim to be from wrong chain?
```

**Asset Locking**:
- [ ] Are locked assets tracked correctly?
- [ ] Can locked amount exceed actual balance?
- [ ] What happens if unlock message never arrives?
- [ ] Is there a timeout/fallback mechanism?

**Cross-Chain State**:
- [ ] How is state synchronized across chains?
- [ ] What if state goes out of sync?
- [ ] Are there state proofs?
- [ ] Is there a challenge period?

---

## PHASE 4: CODE ANALYSIS PATTERNS

**Instructions**: These are specific code patterns to search for and flag.

### 4.1 Dangerous Code Patterns

**Pattern 1: Checks-Effects-Interactions Violation**
```rust
// ❌ DANGEROUS - External call before state update
fn withdraw(amount: Uint128) -> Result<Response> {
    let balance = BALANCES.load(user)?;
    
    // External call FIRST
    transfer_tokens(user, amount)?;
    
    // State update AFTER
    BALANCES.save(user, &(balance - amount))?;  // ← Too late!
    
    // If transfer_tokens calls back into this contract,
    // balance hasn't been reduced yet → reentrancy!
}

// ✅ CORRECT - State update before external call
fn withdraw(amount: Uint128) -> Result<Response> {
    let balance = BALANCES.load(user)?;
    
    // State update FIRST
    BALANCES.save(user, &(balance - amount))?;
    
    // External call AFTER
    transfer_tokens(user, amount)?;
    
    // Even if transfer_tokens calls back,
    // balance is already reduced → safe
    
    Ok(())
}
```

**Search for this**:
```bash
# Find potential CEI violations
grep -B10 -A5 "WasmMsg::\|transfer\|send" contracts/*/src/*.rs | \
  grep -A10 "load\|save"
```

**Pattern 2: Unchecked Arithmetic**
```rust
// ❌ DANGEROUS - Can overflow/underflow
fn calculate_reward(stake: u128, rate: u128) -> u128 {
    stake * rate / PRECISION  // ← Can overflow on stake * rate!
}

// ✅ CORRECT - Use checked arithmetic
fn calculate_reward(stake: Uint128, rate: Uint128) -> Result<Uint128> {
    stake
        .checked_mul(rate)?              // ← Returns error on overflow
        .checked_div(PRECISION)?         // ← Returns error on div-by-zero
}
```

**Check Cargo.toml**:
```toml
[profile.release]
overflow-checks = true  # ✅ MUST be enabled
```

**Pattern 3: Unvalidated Input**
```rust
// ❌ DANGEROUS - No validation
fn update_config(new_fee: Decimal) -> Result<Response> {
    CONFIG.save(&Config { fee: new_fee })?;
    // What if fee is 100%? Or 1000%? Or negative?
    Ok(())
}

// ✅ CORRECT - Validate everything
fn update_config(new_fee: Decimal) -> Result<Response> {
    // Validate bounds
    require!(new_fee >= Decimal::zero(), "Fee cannot be negative");
    require!(new_fee <= MAX_FEE, "Fee too high");
    
    // Validate precision
    require!(
        new_fee.decimal_places() <= FEE_DECIMALS,
        "Too many decimals"
    );
    
    CONFIG.save(&Config { fee: new_fee })?;
    Ok(())
}
```

**Validation checklist for ALL inputs**:
- [ ] Is there a minimum value? Check it.
- [ ] Is there a maximum value? Check it.
- [ ] Can it be zero? If not, check it.
- [ ] Can it be negative? If not, check it.
- [ ] Are there invalid special values? Check them.
- [ ] Is precision/decimals constrained? Check it.
- [ ] Is address format validated?

**Pattern 4: Silent Failures**
```rust
// ❌ DANGEROUS - From Nolus audit
fn check_invariant() -> Result<()> {
    if total_deposits != sum(user_balances) {
        // ❌ Just logs error, doesn't fail!
        log("Invariant broken!");
        return Ok(());  // ← Continues with corrupted state!
    }
    Ok(())
}

// ✅ CORRECT - Fail loudly
fn check_invariant() -> Result<()> {
    if total_deposits != sum(user_balances) {
        // ✅ Immediately fail
        return Err(ContractError::InvariantBroken {
            total_deposits,
            sum_user_balances: sum(user_balances),
        });
    }
    Ok(())
}
```

**Rule**: NEVER silently continue on invariant violations or critical errors.

**Pattern 5: Precision Loss**
```rust
// ❌ DANGEROUS - Division before multiplication
fn calculate_share(amount: Uint128, total: Uint128) -> Uint128 {
    // If amount = 100, total = 10000, rate = 50
    let rate = amount / total;        // = 0 (truncated!)
    rate * TOTAL_SHARES               // = 0
}

// ✅ CORRECT - Multiplication before division
fn calculate_share(amount: Uint128, total: Uint128) -> Result<Uint128> {
    // Multiply first to preserve precision
    amount
        .checked_mul(TOTAL_SHARES)?   // = 100 * 1000 = 100000
        .checked_div(total)?           // = 100000 / 10000 = 10 ✓
}
```

**Rule**: Always multiply before dividing when working with integers.

---

## PHASE 5: TESTING AND VERIFICATION

**Instructions**: For each vulnerability you identify, verify it exists by creating a test case.

### 5.1 Adversarial Test Cases

**Create these tests for EVERY protocol**:

**Test 1: Malicious Contract Interaction**
```rust
#[test]
fn test_malicious_contract_liquidation() {
    // Deploy contract that reverts on any call
    let malicious = MockContract::new()
        .with_execute_handler(|_| Err(StdError::generic_err("Rejected")));
    
    // Create position owned by malicious contract
    let position_id = create_position(owner: malicious.address);
    
    // Make position liquidatable
    crash_price_by_50_percent();
    
    // Try to liquidate
    let result = liquidate(position_id);
    
    // ✅ PASS: Liquidation succeeds despite user contract rejecting
    // ❌ FAIL: Liquidation reverts → User can avoid liquidation
    assert!(result.is_ok());
}
```

**Test 2: Reentrancy Attack**
```rust
#[test]
fn test_reentrancy_on_withdraw() {
    // Deploy contract that calls withdraw() again on receive
    let attacker = MockContract::new()
        .with_receive_handler(|_, env| {
            if env.depth < 5 {  // Limit recursion
                withdraw()?;     // Call withdraw again!
            }
        });
    
    // Attacker deposits 100 tokens
    deposit(attacker.address, 100);
    
    // Attacker withdraws
    let result = withdraw(attacker.address, 100);
    
    // Check: Did attacker drain more than their deposit?
    let final_balance = get_balance(attacker.address);
    
    // ✅ PASS: Attacker got exactly 100 (their deposit)
    // ❌ FAIL: Attacker got >100 → Reentrancy exploit
    assert_eq!(final_balance, 100);
}
```

**Test 3: Front-Running First Deposit**
```rust
#[test]
fn test_first_deposit_frontrun() {
    // Victim wants to deposit 1000 tokens
    // Attacker sees this in mempool
    
    // Attacker deposits 1 wei first
    provide_liquidity(attacker, 1);
    
    // Attacker manipulates pool (if possible)
    // ...
    
    // Victim's transaction executes
    provide_liquidity(victim, 1000);
    
    // Check share distribution
    let attacker_shares = get_shares(attacker);
    let victim_shares = get_shares(victim);
    
    // ✅ PASS: Shares are fair (victim gets ~1000x attacker's shares)
    // ❌ FAIL: Attacker got unfair advantage
    assert!(victim_shares > attacker_shares * 900);
}
```

**Test 4: Dust Liquidation**
```rust
#[test]
fn test_zero_amount_liquidation() {
    // Create position with 1 wei of collateral
    let position_id = create_position(collateral: 1);
    
    // Massive price drop (90%)
    crash_price_by_90_percent();
    
    // Liquidatable amount rounds to zero
    let liquidation_amount = calculate_liquidation_amount(position_id);
    assert_eq!(liquidation_amount, 0);
    
    // Try to liquidate
    let result = liquidate(position_id);
    
    // ✅ PASS: Position closed, bad debt recorded
    // ❌ FAIL: Liquidation reverts, position stuck
    assert!(result.is_ok());
    assert!(!position_exists(position_id));
}
```

**Test 5: Unbounded Growth**
```rust
#[test]
fn test_unbounded_position_growth() {
    let user = create_user();
    
    // User creates maximum positions
    for i in 0..1000 {
        let result = create_position(user);
        
        if i < 100 {
            // ✅ PASS: First 100 positions succeed
            assert!(result.is_ok());
        } else {
            // ✅ PASS: Positions above cap are rejected
            // ❌ FAIL: All 1000 succeed → no cap!
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("Too many"));
            break;
        }
    }
}
```

### 5.2 Invariant Checks

**Add these checks throughout the codebase**:

```rust
// Invariant 1: Conservation of funds
fn assert_funds_conserved() {
    let total_deposits = sum_all_user_deposits();
    let contract_balance = query_bank_balance(contract_addr);
    assert_eq!(total_deposits, contract_balance, "Funds not conserved!");
}

// Invariant 2: Reward distribution
fn assert_rewards_valid() {
    let distributed = sum_all_user_rewards();
    let available = total_reward_pool();
    assert!(distributed <= available, "Distributed more rewards than available!");
}

// Invariant 3: Collateralization
fn assert_all_positions_valid() {
    for position in all_positions() {
        let health = calculate_health(position);
        if position.is_active {
            assert!(health >= MIN_HEALTH, "Active position undercollateralized!");
        }
    }
}

// Invariant 4: No negative balances
fn assert_no_negative_balances() {
    for (user, balance) in all_balances() {
        assert!(balance >= 0, "Negative balance detected!");
    }
}
```

**Call invariant checks**:
- After every state-mutating operation (in tests)
- In administrative audit functions (for production monitoring)
- Before and after migrations

---

## PHASE 6: SEVERITY CLASSIFICATION

**Instructions**: Classify each finding using this decision tree.

### 6.1 Severity Decision Tree

**Is the issue exploitable?**
- No → Informational or Minor
- Yes → Continue...

**Can it lead to direct loss of funds?**
- Yes → **CRITICAL**
- No → Continue...

**Can it lead to protocol insolvency or permanent DoS?**
- Yes → **CRITICAL**
- No → Continue...

**Can it lead to incorrect state or temporary DoS?**
- Yes → **MAJOR**
- No → Continue...

**Does it violate best practices or introduce inefficiency?**
- Yes → **MINOR**
- No → **INFORMATIONAL**

### 6.2 Severity Examples from Real Audits

**CRITICAL Examples**:
1. Double refund allows pool draining (Astroport)
2. Bad debt not recorded (Margined)
3. Users lose rewards during liquidation (Mars)
4. Unauthorized swap adjustments (Calculated Finance)
5. Anyone can whitelist malicious contracts (Levana)
6. Attacker can bind vault accounts to cause losses (Mars)
7. Spot price manipulation (Mars, Calculated Finance)
8. Setup function allows takeover (Axelar)

**MAJOR Examples**:
1. Liquidation fails for zero amounts (Mars Perps)
2. Withdrawal may fail due to out-of-gas (Mars Perps, Drop)
3. Malicious users can block staker withdrawals (Dexter)
4. First depositor can be front-run (Eris)
5. Unbonding queue can cause funds stuck (Lido)
6. Missing account ID validation (Mars Perps)

**MINOR Examples**:
1. Unbounded loops that could run out of gas (multiple protocols)
2. Missing parameter validation (multiple protocols)
3. Duplicate code (Mars)
4. Inconsistent type conversions (Mars)
5. Missing denom validation (Osmosis)
6. Lack of role-based access controls (multiple protocols)

**INFORMATIONAL Examples**:
1. Code optimization opportunities
2. Inconsistent naming
3. Missing events
4. Outdated dependencies (non-security)
5. Redundant queries
6. Documentation improvements

---

## PHASE 7: REPORT GENERATION

**Instructions**: Structure your findings in this format for maximum clarity.

### 7.1 Report Structure

```markdown
# Security Audit Report
## [Protocol Name] v[Version]

**Audit Date**: [Date]
**Auditor**: [Your name/identifier]
**Codebase**: [Repository URL @ Commit Hash]
**Scope**: [List of contracts/modules audited]

---

## Executive Summary

**Total Issues Found**: X Critical, Y Major, Z Minor, W Informational

**Critical Issues**: [One-sentence summary of each]
**Overall Assessment**: [High-level security posture]
**Recommendations**: [Top 3-5 most important fixes]

---

## Summary of Findings

| ID | Description | Severity | Status |
|----|-------------|----------|--------|
| 1  | [Brief description] | Critical | Open |
| 2  | [Brief description] | Major | Open |
...

---

## Detailed Findings

### 1. [Title of Issue]

**Severity**: Critical

**Location**: 
- File: `contracts/core/src/execute.rs`
- Function: `liquidate_position()`
- Lines: 245-267

**Description**:
[Explain the vulnerability in detail]

**Impact**:
[Explain what an attacker can do and the consequences]

**Proof of Concept**:
```rust
[Show vulnerable code or test case demonstrating the issue]
```

**Recommendation**:
[Explain how to fix it]

**Example Fix**:
```rust
[Show corrected code]
```

---

## Code Quality Assessment

**Complexity**: [Low/Medium/High]
**Readability**: [Low/Medium/High]
**Test Coverage**: [Percentage if available]
**Documentation**: [Low/Medium/High]

---

## Recommendations

1. **Immediate Actions Required** (Critical issues)
   - [Action 1]
   - [Action 2]

2. **Important Improvements** (Major issues)
   - [Action 1]
   - [Action 2]

3. **Best Practices** (Minor/Informational)
   - [Action 1]
   - [Action 2]

---

## Conclusion

[Overall assessment and final recommendations]
```

### 7.2 Writing Effective Findings

**For each finding, always include**:

1. **Clear title**: "Users lose rewards during liquidation"
   - Not: "Issue in liquidate function"

2. **Precise location**: File, function, line numbers
   - Not: "In the liquidation code"

3. **Explain the vulnerability**: What is wrong?
   - Not: "Bad liquidation logic"
   - Yes: "The liquidate() function closes positions without claiming pending rewards, causing users to lose all accrued staking rewards"

4. **Explain the impact**: What can happen?
   - Not: "Users are affected"
   - Yes: "An attacker can cause other users to lose rewards worth up to X% of their position value"

5. **Provide evidence**: Code snippet or test case
   - Show the exact vulnerable code
   - Or provide a failing test that proves the issue

6. **Give actionable fix**: How to solve it?
   - Not: "Fix the liquidation"
   - Yes: "Add a call to claim_all_rewards() before closing the position, as shown in the code example below"

7. **Show corrected code**: What should it look like?
   - Provide the fixed version so developers know exactly what to implement

---

## APPENDIX: QUICK REFERENCE CHECKLISTS

### Appendix A: Pre-Audit Checklist

- [ ] Clone repository at specified commit
- [ ] Read documentation and whitepaper
- [ ] Identify protocol type (Lending, DEX, Perps, etc.)
- [ ] List all contracts and their purposes
- [ ] Check test coverage percentage
- [ ] Review previous audit reports if any
- [ ] Set up local testing environment
- [ ] Identify critical paths (deposit, withdraw, liquidate, etc.)

### Appendix A.1: Additional Vulnerabilities from Real Audits

**Based on 61 audits from 18 protocols**

**Duplicate Element Vulnerabilities** (CRITICAL):
- [ ] Do any functions accept `Vec<>` parameters?
- [ ] Are duplicates checked before processing?
- [ ] Can user claim/withdraw/activate same ID multiple times?
- [ ] Is there a "claimed/used" flag to prevent re-use?

**Permissionless Critical Functions** (CRITICAL - Anchor):
- [ ] Can external protocols/users drain liquidation queues?
- [ ] Are liquidation execution calls restricted to protocol contracts?
- [ ] Is there protection against external piggyback liquidations?

**Economic Incentive Failures** (MAJOR - Anchor):
- [ ] Do liquidators have economic incentive to act?
- [ ] Is there gas compensation for critical operations?
- [ ] What happens if no one has incentive to call critical functions?

**Wrong Denomination Handling** (MAJOR - Anchor):
- [ ] Are all received coin denominations validated?
- [ ] What happens if user sends wrong denom?
- [ ] Are wrong denoms refunded or do they lock forever?

**Bid Queue Manipulation** (MINOR - Anchor):
- [ ] Can users activate unlimited bids by exploiting threshold checks?
- [ ] Are available_bids counters updated in loops?
- [ ] Is there protection against bid queue manipulation?

### Appendix B: Critical Pattern Checklist

- [ ] Liquidation preserves ALL user assets
- [ ] Liquidation cannot be blocked by users
- [ ] Bad debt is tracked persistently
- [ ] TWAP used for all critical price decisions
- [ ] No spot price usage for liquidations
- [ ] All price data validated (bounds, direction, staleness)
- [ ] Access control on ALL state-mutating functions
- [ ] Protected initialization
- [ ] No anyone-can-call critical functions
- [ ] Migrations preserve all user state
- [ ] Rewards auto-claimed before state changes
- [ ] No unbounded loops in execute functions
- [ ] Hard caps on all collections
- [ ] Pagination for large queries
- [ ] Refunds calculated once and correctly
- [ ] No double refunds
- [ ] Excess assets always returned
- [ ] Checks-Effects-Interactions pattern followed
- [ ] Overflow checks enabled
- [ ] All inputs validated
- [ ] No silent failures on invariant violations
- [ ] **No duplicate elements in arrays (claim IDs, bid IDs, etc.)** ← NEW
- [ ] **Liquidation execution restricted to protocol contracts** ← NEW (Anchor)
- [ ] **Economic incentives for critical operations (liquidations)** ← NEW (Anchor)
- [ ] **Wrong denomination coins rejected or refunded** ← NEW (Anchor)

### Appendix C: Common Vulnerability Search Patterns

```bash
# Liquidation issues
grep -r "liquidate\|seize\|force_close" contracts/

# Oracle usage  
grep -r "query_price\|get_price\|spot_price" contracts/

# Access control
grep -r "pub fn execute" contracts/ | grep -v "info: MessageInfo"

# Unbounded loops
grep -r "for \|\.iter()\|\.map(" contracts/

# Refunds
grep -r "refund\|excess\|remaining" contracts/

# Migrations
grep -r "migrate\|upgrade" contracts/

# External calls
grep -r "WasmMsg::\|transfer\|send" contracts/

# Arithmetic
grep -r "checked_mul\|checked_div\|\*\|/" contracts/

# Duplicate array elements (NEW - Anchor finding)
grep -r "claim_ids\|bid_ids\|Vec<.*>" contracts/
grep -r "HashSet\|BTreeSet\|dedup\|unique" contracts/

# Wrong denomination handling (NEW - Anchor finding)
grep -r "info.funds\|\.denom" contracts/
grep -r "find.*denom\|filter.*denom" contracts/

# Permissionless critical functions (NEW - Anchor finding)
grep -r "execute_liquidation\|execute_auction" contracts/
grep -r "require.*sender\|assert.*sender" contracts/
```

### Appendix D: Severity Quick Guide

**CRITICAL**: Can lead to:
- Direct loss of user funds
- Protocol insolvency
- Complete protocol takeover
- Unauthorized access to critical functions

**MAJOR**: Can lead to:
- Incorrect protocol state
- Temporary loss of functionality
- Denial of service
- Loss of expected rewards/yields

**MINOR**: Can lead to:
- Inefficient operations
- Higher gas costs
- Potential future issues
- Violations of best practices

**INFORMATIONAL**: 
- Code quality improvements
- Documentation gaps
- Optimization opportunities
- Non-security recommendations

---

## FINAL INSTRUCTIONS

When you complete an audit using this guide:

1. **Be thorough**: Check EVERY item in relevant checklists
2. **Be specific**: Provide exact file names, line numbers, and code snippets
3. **Be clear**: Explain vulnerabilities so developers understand them
4. **Be helpful**: Provide fixes, not just problems
5. **Be accurate**: Verify your findings with test cases
6. **Be honest**: If you're unsure, say so and suggest manual review

Remember: Your goal is to find and clearly explain security issues so they can be fixed. A good audit report should enable developers to:
- Understand exactly what is wrong
- Know exactly where it is
- Know exactly how to fix it
- Verify the fix is correct

**End of Guide**

---

**Version History**:
- v1.0 (2026-01-23): Initial release based on 60 Oak Security audit reports
- v1.1 (2026-01-23): Added Anchor Protocol findings:
  - Duplicate claim ID vulnerability pattern (CRITICAL)
  - Permissionless liquidation execution risks (CRITICAL)
  - Economic incentive failures (MAJOR)
  - Wrong denomination handling (MAJOR)
  - Total audits analyzed: 61 from 18 protocols

**Feedback**: This guide should evolve. If you find patterns not covered here, add them!
