# Solidity port guide

Translates the CosmWasm-flavored Quint spec to Solidity equivalents and lays out the testing-and-verification workflow for the port. This document is the **execution playbook** for taking the Quint spec and making it the contract that the Solidity implementation must satisfy.

## Why this exists

The Quint spec under `quint/` describes protocol-level behavior independent of the implementation language. Every state shape, transition, and invariant in those modules must hold on the Solidity port too. This guide:

1. Gives the verbatim mapping from each CosmWasm primitive to its Solidity equivalent.
2. Tells contributors the exact steps to verify the port against the spec when implementation begins.
3. Names the tools each spec layer hands off to (Foundry, Certora, Lean).

## Primitive mapping

| CosmWasm primitive | Solidity equivalent | Spec location |
|---|---|---|
| `SubMsg::reply_on_success` + reply handler | Synchronous external call + state update inside the same transaction; the "reply" is just the function returning. The `pending_*` propagation state becomes a memory struct passed between internal functions. | `Msg_Reply({ id, data })` |
| `SubMsg::reply_on_error` (no-op success) | `try { external() } catch { /* graceful */ }` | `Msg_Reply` with `BAD_DEBT_REPLY_ID = 8` (Liq Engine) |
| `BankMsg::Send(coin)` | `IERC20(token).safeTransfer(to, amount)` for ERC20; `(bool ok,) = to.call{value: amount}("")` for native | `Msg_BankSend({ to, denom, amount })` |
| `Tokenfactory::Mint` | `IMintableERC20(token).mint(to, amount)` (via OpenZeppelin's `ERC20Mintable` or a custom mint role) | `Msg_Tokenfactory_Mint` |
| `Tokenfactory::Burn` | `IBurnableERC20(token).burnFrom(holder, amount)` | `Msg_Tokenfactory_Burn` |
| `WasmMsg::Execute { contract_addr, msg, funds }` | `IContract(addr).method(args)` — synchronous call, `funds` becomes prior `safeTransfer` or `value:` | All cross-organ `Msg_*` variants |
| `cw_storage_plus::Item<T>` | `T public state;` (public auto-generates getter) | `var x: T` |
| `cw_storage_plus::Map<K,V>` | `mapping(K => V) public state;` (or nested mapping for tuple keys) | `var x: K -> V` |
| `cw_storage_plus::Map<(K1,K2),V>` | `mapping(K1 => mapping(K2 => V)) public state;` | `var x: (K1, K2) -> V` |
| Capped `Vec<T>` | `T[] public state;` + manual push/shift in `_appendCapped(item)` helper | Capped lists in state.rs |
| `Option<T>` | `T value; bool isSet;` paired struct, OR sentinel value (e.g. `address(0)`) | `Option[T]` |
| `info.sender` | `msg.sender` (or `_msgSender()` if using ERC-2771 meta-tx) | action `sender` param |
| `info.funds` validation (1 coin, denom check) | ERC20: `transferFrom(msg.sender, address(this), amount)` with explicit denom contract address. Native: `msg.value` check. | action precondition |
| `cw_serde` enum variants | tagged `enum` + `struct` per variant (or use a discriminated union pattern) | Quint sum types |
| `Reply { id, result }` matching | function dispatch on synchronous call return, OR `try/catch` when a single call's success/failure is enough | Reply action `id` field |
| `Decimal` (10^18 fixed point) | `uint256` with explicit `1e18` scale factor; use OpenZeppelin's `FixedPointMathLib` or `PRBMath` | `Decimal` |
| `Uint128` | `uint128` (or `uint256` with explicit overflow protection at boundaries) | `Uint128` |
| `env.block.time.seconds()` | `block.timestamp` | `block_time` |
| `env.block.height` | `block.number` | `block_height` |
| `Addr` | `address` (after `validate()` → `payable` if needed) | `Addr` |
| `Denom` (e.g. "cdt") | `IERC20` token contract address | `Denom` |
| `events!` macro | `emit EventName(...)` Solidity events | `Event` ADT in `quint/lib/types.qnt` |
| ContractError variant | `error CustomError(args)` revert reason | `error.rs` enum |
| `instantiate` entry point | constructor + `initialize(...)` (UUPS) | `init` action |

## Spec → Solidity ↔ Foundry workflow (when porting begins)

These are the concrete steps to take when starting the Solidity port. Follow them in order; each step builds on the last.

### Step 1: Implement Solidity contracts following the Quint shape

For each Quint module under `quint/organs/` and `quint/shared/`:

1. Create one Solidity file per organ (e.g. `src/Transmuter.sol`, `src/CDP.sol`).
2. Mirror the state vars 1:1: every Quint `var` becomes a Solidity storage variable using the primitive mapping above.
3. Mirror the `Config` struct. Add the **two-role split** from Amendment 4: `address public owner` AND `address public governance`. Enforce `owner != governance` in the constructor.
4. Mirror the hard caps from `quint/lib/access_control.qnt` as Solidity `constant`s. These are code-immutable per Amendment 4 — they cannot be config fields.
5. Mirror each Quint `action` as a Solidity `external` (or `internal` if used by reply chains) function. Function name = action name (camelCase in Solidity).
6. Reflect every action precondition (`require(...)`) as a Solidity `require` or custom `error`. Preserve the order — preconditions read top-to-bottom in both languages.
7. Reflect the order of state writes inside each action verbatim. Quint actions are atomic; Solidity functions should be too. If you need multiple state writes that depend on prior reads, use local memory variables.

### Step 2: Generate the Foundry invariant test suite

For every `inv_*` in `quint/organs/*.qnt` and `quint/shared/*.qnt`:

1. Add a Foundry test contract per organ: `test/CDP.invariants.t.sol`, `test/Transmuter.invariants.t.sol`, etc.
2. Each Quint invariant becomes one `function invariant_<name>() public view { assert(...); }`.
3. Use Foundry's invariant fuzzer (`forge test --match-test invariant_*`) to drive random sequences of execute calls and check the invariants after each one.
4. Add **bounded actor sets** in the test setup (e.g. 5 users, 3 managers, 2 governance signers) — Foundry's invariant fuzzer needs a finite address space to stay tractable.
5. For invariants that reference cross-organ state (e.g. `bad_debt_flow.qnt`), wire the test setup to deploy all involved contracts and let the fuzzer call across them.

Mapping cheat sheet:

| Quint invariant pattern | Foundry equivalent |
|---|---|
| `val inv_x: bool = state.foo == state.bar` | `function invariant_x() public view { assertEq(state.foo(), state.bar()); }` |
| `val inv_x: bool = forall(s in set, p(s))` | Loop over the bounded set and assert per element |
| `val inv_x: bool = state.x <= CAP` | `assertLe(state.x(), CAP);` |
| `temporal tp_x: bool = always(...) ⇒ eventually(...)` | Not directly expressible in Foundry — fall through to Step 3 (Certora) or accept simulator-only coverage |

### Step 3: Add Certora rules for arithmetic-critical paths

Foundry's invariant fuzzer is great for shallow bugs but can't reach unbounded properties. For invariants that **must** hold under any input (especially the seven Lean theorems and anything the hard-cap constants in `access_control.qnt` enforce), use Certora.

1. Install the Certora CLI and configure a project under `certora/`.
2. Write one `.spec` file per arithmetic-critical contract path. Mirror these Lean theorems first:
   - `LiqFeeCap.safe` → `LiqEngineFeeCap.spec` (rule: `caller_fee * repay_value <= collateral_value` post-cap).
   - `VTRate.roundtrip` → `VaultRate.spec` (rule: `calculateBaseTokens(calculateVaultTokens(x, s, v), s, v) <= x`).
   - `IsqrtMonotone.le` → `Isqrt.spec` (rule: `a <= b => isqrt(a) <= isqrt(b)`).
   - `FeeSplit.exact` → `Fee.spec` (rule: `fee + out == in` modulo one ulp).
   - `InterestAccrual.formula` → `Interest.spec` (rule: additivity over non-overlapping intervals).
   - `TrancheWaterfall.bounds` → `Waterfall.spec` (rule: junior-loss + senior-loss == amount when amount ≤ j+s).
   - `DecimalMulRatio.exact` → `MulRatio.spec` (rule: `multiplyRatio(n, num, den) <= n*num/den`).
3. For the **AC.inv_no_unilateral_capital_loss_path** meta-invariant from `compositions/membrane.qnt`, write per-organ Certora rules that assert: every config field belonging to Category A or B in `CAPITAL_LOSS_SURFACE.md` has at least one mitigation gate (`require(msg.sender == governance)`, or a timelock window check, or a hard-cap `require`). These can be parametric rules over the config setter functions.

### Step 4: Keep SPEC_MAP synchronized with the port

Add a `Solidity port` column to `SPEC_MAP.md`:

```
| Invariant ID | Quint location | Rust source | Solidity source | Last synced commit | Notes |
```

Every row references both implementations. When either implementation changes, bump `Last synced commit` for the row and verify in PR review that the Quint spec still mirrors both.

### Step 5: Run the port verification suite on every PR

CI configuration:

1. **Foundry** invariant tests on every PR — fast feedback (≤5 min).
2. **Certora** rules on a separate workflow that runs on `main` merges — slower (5–60 min depending on the rule). Required to be green before tagging a release.
3. **Lean** `lake build` on PRs that touch arithmetic helpers in `src/lib/Math.sol` (or wherever the math primitives live).
4. **Quint drift check** (`scripts/check_drift.sh`) on every PR. Fails if a SPEC_MAP-referenced Rust or Solidity file changed without bumping `Last synced commit`.

### Step 6: Counterexample replay

When Apalache (in dev) or Certora (in CI) finds a violation, the violation comes back as a concrete sequence. Translate it into a Foundry unit test:

1. Read the counterexample's action sequence (e.g. "deposit, request switch, execute switch out of window").
2. Add a `test_<invariant>_counterexample()` Foundry test that replays the same sequence as concrete calls.
3. Confirm the test fails on the buggy commit and passes after the fix.
4. Keep the test in the suite as a regression marker.

This three-layer verification — **Quint at protocol level, Foundry at runtime, Certora for arithmetic** — gives cross-implementation correctness checking. The Quint spec is the source of truth; Foundry and Certora enforce it on the Solidity port.

## Per-organ port notes

This section gets one entry per organ as Phase C completes. Each entry calls out the specific port-time gotchas for that contract.

### Transmuter

- The `RateAssurance` self-call pattern becomes a synchronous internal `_assertRateNonDecreasing()` call at the end of `enterVault`, `exitVault`, `requestTrancheUpdate`, etc. No SubMsg needed.
- Manager-fee state has three separate maps (`transmuter_manager`, `transmuter_manager_fee`, `managed_users`). Solidity port should mirror exactly — combining them into a single struct loses the consistency invariant `inv_managed_users_consistency`.
- The 14d + 2d intent windows are pure timestamp arithmetic on `block.timestamp`. No special Solidity tooling needed.
- `Intent_*` enum variants are tagged structs; Solidity discriminated union pattern: `enum IntentKind { JuniorToSenior, JuniorExit, SeniorOnboardingExit, RemoveAsset }` plus a `bytes data` payload decoded per kind.

### LTV Disco

- `BACKING_DEPOSITS: Map<String, BackingDeposit>` keyed by `"asset:slot:user:deposit_id"` becomes `mapping(bytes32 => BackingDeposit) public backingDeposits;` with key derived as `keccak256(abi.encode(asset, slot, user, depositId))`.
- The compound-swap reply (`COMPOUND_SWAP_REPLY_ID = 3`) becomes a synchronous swap call in `claimRevenueForUser` with the swap result fed back into the deposit accounting in the same transaction.
- The bidirectional `MANAGED_DEPOSITS` ↔ `backing_deposits.manager` index is critical for the `inv_managed_deposits_consistency` invariant. Implement it as paired mapping updates inside `_addManagedDeposit` / `_removeManagedDeposit` private helpers.

### CDP / Liquidation Engine / Liq Queue (Phase B)

- The 9-step reply chain in Liquidation Engine becomes a single `liquidate(positionId, owner)` external function in the Solidity port. Internal steps are private functions called sequentially. The `LiquidationPropagation` struct moves from on-chain `Item` to a `memory` struct passed between internal functions.
- `LIQUIDATION_TIMERS: Map<(Addr, int), LiquidationTimer>` becomes `mapping(address => mapping(uint256 => LiquidationTimer)) public liquidationTimers;`.
- The `apply_fee_cap` Quint helper translates 1:1 to a `pure` Solidity function. Add a Certora rule asserting it satisfies `LiqFeeCap.safe`.
- Liq Queue's pro-rata bid math (`product_snapshot * sum_snapshot`) is sensitive to precision; use `PRBMath` or `FixedPointMathLib` rather than naive `mulDiv`.
- Liq Queue's `Liquidate` auth (`msg.sender == positions_contract`) is the only permitted entry; assert this with `require(msg.sender == positionsContract, "LQ: only positions");`.

### Acquisition

- The "lazy timer" pattern (deposit_end / withdrawal_end set to `u64::MAX` until first deposit) translates as `block.timestamp + 1000 * 365 days` sentinel, OR a separate `bool timersStarted` flag. Prefer the flag for readability.
- `AccruePool` is permissionless and updates the bump rate; the cross-organ message to CDP's `setAcquisitionBumpRate` is a synchronous `cdp.setAcquisitionBumpRate(...)` call.
- `SendAcquisitionRewardsToDisco` calls Disco's `submitDeposit(asset, slot, OWNER=address(this), MANAGER=user, REVENUE_DEST=user)`. The three address params must be passed explicitly — don't rely on `msg.sender` for `MANAGER`.

### Governance

- `Proposal.messages: Option<Vec<ProposalMessage>>` becomes `bytes[] memory calldata` in the Solidity port. Each `ProposalMessage` becomes `(address target, bytes data)`. Order is preserved by the array index.
- The `Pending` → `Active` promotion is a single state-transition assertion: when an `Align` vote pushes `aligned_power >= proposal_required_stake`, atomically move the proposal from `pendingProposals` to `proposals`. Solidity port should implement as one function with both reads.
- Quadratic voting requires an integer square root. Use OpenZeppelin's `Math.sqrt`. Cite the Lean theorem `IsqrtMonotone.le` for the monotonicity guarantee — the same theorem proves correctness on both ports.
- Vesting + delegation vote-power queries are external view calls. Cache results within a transaction by passing `voting_power` arguments explicitly into `castVote` rather than re-querying.

### Staking

- `FeeEvent.amount_per_mbrn_staked` is the rate-encoding trick that avoids O(n) on every fee distribution. Solidity port MUST preserve this — store the per-MBRN rate, not the total. Loop only happens at claim time, bounded by deposit count per user.
- The 2-step Unstake (request → withdraw) is implemented as two consecutive calls in Solidity. The first call sets `unstakeStartTime[depositId]`; the second call (after unstaking_period) actually transfers MBRN. Reuse the timestamp pattern from the Disco unstaking spec.
- Delegations are bidirectional — `delegated` (FROM) and `delegated_to` (TO) lists must be updated atomically. Solidity port uses paired struct arrays; assert `inv_delegations_bidirectional` as a Foundry invariant after every UpdateDelegations call.
- Lock with `perpetual_lock` auto-extends. RefreshLock is permissionless (anyone can refresh anyone's lock). On the EVM port this should remain permissionless to enable keepers.

### Vesting

- Two parallel mint paths: legacy `Allocation` via osmosis_proxy AND per-week `VestingSchedule` via neutron_proxy. The Solidity port should keep these as **two separate functions** (`withdrawUnlocked` and `withdrawVestedUnlocked`) to preserve the auditability boundary.
- `VestingSchedule` keying: `mapping(address => mapping(uint64 => VestingSchedule))` where the second key is `block.timestamp / 604800` (week ID).
- The transmutation invariant `OLD_MBRN_RECEIVED - MINTED_MBRN == TOTAL_OLD_MBRN` MUST be checked at the end of every `addVestedTransmutation` and `withdrawVestedUnlocked`. Add a `verifyTransmutationBalance()` modifier.
- The cliff + linear unlock formula in `unlocked_amount` is identical to the legacy Allocation formula. Solidity port can share the helper between both paths.

### Revenue Distributor

- The bad-debt-active override is critical: when `BAD_DEBT_ACTIVE = true`, all CDT routes to `cdp.fulfillBadDebt()` instead of the normal split. Solidity port: gate the entire distribution function on this flag.
- `RevenueSplit` summing to 1 is an instantiation invariant — assert `disco_ratio + junior_ratio + senior_ratio + reserve_ratio == 1e18` in the constructor and on every `updateConfig`.
- The two-phase distribute flow (affiliate-always + window-gated split) translates cleanly to Solidity: a single `distributePromises()` function with an `if (windowOpen) { … }` branch for the split phase.
- Reply IDs become return-value/event handling. The DISTRIBUTION_REPLY_ID retry-on-fail pattern becomes a try/catch around the affiliate transfer; on revert, push the address into `failedDistributions[]` for later retry.
- The Points System integration (`GivePointsForAffiliateFee`) fires AFTER the affiliate transfer succeeds. Solidity port: place the `pointsSystem.givePointsForAffiliateFee(...)` call at the end of the success branch.

### Auction

- The two-auction model (FeeAuction per asset + singleton MBRNSale) needs separate storage maps. `FeeAuction` keyed by denom; `MBRNSale` is a single mutable struct.
- Discount decay (`get_discount_ratio`) is pure timestamp math: `min(initial + (elapsed_post_delay / timeframe) * increase, 99%)`. Encode as a `pure` Solidity function and add a Foundry invariant: `discount(t1) >= discount(t0)` whenever `t1 >= t0`.
- `BuySuppliedMBRN` recursion (max depth 3) translates to a bounded loop in Solidity. Replace the recursive self-call with a `while (depth < MAX_BUY_RECURSION_DEPTH && hasLeftover)` loop; this is more gas-efficient and avoids reentrancy concerns.
- `StartMBRNSale` auth = `ltv_disco_contract` only. Solidity port: `require(msg.sender == address(ltvDisco));` exactly.
- The deprecated `DebtAuction` storage is preserved in the Rust spec for compatibility but `swap_for_mbrn` reverts. Solidity port: omit the `DebtAuction` struct entirely; the new MBRNSale flow is the only bad-debt resolution path.

### Oracle (Phase D)

- Pyth feed queries become Chainlink AggregatorV3 calls on the EVM port. The hierarchical fallback (Pyth → Osmosis TWAP → median(USD-par TWAP)) translates to Chainlink → Uniswap V3 TWAP → median(USD-par TWAP).
- `aggregate_price` in the Quint helper is a pure function. Encode as a `pure` Solidity function with the same input signature; this becomes a unit-testable building block.
- The circuit breaker (Amendment 5) is critical: the `priceSafe()` view function gates every consumer's read. Add it as a modifier:
  ```solidity
  modifier whenPriceSafe(address denom) {
      require(priceSafe(denom), "Oracle: stale or tripped");
      _;
  }
  ```
  Apply to every CDP / Liquidation Engine / Transmuter price-read function.
- `TripCircuitBreaker` is permissionless on the Cosmos side. Solidity port should keep it permissionless — keepers can trip the breaker on observed deviation faster than governance can react.

### Osmosis / Neutron Proxies (Phase D)

- TokenFactory bindings translate to OZ ERC20Mintable. Each Cosmos denom becomes a deployed ERC20 contract; the proxy holds the `MINTER_ROLE` for each.
- `validate_authority` (multi-owner check) becomes OZ `AccessControl` with a `MINTER_ADMIN_ROLE`. The `non_token_contract_auth` flag becomes a separate role (`CONFIG_ADMIN_ROLE`).
- `restrict_mbrn_mints` is an emergency switch — when set, MBRN mints are gated to `debt_auction` only. Solidity port: keep this as a single `bool` plus a role check on the MBRN ERC20.
- ExecuteSwaps is permissionless on the Cosmos side. On EVM, the caller must have approved the proxy to pull the input token (`safeTransferFrom` from caller). Add the approve+transfer pattern at the top of `executeSwaps`.
- `CreateOsmosisGauge` has no EVM equivalent (Osmosis-specific incentives module). Omit from the Solidity port.

### Discount Vault (Phase D)

- Per-user `VaultedLP[]` is a `mapping(address => VaultedLP[])` in Solidity.
- `EditAcceptedLPs` validates that each pool's underlying assets contain CDT (the debt token) before adding. Solidity port: query the LP token's pool config (e.g., Uniswap V3 `IUniswapV3Pool.token0() / token1()`) and require one matches the CDT token address.
- Currently NOT linked to System Discounts. The Solidity port should preserve this gap UNLESS the user resolves OPEN_QUESTIONS.md #4/#5 in favor of the docs (then add a query path).
- ToggleDeposits is an admin-only kill switch. Useful for emergency response.

### Tokenfactory (Phase D)

- Cosmos-only primitive — has no direct EVM equivalent. The Solidity port replaces TokenFactory denoms with deployed ERC20 contracts:
  - `CreateDenom(creator, subdenom)` becomes `factory.create(name, symbol, supplyCap)` returning a deployed ERC20 address.
  - `Mint(denom, to, amount)` becomes `IERC20Mintable(token).mint(to, amount)` with role check.
  - `Burn(denom, from, amount)` becomes `IERC20Burnable(token).burnFrom(from, amount)`.
  - `ChangeAdmin(denom, newAdmin)` becomes OZ `grantRole(MINTER_ROLE, newAdmin) + revokeRole(MINTER_ROLE, oldAdmin)`.
- The single-creator-only-admin invariant becomes a deployment-time guarantee: only the proxy contract holds the MINTER_ROLE for each token it created.

## Per-action concrete-vs-abstract status

For each organ, list every execute action and its status. **Concrete** = full state mutation in the spec (writes new computed values into Maps/Lists, mutates enum statuses, deducts/credits amounts, etc.). **Abstract** = stub whose body is dominated by reflexive `field' = field` assignments and only emits a reply or a `Msg_*` whose payload is a placeholder zero/empty value. **Semi** = partial: some real state is computed (e.g. fee math, status flip, index update) but the bulk of the lift is deferred to the reply chain or to an abstracted helper. The Solidity port author should treat **abstract** rows as pure TODOs whose bodies must be written from the Rust source, **semi** rows as a partial scaffold to extend, and **concrete** rows as a 1:1 transcription target.

`init` and `step` are excluded — they are spec scaffolding, not execute actions.

### CDP (organs/cdp.qnt)

| Action | Status | Notes for the Solidity port |
|---|---|---|
| UpdateConfig | concrete | Plain config replacement with `liq_fee` and `affiliate_fee_max` caps. |
| Deposit | concrete | Builds a new `Position` (or merges into existing) via `merge_all_funds` / `position_with_collaterals` and bumps `basket.current_position_id`. |
| IncreaseDebt | concrete | `RateSegment` append (peg-aware), `basket.credit_minted` bump, solvency + `debt_minimum` checks. |
| Withdraw | abstract | Body sets only `withdraw_propagation` + emits `WITHDRAW_REPLY_ID`. No collateral deduction or post-solvency check. Solidity port writes the full math. |
| Repay | concrete | `update_position_for_repay` reduces `pending_interest`, `basket_with_credit_subbed`, and emits the burn message. |
| Liquidate | abstract | Initializes `repay_propagation` and queues 4 reply messages. Per-asset repayment / fee-cap / sell-wall logic not in body. |
| Accrue | semi | Updates `rates.rates_last_accrued` / `credit_last_accrued` only. Per-position accrual + `pending_interest` bumps are not in the body. |
| ClosePosition | abstract | Only emits `SELL_COLLATERAL_REPLY_ID`. The percent-close swap and debt-clearance logic is the reply chain. |
| EditcAsset | abstract | Auth + `MAX_LTV_HARD_CAP` precondition only; `basket' = basket` (no mutation). Solidity must implement the actual cAsset row update. |
| EditBasket | concrete | Whole-basket replacement (owner-only). |
| SetAffiliate | concrete | Appends `AffiliateData` into `affiliations[position_id]` with cap + fee bound check. |
| FulfillBadDebt | concrete | `basket_with_bad_debt_subbed` + Tokenfactory burn + refund send. |
| TakeRevenue | concrete | Auth on `revenue_distributor`, zeroes `pending_revenue`, emits BankSend. |
| CheckAndClearDebtDelta | abstract | Auth check only; whole state copied through. Body must compute the delta and write the cleared row. |
| SetAcquisitionBumpRate | concrete | `rates_with_bump` mutates `acquisition_bump_rate`; `MAX_INTEREST_RATE_BPS` check. |
| CollateralRateAssurance | semi | Resets `collateral_rate_assurance' = Map()` (clears the snapshot) but does not assert the rate-non-decrease check the name implies. Solidity must add the actual assertion. |

### Collateral (organs/collateral.qnt)

| Action | Status | Notes for the Solidity port |
|---|---|---|
| AddAssetAtInstantiate | concrete | Inserts a new row into `basket` with `onboarding_window_end == 0`. |
| AddAssetAutonomous | concrete | Stamps `onboarding_window_end = block_time + onboarding_window_duration` and inserts. |
| UpdateAsset | concrete | Replaces row but enforces `onboarding_window_end` immutability across update. |

### Debt (organs/debt.qnt)

| Action | Status | Notes for the Solidity port |
|---|---|---|
| AccrueInterest | concrete | Computes `delta = cdt_minted * rate * elapsed / SECONDS_PER_YEAR` and bumps `pending_revenue` + `last_accrued`. |
| MintBorrow | concrete | Rolls the 24h borrow window if expired, enforces `daily_quota` cap, bumps `cdt_minted` + window mint counter. |
| TakeRevenue | concrete | Auth on `cdp_contract`, zeroes `pending_revenue`, emits `Msg_RevDist_AddRevenue`. |

### Liquidation Engine (organs/liquidation_engine.qnt)

| Action | Status | Notes for the Solidity port |
|---|---|---|
| UpdateConfig | concrete | Owner-or-governance auth, `MAX_LIQ_FEE` and positive-delay checks, full `Config` replacement. |
| Liquidate | concrete | Initializes `liquidation_propagation` and queues `DEBT_ACCRUE_REPLY_ID`. (Concrete because it sets the propagation row — but most of the 9-step lift sits in the replies.) |
| Reply_DebtAccrue (step 1) | concrete | Writes `total_debt`, `credit_price`, `venue_recall_amount` into the propagation struct via `update_prop_after_step1`. |
| Reply_CollateralAssess (step 2) | concrete | Writes 8 propagation fields, computes `LiquidationOutcome` from threshold math, mutates `liquidation_timers` (start vs. clear), appends a stats row when `Outcome_TimerStarted`. The richest reply step. |
| Reply_FeeTransfer (step 9) | abstract | Only queues `LIQ_QUEUE_TRANSFER_REPLY_ID`. |
| Reply_LqQueueTransfer (step 3) | abstract | Only queues `LIQ_QUEUE_REPLY_ID`. |
| Reply_LqQueue (step 4) | concrete | `add_repaid` mutates `total_repaid` and `liq_queue_repayment`, branches between next-asset / sell-wall / skip-to-step6. |
| Reply_SellWall (step 5) | concrete | `add_repaid` increments `total_repaid` only (no LQ counter). |
| Reply_DebtUpdate (step 6) | abstract | Only queues `CLAIM_UPDATE_REPLY_ID`. |
| Reply_ClaimUpdate (step 7) | semi | Records a stats row, clears `liquidation_propagation`, queues bad-debt step. The actual claim update math is deferred to the Solidity body. |
| Reply_BadDebtCheck (step 8) | abstract | Pure passthrough — every state field copied. |
| Callback_BadDebtCheck | semi | Conditionally queues `Msg_Auction_StartMBRNSale` based on `has_bad_debt and amount > 0`. No state mutation beyond `msg_queue`. |

### Liq Queue (organs/liq_queue.qnt)

| Action | Status | Notes for the Solidity port |
|---|---|---|
| AddQueue | concrete | Builds a fresh `Queue` with `initial_slots` (one `PremiumSlot` per premium tier) and inserts. |
| UpdateQueue | concrete | Replaces an existing queue's `max_premium` / `bid_threshold` (slots vector untouched per spec; Solidity port should resize). |
| UpdateConfig | concrete | Plain config replacement. |
| SubmitBid | concrete | Builds active or waiting `Bid`, appends to the chosen list, increments `current_bid_id`, capacity gate via `maximum_waiting_bids`. |
| RetractBid | abstract | Only locates the bid and emits a zero-amount `Msg_BankSend`. The actual bid removal + refund math is missing — Solidity must write it. |
| Liquidate | abstract | Auth + queue-exists check; emits zero-amount `Msg_Tokenfactory_Burn`. The premium-walk, pro-rata math, epoch/scale bumps are all missing. **Highest port-impact abstract in this organ.** |
| ClaimLiquidations | abstract | Emits zero-amount BankSend; the EPOCH_SCALE_SUM walk and per-bid claim math is missing. |

### Transmuter (organs/transmuter.qnt)

| Action | Status | Notes for the Solidity port |
|---|---|---|
| UpdateConfig | concrete | Operational-only mutation gated by `cfg_only_changes_operational(old, new)`. |
| EnterVault | semi | Mutates `acquisition_deposit_total`, `tranche_rate_assurance` snapshot, and `user_principal_base`. The actual VT mint into `tranche_states` / `user_tranches` is deferred to the rate-assurance reply. |
| ExitVault | semi | Decrements `acquisition_deposit_total` for acquisition path; the junior-intent / onboarding-window gating is enforced via preconditions. The actual VT burn + payout is deferred to the reply. |
| Transmute | concrete | Bumps `cumulative_volume` and `undistributed_fees`, computes fee split, emits revenue-distributor BankSend when configured. |
| AssignBadDebt | concrete | Junior-then-senior `total_staked` deduction via `min_int`, decrements `total_tranche_staked`, emits `Msg_CDP_FulfillBadDebt`. |
| RateAssurance | semi | Clears `tranche_rate_assurance` map; the actual non-decreasing-rate check (the namesake!) is structural — Solidity port must add the runtime assertion. |
| RepayUserDebt | abstract | Auth check only; full state pass-through. |
| TransferDepositOwnership | semi | Decrements `acquisition_deposit_total`. The deposit-ownership reassignment itself is not modeled. |
| SetTransmuterManager | concrete | Removes user from previous manager's `managed_users`, adds to new, mutates `transmuter_manager`. |
| RequestManagerFeeChange | concrete | Branches: decreases mutate `transmuter_manager_fee` instantly + emit `Ev_ManagerFeeChanged`; increases create `pending_fee_change` row + emit `Ev_ManagerFeeRequestCreated`. |
| ExecuteManagerFeeChange | concrete | Window check, applies pending fee, removes from `pending_fee_change`, appends `Ev_ManagerFeeChanged`. |
| CancelManagerFeeChange | concrete | Removes manager's pending row. |
| RequestTrancheUpdate | abstract | Validates `added_vt == removed_vt` net-zero, but does not actually move VTs between tranches — the lift is deferred to rate-assurance reply. |
| ClearTransmuterManagerFee | concrete | Removes manager's row from `transmuter_manager_fee` (with empty-managed-set guard). |
| RequestSwitch | concrete | Builds a `SwitchingIntent`, inserts into `switching_intents`, bumps `next_intent_id`, indexes via `user_switching_intents`. |
| ExecuteSwitch | semi | Removes the intent from `switching_intents` + index, queues rate-assurance reply. The actual VT burn/mint between tranches is the deferred reply body. |
| CancelSwitch | concrete | Removes intent from `switching_intents` + per-user index. |
| SyncCollateralOnboarding | concrete | Replaces `collateral_onboarding_end` with the supplied snapshot. |
| PruneExpiredIntents | concrete | Computes `expired_ids` set and excludes from both maps. |

### LTV Disco (organs/ltv_disco.qnt)

| Action | Status | Notes for the Solidity port |
|---|---|---|
| SubmitDeposit | concrete | Inserts `BackingDeposit`, indexes via `user_deposits` + `managed_deposits`. |
| RequestUnstake | concrete | Builds `UnstakeRequest` with `unlock_time` / `expire_time`, inserts into `unstake_requests`. Bad-debt freeze precondition. |
| CompleteUnstake | semi | Removes the unstake row and emits BankSend. The actual VT-burn-and-deposit-rewrite is not modeled in the body. |
| CancelUnstake | concrete | Removes the unstake row. |
| PruneExpiredUnstakes | concrete | Excludes expired keys from `unstake_requests`. |
| MoveDeposit | abstract | Auth + `not(is_lower_ltv_move)` gate; everything else is `field' = field`. The slot reassignment / VT rebalancing is missing. |
| UpdateDeposit | abstract | Auth gates only; pure passthrough. The deposit-mutation logic (owner / manager / revenue_destination updates) must be written. |
| AddRevenue | abstract | Pure passthrough — every state row copied. The per-slot `RevenueEvent` append is missing. **High port-impact: this is how revenue actually flows in.** |
| AddDepositTokenRevenue | abstract | Auth check only; pure passthrough. |
| AddBadDebt | concrete | Bumps `pending_bad_debt[asset]` and emits `Msg_Auction_StartMBRNSale`. |
| SendMBRNForSale | abstract | Auth check + emits zero-amount BankSend. |
| ClearBadDebtFreeze | concrete | Decrements `pending_bad_debt[asset]` via `sat_sub`. |
| ClaimRevenueForUser | abstract | Pure passthrough — no claim math. The revenue computation + compound-swap reply is unmodeled. |
| RequestManagerFeeChange | concrete | Same decrease-instant / increase-pending split as Transmuter; mutates `manager_fee` or `pending_fee_change` and emits the right event. |
| ExecuteManagerFeeChange | concrete | Applies pending fee, removes pending row, emits `Ev_ManagerFeeChanged`. |
| CancelManagerFeeChange | concrete | Removes manager's pending row. |
| CleanManagerFee | concrete | Removes manager's row from `manager_fee` when they have 0 managed deposits. |
| AcquisitionUnstake | semi | Auth + bad-debt-freeze checks; emits BankSend. Actual deposit / VT removal not modeled. |
| RateAssurance | semi | Clears `rate_assurance` map; the rate-non-decreasing check is missing in the body. |
| RequestLowerLTVMove | concrete | Builds `LowerLTVIntent`, inserts into `lower_ltv_intents`, indexes via `deposit_ltv_intent`. |
| ExecuteLowerLTVMove | semi | Removes the intent and queues rate-assurance reply. The slot-to-slot VT move is the deferred reply body. |
| CancelLowerLTVMove | concrete | Removes the intent from both maps. |
| PruneExpiredLTVIntents | concrete | Excludes expired ids and their deposit keys from both maps. |

### Acquisition (organs/acquisition.qnt)

| Action | Status | Notes for the Solidity port |
|---|---|---|
| StartAcquisitionWindow | concrete | Bumps `current_window_id`, builds new `AcquisitionWindow` with `U64MAX` lazy timers, resets state via `reset_state_for_new_window`. |
| Deposit | concrete | Sets first-deposit timers if needed, updates window total + state, inserts new `AcquisitionDeposit`, optionally writes user intents. |
| Withdraw | concrete | Removes or shrinks the deposit, decrements window total via `update_window_on_withdraw`, conditionally emits clawback path messages. |
| AccruePool | concrete | `update_state_on_accrue` writes `accrued_pool` / `current_acquisition_rate` / `bump_rate` / `last_bump_time`; bumps history length; emits CDP-bump-rate message when bump changed. |
| Claim | concrete | Removes the deposit, mints MBRN to user, queues transmuter ownership transfer. |
| SendAcquisitionRewardsToDisco | concrete | Marks the deposit `sent_to_disco` via `mark_sent_to_disco`, mints boosted MBRN to SELF. |
| UpdateConfig | concrete | Owner-only full config replacement. |
| AcquisitionRateAssurance | concrete | Pure precondition assertion that `transmuter_total` is within ±1 of the window total; no state mutation needed. |

### Governance (organs/governance.qnt)

| Action | Status | Notes for the Solidity port |
|---|---|---|
| SubmitProposal | concrete | Builds `Proposal` with full vote/timestamp record, decides Active vs. Pending bucket, bumps `proposal_count`. |
| CastVote | concrete | Quadratic-applies power, dispatches per `VoteOption` to add-helpers, promotes Pending→Active when `aligned_power >= proposal_required_stake`. |
| EndProposal | concrete | Computes quorum, for/amend/remove shares, expedited-extend branch, transitions status via `set_status` / `set_status_and_extend`. |
| ExecuteProposal | concrete | Status flip to `PS_Executed`, queues proposal-messages reply. The actual outbound calls are the reply body. |
| RemoveCompletedProposal | concrete | Removes from `proposals` or `pending_proposals` per status / expiry / submitter precondition. |
| UpdateConfig | concrete | Self-call only, full config replacement with all governance bound checks. |

### Staking (organs/staking.qnt)

| Action | Status | Notes for the Solidity port |
|---|---|---|
| Stake | concrete | Appends `StakeDeposit` to `staked[user]`, bumps `staking_totals.stakers`, lock-ceiling guard. |
| Unstake | semi | Marks every deposit `unstake_start_time = now` via `mark_unstaking`, queues a zero-amount mint message. The actual MBRN release after the cooldown is deferred. |
| Restake | concrete | `clear_unstaking` flips every deposit's flag back to `None`. |
| ClaimRewards | semi | `update_last_accrued` bumps each deposit's `last_accrued`; queues a zero-amount mint. The actual rewards math (looped over `fee_events`) is missing. |
| DepositFee | concrete | Computes `per_mbrn` rate (the O(1) trick) and appends `FeeEvent`. |
| UpdateDelegations | abstract | Commission-cap + self-delegation precondition only; `delegations' = delegations` passthrough. The bidirectional delegation math is missing. **High port-impact.** |
| DeclareDelegate | abstract | Auth check only; pure passthrough. |
| Lock | abstract | Lock-ceiling precondition only; `staked' = staked` passthrough. |
| RefreshLock | abstract | Existence check only; pure passthrough. |
| UpdateConfig | concrete | Owner / ownership-transfer auth, period / commission caps, full config replacement. |
| BuybackAndBurn | abstract | Toggle precondition + queues swap reply. The buyback-swap math is in the deferred reply. |
| TrimFeeEvents | concrete | Filters `fee_events` to entries with `time_of_event >= cutoff`. |

### Vesting (organs/vesting.qnt)

| Action | Status | Notes for the Solidity port |
|---|---|---|
| AddRecipient | concrete | Appends a fresh `Recipient` with no allocation. |
| RemoveRecipient | concrete | `remove_at` index removal, requires `allocation == None`. |
| AddAllocation | concrete | Builds new `Allocation` (or grows existing amount), `replace_at` rewrites the recipient, total-cap check. |
| WithdrawUnlocked | concrete | `unlocked_amount` formula, bumps `amount_withdrawn`, emits Tokenfactory mint. |
| AddVestedTransmutation | concrete | Builds or grows the weekly `VestingSchedule`, bumps `old_mbrn_received` + `total_old_mbrn`. Auth on `neutron_proxy`. |
| WithdrawVestedUnlocked | abstract | Only queues a zero-amount mint. The per-week schedule walk + `minted_mbrn` bump that satisfies the transmutation invariant is **missing — Solidity port author MUST implement** to keep `inv_transmutation_balance` true. |
| ClaimFeesforContract | abstract | Only queues a `claim_staking_rewards` reply. The fee-pull and per-recipient distribution is the reply body. |
| ClaimFeesforRecipient | abstract | Only queues a zero-amount BankSend. The per-recipient claimables walk is missing. |
| SubmitProposal | abstract | Recipient-existence check + queues `submit_proposal` reply forwarder. |
| CastVote | abstract | Recipient-existence check + queues `cast_vote` reply forwarder. |
| UpdateConfig | concrete | Owner / ownership-transfer auth, full config replacement. |

### Revenue Distributor (organs/revenue_distributor.qnt)

| Action | Status | Notes for the Solidity port |
|---|---|---|
| SetPromises | concrete | Appends new promises, accumulates `per_asset_revenue_attribution` and `epoch_revenue_accumulation`. |
| DistributePromises | semi | Snapshots `distribution_prop = promises`, branches on `bad_debt_active` to choose CDP-fulfill vs. affiliate-send reply, advances `last_distribution_time` when the dispersal window has passed. The actual splitting math is in the deferred replies. |
| Reply_Distribution | concrete | Pops head of `distribution_prop`, removes the matching promise, records failure into `failed_distributions[addr]` on `not(success)`, queues points reply on success. |
| TakeRevenueFromBasket | concrete | Snapshots `pre_take_revenue_balance = basket_balance`, queues `TAKE_REVENUE_REPLY_ID`. |
| ToggleBadDebtFulfillment | concrete | CDP-only auth, sets `bad_debt_active = toggle`. |
| WithdrawReserve | concrete | Owner auth, `sat_sub` from `reserve_accumulation`, emits BankSend. |
| UpdateConfig | concrete | Owner-or-governance auth, `split_sums_to_one` invariant guard, full config replacement. |

### Auction (organs/auction.qnt)

| Action | Status | Notes for the Solidity port |
|---|---|---|
| UpdateConfig | concrete | Owner / ownership-transfer auth + 4 discount/timeframe bound checks; full config replacement. |
| StartAuction | concrete | Inserts new `FeeAuction` or merges into existing (sums `auction_asset_amount`, preserves `auction_start_time`). |
| SwapForFee | concrete | `get_discount_ratio` decay math, computes `payout`, decrements `auction_asset_amount` (or removes the row when ≤1 dust), emits BankSend + proceeds-routing reply. |
| StartMBRNSale | concrete | LTV-Disco-only auth, builds new `MBRNSale` or appends `AssetBadDebtAllocation` to existing. |
| FulfillBadDebt | concrete | `find_alloc_idx` lookup, `applied = min(sent, bad_debt - fulfilled)`, increments `cdt_fulfilled`, removes allocation when satisfied (or replaces), wipes `mbrn_sale` when allocations empty. Emits debt notify / freeze-clear / refund. |
| BuySuppliedMBRN | semi | Recursion-depth + active-allocs guard; queues `Msg_Disco_SendMBRNForSale`. The sale-walk and `mbrn_used` accounting (the loop body) is not modeled. |
| RemoveAuction | abstract | Auth only; pure passthrough. The `fee_auctions` row removal is missing. |

## Runtime invariant scaffolding

The Quint invariant catalog is lifted into Solidity via per-organ libraries at
`/Users/EBmic/membrane-solidity/contracts/invariants/<Organ>Invariants.sol`. Each
`val inv_<name>` becomes one `internal pure`/`view` function with the same name.
The libraries are callable from BOTH (a) production contract bodies — as
defense-in-depth runtime guards — AND (b) Foundry handler `invariant_*()` tests.
This is the single source of truth: if formal verification missed a bug, the
runtime check is the second line of defense.

### (a) Cost classifier

Every `inv_*` is annotated in its library docstring with a cost class. The class
determines whether the invariant is safe to call in production.

| Class | Heuristic | Production policy |
|---|---|---|
| `cheap (≤2 SLOAD)` | scalar bound, single SLOAD/struct load | Always-on. Call inside the contract's state-changing functions. |
| `expensive (≥5 SLOAD)` | iterates a small bounded array (collateral types, pending fee changes) | Behind `if (DEBUG_INVARIANTS) { ... }`. Default OFF in mainnet, ON in fork tests and staging. |
| `ghost (iterates maps)` | iterates a Solidity map (no native iteration) or sums all bids/positions/balances | FV-only. Do NOT call from production. Only the handler test reconstructs the state and evaluates. |

Mark the class on every new invariant the day you add it.

### (b) Wiring `_checkInvariants()` calls into a contract

Each organ contract should declare a thin internal wrapper:

```solidity
import {CdpInvariants} from "./invariants/CdpInvariants.sol";
import {Invariants} from "./invariants/Invariants.sol";

contract Cdp {
    bool internal constant DEBUG_INVARIANTS = false; // flip on for staging

    function _checkInvariants() internal view {
        // 1. cheap: always
        require(CdpInvariants.inv_credit_minted_nonneg(_basket()),  "INV_CDP_CREDIT_NEG");
        require(CdpInvariants.inv_owner_governance_distinct(_cfg()), "INV_CDP_OWNER_GOV");
        require(CdpInvariants.inv_liq_fee_capped(_cfg()),            "INV_CDP_LIQ_FEE");
        // 2. expensive: gated
        if (DEBUG_INVARIANTS) {
            require(CdpInvariants.inv_max_ltv_capped(_basket()),      "INV_CDP_MAX_LTV");
            require(CdpInvariants.inv_supply_caps_respected(_basket()), "INV_CDP_SUPPLY_CAP");
        }
        // 3. ghost: never — handler tests cover these.
    }

    function deposit(...) external {
        // ... business logic ...
        _checkInvariants();   // last line of every state-changing external
    }
}
```

The Foundry handler test calls into the SAME library predicates from
`safety()` (already wired in `test/invariants/<Organ>Invariants.t.sol`) so the
test and the runtime check cannot drift apart.

### (c) Failure-mode policy

Each `inv_*` is annotated `FAIL-MODE: revert | circuit-breaker | log` in the
library docstring. The production wrapper picks the matching policy:

| FAIL-MODE | Use when | How to wire |
|---|---|---|
| `revert` | Bound on a config field or scalar that should never be violated mid-tx (cap, monotone counter, owner != gov) | `require(CdpInvariants.inv_*(...), "INV_*");` |
| `circuit-breaker` | Aggregate solvency / supply / bad-debt gauge — violation means the system is in a state where new operations should be paused while a guardian investigates | Set a `paused` flag and emit; do NOT revert the current tx unilaterally if doing so would lock user funds. Pattern: write the flag and continue, with a `whenNotPaused` modifier blocking subsequent state-changing entrypoints. |
| `log` | Ghost-only invariants surfaced from off-chain iteration, or sanity checks that may have transient false positives during reorgs / multi-step replies | `if (!CdpInvariants.inv_*(...)) emit InvariantViolated(InvariantId.CDP_*);` and continue. Off-chain monitor consumes the event. |

The shared aggregator `contracts/invariants/Invariants.sol` exposes:

- `enum InvariantId` — stable, append-only IDs covering all 155 organ invariants
  (sentinel `_SENTINEL_END` last). New invariants MUST be appended; existing IDs
  must never be reordered (off-chain dashboards index by ordinal).
- `_checkAll()` returning `bool[] memory failures` indexed by `InvariantId`.
- `_markFailed`, `_allHeld` helpers for composing per-contract checks.

A contract whose policy mixes revert/circuit-breaker/log should call the per-org
predicate, mark failures into the `failures[]` array via
`Invariants._markFailed(failures, InvariantId.X)`, then dispatch on policy at
the end (e.g. `require(noRevertFailure, "...")`, `if (anyCBFailure) _pause()`,
emit per-log entry).

## Post-port checklist

Before tagging the Solidity port as production-ready:

- [ ] Every Quint invariant has a corresponding Foundry test.
- [ ] Every Lean theorem in `lean/MembraneSpec/Arithmetic/` has a Certora rule on the Solidity port.
- [ ] `scripts/check_drift.sh` is clean.
- [ ] `forge test --match-test invariant_* --runs 5000` passes.
- [ ] Certora workflow passes on the latest commit.
- [ ] Every entry in `CAPITAL_LOSS_SURFACE.md` has a Solidity-side mitigation: hard-cap constant, two-role split, timelock function, or onboarding window. No row may end up unmitigated on the Solidity port.
- [ ] The 14d + 2d switching windows (Amendments 1, 2, 3, 6) are wired through `block.timestamp` checks on every action that touches their preconditions.
- [ ] The Amendment 5 oracle circuit breaker is consulted via a `priceSafe(denom)` modifier on every CDP/liquidation/Transmuter price read.
