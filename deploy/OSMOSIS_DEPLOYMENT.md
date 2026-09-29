# Osmosis Deployment — What Gets Set Up

This document explains what `osmosis_deploy.sh` deploys, in what order, and why.

---

## Overview

The script deploys **15 contracts** that form the Membrane protocol on Osmosis, then wires them together via UpdateConfig messages. It also creates two native token denoms (CDT and MBRN) via the token factory.

**Architecture:** The CDP is modular — split into three contracts (Collateral, Debt, Liquidation Engine) instead of a single monolithic contract. LTV Disco uses an unstaking model (RequestUnstake → CompleteUnstake) with no dispersal mechanics.

All state (code IDs, contract addresses, denoms, current step) is persisted to `deploy_state.json` so the script can resume from any failure point without re-doing completed work.

---

## Contracts Deployed (Not Included: Vesting, Mars Vault Token, Yield Arb)

| # | Contract | Purpose |
|---|----------|---------|
| 1 | **Osmosis Proxy** | Token factory wrapper — creates/manages CDT and MBRN denoms, tracks authorized minters, holds vault references |
| 2 | **Oracle** | Price feed aggregator — provides asset prices to Collateral for collateral valuation and liquidation thresholds |
| 3 | **Staking** | MBRN staking — earns protocol revenue, powers governance weight for emissions voting |
| 4 | **Collateral** | Manages collateral types, supply caps, deposits/withdrawals, dynamic LTV via Disco queries |
| 5 | **Debt** | Credit issuance, interest rates (IRM), revenue distribution, peg debt via transmuter |
| 6 | **Liquidation Engine** | Orchestrates liquidations — accrue debt → assess collateral → LQ → sell wall → bad debt check |
| 7 | **Liquidation Queue** | Dutch-auction liquidation mechanism — users bid CDT to purchase liquidated collateral at a premium |
| 8 | **LTV Disco** | LTV-based discount market — depositors earn yield by backing CDT at specific LTV tiers. Unstaking model (no dispersal) |
| 9 | **Transmuter** | CDT↔USDC swap facility — maintains the CDT peg by allowing 1:1 swaps with rate limits and fees |
| 10 | **Revenue Distributor** | Fee router — collects protocol revenue and distributes it to LTV Disco markets and other destinations |
| 11 | **Acquisition** | User acquisition via lockdrop incentives — deposits earn boosted MBRN rewards for locking CDT |
| 12 | **Points System** | Tracks user activity across the protocol (interest paid, liquidations, swaps) and distributes MBRN rewards |
| 13 | **System Discounts** | Calculates fee discounts for users based on staking duration, MBRN staked, and LTV Disco participation |
| 14 | **Emissions Voting** | On-chain parameter governance — stakers vote on points multipliers, acquisition budgets |
| 15 | **Auction** | Debt auction — when bad debt occurs, mints and sells MBRN to cover the shortfall; also routes fees with a delay window |

---

## Deployment Phases

### Phase 1: Store WASMs (Step 1)

All 15 contract binaries are uploaded to the chain via `wasm store`. Each returns a **code_id** that is saved to `deploy_state.json`. If a code_id already exists in state for a contract, the store is skipped.

**Required WASM files** (must exist in `artifacts/` before running):
- `osmosis_proxy.wasm`
- `oracle.wasm`
- `staking.wasm`
- `collateral.wasm`
- `debt.wasm`
- `liquidation_engine.wasm`
- `liq_queue.wasm`
- `ltv_disco.wasm`
- `transmuter.wasm`
- `revenue_distributor.wasm`
- `acquisition.wasm`
- `points_system.wasm`
- `system_discounts.wasm`
- `auction.wasm`
- `emissions_voting.wasm`

### Phase 2: Instantiate Contracts (Steps 2–18)

Contracts are instantiated in dependency order. Each contract's `InstantiateMsg` requires addresses of previously-deployed contracts, so order matters. Fields that reference not-yet-deployed contracts are set to `null` or use placeholder addresses and are filled in during the wiring phase.

**Circular dependency resolution:** Collateral requires `ltv_disco` (addr_validated at init), and Disco requires `cdp_contract` (addr_validated at init). Solution: Collateral is instantiated first with the deployer's address as a valid placeholder for `ltv_disco`, then updated via `UpdateConfig` after Disco is deployed (Step 10).

```
Osmosis Proxy          ← no dependencies
  ├─ Create CDT denom
  ├─ Create MBRN denom
  │
  Oracle               ← no contract deps (positions set later)
  │
  Staking              ← needs: proxy, mbrn_denom
  │
  Collateral           ← needs: oracle, proxy; ltv_disco = deployer placeholder
  │
  Debt                 ← needs: collateral, staking, proxy, oracle, cdt_denom
  │
  Liquidation Queue    ← needs: collateral, proxy
  │
  LTV Disco            ← needs: collateral, oracle, proxy, mbrn_denom, cdt_denom
  │
  [Wire Collateral]    ← set ltv_disco = real Disco addr
  │
  Liquidation Engine   ← needs: collateral, debt, proxy, staking, cdt_denom
  │
  Transmuter           ← needs: debt, cdt_denom, USDC denom
  │
  Revenue Distributor  ← needs: ltv_disco, transmuter, cdt_denom
  │
  Acquisition          ← needs: transmuter, proxy, staking, ltv_disco, cdt/mbrn denoms
  │
  Points System        ← needs: oracle, collateral, liq_queue, proxy, cdt/mbrn denoms
  │
  System Discounts     ← needs: oracle, collateral, staking, ltv_disco
  │
  Emissions Voting     ← needs: ltv_disco, staking
  │
  Auction              ← needs: oracle, proxy, debt, staking, mbrn_denom, rev_dist, ltv_disco
```

### Phase 3: Wire Configs (Steps 19–30)

After all contracts exist, UpdateConfig messages fill in the cross-references that couldn't be set at instantiation time.

| Step | Target | What Gets Wired |
|------|--------|-----------------|
| 19 | Oracle | `positions_contract` → Collateral |
| 20 | Collateral | `debt_contract` → Debt, `liquidation_contract` → Liquidation Engine |
| 21 | Debt | `debt_auction`, `discounts`, `revenue_distributor`, `transmuter`, `points`, `liquidation_contract`, `acquisition_contract` |
| 22 | Debt Basket | `liq_queue`, `negative_rates` = false, `cpc_margin_of_error` = 1% |
| 23 | Liquidation Engine | `liq_queue`, `debt_auction` |
| 24 | Staking | `positions` → Debt, `auction`, `osmosis_proxy`, `emissions_voting` |
| 25 | Transmuter | `discounts`, `revenue_distributor`, `points_system`, `retention_multiplier` = 0.5 |
| 26 | LTV Disco | `emissions_voting`, `points_system`, `revenue_distributor`, `auction`, `mbrn_denom` |
| 27 | Acquisition | `discounts_contract`, `emissions_voting` |
| 28 | Points System | `emissions_voting`, `revenue_distributor` |
| 29 | Revenue Distributor | `points_system`, `cdp` → Debt, `acquisition`, `ltv_disco`, `auction`, `dispersal_window` = 7 days |
| 30 | Osmosis Proxy | Add owners (Debt, Staking, LQ, Auction, Acquisition), set `debt_auction` and `transmuter_contract` |

### Phase 4: Create Emissions Voting Graphs (Step 31)

The emissions voting contract needs voting graphs created for each governable parameter:

| Graph Label | Type | Range | Period | Callback Contract |
|------------|------|-------|--------|-------------------|
| `interest_rate` | Decimal | 1.0–10.0 | 7 days | Points System |
| `liquidation_execution` | Decimal | 1.0–10.0 | 7 days | Points System |
| `liquidation_claims` | Decimal | 1.0–10.0 | 7 days | Points System |
| `governance_votes` | Decimal | 1.0–10.0 | 7 days | Points System |
| `transmuter_swap_fees` | Decimal | 1.0–10.0 | 7 days | Points System |
| `disco_revenue` | Decimal | 1.0–10.0 | 7 days | Points System |
| `acquisition` | Uint128 | 0–1T | 7 days | Acquisition |

Transmuter retention emissions are derived from the acquisition rate multiplied by `retention_multiplier` (0.5) — no separate voting graphs needed.

### Phase 5: Add Collateral Assets (Steps 32–35)

**Step 32 — Oracle Price Feeds:**

| Asset | Denom | Pyth Feed | TWAP Pool | Decimals |
|-------|-------|-----------|-----------|----------|
| OSMO | `uosmo` | `5867f568...` | Pool 1464 (OSMO/USDC) | 6 |
| Noble USDC | `ibc/498A07...` | `eaa020c6...` | Pool 1464 (USDC/OSMO) | 6 |
| allBTC | `factory/osmo1z6r6q.../alloyed/allBTC` | None (TWAP only) | Pool 1943 (allBTC/USDC) | 8 |
| ATOM | `ibc/27394F...` | `b00b60f8...` | Pool 1251 (ATOM/USDC) | 6 |

**Step 33 — Collateral Assets (EditBasket on Collateral contract):**

`max_borrow_LTV` is set to `0` because it is **automatically derived** by the Collateral contract as `max_LTV - ltv_borrow_distance` (default 10%).

| Asset | max_LTV (liquidation) | supply_cap_ratio | force_redemptions |
|-------|----------------------|------------------|-------------------|
| OSMO | 50% | 100% | No |
| ATOM | 55% | 100% | No |
| allBTC | 70% | 100% | No |
| Noble USDC | 96% | 100% | Yes |

**Step 34 — Liquidation Queues (AddQueue):**

| Asset | Max Premium Slots | Bid Threshold |
|-------|------------------|---------------|
| OSMO | 30 | 1T |
| ATOM | 30 | 1T |
| allBTC | 30 | 1T |
| Noble USDC | 4 | 1T |

**Step 35 — LTV Disco Queues (CreateQueue):**

Queues are created with `min_ltv` and `max_ltv`. Slots are automatically generated at 1% intervals. `min_ltv` is set 10% below the corresponding max LTV in CDP.

| Asset | min_ltv | max_ltv |
|-------|---------|---------|
| OSMO | 40% | 65% |
| ATOM | 45% | 65% |
| allBTC | 60% | 80% |
| Noble USDC | 88% | 98% |

---

## Token Denoms Created

| Token | Subdenom | Full Denom | Max Supply |
|-------|----------|------------|------------|
| **CDT** (stablecoin) | `ucdt` | `factory/{proxy}/ucdt` | Unlimited |
| **MBRN** (governance) | `umbrn` | `factory/{proxy}/umbrn` | 80,000,000 MBRN (80T uMBRN) |
| **Transmuter VT** | `usdc-transmuter` | `factory/{transmuter}/usdc-transmuter` | Created by transmuter contract |

---

## Key Parameters

| Parameter | Value | Set In |
|-----------|-------|--------|
| Oracle time limit | 600 seconds | Collateral |
| Debt minimum | 20 CDT | Debt |
| Collateral TWAP timeframe | 60 seconds | Collateral |
| Credit TWAP timeframe | 480 seconds | Debt |
| Rate slope multiplier | 1.618 | Debt |
| Base interest rate | 3% | Debt |
| IRM adjustment speed | 50 | Debt |
| IRM min adaptive rate | 0.1% | Debt |
| IRM max adaptive rate | 200% | Debt |
| LTV borrow distance | 10% (default) | Collateral |
| LQ waiting period | 60 seconds | Liquidation Queue |
| LQ minimum bid | 5 CDT | Liquidation Queue |
| LTV Disco unstaking period | 2 days (default) | LTV Disco |
| Transmuter usage fee | 0.05% | Transmuter |
| Transmuter rate limit | 5% per 8 hours | Transmuter |
| Transmuter global rate limit | 20% per 24 hours | Transmuter |
| Transmuter lock ceiling | 1460 days (~4 years) | Transmuter |
| Transmuter retention multiplier | 0.5 | Transmuter |
| System discount max | 75% | System Discounts |
| Min time in network for discount | 7 days | System Discounts |
| Acquisition deposit period | 5 days | Acquisition |
| Acquisition withdrawal period | 2 days | Acquisition |
| Acquisition max boost | 200% | Acquisition |
| Acquisition min lock | 30 days | Acquisition |
| Auction initial discount | 1% | Auction |
| Auction discount increase | 1% per 36 intervals (~100% in 1 hour) | Auction |
| Auction delay window | 60 minutes | Auction |
| Revenue dispersal window | 7 days | Revenue Distributor |
| Affiliate fee | 1% | Transmuter, LTV Disco |

---

## Excluded From This Deployment

| Contract | Reason |
|----------|--------|
| **Vesting** | No TGE recipients on Osmosis. Staking handles `vesting_contract: None` gracefully. |
| **Mars Vault Token** | Not needed for this deployment. |
| **Yield Arb** | Depends on Mars VT — excluded since Mars VT is excluded. |
| **Discount Vault** | Legacy contract, replaced by LTV Disco for discount calculations. |
| **Stability Pool** | Not used in current architecture. |
| **Liquidity Check** | Removed from current architecture. |
| **Governance** | Governance module removed — staking contract serves as governance placeholder. |

---

## Configuration

Edit the top of `osmosis_deploy.sh` to customize:

```bash
NODE="https://osmosis-rpc.polkachu.com:443"  # RPC endpoint
CHAIN_ID="osmosis-1"                          # Chain ID
WALLET="wallet"                               # osmosisd key name
ARTIFACTS_DIR="../artifacts"                   # Path to .wasm files
NOBLE_USDC_DENOM="ibc/498A..."                # Noble USDC IBC denom
```

---

## Usage

```bash
# First run — full deployment
./osmosis_deploy.sh

# Resume after failure (automatic — picks up where it left off)
./osmosis_deploy.sh

# Force restart from a specific step
./osmosis_deploy.sh --from-step 10

# Wipe state and start fresh
./osmosis_deploy.sh --reset
```

---

## State File

`deploy_state.json` tracks all deployment progress:

```json
{
  "code_ids": {
    "osmosis_proxy": "1234",
    "oracle": "1235",
    "collateral": "1236",
    "debt": "1237",
    "liquidation_engine": "1238",
    ...
  },
  "addresses": {
    "osmosis_proxy": "osmo1abc...",
    "oracle": "osmo1def...",
    "collateral": "osmo1ghi...",
    "debt": "osmo1jkl...",
    "liquidation_engine": "osmo1mno...",
    ...
  },
  "denoms": {
    "cdt": "factory/osmo1abc.../ucdt",
    "mbrn": "factory/osmo1abc.../umbrn",
    "transmuter_vt": "factory/osmo1xyz.../usdc-transmuter"
  },
  "step": 36
}
```

**Never delete this file** unless you want to start over. The script uses it to skip already-completed steps.

---

## Post-Deployment Checklist

The script handles contract deployment, wiring, oracle feeds, collateral setup, liquidation queues, and disco queues. After it completes, you may still need to:

- [ ] **Fund the acquisition contract** with MBRN for lockdrop incentives (mint via proxy)
- [ ] **Configure transmuter revenue distributions** (which assets revenue is converted to)
- [ ] **Set revenue distributor promises** (LTV Disco distribution map)
- [ ] **Verify all contracts** by querying configs and checking addresses match
