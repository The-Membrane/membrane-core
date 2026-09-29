# marsUSDC Liquidation Flow Documentation

## Table of Contents
1. [Overview](#overview)
2. [Bad Debt Accounting](#bad-debt-accounting)
3. [Liquidation Flow Overview](#liquidation-flow-overview)
4. [Branching Scenarios](#branching-scenarios)
   - [Scenario A: Liquidation Queue Path](#scenario-a-liquidation-queue-path)
   - [Scenario B: Direct Liquidation via Neutron Proxy](#scenario-b-direct-liquidation-via-neutron-proxy)
5. [Vault Token Handling](#vault-token-handling)
6. [Technical Details](#technical-details)

---

## Overview

**marsUSDC** is a Mars Vault Token that represents USDC deposits in the Mars Redbank protocol. When a CDP position using marsUSDC as collateral becomes insolvent (exceeds its maximum LTV), it can be liquidated through multiple pathways. This document explains the complete liquidation flow, including how bad debt is accounted for and how vault tokens are handled in each scenario.

### Key Components

- **mars-vault-token contract**: Manages vault token minting/burning and Mars Redbank deposits
- **CDP contract**: Manages positions and orchestrates liquidations
- **Liquidation Queue (LQ)**: First-layer liquidation mechanism with premium slots
- **Neutron Proxy**: Handles vault token redemption and token swaps

---

## Bad Debt Accounting

### Overview

The mars-vault-token contract implements a bad debt accounting mechanism to protect vault token holders from Mars Redbank insolvency. This ensures that if Mars Redbank loses funds (due to exploits, bad debt, or other issues), all vault token holders share the loss proportionally rather than creating a race condition.

### Mechanism

The bad debt accounting is performed in the `get_total_deposit_tokens()` function:

```rust
fn get_total_deposit_tokens(
    deps: Deps,
    env: Env,
    config: Config,
) -> StdResult<Uint128>
```

#### Step-by-Step Bad Debt Calculation

1. **Query Vault's Collateral in Mars Redbank**
   - Queries `UserCollateral` from Mars Redbank for the vault contract's address
   - Returns the vault's deposited USDC amount in Mars Redbank

2. **Query Mars Redbank Market State**
   - Queries `MarketV2` from Mars Redbank
   - Retrieves:
     - `collateral_total_amount`: Total USDC collateral in the market
     - `debt_total_amount`: Total debt against that collateral

3. **Calculate Expected Balance**
   ```
   expected_balance = collateral_total_amount - debt_total_amount
   ```
   This represents what the balance *should* be if Mars Redbank is solvent.

4. **Query Actual Balance**
   - Queries the actual USDC balance held by Mars Redbank contract
   - This is the *real* amount available for withdrawal

5. **Calculate Discount Ratio**
   ```
   discount_ratio = actual_balance / expected_balance
   ```
   - If `discount_ratio < 1.0`: Mars Redbank is insolvent (bad debt exists)
   - If `discount_ratio = 1.0`: Mars Redbank is fully solvent
   - If `discount_ratio > 1.0`: Should not occur (indicates query error)

6. **Apply Discount to Vault's Collateral**
   ```
   discounted_total = vault_collateral_amount * discount_ratio
   ```
   This discounted amount is used for all vault token calculations.

### Impact on Liquidations

When marsUSDC is liquidated:
- The vault token amount is converted to underlying USDC using the **discounted** total deposit tokens
- This ensures liquidators receive the correct amount accounting for any bad debt
- All vault token holders share the loss proportionally

### Example

**Scenario**: Mars Redbank has bad debt
- Expected balance: 1,000,000 USDC
- Actual balance: 950,000 USDC
- Discount ratio: 0.95 (5% bad debt)
- Vault has 100,000 USDC deposited
- Discounted value: 95,000 USDC

If a user holds 10% of vault tokens:
- Without discount: Would receive 10,000 USDC
- With discount: Receives 9,500 USDC (shares in the 5% loss)

---

## Liquidation Flow Overview

When a CDP position becomes insolvent, the liquidation process follows this general flow:

1. **Insolvency Check**: CDP verifies position exceeds max LTV
2. **Repayment Calculation**: Calculates how much debt needs to be repaid
3. **Liquidation Attempts**: Tries multiple liquidation pathways in order:
   - Liquidation Queue (if configured)
   - Direct liquidation (via proxy/swap)
4. **Collateral Distribution**: Distributes liquidated collateral to liquidators
5. **Bad Debt Check**: After liquidation, checks if position still has bad debt
6. **Position Update**: Updates or closes the position

---

## Branching Scenarios

The liquidation flow branches based on:
- Whether a Liquidation Queue is configured for the collateral asset
- Whether the Liquidation Queue has sufficient bids
- Whether direct liquidation is needed

### Decision Tree

```
Position Liquidated
    │
    ├─→ Liquidation Queue Exists?
    │   │
    │   ├─→ YES → Query LQ for liquidatable amount
    │   │   │
    │   │   ├─→ LQ Can Liquidate Full Amount?
    │   │   │   │
    │   │   │   ├─→ YES → Liquidate via LQ
    │   │   │   │   └─→ Complete
    │   │   │   │
    │   │   │   └─→ NO → Liquidate Partial via LQ
    │   │   │       └─→ Continue to Direct Liquidation
    │   │   │
    │   │   └─→ NO → Continue to Direct Liquidation
    │   │
    │   └─→ NO → Continue to Direct Liquidation
    │
    └─→ Direct Liquidation (via Proxy/Swap)
        └─→ Swap collateral to CDT and burn debt
```

---

## Scenario A: Liquidation Queue Path

### Overview

The Liquidation Queue is the first layer of liquidation. Users bid CDT (credit tokens) at various premium levels to liquidate specific collateral types. When a position is liquidated, the LQ attempts to liquidate as much as possible, starting from the lowest premium slots.

### Step-by-Step Flow

#### Phase 1: Initial Liquidation Call

1. **Liquidator calls CDP::Liquidate**
   - Parameters: `position_id`, `position_owner`
   - CDP performs insolvency check
   - Calculates repayment amount needed

2. **CDP Queries Liquidation Queue**
   - For each collateral asset (including marsUSDC):
     - Calls `LQ::CheckLiquidatible` query
     - Parameters:
       - `bid_for`: marsUSDC asset info
       - `collateral_price`: Current price of marsUSDC
       - `collateral_amount`: Amount of marsUSDC to liquidate
       - `credit_info`: CDT asset info
       - `credit_price`: Current price of CDT

3. **LQ Response**
   - Returns:
     - `total_debt_repaid`: How much CDT debt can be repaid
     - `leftover_collateral`: Collateral that couldn't be liquidated
   - If LQ has insufficient bids, returns error (handled gracefully)

#### Phase 2: Liquidation Queue Execution

4. **CDP Sends Liquidation Message to LQ**
   - Creates `SubMsg` with `LIQ_QUEUE_REPLY_ID`
   - Message: `LQ::Liquidate`
   - Includes collateral amount to liquidate
   - **Note**: At this point, marsUSDC is still a vault token

5. **LQ Processes Liquidation**
   - Iterates through premium slots (lowest to highest)
   - For each slot:
     - Calculates how much collateral can be liquidated
     - Distributes collateral pro-rata to bidders in that slot
     - Burns CDT from bidders' deposits
   - Returns:
     - `repay_amount`: Total CDT debt repaid
     - `collateral_amount`: Total collateral liquidated
     - `collateral_token`: Token address/denom
     - `collateral_info`: "token" or "native"

6. **CDP Reply Handler Processes LQ Response**
   - Handler: `handle_liq_queue_reply()`
   - Extracts `repay_amount` and `collateral_amount` from events
   - Updates position:
     - Reduces `credit_amount` by `repay_amount`
     - Reduces collateral amount by `collateral_amount`
   - **Sends marsUSDC to Liquidation Queue**
     - Creates withdrawal message
     - Sends vault tokens to LQ contract address

#### Phase 3: Liquidation Queue Receives Vault Tokens

7. **LQ Receives marsUSDC Vault Tokens**
   - LQ now holds marsUSDC tokens
   - LQ distributes tokens to bidders pro-rata based on their bids
   - Bidders receive marsUSDC vault tokens as rewards

8. **Bidders Can Redeem Vault Tokens**
   - Bidders can:
     - Hold marsUSDC (exposed to Mars Redbank risk)
     - Send to Neutron Proxy to exit and swap to USDC/CDT
     - Send directly to mars-vault-token contract to exit

#### Phase 4: Leftover Handling

9. **Leftover Collateral**
   - If LQ couldn't liquidate all collateral:
     - Remaining collateral proceeds to direct liquidation (via proxy/swap)

---

## Scenario B: Direct Liquidation via Neutron Proxy

### Overview

When the Liquidation Queue cannot fully liquidate a position (or doesn't exist), or when a liquidator directly sends marsUSDC to the Neutron Proxy, the proxy handles the vault token redemption and swapping.

### Step-by-Step Flow

#### Phase 1: Vault Token Detection

1. **marsUSDC Sent to Neutron Proxy**
   - User/liquidator sends marsUSDC to proxy
   - Calls `NeutronProxy::SwapTokens` or similar function
   - Parameters:
     - `token_out`: Target token (usually CDT or USDC)
     - `max_slippage`: Maximum acceptable slippage

2. **Proxy Detects Vault Token**
   - Proxy checks if received token is in `config.vaults` list
   - Finds matching vault configuration:
     ```rust
     let vault_for_token = config.vaults
         .iter()
         .find(|v| v.vault_token == coin.denom);
     ```

#### Phase 2: Vault Token Exit

3. **Proxy Calls ExitVault**
   - Creates `SubMsg` with `USE_BALANCE_SWAP_REPLY_ID`
   - Message: `mars_vault_token::ExecuteMsg::ExitVault`
   - Sends marsUSDC tokens as funds
   - **This is where bad debt accounting happens**

4. **mars-vault-token Processes Exit**
   - Receives marsUSDC vault tokens
   - Calculates underlying USDC using `get_total_deposit_tokens()`
     - Queries Mars Redbank for bad debt
     - Applies discount ratio if bad debt exists
   - Calculates USDC to withdraw:
     ```
     usdc_amount = (vault_tokens / total_vault_tokens) * discounted_total_deposit_tokens
     ```
   - Burns marsUSDC vault tokens
   - Withdraws USDC from Mars Redbank
   - Sends USDC to `info.sender` (which is Neutron Proxy when called by proxy)

#### Phase 3: Swap Processing

5. **Proxy Reply Handler Receives USDC**
   - Handler: `handle_swap_balances_reply()`
   - Queries contract balance
   - Receives USDC from vault exit

6. **Proxy Checks Swap Type**
   - If USDC → CDT: Uses Transmuter
   - If USDC → Other: Uses DEX swap route

7. **Transmuter Path (USDC → CDT)**
   - If swapping to CDT:
     - Calls `Transmuter::Transmute`
     - Sends USDC to transmuter
     - Transmuter converts USDC to CDT at 1:1 ratio
     - Returns CDT to proxy

8. **DEX Swap Path (USDC → Other Token)**
   - If swapping to other token:
     - Queries swap route configuration
     - Executes swap via configured DEX
     - Returns swapped tokens to proxy

9. **Proxy Sends Final Tokens**
   - After swap completes:
     - Sends final tokens (CDT or other) to original caller
     - Completes the liquidation/swap

---

## Vault Token Handling

### In Liquidation Queue

**When LQ Receives marsUSDC:**
- LQ holds vault tokens
- Distributes to bidders pro-rata
- Bidders receive marsUSDC directly
- **Bad debt is NOT accounted for at this stage**
- Bad debt is only accounted for when bidders exit the vault

**Bidder Options:**
1. **Hold marsUSDC**: Exposed to Mars Redbank risk, earns yield
2. **Exit via Proxy**: Send to Neutron Proxy → Exit → Swap
3. **Exit Directly**: Send to mars-vault-token → Exit → Receive USDC

### In Direct Liquidation

**When Proxy Receives marsUSDC:**
- Proxy immediately exits vault
- Bad debt IS accounted for during exit
- Liquidator receives USDC (already discounted)
- Then swaps to target token

**Key Difference:**
- LQ path: Bad debt accounted for later (when holder exits)
- Direct path: Bad debt accounted for immediately (during liquidation)

---

## Technical Details

### Bad Debt Query Implementation

The bad debt accounting uses these Mars Redbank queries:

1. **UserCollateral Query**
   ```rust
   Mars_QueryMsg::UserCollateral {
       user: vault_contract_address,
       account_id: None,
       denom: "usdc" // or configured deposit token
   }
   ```

2. **MarketV2 Query**
   ```rust
   Mars_QueryMsg::MarketV2 {
       denom: "usdc"
   }
   ```

3. **Balance Query**
   ```rust
   querier.query_balance(mars_redbank_address, "usdc")
   ```

### Vault Token Exit Process

When exiting vault tokens, the contract:

1. Loads total vault tokens from state
2. Queries discounted total deposit tokens (accounts for bad debt)
3. Calculates withdrawal amount:
   ```rust
   deposit_tokens_to_withdraw = calculate_base_tokens(
       vault_tokens,
       total_deposit_tokens,  // Already discounted
       total_vault_tokens
   )
   ```
4. Burns vault tokens
5. Withdraws from Mars Redbank
6. Sends USDC to caller

### Neutron Proxy Vault Detection

The proxy maintains a list of vault configurations:

```rust
pub struct VaultConfig {
    pub vault_token: String,      // marsUSDC denom
    pub vault_addr: Addr,         // mars-vault-token contract
}
```

When receiving tokens, it checks:
```rust
config.vaults.iter().find(|v| v.vault_token == coin.denom)
```

### Reply ID Constants

- `LIQ_QUEUE_REPLY_ID`: Used when LQ processes liquidation
- `USE_BALANCE_SWAP_REPLY_ID`: Used when proxy exits vault tokens
- `SWAP_REPLY_ID`: Used for DEX swaps
- `BAD_DEBT_REPLY_ID`: Used for bad debt checks

### Error Handling

**Liquidation Queue Errors:**
- If LQ query fails: Gracefully continues to direct liquidation
- If LQ execution fails: Reply handler logs error, continues to direct liquidation

**Vault Exit Errors:**
- If Mars Redbank query fails: Returns error, transaction reverts
- If withdrawal fails: Returns error, transaction reverts

**Swap Errors:**
- If swap fails: Returns error to caller
- Slippage protection: Reverts if slippage exceeds `max_slippage`

---

## Summary

The marsUSDC liquidation flow is designed to:

1. **Account for bad debt**: Uses Mars Redbank queries to detect and account for insolvency
2. **Support multiple paths**: LQ → Direct liquidation
3. **Handle vault tokens**: Automatically exits vault tokens when sent to proxy
4. **Fair distribution**: Bad debt losses shared proportionally among vault token holders

The key insight is that bad debt accounting happens at different stages depending on the liquidation path:
- **LQ path**: Bad debt accounted for when holder exits vault (later)
- **Direct path**: Bad debt accounted for immediately during liquidation

This design ensures that liquidators and vault token holders are always aware of the true value of their assets, accounting for any risks in the underlying Mars Redbank protocol.

