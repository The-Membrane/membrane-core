# membrane-core formal specs

Quint (primary) + Lean 4 (arithmetic carve-out) specifications for the Membrane protocol.

## What's in here

| Path | Purpose |
|------|---------|
| `quint/lib/` | Shared types, arithmetic helpers, environment abstractions |
| `quint/organs/` | One module per contract (state, actions, invariants) |
| `quint/shared/` | Oracle, system discounts, points system |
| `quint/infrastructure/` | Osmosis/Neutron proxies, discount vault, tokenfactory |
| `quint/compositions/` | Cross-organ invariants (bad-debt flow, revenue flow, liquidation flow, acquisition-to-disco flow) |
| `quint/tests/` | Per-module simulation/property witnesses |
| `lean/` | Lean 4 arithmetic proofs (VT rate, isqrt monotonicity, fee split, interest accrual, tranche waterfall, liquidation fee cap, decimal mul_ratio) |
| `scripts/` | Drift check, batch runners |
| `SPEC_MAP.md` | Invariant -> Rust source (file:line) index |
| `PR_CHECKLIST.md` | Snippet contributors paste when touching spec'd code |

## Grounding discipline

Every invariant and action in these modules cites a specific Rust file and line range. Docs are supplementary; the source is authoritative. When the Rust implementation changes, the corresponding Quint/Lean artifact updates in the same PR (see `PR_CHECKLIST.md` and `scripts/check_drift.sh`).

## Scope

Covered organs (from `docs/README.md`):
- CDP: `cdp`, `collateral`, `debt`, `liquidation-engine`, `liq-queue`
- Transmuter: `transmuter` (includes the new **per-user manager subsystem**)
- LTV Disco: `ltv_disco`
- Acquisition: `acquisition`
- Governance: `governance`, `staking`, `vesting`
- Revenue: `revenue-distributor`, `auction`
- Shared: `oracle`, `system_discounts`, `points-system`
- Infrastructure: `osmosis-proxy`, `neutron-proxy`, `discount_vault`, `tokenfactory`

Explicitly excluded:
- `emissions-voting` (user instruction)
- `stability-pool`, `stability-pool-vault` — legacy per `docs/CDP_MODULAR_PARITY.md:163,422`
- Gaming/experimental contracts (rps-engine, tournament, race-engine, racing-revenue, mars-*, byte-minter, car, vol-earn-vault, stable-earn-vault, range-bound-lp-vault, yield-arb, launch, neutron_launch, managed-market, market-manager, mm_*, neutron-oracle, liquidity_check, track-manager)

If any of the "excluded" contracts turns out to be production, move it back in and add a SPEC_MAP row.

## Toolchain

Pin exact versions in `lean/lean-toolchain` and wherever you track Quint/Apalache. Current guidance:

| Tool | Install | Version (TBD — pin at install) |
|------|---------|--------------------------------|
| Quint | `npm i -g @informalsystems/quint` | latest stable |
| Apalache | Informal Systems release; JDK 17+ required | latest stable |
| Lean 4 + Lake | `curl -sSf https://raw.githubusercontent.com/leanprover/elan/master/elan-init.sh \| sh` | record in `lean-toolchain` |
| Mathlib | pinned commit hash | set in `lakefile.lean` |
| JDK | 17+ | system-managed |

## Running specs

```bash
# Type-check every Quint module
for f in specs/quint/**/*.qnt; do quint typecheck "$f"; done

# Randomized simulation (fast feedback)
quint run --max-steps=100 --max-samples=10000 \
  --invariant=inv_tranche_sum_matches_total \
  specs/quint/organs/transmuter.qnt

# Bounded model-check via Apalache
quint verify --length=20 \
  --invariant=inv_bad_debt_never_underflows \
  specs/quint/organs/transmuter.qnt

# Cross-organ check
quint verify --length=20 \
  --invariant=inv_bad_debt_conserved \
  specs/quint/compositions/bad_debt_flow.qnt

# Lean arithmetic proofs
cd specs/lean && lake build

# Drift check (CI-safe, cheap)
bash specs/scripts/check_drift.sh
```

Start Apalache at `--length=10`; raise per invariant once green. Cross-organ flows typically need `--length=20+` to observe a full SubMsg chain.

## Definition of done (per organ)

1. `.qnt` module type-checks clean.
2. Every `execute_*` handler has a matching Quint `action`; every `ExecuteMsg` variant is covered.
3. Every `require!` / `return Err` branch is reflected in either an action precondition or an invariant.
4. `quint run` passes all invariants for >=10,000 samples, >=100 steps.
5. `quint verify --length=N` passes safety invariants at N>=15.
6. Every invariant has a `SPEC_MAP.md` row with a current `Last synced commit`.
7. `scripts/check_drift.sh` reports clean.
8. If the organ has a Lean dependency: `cd specs/lean && lake build` is green.

## Transmuter manager subsystem (new)

Defined first-time here, modeled on LTV Disco's manager (`contracts/ltv_disco/src/execute.rs:971-2176`, `contracts/ltv_disco/src/state.rs:61-130`) but with a **per-user** binding rather than Disco's per-deposit binding.

Powers granted to a manager:
- Request a management-fee change (bounded by `config.max_management_fee`; subject to the 14d + 2d window below)
- Request a net-zero tranche reshuffle for a managed user (`RequestTrancheUpdate`)

Explicitly forbidden:
- `ExitVault`, `RepayUserDebt`, `TransferDepositOwnership`
- Changing the binding (only the user can set/clear their manager — no manager-reassigns-manager)
- Instant fee changes

Fee mechanics:
- Fee is taken on **yield only** (never on principal, never on bad-debt loss), deducted at `ExitVault` time and routed to the manager via `BankMsg::Send`.

See `quint/organs/transmuter.qnt` (sections marked `// NEW: manager subsystem`) and `SPEC_MAP.md` rows `TR.inv_manager_*`.

## Switching-period / withdraw-intent pattern (new)

Both Transmuter and LTV Disco use a **14-day wait + 2-day permissionless execution window** for time-sensitive state changes. After the execution window expires, the intent resets (no tokens moved, must re-request).

```
T0                          T0 + 14d                  T0 + 16d
 |                             |                          |
 | RequestXxx / SetIntent      | ExecuteXxx window open   | expired (reset)
 +-----------------------------+--------------------------+
        wait (intent visible)     permissionless execute
```

### Transmuter (`quint/organs/transmuter.qnt`)

| Trigger | Who requests | Who executes | Notes |
|---|---|---|---|
| Junior → Senior tranche switch | user | anyone | `Intent_JuniorToSenior` |
| Junior `ExitVault` | user | anyone | `Intent_JuniorExit`. Senior-only withdrawals bypass this entirely (immediate) |
| Remove all of a user's positions for a denom | user | anyone | `Intent_RemoveAsset` |
| Manager-fee change | manager | anyone | `RequestManagerFeeChange` / `ExecuteManagerFeeChange` |

During the 14-day wait, juniors remain first-loss for bad-debt assignment. Only when `ExecuteSwitch` fires (inside the window) does the position actually move.

### LTV Disco (`quint/organs/ltv_disco.qnt`)

| Trigger | Who requests | Who executes | Notes |
|---|---|---|---|
| Unstake MBRN (`RequestUnstake` / `CompleteUnstake`) | deposit owner | anyone | Cooldown upgraded from 2d to **14d**; deposit keeps earning during wait. |
| Move to a LOWER-LTV slot | owner or manager | anyone | `RequestLowerLTVMove` / `ExecuteLowerLTVMove`. Higher-LTV moves remain immediate via `MoveDeposit`. |
| Manager-fee change | manager | anyone | `RequestManagerFeeChange` / `ExecuteManagerFeeChange`. Replaces instant `SetManagerFee`. |

**Bad-debt waterfall interaction**: the Disco query helper `querier_current_ltv(deposit_key, current_slot)` returns the PRE-intent slot for any deposit with a pending lower-LTV intent. Slashing and `QueueAverageLTV` both read through this helper, so a user with intent to move 80%→50% stays at the 80% risk tier for the full 14-day wait.

See `SPEC_MAP.md` sections "Transmuter switching-period" and "LTV Disco switching-period / unstaking" for the full invariant list.

### Fee-decrease asymmetry (manager fee changes)

Fee **decreases apply instantly**. `RequestManagerFeeChange(new_fee)` branches:

- `new_fee <= current_fee`: apply now, emit `Ev_ManagerFeeChanged(instant = true)`. No pending record.
- `new_fee >  current_fee`: create a 14d + 2d pending request, emit `Ev_ManagerFeeRequestCreated`. `ExecuteManagerFeeChange` (permissionless) later applies the hike and emits `Ev_ManagerFeeChanged(instant = false)`.

Rationale: managers can voluntarily lower their cut with zero UX friction; users are never surprised by a hike without notice.

### Acquisition bypass

Acquisition-contract–owned deposits skip the new 14-day gates. Acquisition is the only deposit owner with this exemption — it exists because acquisition-routed positions belong to the protocol's clawback schedule, not individual users.

- **Transmuter `ExitVault`**: if `info.sender == config.acquisition_contract`, the junior-exit intent gate is bypassed. Senior-only behavior unchanged.
- **Disco `AcquisitionUnstake(key, amount)`**: new action, auth = acquisition contract (recorded as `BackingDeposit.depositor`). Immediately burns VTs and sends MBRN back, with **no cooldown**. Bad-debt freeze still applies — if `pending_bad_debt[asset] > 0`, bypass is blocked like any other unstake.

## Manager P&L tracking — events, not storage

History of manager bad-debt absorption and fee earnings is **emitted as CosmWasm events only**. No on-chain storage of this history (the legacy `MANAGER_PERFORMANCE` map in Disco is targeted for removal — see `OPEN_QUESTIONS.md` #2). Indexers are the authoritative reader; leaderboards and historical charts are off-chain.

Event schema (see `quint/lib/types.qnt :: Event` and `SPEC_MAP.md` "Manager P&L event schema"):

| Event (`action` attr) | When emitted | Attributes |
|---|---|---|
| `manager_bad_debt_absorbed` | `AssignBadDebt` / `SendMBRNForSale` slashes a managed deposit | `manager`, `user`, `organ`, `asset`, `loss_amount`, `ts` |
| `manager_fee_earned` | Claim / exit pays the manager their fee | `manager`, `user`, `organ`, `asset`, `fee_amount`, `yield_base`, `ts` |
| `manager_user_added` / `manager_user_removed` | Manager binding transitions | `manager`, `user`, `ts` |
| `switch_intent_created` / `_executed` / `_cancelled` / `_expired` | 14d + 2d intent-flow lifecycle | `organ`, `kind`, `intent_id`, `user`, `unlock_time`, `expire_time`, `executor?`, `ts?` |
| `manager_fee_request_created` | Fee increase requested (hike only) | `organ`, `manager`, `new_fee`, `current_fee`, `unlock_time`, `expire_time` |
| `manager_fee_changed` | Fee actually applied (instant decrease OR hike execution) | `organ`, `manager`, `old_fee`, `new_fee`, `instant` (bool), `executor`, `ts` |
| `manager_fee_request_cancelled` | Manager cancels pending hike | `organ`, `manager`, `ts` |

Indexer rules of thumb: use `ts` + `block_height` for ordering, dedupe by tx hash + event index, compute manager P&L as `Σ fee_amount - Σ loss_amount` over the window of interest.

## Autonomous collateral onboarding — the safety belt

Every cAsset carries an `onboarding_window_end: Timestamp`:

- `0` — set only by the instantiate-time path (`AddAssetAtInstantiate`). Asset is "always onboarded", no extra restrictions.
- `> 0` — set by `AddAssetAutonomous` to `block_time + config.onboarding_window_duration`. While `block_time < onboarding_window_end`, the **Transmuter** treats every LP (junior AND senior) as if they held the junior tranche for the purposes of `ExitVault`: they must create a switching-intent (`Intent_JuniorExit`, the new `Intent_SeniorOnboardingExit`, or `Intent_RemoveAsset`) and wait 14 days + 2-day execution window. After the window closes, seniors regain their free-withdraw privilege.

Three structural guarantees:
- The field is **immutable** once set — `UpdateAsset` rejects any change to it, so an admin cannot retroactively re-restrict seniors.
- Instantiate-path assets have `onboarding_window_end = 0` by **contract-level require** — cannot be bypassed with a crafted param payload.
- Acquisition-contract callers still bypass the gate (Amendment 2) — they unwind on their own schedule independent of the onboarding window.

This is the risk-absorption counterpart to whatever the team picks for the autonomous-onboarding LTV question (see OPEN_QUESTIONS.md #1). Regardless of the LTV choice, senior LPs cannot flee the Transmuter's bad-debt waterfall during the first `onboarding_window_duration` of a freshly autonomous-added cAsset.

See `quint/organs/collateral.qnt` and `quint/organs/transmuter.qnt` (sections keyed to `onboarding_window_end`, `asset_in_onboarding`, `tranche_entry_covered`).

## Capital-loss access control — top-level invariant

**No singular access-control path can result in user capital loss.** Enforced as `AC.inv_no_unilateral_capital_loss_path` in `compositions/membrane.qnt`, which conjoins per-organ witnesses.

Every parameter or admin action that could cause capital loss is enumerated in [`CAPITAL_LOSS_SURFACE.md`](CAPITAL_LOSS_SURFACE.md). Each row carries the required mitigation:

| Code | Mitigation | What it gives users |
|------|------------|---------------------|
| **TL** | **Timelock** — 14d request + 2d permissionless execute (mirroring the manager-fee-change pattern) | Window to exit before the change takes effect |
| **GOV** | **Governance multisig** (`config.governance` ≠ `config.owner`) | Requires multiple independent signers |
| **CAP** | **Hard contract-level constant** (e.g. `MAX_LTV_HARD_CAP = 90%`) | Magnitude bounded even if both governance and timelock are breached |
| **WIN** | **Onboarding/switching window** (e.g. `onboarding_window_end`) | Senior LPs flee before the change is effective |

Two-role split: every organ's `Config` now carries both `owner` (routine ops) and `governance` (catastrophic actions: oracle/CDP/proxy address switches, paired_asset switches, instantiate-only collateral additions, mint-authority grants). The spec asserts `owner != governance` per organ.

Hard caps (defined in [`quint/lib/access_control.qnt`](quint/lib/access_control.qnt)):

| Constant | Value | Purpose |
|----------|-------|---------|
| `MAX_LTV_HARD_CAP` | 90% | Ceiling on any cAsset's max_ltv (prevents toxic collateral additions even via governance) |
| `MAX_USAGE_FEE` | 5% | Ceiling on Transmuter swap usage fee |
| `MAX_REVDIST_PERCENTAGE` | 50% | Ceiling on % of swap fees diverted to Revenue Distributor |
| `MAX_MAX_MGT_FEE` | 25% | Ceiling on the per-organ `max_management_fee` config |
| `MAX_LIQ_FEE` | 10% | Ceiling on liquidation protocol fee |
| `MAX_INTEREST_RATE_BPS` | **100%/yr** (Amendment 5) | Ceiling on CDP base interest rate |
| `MAX_COMPARATIVE_RATE` | **100%/yr** (Amendment 5) | Ceiling on cross-asset spread accrual rate |
| `MAX_AFFILIATE_FEE` | 5% | Ceiling on affiliate fee config |
| `MIN_SWITCHING_PERIOD_S` | 7 days | Floor on Transmuter switching_period |
| `MIN_UNSTAKING_PERIOD_S` | 7 days | Floor on Disco unstaking_period |
| `MAX_ORACLE_STALENESS_S` | 1 hour | Ceiling on oracle staleness window |
| `MAX_PRICE_DEVIATION_PCT` | 10%/update | Ceiling on per-update oracle price deviation (enforced in Oracle `SetPrice`, Amendment 5) |
| `MAX_DAILY_BORROW_PCT` | **10%/day** (Amendment 5) | Ceiling on the daily mint-flow limit; `borrow_limit_pct` config cannot exceed this |
| `DAILY_WINDOW_S` | 24h | Borrow-flow window length |

**Adding a new param**: append a row to [`CAPITAL_LOSS_SURFACE.md`](CAPITAL_LOSS_SURFACE.md) **before** writing the Rust handler. If no mitigation fits, the param is rejected at design review.

## TWAL oracle and supply caps

[`shared/twal_oracle.qnt`](quint/shared/twal_oracle.qnt) defines a **Time-Weighted Average Liquidity** oracle that samples Uniswap V3 pool depth and produces the long-run input to per-asset supply caps.

Key design choices:

- **Append on contract calls.** No dedicated keeper bot. Routine execute paths (Transmute, EnterVault/ExitVault, MintBorrow, …) carry a SubMsg that calls `ObserveLiquidity` on the TWAL module. State growth is bounded by 30-minute bucket dedup (`SAMPLE_BUCKET_S = 1800`).
- **Bounded buffer.** `TWAL_BUFFER_CAP = 1440` samples per pool — 30 days at 30-minute buckets.
- **Default window: 30 days.** `query_twal(asset, now)` returns the time-weighted-average of pool liquidity over the last `config.twal_window_s` seconds.
- **Supply caps (deferred).** `supply_cap_for(asset, cap_pct_of_twal)` is the spec hook the CDP Collateral contract will consume. The exact cap percentage is left for the team to decide.
- **Volatility tracker is suspended** (see `OPEN_QUESTIONS.md` #3). The TWAL buffer shape is reusable if/when the team revisits this; for now no `Vol_*` state, events, or invariants are added.

## Oracle circuit breaker

[`shared/oracle.qnt`](quint/shared/oracle.qnt) adds a per-denom circuit breaker:

- `SetPrice` (governance-only) rejects updates exceeding `max_per_update_deviation_pct` and **auto-trips** the breaker if `cb_trip_threshold_pct` is exceeded.
- `TripCircuitBreaker(denom, observed_price)` is **permissionless**. Anyone can submit a fresh observation; if it deviates beyond the trip threshold, the breaker is set without governance latency.
- `ResetCircuitBreaker` is **governance-only**. Once tripped, the breaker stays tripped until governance clears it — the `inv_cb_only_governance_resets` invariant captures this structurally.
- `price_safe(prices, cb, denom, now, window)` is the predicate every consumer (CDP, liquidation engine, Transmuter price reads) MUST call before acting. A tripped breaker or a stale price returns `false` and blocks the path.

## Daily borrow flow limit

[`organs/debt.qnt`](quint/organs/debt.qnt) now enforces a **static % of total outstanding borrow per 24h window** as a code-level rate limiter:

- `borrow_limit_pct` (config) — capped at `MAX_DAILY_BORROW_PCT = 10%/day`. Admin can never exceed.
- `BorrowWindow { window_start, minted_at_window_start, minted_in_window }` — rolls forward on first mint after `DAILY_WINDOW_S` elapsed.
- `MintBorrow(amount)` reverts when `minted_in_window + amount > borrow_limit_pct × minted_at_window_start`.

Effect: even with a compromised admin, the protocol cannot mint more than ~10% of outstanding CDT in 24 hours. Buys monitoring + governance time to react.

## Points & discounts — full coverage (Amendment 6)

[`shared/points_system.qnt`](quint/shared/points_system.qnt) and [`shared/system_discounts.qnt`](quint/shared/system_discounts.qnt) are now full code-grounded mirrors of the Rust implementation, not docs-driven skeletons.

**Points System highlights:**
- All 14 ExecuteMsg variants modeled (`UpdateConfig`, `CheckClaims`, `RepayAndGivePoints`, `ClaimDiscoRevenueAndGivePoints`, `TransmuteAndGivePoints`, `GivePoints`, `Liquidate`, `ClaimMBRN`, `ReceiveVotingResult`, `GivePointsForAffiliateFee`, `GivePointsForManagerFee`, `CheckTransmuterYield`, `CDPGivesUserManagementPoints`, `CheckManagementPoints`).
- All 5 reply IDs documented with their handler line cites (`LIQUIDATION_REPLY_ID = 1`, `CDP_REPAY_REPLY_ID = 3`, `DISCO_CLAIM_REPLY_ID = 4`, `TRANSMUTER_TRANSMUTE_REPLY_ID = 5`, `CHECK_MANAGEMENT_POINTS_REPLY_ID = 6`).
- `PointsMultipliers` struct (per-source) modeled; `ReceiveVotingResult` is the only path that mutates them (auth = emissions_voting_contract).
- Authority registry per fee-award entry is structural — affiliate-fee accepts only RevDist / Transmuter / LTV Disco; manager-fee accepts only LTV Disco; management-point accepts only CDP.
- `ClaimMBRN` cap: `total_mbrn_distribution + payout ≤ max_mbrn_distribution` enforced.
- Constant: `MANAGEMENT_POINTS_PER_ACTION = 5_000_000` is in code, not config.

**System Discounts highlights:**
- All 3 ExecuteMsg variants modeled (`UpdateConfig`, `SetDiscountPeriod`, `ClearDiscountPeriod`).
- The actual `get_discount` priority stack is encoded: **Timed > Static > Linear**.
- The linear curve is `min((wallet_mbrn + disco_deposits) / (total_supply * ratio), 1.0) * max_discount`.
- Hard caps: `max_discount ≤ 1`, `ratio ≤ 1`, every static entry's `discount ≤ 1`.

> ⚠️ **Documented mismatch**: `docs/README.md:131-137` describes a two-phase time-curve formula that is **not** what the contract computes. The Quint spec mirrors the code (the truthful behavior). The mismatch is now tracked in [`OPEN_QUESTIONS.md`](OPEN_QUESTIONS.md) #4 (formula choice) and #5 (Discount Vault link). A `SetDiscountPeriod` unit bug (`* 60` vs `* 3600`) is flagged in #6.

**Flywheel composition** ([`compositions/flywheel.qnt`](quint/compositions/flywheel.qnt)) ties them together:
- Static cross-organ authority invariants (CDP-only management points, Disco-only manager-fee points, RevDist+Transmuter+Disco affiliate-fee points, emissions-voting multiplier updates).
- Discount-priority structural witness; `UserBoost` / `IntentBoosts` documented as currently disabled.
- Three deferred temporal properties (Apalache-checkable once cdp.qnt is thickened): `tp_every_management_point_event_is_repay`, `tp_manager_fee_pairing`, `tp_revenue_eventually_yields_points`.

## CDP organ trio — full coverage (Amendment 7 / Phase B)

The CDP / Liquidation Engine / Liq Queue trio is now fully spec'd against the Rust implementation, replacing the earlier skeleton stubs.

**CDP / Positions** ([`organs/cdp.qnt`](quint/organs/cdp.qnt))
- 19 ExecuteMsg variants modeled (Deposit, IncreaseDebt, Withdraw, Repay, Liquidate, Accrue, ClosePosition, EditcAsset, EditBasket, SetAffiliate, SetUserIntents, FulfillIntents, SetDeploymentVenue, FulfillBadDebt, TakeRevenue, CheckAndClearDebtDelta, SetAcquisitionBumpRate, CollateralRateAssurance, UpdateConfig).
- All 7 reply IDs as Quint constants (LIQ_QUEUE=1, WITHDRAW=4, REVENUE=5, CLOSE_POSITION=6, SELL_COLLATERAL=7, DEPLOYABLE_VENUE=8, BAD_DEBT=999999).
- `Position` struct includes `rate_segments` + `peg_rate_segments` (variable + fixed-rate debt) + `vol_window_initial_debt` (the field that gates management-points awards).
- `insolvent`, `total_position_debt`, `collateral_value`, `avg_max_ltv` helpers grounded in `query.rs:881-914`.
- `MAX_POSITIONS_AMOUNT = 9`, `AFFILIATE_LIMIT_PER_POS = 3`, `BAD_DEBT_CALLER_FEE = 1%`.

**Liquidation Engine** ([`organs/liquidation_engine.qnt`](quint/organs/liquidation_engine.qnt))
- Full 9-step reply chain (1→2→9→3→4→5→6→7→8) with one Quint action per reply handler.
- `LiquidationOutcome` enum: Pending / Immediate / BrokeWindow / DelayExpired / TimerStarted / SavedByDelay.
- Per-position `liquidation_timers` for the delay-window mechanic (`docs/liquidation_delay_window.md`).
- `apply_fee_cap` pure helper encoding the exact formula from `reply.rs:362-388` (matches `docs/README.md:106-111` and Lean theorem `LiqFeeCap.safe`).
- `immediate_threshold = avg_max_ltv * (1 + avg_max_threshold_to_delay)`.
- Default `liquidation_delay = 28800s` (8 hours).
- Stats capped at `LIQUIDATION_STATS_CAP = 500` (FIFO).

**Liq Queue** ([`organs/liq_queue.qnt`](quint/organs/liq_queue.qnt))
- Full Liquity-style scalable bid auction: per-slot `product_snapshot`, `sum_snapshot`, `epoch_snapshot`, `scale_snapshot`, residue accumulators.
- Bid lifecycle: Submit → Active OR Waiting (if total_bid_amount + amount > threshold + minimum_bid) → consumed pro-rata on Liquidate or refunded on Retract.
- **Lowest-premium-first consumption** (resolved from earlier TODO), confirmed at `bid.rs:379`.
- Premium granularity: 1% increments, range `MIN_PREMIUM = 1` .. `MAX_PREMIUM = 50`.
- Bid threshold range: `MIN_BID_THRESHOLD = 1M` .. `MAX_BID_THRESHOLD = 10M`.
- **No reply handlers** in this contract — reply chains live in CDP/Liquidation Engine. Asserted structurally.
- **Stability Pool removed** per `docs/CDP_MODULAR_PARITY.md:163,422`. Asserted structurally.
- `Liquidate` auth: `positions_contract` only.

The composition module [`compositions/liquidation_flow.qnt`](quint/compositions/liquidation_flow.qnt) can now be checked end-to-end at Apalache against the trio. The deferred temporal property `tp_chain_completes` (every initiated chain reaches a terminal step) and the `inv_bad_debt_conserved` cross-organ invariant in `bad_debt_flow.qnt` are now closeable.

## Supporting organs — full coverage (Amendment 8 / Phase C)

Six previously-thin organs are now full code-grounded mirrors:

- **[`organs/acquisition.qnt`](quint/organs/acquisition.qnt)** — All 9 ExecuteMsg variants, adaptive emission window with ramp + bump-rate feedback to CDP, lazy-timer activation pattern, clawback path during `Withdraw`, `SendAcquisitionRewardsToDisco` documenting the (deposit_owner=contract, manager=user, revenue_destination=user) Disco binding.
- **[`organs/governance.qnt`](quint/organs/governance.qnt)** — All 11 ExecuteMsg variants, 7 ProposalStatus states, 5 VoteOptions including Align (which promotes Pending → Active when `aligned_power ≥ proposal_required_stake`), expedited-proposal extension on missed quorum, exact threshold/quorum math from `contracts.rs:490-552`.
- **[`organs/staking.qnt`](quint/organs/staking.qnt)** — All 13 ExecuteMsg variants, the per-MBRN-staked rate trick on `FeeEvent.amount_per_mbrn_staked` (avoids O(n) on every DepositFee), 2-step Unstake with `unstake_start_time` gating, Lock + RefreshLock with perpetual auto-extend, delegation bidirectional consistency.
- **[`organs/vesting.qnt`](quint/organs/vesting.qnt)** — All 11 ExecuteMsg variants, two parallel mint paths (legacy Allocation via osmosis_proxy, per-week VestingSchedule via neutron_proxy), cliff + linear `unlocked_amount` formula, transmutation balance invariant `OLD_MBRN - MINTED == TOTAL_OLD`.
- **[`organs/revenue_distributor.qnt`](quint/organs/revenue_distributor.qnt)** — All 11 ExecuteMsg variants, bad-debt-active override route to CDP, RevenueSplit summing to 1, two-phase DistributePromises (affiliate-always + window-gated split), all 3 reply IDs.
- **[`organs/auction.qnt`](quint/organs/auction.qnt)** — 8 ExecuteMsg variants, FeeAuction + MBRNSale dual-auction model, discount decay formula `get_discount_ratio`, StartAuction rejects CDT, StartMBRNSale ltv_disco-only auth (pull model), `ClearBadDebtFreeze` emitted to Disco when an asset's allocation is fully resolved.

Two cross-cutting findings flagged during the pass:
- **Acquisition's documented intent-boost is not implemented in code** (`boost_multiplier = Decimal::one()` hardcoded at `contract.rs:1068,1214`). Joined to the existing System Discounts mismatch in `OPEN_QUESTIONS.md`.
- **DebtAuction storage is preserved but unreachable**. `SwapForMBRN` reverts; bad debt resolves via `MBRNSale + FulfillBadDebt + BuySuppliedMBRN`. Future cleanup candidate.

## Solidity port workflow

[`SOLIDITY_PORT_GUIDE.md`](SOLIDITY_PORT_GUIDE.md) is the playbook for using this Quint spec to implement and verify the Solidity port:

- Primitive mapping table (CosmWasm → Solidity).
- 6-step workflow: Solidity implement → Foundry invariants → Certora rules → SPEC_MAP update → CI integration → counterexample replay.
- Per-organ port notes for tricky translations (e.g., LE's 9-step reply chain becomes a single transaction with internal calls; LD's bidirectional `MANAGED_DEPOSITS` index needs paired mapping updates).
- Post-port checklist tying every Quint invariant + Lean theorem + CAPITAL_LOSS_SURFACE row to a concrete deliverable.

This makes the spec→Solidity port an executable plan, not an aspiration.

## Infrastructure — full coverage (Amendment 9 / Phase D)

Five infrastructure modules now full code-grounded mirrors:

- **[`shared/oracle.qnt`](quint/shared/oracle.qnt)** — Multi-source aggregation with hierarchical fallback (Pyth → Osmosis TWAP → median(USD-par TWAP)) plus the Amendment-5 circuit breaker. `aggregate_price` is a pure function; `price_safe` is the consumer-side gate. AssetOracleInfo per (denom, basket_id) with USD-par $1 cap.
- **[`infrastructure/osmosis_proxy.qnt`](quint/infrastructure/osmosis_proxy.qnt)** — 11 ExecuteMsg variants. Multi-owner registry with `non_token_contract_auth` admin gating. MBRN-mint restriction toggle. 3 reply IDs. Per-denom supply tracking + 365-day history. ExecuteSwaps for multihop GAMM/CL routing. CreateOsmosisGauge passthrough (no auth — Osmosis incentives module gates internally).
- **[`infrastructure/neutron_proxy.qnt`](quint/infrastructure/neutron_proxy.qnt)** — Mirror of osmosis_proxy without the gauge surface. Adds `vesting_contract` authorization for transmutation mints.
- **[`infrastructure/discount_vault.qnt`](quint/infrastructure/discount_vault.qnt)** — Per-user VaultedLP arrays with `deposit_time` populated at deposit. Accepted-LP validation requires the pool's underlying assets to include CDT. **No cross-call to system_discounts** asserted structurally (`inv_no_discount_vault_link`) — code/docs gap tracked in OPEN_QUESTIONS.md.
- **[`infrastructure/tokenfactory.qnt`](quint/infrastructure/tokenfactory.qnt)** — Abstract authority oracle. `created_denoms` set + `authority_for: Denom -> Addr`. Mint/Burn/ChangeAdmin gated by registered authority.

The SOLIDITY_PORT_GUIDE.md per-organ notes section is now complete for all 13 organs covered in Phases B–D.

## Cross-organ compositions — full coverage (Amendment 10 / Phase E)

The five composition modules are now wired against the full set of organ specs and per-organ tests are in place:

- **[`compositions/membrane.qnt`](quint/compositions/membrane.qnt)** — Top-level dispatcher. Imports every organ + shared + infrastructure module. Exposes `step` action (advance time / deliver msg queue head / per-organ step) and the `safety = all_safety ∧ inv_no_unilateral_capital_loss_path ∧ inv_cdt_monetary_conservation` aggregate.
- **[`compositions/bad_debt_flow.qnt`](quint/compositions/bad_debt_flow.qnt)** — Bad-debt conservation across CDP → Transmuter / Disco → Auction → Debt. `inv_bad_debt_visible_aggregate` sums in-flight messages + sink state. Apalache temporal: `tp_bad_debt_eventually_zeroed`.
- **[`compositions/revenue_flow.qnt`](quint/compositions/revenue_flow.qnt)** — CDP → RevenueDistributor → Disco / Staking / Affiliates / Reserve. Includes the bad-debt-active override path. Apalache temporal: `tp_revenue_eventually_distributes`, `tp_no_revenue_pinned_in_promises`.
- **[`compositions/liquidation_flow.qnt`](quint/compositions/liquidation_flow.qnt)** — The 9-step reply chain modeled with `valid_transition` predicate. Apalache temporal: `tp_chain_completes`, `tp_no_skipped_step`, `tp_timer_eventually_resolves`.
- **[`compositions/acquisition_to_disco_flow.qnt`](quint/compositions/acquisition_to_disco_flow.qnt)** — Clawback-eligible Disco binding (deposit_owner=acquisition, manager=user, revenue_destination=user). Apalache temporal: `tp_user_yields_pre_cliff`.
- **[`compositions/flywheel.qnt`](quint/compositions/flywheel.qnt)** — End-to-end loop: revenue → points → discounts → CDP attractiveness → more borrows → more revenue.

## Per-organ test runners — Phase E

Seven test files under `quint/tests/` exercise the highest-leverage scenarios:

| File | Scenarios |
|------|-----------|
| [`tests/cdp_test.qnt`](quint/tests/cdp_test.qnt) | deposit/borrow/repay, insolvent liquidation, bad-debt fulfillment, affiliate registration, governance config update |
| [`tests/liquidation_engine_test.qnt`](quint/tests/liquidation_engine_test.qnt) | Full 9-step chain (immediate), below-threshold timer start, delay-expired liquidation, partial LQ + sell-wall, bad-debt + auction kickoff |
| [`tests/transmuter_test.qnt`](quint/tests/transmuter_test.qnt) | Manager fee-decrease-instant, expired intent rejection, manager-cannot-exit, fee-cap rejected at request |
| [`tests/ltv_disco_test.qnt`](quint/tests/ltv_disco_test.qnt) | 14-day unstake lifecycle, expired unstake, lower-LTV intent flow, manager fee asymmetry, bad-debt freeze |
| [`tests/governance_test.qnt`](quint/tests/governance_test.qnt) | Pass path, pending alignment, expedited extension, self-vote rejection, rejected-then-removed |
| [`tests/staking_test.qnt`](quint/tests/staking_test.qnt) | Stake/Unstake lifecycle, governance-lock blocks unstake, fee distribution, Lock + RefreshLock, Restake reverses |
| [`tests/auction_test.qnt`](quint/tests/auction_test.qnt) | Fee auction with discount decay, CDT rejection, bad-debt MBRN sale, BuySuppliedMBRN recursion, governance update |
| [`tests/acquisition_test.qnt`](quint/tests/acquisition_test.qnt) | Window happy path, Disco routing with clawback, rate assurance pass + revert |
| [`tests/flywheel_test.qnt`](quint/tests/flywheel_test.qnt) | 6 cross-organ scenarios as scaffolds (CDP volatility points, Disco affiliate fee, discount priority stack, fee-decrease-instant, Acquisition→Disco→claim, full bad-debt loop) |

Each test file is meant to be run with `quint run --max-steps=200 --max-samples=10000 --invariant=safety <file>`. Apalache `quint verify --length=15 --invariant=safety` covers bounded-depth model checking once per organ.

## Lean arithmetic carve-out — fully proved (Amendment 11 / Phase F)

All seven theorem files under [`lean/MembraneSpec/Arithmetic/`](lean/MembraneSpec/Arithmetic) now carry concrete Mathlib-backed proofs (no remaining `sorry`s):

| File | Theorem(s) | Key Mathlib lemmas |
|------|------------|--------------------|
| [VTRate.lean](lean/MembraneSpec/Arithmetic/VTRate.lean) | `roundtrip_le`, `roundtrip_identity_at_unit_rate`, `mint_empty_pool` | `Nat.div_mul_le_self`, `Nat.div_le_div_right`, `Nat.mul_div_cancel_left` |
| [IsqrtMonotone.lean](lean/MembraneSpec/Arithmetic/IsqrtMonotone.lean) | `isqrt_le_isqrt`, `isqrt_bounds`, `isqrt_strict_mono_lower_bound` | `Nat.sqrt_le_sqrt`, `Nat.sqrt_le`, `Nat.lt_succ_sqrt` |
| [FeeSplit.lean](lean/MembraneSpec/Arithmetic/FeeSplit.lean) | `fee_le_input`, `fee_split_exact` | `Nat.add_sub_cancel'`, `Nat.mul_le_mul_right`, `Nat.mul_div_cancel_left` |
| [InterestAccrual.lean](lean/MembraneSpec/Arithmetic/InterestAccrual.lean) | `accrued_additive`, `accrued_zero`, `accrued_mono_elapsed` | `Nat.le_div_iff_mul_le`, `Nat.div_mul_le_self`, `omega` |
| [TrancheWaterfall.lean](lean/MembraneSpec/Arithmetic/TrancheWaterfall.lean) | `waterfall_bounds`, `waterfall_nonneg` | `min_le_right`, `min_eq_left/right`, `omega` |
| [LiqFeeCap.lean](lean/MembraneSpec/Arithmetic/LiqFeeCap.lean) | `liq_fee_cap_safe` (both `min` arms) | `Nat.div_mul_le_self`, `Nat.mul_div_cancel`, `min_le_right`, `linarith` |
| [DecimalMulRatio.lean](lean/MembraneSpec/Arithmetic/DecimalMulRatio.lean) | `multiplyRatio_eq_div`, `multiplyRatio_le`, `multiplyRatio_eq_iff_div` | trivial branching on `den = 0` |

These theorems are language-agnostic — they apply equally to the Rust source and the Solidity port. When porting, the corresponding Solidity helper functions (`calculateVaultTokens`, `isqrt`, `feeSplit`, etc.) should be checked against these theorems via Certora rules; see [`SOLIDITY_PORT_GUIDE.md`](SOLIDITY_PORT_GUIDE.md).

**Build status**: `cd specs/lean && lake update && lake build` — Mathlib pin is `master`. Before the first CI run, lock to a specific commit hash by following the comment in [lakefile.lean](lean/lakefile.lean).

**Lean toolchain**: `leanprover/lean4:v4.13.0` (pinned in [lean-toolchain](lean/lean-toolchain)).

## Open spec questions

See `OPEN_QUESTIONS.md` for unresolved design decisions that gate certain invariants:

1. **New collateral onboarding — starting LTV without Disco**. Candidate resolutions A–E enumerated; spec does not prescribe one yet. The onboarding-window safety belt above is landed independently and does not depend on the LTV choice.
2. **Legacy `MANAGER_PERFORMANCE` storage** (Disco) — keep as migration target or delete.
3. **Volatility tracker** — SUSPENDED.
4. **System Discounts docs/code mismatch** — pick whether the linear MBRN-supply formula or the documented two-phase time curve is canonical. Spec currently mirrors code.
5. **Discount Vault → System Discounts link** — companion to #4. The cross-contract query exists in docs but not in code.
6. **`SetDiscountPeriod` duration unit bug** (`* 60` vs `* 3600`) — flag for implementer.
