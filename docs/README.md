# Membrane Protocol

Membrane is a modular CDP stablecoin protocol built on Cosmos, deployed on Osmosis and Neutron. Users deposit collateral to mint **CDT**, a pegged stablecoin, while **MBRN** serves as the governance and utility token that backstops the system.

## Core Tokens

| Token | Role | Mechanism |
|-------|------|-----------|
| **CDT** | Pegged stablecoin | Minted against collateral in CDPs, redeemable via Transmuter at 1:1 with USDC |
| **MBRN** | Governance & backstop | Staked for governance revenue, deposited in Disco for risk-weighted yield, slashed to cover bad debt |

## Organ Architecture

Membrane's contracts are organized into functional **organs** -- groups of contracts that work together to serve a specific system function.

| Organ | Purpose | Key Contracts |
|-------|---------|---------------|
| [**CDP**](organs/cdp/README.md) | Lending & borrowing | Collateral, Debt, Liquidation Engine, Liq Queue |
| [**Transmuter**](organs/transmuter/README.md) | CDT peg maintenance | Transmuter (1:1 vault swap) |
| [**LTV Disco**](organs/ltv-disco/README.md) | Risk-weighted MBRN yield | LTV Disco, Emissions Voting |
| [**Acquisition**](organs/acquisition/README.md) | User growth via adaptive MBRN emission windows | Acquisition |
| [**Governance**](organs/governance/README.md) | Staking, voting, vesting | Staking, Governance, Vesting |
| [**Revenue**](organs/revenue/README.md) | Revenue collection & distribution | Revenue Distributor, Auction |

### Shared Contracts

| Contract | Used By |
|----------|---------|
| [Oracle](shared/oracle.md) | CDP, Transmuter, LTV Disco, Auction |
| [System Discounts](shared/system-discounts.md) | CDP, Acquisition, Transmuter |
| [Points System](shared/points-system.md) | CDP, Transmuter, LTV Disco, Staking, Governance |

### Infrastructure

| Contract | Purpose |
|----------|---------|
| [Osmosis Proxy](infrastructure/osmosis-proxy.md) | Token factory (CreateDenom, Mint, Burn), DEX swaps, gauge creation on Osmosis |
| [Neutron Proxy](infrastructure/neutron-proxy.md) | Token factory, Astroport/Duality swaps, multi-hop routing on Neutron |
| [Discount Vault](infrastructure/discount-vault.md) | LP token deposits for time-based discount credit |

## System Diagram

```
                    +----------------------------------------------+
                    |              USERS                            |
                    +------+----------+--------------+-------------+
                           |          |              |
                    Deposit/Borrow  Swap CDT<>USDC  Deposit MBRN
                           |          |              |
                +----------v--+  +----v-------+  +--v-----------+
                |  Collateral  |  | Transmuter |  |  LTV Disco   |
                |    + Debt    |  | (1:1 vault)|  | (risk slots) |
                +------+------+  +-----+------+  +------+-------+
                       |               |                 |
                       v               v                 v
                +------+------+  +-----+------+  +------+-------+
                | Liquidation |  |  Revenue   |  |  Emissions   |
                |   Engine    |  | Distributor|  |   Voting     |
                +------+------+  +-----+------+  +--------------+
                       |               |
                       v               v
                +------+------+  +-----+------+
                |  Liq Queue  |  |  Staking   |
                | (stability) |  | (fee share)|
                +--------------+  +-----+------+
                                        |
                                        v
                                 +------+------+
                                 | Governance  |
                                 +--------------+
```

## Cross-Organ Flows

These documents describe the actual message chains and SubMsg reply sequences that connect organs:

| Flow | Description |
|------|-------------|
| [Revenue Flow](cross-organ/revenue-flow.md) | CDP interest accrual -> TakeRevenue -> SetPromises -> DistributePromises -> Staking/Disco/Affiliates |
| [Liquidation Flow](cross-organ/liquidation-flow.md) | 9-step SubMsg reply chain: Debt accrue -> Collateral assess -> Fee transfer -> LQ -> Sell wall -> Debt update -> Claim update -> Bad debt |
| [Acquisition to Disco](cross-organ/acquisition-to-disco-flow.md) | Deposit USDC -> Transmuter EnterVault -> Adaptive pool accrual -> Mint MBRN -> Disco deposit with clawback |
| [Flywheel](cross-organ/flywheel.md) | How the system components create self-reinforcing growth: product wedges, market position, and growth strategy |

## Key Formulas

### Interest Accrual (Debt Contract)

```
accumulated_interest = principal * rate * elapsed / SECONDS_PER_YEAR
```

Stored as `pending_revenue` in the basket. Collected via `TakeRevenue`.

### Liquidation Threshold

```
immediate_threshold = avg_max_ltv * (1 + avg_max_threshold_to_delay)
```

- Above threshold: liquidate immediately
- Below threshold: start timer (configurable delay, default tuned per analysis in `docs/liquidation_delay_window.md`)
- Timer expired: full liquidation proceeds

### Liquidation Fees

```
caller_fee_rate  = current_ltv - avg_max_ltv   (dynamic, scales with insolvency depth)
protocol_fee_rate = config.liq_fee              (static)
```

Fee cap: if `(caller_fee + protocol_fee) * repay_value > collateral_value`, then `caller_fee = min(1%, collateral_value / repay_value)` and `protocol_fee = 0`.

### Transmuter Fee Split

```
usage_fee = 1% (default, on all non-allowlisted swaps)
revenue_distributor_fee_percentage = 20% (default)
LP share = 80% (stays in contract, accrues to VT holders via rate appreciation)
```

### LTV Disco Revenue Weight

```
weight(slot) = 10^18 / isqrt(deposits_above + avg_slot_size)
```

Where `deposits_above` is the cumulative MBRN in higher-LTV (riskier) slots that buffer a given slot. Riskier slots receive proportionally more revenue per unit deposited.

### System Discounts Time Curve

```
discount = first_month_discount * min(effective_days, 30) / 30
         + remaining_discount * min(max(effective_days - 30, 0), remaining_duration) / remaining_duration

effective_days = days_since_deposit + lock_duration_days
```

### Intent Boosts (Acquisition)

```
boost = min(lock_duration_days / lock_ceiling, 1.0) * max_boost
```

Where `lock_ceiling` is queried from the target contract (Staking or LTV Disco config). `UserBoost` is currently disabled (hardcoded to zero).
