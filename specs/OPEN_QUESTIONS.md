# Open spec questions

Questions that are intentionally left unanswered so the team can resolve them before the corresponding Quint actions/invariants are finalized. Each item lists the concrete candidate resolutions and the tradeoffs we already know about.

Update this file — or resolve an entry by moving it into the relevant organ's `.qnt` module — whenever a question is decided.

---

## 1. New collateral onboarding — how does the CDP set the starting LTV without Disco?

### Context

- Today `contracts/collateral/` and `contracts/cdp/` consult the LTV Disco for dynamic LTV targets via `QueueAverageLTV` (`docs/organs/ltv-disco/ltv-disco.md:245-248`). The result is a deposit-weighted average LTV across all Disco depositors in that asset queue.
- At onboarding, a fresh collateral has **no Disco queue or zero deposits** — `QueueAverageLTV` has no sensible value to return. The CDP still needs a `max_ltv` and `max_threshold_to_delay` to accept positions.
- We need a clear specification for:
  1. The CDP's *starting* LTV parameters for a brand-new collateral.
  2. The transition to Disco-driven dynamic LTV once Disco coverage exists.
  3. The fallback semantics if a previously-covered collateral's Disco queue empties out (rare but possible after full unstake and bad-debt clearance).

### Candidate resolutions

The spec does **not** prescribe one. Below is an enumeration of the options we have considered so far. If one is chosen, move the corresponding action/invariant skeleton into `quint/organs/collateral.qnt` and/or `quint/organs/cdp.qnt` and add the SPEC_MAP rows.

| # | Approach | Pros | Cons |
|---|---|---|---|
| A | **Bootstrap constant in CDP Collateral config**: each asset carries `initial_max_ltv` + `initial_max_threshold_to_delay`. If `QueueAverageLTV` returns zero / `NotFound`, fall back to these. No timed transition — dynamic values always win once non-zero. | Simplest. No new state machine. Matches how many CDP protocols handle cold starts. | No formal "activation" event — auditor has to reason about the fallback branch. The bootstrap values sit in config forever. |
| B | **Phased activation**: explicit `CollateralPhase = Bootstrap \| Dynamic` stored per asset. Bootstrap uses admin-set constants; transition happens when Disco TVL crosses a configured threshold (or governance flips it). | Clean audit story: "this asset is in phase X with these rules". Lets us harden the transition with an invariant (one-way phase change during normal operation). | New state and a new execute message (transition). Governance / threshold decision is another design point. |
| C | **Governance-set floor**: dynamic LTV is always used, but lower-bounded by a governance-set `min_max_ltv` floor. At onboarding the floor is the effective max LTV. | Monotonic behavior. No phase flag. | During zero-Disco phase, effective LTV is always `floor` — the `QueueAverageLTV` branch is dead until someone deposits. Auditor must notice. |
| D | **Gate borrowing until Disco coverage exists**: new collateral cannot back debt until Disco queue non-empty and above a threshold. | Protocol-safe — real risk capital before real debt. | Worst UX at launch; blocks the normal flywheel where CDP revenue seeds Disco. |
| E | **Oracle-derived initial LTV**: compute initial LTV from realized-vol / liquidity data supplied by the oracle. | Principled, removes admin judgment from the value. | Oracle surface expands significantly; delay-before-first-borrow while volatility window fills. |

### What the spec needs regardless of choice

- A CDP-side invariant: `max_ltv_for(asset) > 0` **before** any `IncreaseDebt` against that asset succeeds.
- A structural note for `SendMBRNForSale` / `AssignBadDebt`: during bootstrap, bad-debt waterfall still has to have a sink. If Disco is empty, the transmuter's tranches absorb as today (`contracts/transmuter/src/contract.rs:827-894`). Confirm this path is reachable without Disco setup.
- A clearly-named query: `CurrentMaxLTV(asset)` returning `(ltv, threshold_to_delay, source: Enum{Config, Disco, Oracle})` so the audit and tests can inspect which branch fired.

### Partial resolution (2026-04-24) — the onboarding-window safety belt

Independent of which LTV approach above is chosen (A–E), a **Transmuter-side protection** was added to cover the risk on any asset added via autonomous onboarding:

- Every cAsset carries an `onboarding_window_end: Timestamp`.
- Assets added at contract instantiation have `onboarding_window_end = 0` — always "already onboarded", no restrictions.
- Assets added at runtime via `AddAssetAutonomous` have `onboarding_window_end = now + config.onboarding_window_duration`.
- While `block_time < onboarding_window_end`, the Transmuter `ExitVault` gate requires a switching-intent for **every** tranche exit of that asset, junior AND senior. Seniors lose their "withdraw freely" privilege for the duration of the window.
- The field is **immutable** post-creation (enforced in `collateral.qnt::UpdateAsset` precondition) — an admin cannot reopen the window on an asset after it has closed.

This gives the system a safe margin on freshly autonomous-onboarded cAssets: senior LPs cannot flee the Transmuter's bad-debt waterfall during the first `onboarding_window_duration` of that asset's life. The final LTV choice (A–E) still needs to land for the `CDP.inv_position_solvent_before_mint` shape, but the senior-withdraw risk is bounded immediately.

New invariants (Quint):
- `COL.inv_instantiate_assets_bypass_onboarding` — instantiate-path sets `onboarding_window_end == 0`.
- `COL.inv_autonomous_windows_in_future_when_created` — autonomous-path stamp is `now + duration >= now`.
- `COL.inv_onboarding_end_immutable` — no action may mutate `onboarding_window_end` post-creation.
- `TR.inv_onboarding_exit_gated` — all tranche exits during the window require an intent.
- `TR.inv_senior_free_withdraw_post_onboarding` — outside the window, seniors are free.
- `TR.inv_onboarding_mirror_nonneg` — Transmuter's mirror of Collateral's `onboarding_window_end` holds non-negative values.

### Decision owner / target

Core LTV resolution (A–E) still TBD. Recommend deciding before Transmuter + CDP organ specs are closed, since `CDP.inv_position_solvent_before_mint` depends on the chosen semantic. The onboarding-window safety belt does NOT replace this decision — it narrows the blast radius of whatever choice is made.

---

## 2. Manager bad-debt / fee history — storage-free, indexer-friendly

### Context

- We want historical charts and leaderboards for manager P&L (fees earned, bad-debt absorbed on their managed users).
- We do **not** want this history stored on-chain. The existing `MANAGER_PERFORMANCE: Map<Addr, Vec<ManagerPerformanceEntry>>` (`contracts/ltv_disco/src/state.rs:77-78`) is a candidate for removal in a follow-up Disco cleanup; at minimum, no new storage of this shape should be added.
- Instead, every relevant state change emits a CosmWasm event with a structured attribute set that indexers can parse.

### Resolved event schema

Spec'd in the Quint layer (`specs/quint/lib/types.qnt :: Event`) and documented in `specs/README.md`. Event-emission invariants are in `SPEC_MAP.md` under `MGR.inv_*`. See those for the canonical contract.

### Open question

Whether the legacy `MANAGER_PERFORMANCE` storage should be deleted outright or kept as a migration target until indexers are verified in production. Spec assumes deletion; add a migration story if the team prefers to keep it as a safety belt during rollout.

---

## 3. Volatility tracker — SUSPENDED

### Context

A natural extension of the TWAL oracle (`specs/quint/shared/twal_oracle.qnt`) would be a parallel volatility tracker — a per-asset rolling buffer of price observations producing a realized-volatility metric for use in dynamic LTV / liquidation-threshold tuning.

### Decision (2026-04-30)

**Suspended.** Do not implement the volatility tracker at this time. Rationale:

- The oracle circuit breaker (Amendment 5) plus the TWAL-driven supply caps already address the highest-priority capital-protection concerns.
- A volatility tracker introduces a new param surface (window length, weight scheme, threshold mappings) that would expand the access-control review burden before its risk benefit is proven.
- The TWAL buffer shape is reusable if/when the team revisits this — same circular-buffer + bucket-deduplication semantics will apply to a price observation log.

### What the spec MUST NOT do

- Add any `var volatility_for: ...` or `VOLATILITY_BUFFER` storage.
- Add any `Vol_*` event variant.
- Reference a volatility metric from any organ's invariant or action.

If the team un-suspends this, append a fourth question section (or move this one) and write the spec module before adding state.

## 4. Docs/code mismatch — System Discounts formula

### Context

`docs/README.md:131-137` describes the discount as a **two-phase time curve**:

```
discount = first_month_discount * min(effective_days, 30) / 30
         + remaining_discount * min(max(effective_days - 30, 0), remaining_duration) / remaining_duration
effective_days = days_since_deposit + lock_duration_days
```

The Rust implementation at `contracts/system_discounts/src/contracts.rs:219-313` (`get_discount`) uses a **linear MBRN-supply curve** with timed/static priority overrides:

```
discount = min((wallet_mbrn + disco_deposits) / (total_supply * ratio), 1.0) * max_discount
```

Time, lock duration, and Discount Vault deposits are NOT inputs to the actual formula. `docs/cross-organ/flywheel.md:12` references "45% discount by month 1, 75% by month 3" which the code cannot deliver.

### Decision needed

Which is canonical — the docs or the code?

| Option | What changes |
|--------|--------------|
| **Code is right** | Update `docs/README.md:131-137`, remove the time-curve narrative from `flywheel.md`, mark `Config.minimum_time_in_network` as unused, remove the lock-duration-feed planning. |
| **Docs are right** | Implement the time curve in Rust: deposit timestamps per user, query `staking_contract` and `ltv_disco_contract` for lock_duration, query `discount_vault_contract` for LP deposits and add to `effective_days`. The skeleton already names a `discount_vault_contract: Vec<Addr>` field that is currently unused. |
| **Hybrid** | Keep the linear MBRN curve as the floor; layer the time-based component on top so loyal borrowers get an additional bonus. Largest implementation cost. |

### Resolved spec position

Until decided, the Quint module mirrors the **code** (`shared/system_discounts.qnt` Amendment 6). When the team picks an option, update both this doc and the spec.

## 5. Discount Vault link to System Discounts (companion to #4)

### Context

The contract `contracts/discount_vault/src/contracts.rs` stores `VaultedLP { gamm, amount, deposit_time }` per user but never updates `system_discounts`. `system_discounts` does not query Discount Vault either — it only queries `ltv_disco_contract` for `disco_tokens`. Yet the docs implicitly use Discount Vault deposits as a flywheel input.

### Decision needed

Resolve as part of #4. If "code is right", document the disconnect and consider deprecating Discount Vault. If "docs are right" or "hybrid", add the cross-contract query.

## 6. Suspected `SetDiscountPeriod` duration unit bug

### Context

`contracts/system_discounts/src/contracts.rs:330` computes `duration_seconds = duration * 60`. The parameter is documented as **hours**. `1 hour * 60 = 60 seconds`, not 3600. Either:
- The doc comment is wrong and `duration` is in minutes (then code is correct).
- The code is wrong and should be `duration * 3600`.

### Decision needed

Confirm with the implementer's intent and either fix the comment or fix the constant. The Quint module mirrors the existing code (`* 60`) and flags this for review.

## 7. (reserved) — future open questions

Append new sections as they come up.
