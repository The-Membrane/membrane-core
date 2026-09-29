# Capital-loss surface — admin/governance attack model

This document enumerates every parameter change or admin action across the protocol that **can result in user capital loss if executed maliciously or in error**, and the mitigation each one is gated by under the spec.

The top-level invariant the spec enforces (`AC.inv_no_unilateral_capital_loss_path`):

> No single address (owner, governance signer, manager, oracle operator) can, in a single transaction or before a publicly observable mitigation window elapses, take an action that results in user capital loss.

Mitigations recognized by the spec — every Category A/B row below MUST be covered by at least one:

| Code | Mitigation | What it gives the user |
|------|------------|-------------------------|
| **TL** | **Timelock** — request/execute pattern (14-day request + 2-day permissionless execute, mirroring the manager-fee-change pattern in Amendment 2). | A window to exit before the change takes effect. Permissionless execute means even an offline admin can't slip a change in past the window. |
| **GOV** | **Governance multisig** — `info.sender == config.governance`, where `config.governance` is the multi-sig DAO address, distinct from `config.owner`. | Multiple independent signers required; no rogue single key. |
| **CAP** | **Hard contract-level cap** — value bounded above (or below) by a constant baked into the contract code, immutable by either owner or governance. | Even with both governance and a timelock breach, the magnitude is bounded. |
| **WIN** | **Onboarding/switching window** — already-spec'd 14d+2d pattern around the action itself (e.g. `onboarding_window_end`, `switching_period`). | Senior LPs can flee before the change is effective. |
| **N/A** | Not applicable — the action does not cause direct capital loss (cosmetic/operational). | — |

A row is **violating** the top-level invariant if it lists no mitigation, or only mitigations the contract does not actually enforce.

---

## A. Direct capital-loss vectors

| # | Organ / Contract | Action / Param | Loss mechanism | Required mitigation | Spec invariant |
|---|---|---|---|---|---|
| A1 | Oracle | `SetPrice(denom, price)` | Adversarial price → triggers wrongful liquidations OR lets attacker mint excess CDT against under-priced collateral | **GOV** + **CAP** (max-deviation-per-update bound) + **CB** (auto-trip circuit breaker, Amendment 5) | `OR.inv_setprice_governance_only`, `OR.inv_max_deviation_capped`, `OR.inv_trip_threshold_at_least_max_deviation`, `OR.inv_cb_only_governance_resets` |
| A1b | Oracle | Stale or anomalous feed used downstream | Liquidation engine fires on bad data | **CB** (Amendment 5: `price_safe` predicate consumers MUST call) | `OR.inv_consumers_use_price_safe` (structural — every CDP / liquidation / Transmuter price read goes through `price_safe`) |
| A2 | Tokenfactory / Osmosis-Proxy / Neutron-Proxy | Add `owner_contract` (mint authority) | Granted address can mint CDT/MBRN out of thin air | **GOV** | `OX.inv_authority_grant_governance_only`, mirror in `NX` |
| A3 | Collateral | `AddAssetAtInstantiate` | Adds toxic collateral asset with cooked params | **GOV** + **CAP** (max_ltv ≤ MAX_LTV_HARD_CAP, default 90%) — instantiate-only path | `COL.inv_instantiate_collateral_governance_only`, `COL.inv_max_ltv_hard_cap` |
| A4 | Collateral | `AddAssetAutonomous` | Adds toxic collateral post-instantiate | **WIN** (`onboarding_window_end > now`) + **CAP** (max_ltv ≤ MAX_LTV_HARD_CAP) — no GOV required because the WIN bounds blast radius | `COL.inv_autonomous_window_set` (already), `COL.inv_max_ltv_hard_cap` |
| A5 | Collateral | `UpdateAsset` raises `max_ltv` | Existing positions become under-collateralized at next liquidation pass | **TL** + **CAP** | `COL.inv_max_ltv_increase_timelocked` |
| A6 | Collateral | `UpdateAsset` switches `oracle_source` | Reroutes price feed to attacker oracle | **GOV** + **TL** | `COL.inv_oracle_switch_governance_and_timelocked` |
| A7 | Liquidation Engine | Update `liq_fee` upward | Liquidators / protocol siphon more on each liquidation | **TL** + **CAP** | `LE.inv_liq_fee_increase_timelocked`, `LE.inv_liq_fee_capped` |
| A8 | Debt | Update interest-rate base upward | Existing debt accrues at higher rate, faster insolvency | **TL** + **CAP** (100%/yr — Amendment 5 raised from 50%) | `DBT.inv_rate_increase_timelocked`, `DBT.inv_interest_rate_capped` |
| A8b | Debt | Update comparative-rate base upward | Same loss mechanism applied via cross-asset spread | **TL** + **CAP** (100%/yr) | `DBT.inv_comparative_rate_capped` (Amendment 5) |
| A8c | Debt | Update `borrow_limit_pct` upward | Faster mint flow → less time for liquidations / risk teams to react | **TL** + **CAP** (≤ 10%/day, code-immutable) | `DBT.inv_borrow_limit_pct_capped` (Amendment 5) |
| A9 | Debt | Update `debt_cap` downward | Forces liquidations / blocks repayments mid-flight | **TL** | `DBT.inv_debt_cap_decrease_timelocked` |
| A10 | Transmuter | Update `usage_fee` upward | Each LP swap loses more value to fee | **TL** + **CAP** (≤ 5%) | `TR.inv_usage_fee_increase_timelocked`, `TR.inv_usage_fee_capped` |
| A11 | Transmuter | Update `revenue_distributor_fee_percentage` upward | Diverts vault yield away from LPs | **TL** + **CAP** (≤ 50%) | `TR.inv_revdist_pct_increase_timelocked`, `TR.inv_revdist_pct_capped` |
| A12 | Transmuter | Switch `deposit_pair.paired_asset` | New paired asset could be valueless / attacker-controlled | **GOV** | `TR.inv_paired_asset_switch_governance_only` |
| A13 | Transmuter | Update `cdp_contract` | Reroutes the privileged CDT→paired swap callsite to attacker-controlled contract | **GOV** + **TL** | `TR.inv_cdp_addr_switch_governance_and_timelocked` |
| A14 | Transmuter | Update `acquisition_contract` | Grants new address the deposit-bypass + ExitVault auth | **GOV** + **TL** | `TR.inv_acquisition_addr_switch_governance_and_timelocked` |
| A15 | Transmuter | Add to `allowlist` | Allowlisted addresses skip usage_fee → revenue diversion | **TL** | `TR.inv_allowlist_add_timelocked` |
| A16 | Transmuter | Update `max_management_fee` upward | Future manager fee hikes can be larger | **TL** + **CAP** (≤ 25%) | `TR.inv_max_mgt_fee_increase_timelocked`, `TR.inv_max_mgt_fee_capped` |
| A17 | Transmuter | Update `switching_period` downward | Shrinks the safety window for junior LPs / onboarding seniors | **TL** + **CAP** (≥ 7 days) | `TR.inv_switching_period_decrease_timelocked`, `TR.inv_switching_period_floor` |
| A18 | LTV Disco | Update `unstaking_period` downward | Lets MBRN holders flee bad-debt waterfall faster | **TL** + **CAP** (≥ 7 days) | `DI.inv_unstaking_period_decrease_timelocked`, `DI.inv_unstaking_period_floor` |
| A19 | LTV Disco | Update `max_management_fee` upward | Future hikes can be larger | **TL** + **CAP** (≤ 25%) | `DI.inv_max_mgt_fee_increase_timelocked`, `DI.inv_max_mgt_fee_capped` |
| A20 | LTV Disco | Update `auction_contract` | Grants slashing authority to attacker contract | **GOV** + **TL** | `DI.inv_auction_addr_switch_governance_and_timelocked` |
| A21 | LTV Disco | `CreateQueue` / `UpdateQueue` widens LTV range | New riskier slot lures depositors who absorb future bad debt | **TL** + **CAP** (max LTV slot ≤ MAX_LTV_HARD_CAP) | `DI.inv_queue_range_widen_timelocked`, `DI.inv_queue_range_capped` |
| A22 | Acquisition | Update `emission_max` upward | Excess MBRN minting → MBRN holder dilution | **TL** + **CAP** | `AQ.inv_emission_max_increase_timelocked`, `AQ.inv_emission_max_capped` |
| A23 | Acquisition | Update `transmuter` / `disco` addresses | Reroutes user deposits to attacker contracts | **GOV** | `AQ.inv_routing_addr_switch_governance_only` |
| A24 | Revenue Distributor | Update split / recipients | Diverts revenue to attacker-controlled addresses | **GOV** + **TL** | `RD.inv_recipient_switch_governance_and_timelocked` |
| A25 | Staking | Update unbonding period downward | Lets stakers flee with fee share before slashing | **TL** + **CAP** (≥ 7 days) | `ST.inv_unbond_decrease_timelocked`, `ST.inv_unbond_floor` |
| A26 | All organs | Transfer `config.owner` | New owner inherits all routine powers | **GOV** + **TL** | `AC.inv_owner_transfer_governance_and_timelocked` |
| A27 | All organs | Transfer `config.governance` | New governance inherits all catastrophic powers | **GOV** (≥ 2-of-N current set) + **TL** | `AC.inv_governance_transfer_double_gated` |

## B. Indirect capital-loss vectors (compounded effects)

| # | Organ / Contract | Action / Param | Loss mechanism | Required mitigation | Spec invariant |
|---|---|---|---|---|---|
| B1 | Transmuter | Update `cdt_target_ratio` | Forces deposit-alignment errors that lock LPs out of EnterVault | **TL** | `TR.inv_target_ratio_timelocked` |
| B2 | Transmuter | Update `composition_leeway` downward | Same as B1 — narrows entry path | **TL** | `TR.inv_composition_leeway_timelocked` |
| B3 | LTV Disco | Update `affiliate_fee` upward | Diverts a slice of revenue from depositors | **TL** + **CAP** (≤ 5%) | `DI.inv_affiliate_fee_capped` |
| B4 | LTV Disco | Update `chain_proxy_contract` | Compound swaps execute on attacker-controlled router | **GOV** + **TL** | `DI.inv_chain_proxy_governance_and_timelocked` |
| B5 | Manager (per-deposit/user) | RequestManagerFeeChange (increase) | Fee hike on existing managed users | **TL** (already in Amendment 1/2) | `MGR.inv_fee_increase_timelocked` |
| B6 | LTV Disco | Slot reorder / `UpdateQueue` shifts | Could change weight distribution in adverse way | **TL** | `DI.inv_slot_reorder_timelocked` |
| B7 | All organs | Disable rate-assurance via config | Removes the safety check on VT-rate decrease | **CAP** — rate-assurance is structural, not configurable. (Witnessed: no config field exists to disable it.) | `AC.inv_rate_assurance_not_configurable` |
| B8 | Oracle | Update staleness window upward | Stale prices accepted → bad liquidation decisions | **TL** + **CAP** (≤ 1 hour) | `OR.inv_staleness_capped` |
| B9 | TWAL Oracle | Tampered Uniswap pool address mapping | Reroutes supply-cap input to attacker-controlled pool | **GOV** | `TWAL.inv_pool_mapping_governance_only` |
| B10 | TWAL Oracle | Shorten `twal_window_s` | Lets recent liquidity spikes inflate supply caps | **TL** + **CAP-floor** (≥ 7 days, suggested) | `TWAL.inv_window_floor` |

## C. Operational changes (no direct capital loss; documented for completeness)

| # | Organ | Action | Mitigation needed | Why none |
|---|---|---|---|---|
| C1 | Transmuter | Update `swap_history_cap` / `volume_history_cap` | None | Bookkeeping caps — affects query pagination, not user funds. |
| C2 | LTV Disco | Tracker caps (TVL, daily deposit, etc.) | None | Same as C1. |
| C3 | All organs | Pausing / circuit-breaker | None for activation; **TL** for permanent enable in non-emergency | Pause is a safety feature; abuse is a liveness issue, not capital-loss. |
| C4 | All organs | Updating event/attribute labels | None | Off-chain breakage only. |

---

## Top-level invariant: `AC.inv_no_unilateral_capital_loss_path`

For every action `a` in `Category A ∪ Category B`:

```
∃ mitigation m ∈ {TL, GOV, CAP, WIN} : a satisfies m
```

Equivalently in Quint:

```quint
val inv_no_unilateral_capital_loss_path: bool =
  dangerous_actions.forall(a => mitigation_satisfied(a))
```

Where `dangerous_actions` is the union of A and B above and `mitigation_satisfied` is a per-action predicate defined in `specs/quint/lib/access_control.qnt`. Each per-organ invariant in the table is a structural witness that the corresponding `a` carries its required mitigation.

---

## Two-role split: `owner` vs `governance`

Every organ's `Config` gains a second authority address:

| Field | Used for | Recommended holder |
|-------|----------|--------------------|
| `owner` | Routine config tweaks (caps, allowlist add/remove with TL, history sizes, etc.) | Multisig (3-of-5 ops) |
| `governance` | Catastrophic actions (oracle/CDP/proxy address switches, owner transfer, instantiate-only collateral additions, mint authority grants) | DAO multisig (e.g. 5-of-9 with timelock); ideally on-chain governance contract |

The two-role split is **enforced structurally** by the spec — no action requires only `info.sender == config.owner` for a Category-A item. Existing handlers that today take `info.sender == config.owner` (e.g. `UpdateConfig` in the Transmuter at `contracts/transmuter/src/contract.rs:1675-1680`) must be split: routine fields stay on `owner`; Category-A fields move to `governance` and route through the timelock.

## What still requires manual review at audit time

1. **Multisig setup correctness**: the spec invariants ensure `governance` is a separate Addr; whether the multisig is healthy (fresh keys, off-chain key custody) is out of scope.
2. **Oracle cartel**: `OR.inv_setprice_governance_only` does not stop the multisig itself from being adversarial. CAP via max-deviation-per-update (`OR.inv_price_deviation_capped`) is the structural defense.
3. **Token-factory minting**: even with GOV gating on grants, an existing grantee can mint indefinitely. A burn-rate cap is OUT of scope here — track as a follow-up if minting velocity becomes a vector.

---

## Implementation note for the Rust pass

This spec adds:
- A new `Config.governance: Addr` field to every organ.
- A `RequestConfigChange` / `ExecuteConfigChange` / `CancelConfigChange` triplet on every organ that holds Category-A/B fields. Body mirrors the manager-fee-change pattern from Amendment 2: 14d wait + 2d permissionless execute window + decrease-instant for fields where applicable.
- Hard caps on the values listed (constants, not config — code-level immutable).

The ASCII table above is the audit-time source of truth. When a new param is added to any organ, append a row here BEFORE writing the corresponding execute handler.
