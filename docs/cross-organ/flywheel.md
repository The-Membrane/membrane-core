# Economic Flywheel and Market Position

How Membrane creates a competitive product for borrowers, attracts risk managers to price protocol parameters, and compounds user ownership over time.

## How Membrane Succeeds

Membrane succeeds by building a better product for **borrowers** — the paying users of any CDP system. Two mechanisms create the borrower wedge:

1. **Delayed liquidations** protect users from wick liquidations. Positions near the liquidation boundary receive a configurable grace period (default 8 hours, empirically tuned per asset). This makes wick hunting expensive — an attacker must sustain the price depression for the full delay window, not just a single block. The `immediate_threshold` parameter (`avg_max_ltv * (1 + avg_max_threshold_to_delay)`) separates "slightly undercollateralized" from "severely undercollateralized," applying the delay only where it helps honest borrowers without protecting genuinely insolvent positions.

2. **Acquisition dampens rate spikes.** When the MBRN emission pool is maxed and Transmuter utilization is high, the `bump_rate` mechanism gradually increases peg debt rates — but only on USDC (peg) debt, and only by `bump_increment` per 4.8-hour tick. Below target utilization, the bump decreases at 2x speed. This creates a smooth rate curve instead of sudden interest rate jumps. Combined with System Discounts (time-curve discounts that reward loyalty: 45% in month 1, 75% by month 3), long-term borrowers pay progressively less.

The result: Membrane borrowers face lower liquidation risk at high LTVs and more predictable borrowing costs than any other CDP system.

## The Risk Manager Marketplace

LTV Disco creates a marketplace for risk management. Instead of governance committees setting LTV parameters, Membrane pays a diverse set of risk managers to do it:

**How it works:**
- MBRN holders deposit into asset queues at chosen LTV slots (50-90%)
- Revenue is distributed by inverse-sqrt weighting — riskier slots earn more
- The weighted average LTV across Disco deposits feeds back to the CDP Collateral contract's dynamic LTV parameters
- Disco MBRN is the protocol's **first line of defense** for bad debt: slashed from the riskiest slots first via `SendMBRNForSale`

**Two participation modes:**
- **Managers**: Set a fee (up to 5%), manage others' deposits between slots. They earn fees for active risk management without needing to own MBRN themselves.
- **Direct depositors**: Own their MBRN, choose their own risk tier, earn revenue directly.

**Incentive alignment**: Disco depositors earn the most revenue when they correctly price risk. If they choose an LTV slot that's too aggressive and the asset defaults, they're slashed first. If they choose correctly, they earn outsized yield via the inverse-sqrt weighting formula. This is insurance underwriting, not governance voting.

## Points as Gradual Ownership

The Points System rewards all protocol users, gradually giving them increased stake:

- Every protocol action earns points: repaying debt, claiming Disco revenue, transmuting, voting, liquidating
- Points convert to MBRN at `1 MBRN per point`, and claimed MBRN is **automatically staked** (not sent as liquid tokens)
- Governance votes earn 3x multiplier — active governance participants accumulate faster
- Management points (5 per action) reward volatility-window debt management

**Lenders get the bulk via Acquisition.** The acquisition model's MBRN emission windows are the primary MBRN distribution mechanism. Users who deposit USDC (the core lending flow) receive pro-rata MBRN shares. This means the protocol's paying users — borrowers and LPs — accumulate the most governance power over time, aligning protocol control with protocol usage.

## The Flywheel

```
     More CDT Demand (borrowing + swapping)
              |
              v
     More Interest + Transmuter Fees
              |
              v
     More Revenue to MBRN Holders (Staking + Disco)
              |
              v
     MBRN Price Appreciation
              |
              v
     Acquisition Emissions More Valuable
              |
              v
     More USDC Deposited (Transmuter liquidity)
              |
              v
     Better CDT Peg -> More CDT Demand
              |
              +---> (loop)
```

### Concrete Mechanics

Each arrow maps to actual contract calls:

**CDT Demand -> Revenue:**
- Borrowers pay interest: `Debt.AccrueAndReturnDebt` accumulates `pending_revenue`
- Swappers pay fees: Transmuter `usage_fee` (1%) on every non-allowlisted swap
- Revenue Distributor collects via `TakeRevenueFromBasket` (SubMsg reply chain)

**Revenue -> MBRN Holders:**
- Stakers: `StakingExecuteMsg::DepositFee` creates `FeeEvent` at `amount / total_staked_mbrn`
- Disco depositors: `LtvDisco.AddRevenue` distributes CDT weighted by `10^18 / isqrt(deposits_above + avg_slot_size)`
- Vesting MBRN earns at 20% multiplier

**MBRN Value -> Acquisition:**
- Acquisition windows emit MBRN at adaptive rates tied to Transmuter utilization
- Higher MBRN price = more valuable emissions = more USDC deposited
- Adaptive control prevents overspending: rate decreases when deposits outpace emissions (no cooldown), rate increases when emissions outpace deposits (8hr cooldown)
- Efficiency clamp at withdrawal end caps realized MBRN/deposit ratio

**Transmuter Liquidity -> CDT Peg:**
- More USDC in Transmuter = deeper 1:1 swap pool = tighter CDT peg
- CDT->USDC swaps restricted to CDP contract only (prevents external bank runs)
- Rate limiting and usage fees slow drain during stress

## Product Wedges

### Wedge 1: Borrower Safety (Delayed Liquidations + Rate Smoothing)

**What borrowers get:**
- High LTVs with wick protection: the 8-hour delay means flash crashes don't liquidate sound positions
- Immediate liquidation only for severely undercollateralized positions (above `immediate_threshold`)
- Dynamic caller fees (`current_ltv - avg_max_ltv`) make wick hunting progressively more expensive — the attacker pays higher liquidation premiums for marginal positions
- Rate discounts that grow over time: 45% discount by day 30, 75% by day 90 for loyal borrowers
- Peg debt bump rate increases gradually (4.8hr ticks), not instantly

**Why this wins:** Every other CDP protocol liquidates at the first block a position crosses the threshold. This creates a MEV opportunity that punishes honest borrowers during volatile markets. Membrane's delay window transforms liquidation from a race into a process, giving borrowers time to act while still protecting the protocol through immediate liquidation of severely underwater positions.

### Wedge 2: Risk Manager Marketplace (LTV Disco)

**What risk managers get:**
- Transparent risk/reward tiers: choose exactly how much risk to take
- Revenue weighted by inverse-sqrt formula — riskier choices earn disproportionately more
- Manager fees (up to 5%) for professional risk managers who actively rebalance positions
- Affiliate fees (1%) for bringing capital into the system
- Their LTV choices directly influence the protocol's collateral parameters via dynamic LTV feedback

**What the protocol gets:**
- Market-driven risk pricing instead of governance-set parameters
- First-line bad debt insurance: Disco MBRN is slashed before any other mechanism activates
- Diverse risk perspectives: many managers with different strategies vs. one governance committee

### Wedge 3: Adaptive User Growth (Acquisition)

**What the acquisition model does:**
- MBRN emission windows attract USDC deposits to the Transmuter
- Adaptive control system prevents overspending: efficiency mutations track deposits/MBRN ratio
- Bump rate feedback loop: when emissions are maxed, peg debt rates gradually increase, self-regulating demand
- Users who send MBRN to Disco earn yield during cliff (via `revenue_destination`) but face clawback on early withdrawal

**Why this wins:** Most token emission programs burn through supply with no feedback mechanism. Membrane's acquisition contract has a PID-like control system that automatically adjusts emission rates. When deposits flow efficiently, the rate drops (spending less MBRN). When deposits slow, the rate rises. The efficiency clamp at withdrawal end prevents any single window from overpaying.

### Wedge 4: Points as Progressive Ownership

**What users earn:**
- 1 point per $1 of protocol activity (repayment, claiming, transmuting, voting)
- Governance votes at 3x multiplier — protocol users who govern earn fastest
- Claimed MBRN is auto-staked, not sent as liquid tokens
- Acquisition model ensures lenders receive the largest MBRN allocation

**Why this matters:** Over time, the protocol's most active users accumulate the most governance power. Borrowers (via Acquisition) get the bulk of MBRN. This creates a protocol governed by its paying/at-risk users, not speculators.

## Market Position (March 2026)

### Cosmos Landscape: Contraction Creates Opportunity

The Cosmos ecosystem has undergone severe contraction through 2025-2026:

**Dead:**
- **Mars Protocol** — shut down March 2026 after a $960K bad debt exploit. Was the primary lending protocol on Neutron and Membrane's closest competitor.
- **Kujira** — shut down. Had a liquidation queue mechanism similar to Membrane's.
- **Comdex, Evmos, Picasso, Penumbra, Quasar** — all shut down.

**Leaving Cosmos:**
- **Noble** — switching from Cosmos SDK to a new EVM Layer 1 (announced March 2026). IBC connections maintained. Was the primary USDC infrastructure for Cosmos with $450M+ USDC in circulation.
- **Stride** — exploring revenue outside Cosmos. Liquid staking continues but the project is pivoting.

**Diminished:**
- **Osmosis** — reportedly in maintenance mode. OSMO down 79%.
- **Leap Wallet** — shutting down May 28, 2026, forcing user migration.
- **Neutron** — in maintenance mode.

**Still active:**
- **IST (Agoric)** — still in development but very low profile, no significant scale.

**Membrane's position:** Effectively the last standing CDP/stablecoin protocol on Cosmos/Neutron. Mars (direct competitor) is dead. Kujira (liquidation queue competitor) is dead. Noble (USDC infrastructure) is leaving Cosmos SDK but maintaining IBC. This creates a window where Membrane can be the primary DeFi in Cosmos.

### Broader Stablecoin Market

Total stablecoin market: **~$317B** (early 2026). USDT ($187B) + USDC ($76B) control 80%+.

CDP/overcollateralized protocols:

| Protocol | Stablecoin | Supply | Chain | Status |
|----------|-----------|--------|-------|--------|
| Sky (ex-Maker) | USDS/DAI | ~$7.8B | Ethereum | Growth flatlined |
| Liquity V2 | BOLD | — | Ethereum | Crypto-only collateral, fully decentralized |
| Liquity V1 | LUSD | — | Ethereum | Battle-tested, still active |
| Curve | crvUSD | — | Ethereum | LLAMMA continuous rebalancing |
| Aave | GHO | ~$500M | Multi-chain | Growing fast (245% since early 2025) |
| Ethena | USDe | ~$5.9B | Ethereum | Down from $15B peak, 3.72% yield |
| **Membrane** | **CDT** | — | **Osmosis** | **Only CDP protocol on Cosmos** |

### Growth Strategy Encoded in Contracts

Membrane's growth strategy isn't a business plan — it's encoded in contract logic:

1. **Phase 1: Transmuter TVL.** Acquisition emissions attract USDC deposits to the Transmuter. Adaptive control ensures efficient MBRN spend.
2. **Phase 2: Yield-arb revenue.** Transmuter TVL enables borrowers to mint and benefit from stabilized rates and delayed liquidations.
3. **Phase 3: Disco flywheel.** Revenue distributed to Disco depositors attracts risk managers. Their LTV choices influence CDP parameters, creating a self-governing system.
4. **Phase 4: Borrower retention.** System Discounts reward long-term borrowers. Points accumulate governance power. The protocol's paying users become its governors.

### Key Health Metrics

These metrics are queryable from contract state:

| Metric | Source | Indicates |
|--------|--------|-----------|
| Transmuter utilization | `Transmuter.VaultInfo` | Peg health and LP demand |
| Disco TVL per asset | `LtvDisco.GetAssetQueue` | Insurance depth per collateral |
| Acquisition efficiency | `Acquisition.AcquisitionModelState` | MBRN spend efficiency |
| Bump rate | `Acquisition.AcquisitionModelState.bump_rate` | Peg debt rate pressure |
| Pending bad debt | `LtvDisco.PENDING_BAD_DEBT` | Active bad debt events |
| Revenue per epoch | `RevenueDistributor.CurrentEpochRevenue` | Protocol revenue run rate |
| Points distributed | `PointsSystem.Config.total_mbrn_distribution` | User ownership accumulation |
