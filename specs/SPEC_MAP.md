# SPEC_MAP

Invariant → Rust source index. Every row pins a Quint invariant (or Lean theorem) to the specific Rust file and line range it mirrors, plus the commit hash the spec was last synchronized against.

When code in the referenced lines changes, the corresponding spec must be updated in the same PR and the `Last synced commit` column bumped.

`c87c2ff70035603998fdb9d7d77d07b68339d8df` means the row was authored before the first sync pass; set it to the current `HEAD` once the organ's Quint module is reviewed end-to-end.

## Transmuter (`quint/organs/transmuter.qnt`)

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| TR.inv_tranche_sum_matches_total | organs/transmuter.qnt:inv_tranche_sum_matches_total | contracts/transmuter/src/state.rs:16,22; packages/membrane/src/transmuter.rs:18-23 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | `TOTAL_TRANCHE_STAKED` == Σ `TrancheState.total_staked`. |
| TR.inv_bad_debt_never_underflows | organs/transmuter.qnt:inv_bad_debt_never_underflows | contracts/transmuter/src/contract.rs:827,859-872 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | AssignBadDebt must not drive `total_staked` below zero; waterfall: junior absorbs first, senior takes remainder. |
| TR.inv_bad_debt_within_max | organs/transmuter.qnt:inv_bad_debt_within_max | contracts/transmuter/src/contract.rs:845-850; src/error.rs:48-49 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | `AssignBadDebt.amount <= junior.total_staked + senior.total_staked`. |
| TR.inv_rate_monotonic | organs/transmuter.qnt:inv_rate_monotonic | contracts/transmuter/src/contract.rs:898-933; src/error.rs:51-52 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per-tranche: `post_rate >= pre_rate` at `RateAssurance` self-call. |
| TR.inv_cdt_paired_swap_restriction | organs/transmuter.qnt:inv_cdt_paired_swap_restriction | contracts/transmuter/src/contract.rs:937-1030 (CDT→paired branch 959-964); src/error.rs:33-34 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | CDT→paired_asset Transmute callable only by CDP or self. |
| TR.inv_exit_user_is_sender | organs/transmuter.qnt:inv_exit_user_is_sender | contracts/transmuter/src/contract.rs:525-528,550 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | `ExitVault.user` override only for `sender == contract` or `sender == acquisition_contract`. |
| TR.inv_repay_auth | organs/transmuter.qnt:inv_repay_auth | contracts/transmuter/src/contract.rs:1156-1159 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | `RepayUserDebt.sender ∈ {user_info.position_owner, cdp_contract}`. |
| TR.inv_transfer_ownership_auth | organs/transmuter.qnt:inv_transfer_ownership_auth | contracts/transmuter/src/contract.rs:1238-1244 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | `TransferDepositOwnership` callable only by `acquisition_contract`. |
| TR.inv_usage_fee_applied_or_allowlisted | organs/transmuter.qnt:inv_usage_fee_applied_or_allowlisted | contracts/transmuter/src/contract.rs:972-995 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Non-allowlisted senders pay `usage_fee * input`; documented utilization threshold unused. |
| TR.inv_deposit_alignment | organs/transmuter.qnt:inv_deposit_alignment | contracts/transmuter/src/contract.rs:1744-1792 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | New CDT ratio within leeway of target, or moving toward target. |
| TR.inv_affiliate_limit | organs/transmuter.qnt:inv_affiliate_limit | contracts/transmuter/src/state.rs:44-45; contract.rs:1307,1321 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per-user affiliate list size ≤ `AFFILIATE_LIMIT = 10`. |
| TR.inv_volume_history_capped | organs/transmuter.qnt:inv_volume_history_capped | contracts/transmuter/src/state.rs:130-148 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Volume history capped at `config.volume_history_cap`. |
| TR.inv_transmute_history_capped | organs/transmuter.qnt:inv_transmute_history_capped | contracts/transmuter/src/state.rs:110-128 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Transmute history capped at `config.swap_history_cap`. |
| TR.inv_rate_history_capped | organs/transmuter.qnt:inv_rate_history_capped | contracts/transmuter/src/state.rs:175-193 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Rate history capped at 365 entries, 24h rate-limited. |

### Transmuter manager subsystem (new in this proposal)

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| TR.inv_manager_fee_bounded | organs/transmuter.qnt:inv_manager_fee_bounded | contracts/transmuter/src/state.rs:NEW (TRANSMUTER_MANAGER_FEE); packages/membrane/src/transmuter.rs:NEW (Config.max_management_fee) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Modeled on ltv_disco execute.rs:2156-2165. |
| TR.inv_manager_cannot_exit | organs/transmuter.qnt:inv_manager_cannot_exit | contracts/transmuter/src/contract.rs:525-528,550 (existing), new SetTransmuterManager grants no exit rights | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Structural — no manager-aware branch in ExitVault/RepayUserDebt/TransferDepositOwnership. |
| TR.inv_manager_update_is_net_zero | organs/transmuter.qnt:inv_manager_update_is_net_zero | contracts/transmuter/src/contract.rs:NEW (RequestTrancheUpdate handler) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Σ added base value == Σ removed base value (± one ulp). |
| TR.inv_manager_fee_only_from_yield | organs/transmuter.qnt:inv_manager_fee_only_from_yield | contracts/transmuter/src/contract.rs:NEW (ExitVault yield branch) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Fee = `rate * max(0, realized_yield)`. Zero on losses, zero on principal. |
| TR.inv_manager_reassign_requires_user | organs/transmuter.qnt:inv_manager_reassign_requires_user | contracts/transmuter/src/contract.rs:NEW (SetTransmuterManager handler) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Only `info.sender == user` can transition TRANSMUTER_MANAGER[u]. |
| TR.inv_managed_users_consistency | organs/transmuter.qnt:inv_managed_users_consistency | contracts/transmuter/src/state.rs:NEW (MANAGED_USERS helpers) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | MANAGED_USERS[m] == `{u : TRANSMUTER_MANAGER[u] == Some(m)}`. |
| TR.inv_fee_cleanup_requires_empty | organs/transmuter.qnt:inv_fee_cleanup_requires_empty | contracts/transmuter/src/contract.rs:NEW (ClearTransmuterManagerFee handler) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Mirrors ltv_disco execute.rs:2179-2203 (`CleanManagerFee`). |

### Transmuter switching-period (NEW)

14-day request + 2-day permissionless execution window pattern for junior exits, junior→senior moves, removing assets, and manager-fee changes. Grounded in NEW Rust handlers (to be added).

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| TR.inv_switching_config_bounds | organs/transmuter.qnt:inv_switching_config_bounds | packages/membrane/src/transmuter.rs:NEW (Config.switching_period, switching_execution_window); default 14d + 2d | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Both > 0. |
| TR.inv_intent_window_wellformed | organs/transmuter.qnt:inv_intent_window_wellformed | contracts/transmuter/src/contract.rs:NEW (RequestSwitch) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | unlock = created + period; expire = unlock + window. |
| TR.inv_user_intent_index_consistency | organs/transmuter.qnt:inv_user_intent_index_consistency | contracts/transmuter/src/state.rs:NEW (SWITCHING_INTENTS, USER_SWITCHING_INTENTS) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Bidirectional user↔intent index. |
| TR.inv_intent_ids_monotone | organs/transmuter.qnt:inv_intent_ids_monotone | contracts/transmuter/src/state.rs:NEW (NEXT_INTENT_ID monotone) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Generated IDs strictly increase. |
| TR.inv_junior_exit_requires_intent | organs/transmuter.qnt:ExitVault precondition (`all_juniors_covered`) | contracts/transmuter/src/contract.rs:NEW (ExitVault junior-gate branch) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Junior tranche_withdrawals require a READY JuniorExit or RemoveAsset intent. |
| TR.inv_senior_free_withdraw_structural | organs/transmuter.qnt:inv_senior_free_withdraw_structural | contracts/transmuter/src/contract.rs:NEW (ExitVault senior branch) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Senior-only `tranche_withdrawals` bypass the intent gate. |
| TR.inv_permissionless_execution | organs/transmuter.qnt:ExecuteSwitch (any sender; window-gated) | contracts/transmuter/src/contract.rs:NEW (ExecuteSwitch) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | No auth check on sender; only `intent_is_ready` guard. |
| TR.inv_intents_expire_after_window | organs/transmuter.qnt:PruneExpiredIntents | contracts/transmuter/src/contract.rs:NEW (PruneExpiredIntents) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Intents past expire_time yield no execution; GC is safe. |
| TR.inv_pending_fee_bounded | organs/transmuter.qnt:inv_pending_fee_bounded | contracts/transmuter/src/contract.rs:NEW (RequestManagerFeeChange) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Pending new_fee ∈ [0, max_management_fee] ∩ [0, 1]. |
| TR.inv_fee_change_window_wellformed | organs/transmuter.qnt:inv_fee_change_window_wellformed | contracts/transmuter/src/contract.rs:NEW (RequestManagerFeeChange / ExecuteManagerFeeChange) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Same 14d + 2d window; permissionless execution. |

## LTV Disco (`quint/organs/ltv_disco.qnt`)

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| DI.inv_slot_weight_sum_one | organs/ltv_disco.qnt:inv_slot_weight_sum_one | contracts/ltv_disco/src/execute.rs (calculate_slot_weights); docs/README.md:121-127 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Normalized weights sum to 1. |
| DI.inv_slot_weight_riskiest_largest | organs/ltv_disco.qnt:inv_slot_weight_riskiest_largest | contracts/ltv_disco/src/execute.rs (calculate_slot_weights) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Slot with `deposits_above=0` has largest raw weight. Proved in Lean (`IsqrtMonotone`). |
| DI.inv_bad_debt_slashing_order | organs/ltv_disco.qnt:inv_bad_debt_slashing_order | contracts/ltv_disco/src/execute.rs (SendMBRNForSale) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Slots iterated descending by LTV; highest slashed first. |
| DI.inv_bad_debt_freezes_withdrawals | organs/ltv_disco.qnt:inv_bad_debt_freezes_withdrawals | contracts/ltv_disco/src/state.rs:35 (PENDING_BAD_DEBT); execute.rs (RequestUnstake check) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | RequestUnstake reverts if `PENDING_BAD_DEBT[asset] > 0`. |
| DI.inv_unstake_cooldown_elapsed | organs/ltv_disco.qnt:inv_unstake_cooldown_elapsed | contracts/ltv_disco/src/execute.rs (CompleteUnstake) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | `block_time >= request.unlock_time` required. |
| DI.inv_manager_fee_bounded | organs/ltv_disco.qnt:inv_manager_fee_bounded | contracts/ltv_disco/src/execute.rs:2156-2165 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | `fee <= config.max_management_fee` and `<= 1`. |
| DI.inv_manager_cannot_withdraw | organs/ltv_disco.qnt:inv_manager_cannot_withdraw | contracts/ltv_disco/src/execute.rs (RequestUnstake/CompleteUnstake auth — owner only) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Neither unstake entry admits `manager` sender. |
| DI.inv_managed_deposits_consistency | organs/ltv_disco.qnt:inv_managed_deposits_consistency | contracts/ltv_disco/src/state.rs:84-130 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | MANAGED_DEPOSITS[m] == `{key : BackingDeposit[key].manager == Some(m)}`. |
| DI.inv_rate_assurance_per_slot | organs/ltv_disco.qnt:inv_rate_assurance_per_slot | contracts/ltv_disco/src/state.rs:33; execute.rs (RateAssurance) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | `base_tokens_per_trillion_vt` never decreases during a single operation. |
| DI.inv_revenue_event_ordering | organs/ltv_disco.qnt:inv_revenue_event_ordering | contracts/ltv_disco/src/execute.rs (claim_revenue_for_deposit loop filters `timestamp > deposit.last_claimed`) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Revenue events consumed strictly in order. |
| DI.inv_compound_reply_routed | organs/ltv_disco.qnt:inv_compound_reply_routed | contracts/ltv_disco/src/reply.rs (COMPOUND_SWAP_REPLY_ID=3) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Reply payload deposit_contributions sums preserved across swap. |
| DI.inv_affiliate_limit | organs/ltv_disco.qnt:inv_affiliate_limit | contracts/ltv_disco/src/state.rs:67-68 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | `AFFILIATE_LIMIT = 10`. |
| DI.inv_all_caps_respected | organs/ltv_disco.qnt:inv_all_caps_respected | contracts/ltv_disco/src/state.rs:30,43,52,55,77-78 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Revenue tracking (100), lifetime revenue (100), TVL (100), deposits (100), manager performance (100). |

### LTV Disco switching-period / unstaking (NEW)

14-day unstaking + 2-day permissionless-execute window. Lower-LTV moves require a separate 14-day intent the querier honors for bad-debt waterfall calculations. Manager-fee changes go through the same 14d + 2d window.

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| DI.inv_unstake_cooldown_is_14d | organs/ltv_disco.qnt:inv_unstake_cooldown_is_14d | packages/membrane/src/ltv_disco.rs:Config.unstaking_period; contracts/ltv_disco/src/execute.rs (RequestUnstake) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Previous default 2d → 14d per spec. |
| DI.inv_unstake_execution_window_is_2d | organs/ltv_disco.qnt:inv_unstake_execution_window_is_2d | contracts/ltv_disco/src/execute.rs:NEW (CompleteUnstake window guard) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | CompleteUnstake permissionless only inside window; past expire_time, resets. |
| DI.inv_unstake_window_wellformed | organs/ltv_disco.qnt:inv_unstake_window_wellformed | contracts/ltv_disco/src/state.rs:NEW (UnstakeRequest.created_at/unlock_time/expire_time) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Windows derived from config. |
| DI.inv_ltv_switching_period_is_14d | organs/ltv_disco.qnt:inv_ltv_switching_period_is_14d | packages/membrane/src/ltv_disco.rs:Config.ltv_switching_period:NEW | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Lower-LTV moves only. Higher-LTV immediate. |
| DI.inv_ltv_switching_window_is_2d | organs/ltv_disco.qnt:inv_ltv_switching_window_is_2d | packages/membrane/src/ltv_disco.rs:Config.ltv_switching_execution_window:NEW | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | 2-day permissionless exec. |
| DI.inv_lower_ltv_intent_wellformed | organs/ltv_disco.qnt:inv_lower_ltv_intent_wellformed | contracts/ltv_disco/src/state.rs:NEW (LOWER_LTV_INTENTS) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Windows derived from config. |
| DI.inv_deposit_ltv_intent_consistency | organs/ltv_disco.qnt:inv_deposit_ltv_intent_consistency | contracts/ltv_disco/src/state.rs:NEW (DEPOSIT_LTV_INTENT index) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Bidirectional deposit_key ↔ intent id. |
| DI.inv_querier_uses_pre_intent_slot | organs/ltv_disco.qnt:querier_current_ltv (pure helper) | contracts/ltv_disco/src/query.rs:NEW (QueueAverageLTV reads pre-intent slot); contracts/cdp/src/ consumes | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Bad-debt waterfall stays at original (higher) slot until ExecuteLowerLTVMove fires. |
| DI.inv_pending_fee_change_bounded | organs/ltv_disco.qnt:inv_pending_fee_change_bounded | contracts/ltv_disco/src/execute.rs:NEW (RequestManagerFeeChange) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Pending new_fee bounded; windows derived. |
| DI.inv_fee_change_period_is_14d | organs/ltv_disco.qnt:inv_fee_change_period_is_14d | packages/membrane/src/ltv_disco.rs:Config.manager_fee_change_period:NEW | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Replaces direct SetManagerFee. |
| DI.inv_fee_change_window_is_2d | organs/ltv_disco.qnt:inv_fee_change_window_is_2d | packages/membrane/src/ltv_disco.rs:Config.manager_fee_change_execution_window:NEW | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Permissionless ExecuteManagerFeeChange. |

### Manager P&L event schema (NEW) — off-chain, storage-free

Every row below corresponds to a CosmWasm `Event` (type + attributes) the contracts MUST emit. Indexers are the authoritative reader of manager P&L history; the contracts store none of it. See `specs/quint/lib/types.qnt::Event` for the Quint shape and `specs/OPEN_QUESTIONS.md#2`.

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| MGR.inv_no_storage_of_manager_pnl | organs/{transmuter,ltv_disco}.qnt:inv_no_storage_of_manager_pnl | contracts/transmuter/src/state.rs (absence); contracts/ltv_disco/src/state.rs:77-78 (remove MANAGER_PERFORMANCE) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | No on-chain mirror of manager P&L. Legacy `MANAGER_PERFORMANCE` map in Disco SHOULD be removed; decision pending. |
| MGR.inv_events_monotone | organs/{transmuter,ltv_disco}.qnt:inv_events_monotone | — | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Model-level invariant: events list is append-only. |
| MGR.inv_pending_fee_is_increase_only | organs/{transmuter,ltv_disco}.qnt:inv_pending_fee_is_increase_only | contracts/{transmuter,ltv_disco}/src/contract.rs:NEW (RequestManagerFeeChange) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Decrease path applies instantly; pending records hold strictly higher fees only. |
| MGR.ev_ManagerBadDebtAbsorbed | types.qnt::Ev_ManagerBadDebtAbsorbed | contracts/transmuter/src/contract.rs:827-894 (AssignBadDebt branch for managed users); contracts/ltv_disco/src/execute.rs (SendMBRNForSale branch for managed deposits) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Attrs: `action=manager_bad_debt_absorbed`, `manager`, `user`, `organ`, `asset`, `loss_amount`, `ts`. |
| MGR.ev_ManagerFeeEarned | types.qnt::Ev_ManagerFeeEarned | contracts/transmuter/src/contract.rs:NEW (ExitVault yield-branch payout); contracts/ltv_disco/src/execute.rs (ClaimRevenueForUser manager-fee branch) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Attrs: `action=manager_fee_earned`, `manager`, `user`, `organ`, `asset`, `fee_amount`, `yield_base`, `ts`. |
| MGR.ev_ManagerUserAdded / Removed | types.qnt::Ev_ManagerUserAdded / Ev_ManagerUserRemoved | contracts/transmuter/src/contract.rs:NEW (SetTransmuterManager); contracts/ltv_disco/src/execute.rs:1161-1244 (UpdateDeposit manager-transition branch) + 971-1156 (MoveDeposit full-move cleanup) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Emitted whenever a manager↔user binding transitions. |
| MGR.ev_SwitchIntentCreated / Executed / Cancelled / Expired | types.qnt::Ev_SwitchIntent* | contracts/{transmuter,ltv_disco}/src/contract.rs:NEW (Request/Execute/Cancel/Prune for switching intents, lower-LTV intents, unstake requests) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Full audit trail for the 14d+2d intent flows. |
| MGR.ev_ManagerFeeRequestCreated / Changed / Cancelled | types.qnt::Ev_ManagerFee* | contracts/{transmuter,ltv_disco}/src/contract.rs:NEW (Request/Execute/Cancel ManagerFeeChange) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Changed events carry `instant: bool` to distinguish decrease-instant vs. hike-after-wait. |

### Acquisition bypass (NEW)

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| AQ.inv_transmuter_exit_acq_bypass | organs/transmuter.qnt:ExitVault junior-intent gate | contracts/transmuter/src/contract.rs:525-528,550 (existing) + NEW `is_acquisition` bypass branch | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Acquisition-contract caller skips the junior-exit intent gate. |
| AQ.inv_disco_acquisition_unstake_bypass | organs/ltv_disco.qnt:AcquisitionUnstake | contracts/ltv_disco/src/execute.rs:NEW (AcquisitionUnstake) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Direct unstake by acquisition-owner address, no cooldown; still blocked during active bad-debt freeze. |

### Capital-loss access control (NEW — Amendment 4)

Top-level invariant: **no singular access-control path can result in user capital loss**. Full enumeration of dangerous actions and their required mitigations is in [`specs/CAPITAL_LOSS_SURFACE.md`](CAPITAL_LOSS_SURFACE.md). Each per-organ row below is a structural witness that a Category-A or B action is gated by Timelock (TL), Governance (GOV), Hard Cap (CAP), or Window (WIN).

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| AC.inv_no_unilateral_capital_loss_path | compositions/membrane.qnt:inv_no_unilateral_capital_loss_path | (Conjunction of per-organ rows below) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Top-level meta-invariant. |
| AC.MAX_LTV_HARD_CAP=90% | lib/access_control.qnt:MAX_LTV_HARD_CAP | contracts/collateral/src/ (NEW const) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | CAP. Used by COL.inv_max_ltv_hard_cap. |
| AC.MAX_USAGE_FEE=5% | lib/access_control.qnt:MAX_USAGE_FEE | contracts/transmuter/src/ (NEW const) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | CAP. Used by TR.inv_usage_fee_capped. |
| AC.MAX_REVDIST_PERCENTAGE=50% | lib/access_control.qnt:MAX_REVDIST_PERCENTAGE | contracts/transmuter/src/ (NEW const) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | CAP. Used by TR.inv_revdist_pct_capped. |
| AC.MAX_MAX_MGT_FEE=25% | lib/access_control.qnt:MAX_MAX_MGT_FEE | contracts/{transmuter,ltv_disco}/src/ (NEW const) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | CAP. Used by TR/DI.inv_max_management_fee_capped. |
| AC.MAX_LIQ_FEE=10% | lib/access_control.qnt:MAX_LIQ_FEE | contracts/liquidation-engine/src/ (NEW const) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | CAP. Used by LE.inv_liq_fee_capped. |
| AC.MAX_INTEREST_RATE_BPS=50%/yr | lib/access_control.qnt:MAX_INTEREST_RATE_BPS | contracts/cdp/src/rates.rs (NEW const) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | CAP. Used by DBT.inv_interest_rate_capped. |
| AC.MAX_AFFILIATE_FEE=5% | lib/access_control.qnt:MAX_AFFILIATE_FEE | contracts/{transmuter,ltv_disco}/src/ (NEW const) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | CAP. Used by DI.inv_affiliate_fee_capped. |
| AC.MIN_SWITCHING_PERIOD_S=7d | lib/access_control.qnt:MIN_SWITCHING_PERIOD_S | contracts/transmuter/src/ (NEW const) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | CAP-floor. Used by TR.inv_switching_period_floor. |
| AC.MIN_UNSTAKING_PERIOD_S=7d | lib/access_control.qnt:MIN_UNSTAKING_PERIOD_S | contracts/ltv_disco/src/ (NEW const) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | CAP-floor. Used by DI.inv_unstaking_period_floor. |
| AC.MAX_ORACLE_STALENESS_S=1h | lib/access_control.qnt:MAX_ORACLE_STALENESS_S | contracts/oracle/src/ (NEW const) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | CAP. Used by OR.inv_staleness_capped. |
| AC.owner_governance_distinct | per-organ inv_owner_governance_distinct | Each Config.owner / Config.governance two-role split | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Structural — owner != governance in every organ. |
| TR.inv_no_unilateral_capital_loss_path_TR | organs/transmuter.qnt | (composite) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per-organ meta-invariant. |
| DI.inv_no_unilateral_capital_loss_path_DI | organs/ltv_disco.qnt | (composite) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per-organ meta-invariant. |
| COL.inv_no_unilateral_capital_loss_path_COL | organs/collateral.qnt | (composite) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per-organ meta-invariant. |
| LE.inv_no_unilateral_capital_loss_path_LE | organs/liquidation_engine.qnt | (composite) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per-organ meta-invariant. |
| DBT.inv_no_unilateral_capital_loss_path_DBT | organs/debt.qnt | (composite) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per-organ meta-invariant. |
| OR.inv_no_unilateral_capital_loss_path_OR | shared/oracle.qnt | (composite) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per-organ meta-invariant. |
| AQ.inv_no_unilateral_capital_loss_path_AQ | organs/acquisition.qnt | (composite) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per-organ meta-invariant. |

Per-row TL / GOV / CAP / WIN attribution is given in [`CAPITAL_LOSS_SURFACE.md`](CAPITAL_LOSS_SURFACE.md). When a new param is added to any organ, append a row there BEFORE writing the corresponding execute handler.

### Amendment 5 (rate caps, daily borrow flow, TWAL, oracle CB)

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| AC.MAX_INTEREST_RATE_BPS=100% | lib/access_control.qnt:MAX_INTEREST_RATE_BPS | contracts/cdp/src/rates.rs (NEW const) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Bumped from 50% → 100% per Amendment 5. |
| AC.MAX_COMPARATIVE_RATE=100% | lib/access_control.qnt:MAX_COMPARATIVE_RATE | contracts/cdp/src/rates.rs (NEW const) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Cross-asset spread accrual cap. |
| AC.MAX_DAILY_BORROW_PCT=10%/day | lib/access_control.qnt:MAX_DAILY_BORROW_PCT | contracts/debt/src/ (NEW const) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per-window mint flow ceiling. |
| AC.DAILY_WINDOW_S=24h | lib/access_control.qnt:DAILY_WINDOW_S | contracts/debt/src/ (NEW const) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Window length. |
| DBT.inv_borrow_limit_pct_capped | organs/debt.qnt:inv_borrow_limit_pct_capped | contracts/debt/src/state.rs (NEW BORROW_LIMIT_PCT item); execute.rs (UpdateConfig branch) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | `borrow_limit_pct ≤ MAX_DAILY_BORROW_PCT`. |
| DBT.inv_minted_in_window_under_quota | organs/debt.qnt:inv_minted_in_window_under_quota | contracts/debt/src/state.rs (NEW BORROW_WINDOW item); execute.rs (MintBorrow precondition) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Σ minted in 24h ≤ pct × outstanding-at-window-start. |
| DBT.inv_window_wellformed | organs/debt.qnt:inv_window_wellformed | contracts/debt/src/state.rs | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Non-negative window state. |
| TWAL.inv_buffer_capped | shared/twal_oracle.qnt:inv_buffer_capped | contracts/oracle/src/twal.rs (NEW); state.rs (TWAL_BUFFER) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | `samples ≤ TWAL_BUFFER_CAP = 1440`. |
| TWAL.inv_at_most_one_sample_per_bucket | shared/twal_oracle.qnt:inv_at_most_one_sample_per_bucket | contracts/oracle/src/twal.rs (ObserveLiquidity precondition) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | 30-minute bucket dedup. |
| TWAL.inv_owner_governance_distinct | shared/twal_oracle.qnt:inv_owner_governance_distinct | contracts/oracle/src/state.rs:NEW (TwalConfig.owner / .governance) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Two-role split. |
| OR.inv_max_deviation_capped | shared/oracle.qnt:inv_max_deviation_capped | contracts/oracle/src/state.rs:NEW (max_per_update_deviation_pct) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | `≤ MAX_PRICE_DEVIATION_PCT = 10%/update`. |
| OR.inv_trip_threshold_at_least_max_deviation | shared/oracle.qnt:inv_trip_threshold_at_least_max_deviation | contracts/oracle/src/state.rs:NEW (cb_trip_threshold_pct) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Sanity: trip threshold ≥ per-update cap. |
| OR.inv_cb_only_governance_resets | shared/oracle.qnt:inv_cb_only_governance_resets | contracts/oracle/src/execute.rs:NEW (ResetCircuitBreaker, governance-only) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Once tripped, only governance can clear. |
| OR.inv_consumers_use_price_safe | (composition assertion across CDP, liq engine, Transmuter price reads) | contracts/cdp/src/, contracts/liquidation-engine/src/, contracts/transmuter/src/ — every price read | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Structural — `price_safe(d) == false` blocks the path. |

### Points System (Amendment 6) — full coverage

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| PS.inv_user_stats_nonneg | shared/points_system.qnt:inv_user_stats_nonneg | contracts/points-system/src/state.rs:31 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | total / claimable points are non-negative. |
| PS.inv_claimable_le_total | shared/points_system.qnt:inv_claimable_le_total | contracts/points-system/src/contracts.rs:836-978 (allocate_points) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | claimable_points ≤ total_points. |
| PS.inv_total_mbrn_under_cap | shared/points_system.qnt:inv_total_mbrn_under_cap | contracts/points-system/src/contracts.rs:744-805 (ClaimMBRN) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Lifetime MBRN distribution bounded by `max_mbrn_distribution`. |
| PS.inv_management_points_constant | shared/points_system.qnt:inv_management_points_constant | contracts/points-system/src/contracts.rs:2073 (const), 2092 (use), 1521 (reply) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Fixed 5_000_000 per action; immutable by config. |
| PS.inv_authority_registry_enforced | shared/points_system.qnt:inv_authority_registry_enforced | contracts.rs:1748-1762 (affiliate auth), 1832 (manager auth), 2085 (CDP auth), 1678 (emissions auth) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Fee-award entries reject senders outside the configured contract registry. |
| PS.inv_owner_governance_distinct | shared/points_system.qnt:inv_owner_governance_distinct | contracts/points-system/src/state.rs:30 (Config) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Two-role split. |
| PS.inv_no_unilateral_capital_loss_path_PS | (composite) | — | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per-organ AC witness. |
| PS.reply.LIQUIDATION_REPLY_ID=1 | shared/points_system.qnt:LIQUIDATION_REPLY_ID | contracts/points-system/src/contracts.rs:41,1180 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Reply ID. |
| PS.reply.CDP_REPAY_REPLY_ID=3 | shared/points_system.qnt:CDP_REPAY_REPLY_ID | contracts/points-system/src/contracts.rs:42,1301 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Reply ID. |
| PS.reply.DISCO_CLAIM_REPLY_ID=4 | shared/points_system.qnt:DISCO_CLAIM_REPLY_ID | contracts/points-system/src/contracts.rs:43,1365 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Reply ID. |
| PS.reply.TRANSMUTER_TRANSMUTE_REPLY_ID=5 | shared/points_system.qnt:TRANSMUTER_TRANSMUTE_REPLY_ID | contracts/points-system/src/contracts.rs:44,1439 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Reply ID. |
| PS.reply.CHECK_MANAGEMENT_POINTS_REPLY_ID=6 | shared/points_system.qnt:CHECK_MANAGEMENT_POINTS_REPLY_ID | contracts/points-system/src/contracts.rs:2111,1521 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Reply ID. |

### System Discounts (Amendment 6) — full coverage

**DOCS/CODE MISMATCH NOTED**: docs/README.md:131-137 describes a two-phase time-curve formula that is NOT implemented. The contract uses a linear MBRN-supply curve with timed/static priority overrides (contracts/system_discounts/src/contracts.rs:219-313). Spec mirrors the code. Tracked in OPEN_QUESTIONS.md.

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| SD.inv_max_discount_capped | shared/system_discounts.qnt:inv_max_discount_capped | contracts/system_discounts/src/contracts.rs:170-172 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | `max_discount ≤ 1`. |
| SD.inv_ratio_capped | shared/system_discounts.qnt:inv_ratio_capped | contracts/system_discounts/src/contracts.rs:193-195 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | `ratio_of_mbrn_supply_at_max_discount ≤ 1`. |
| SD.inv_static_entries_capped | shared/system_discounts.qnt:inv_static_entries_capped | contracts/system_discounts/src/contracts.rs:181-182 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per-entry discount ≤ 1. |
| SD.inv_timed_period_wellformed | shared/system_discounts.qnt:inv_timed_period_wellformed | contracts/system_discounts/src/contracts.rs:316-341 (SetDiscountPeriod) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | end ≥ start; discount in [0, 1]. |
| SD.inv_get_discount_capped | shared/system_discounts.qnt:inv_get_discount_capped | contracts/system_discounts/src/contracts.rs:219-313 (get_discount) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Linear formula bounded by max_discount; overrides bounded by 1. |
| SD.inv_priority_timed_static_linear | shared/system_discounts.qnt:get_discount | contracts/system_discounts/src/contracts.rs:228-247 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Priority order: timed > static > linear. |
| SD.inv_owner_governance_distinct | shared/system_discounts.qnt:inv_owner_governance_distinct | contracts/system_discounts/src/state.rs (Config) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Two-role split. |
| SD.inv_user_boost_disabled | (witnessed at flywheel.qnt) | contracts/system_discounts/src/contracts.rs:210 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | UserBoost always returns 0. |
| SD.inv_intent_boosts_disabled | (witnessed at flywheel.qnt) | contracts/system_discounts/src/contracts.rs:211 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | IntentBoosts returns empty. |
| SD.inv_no_discount_vault_link | (composition / OPEN_QUESTIONS.md) | (absence of cross-call between contracts/discount_vault and contracts/system_discounts) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Documented gap — Discount Vault deposits do NOT feed discounts in code. |

### Flywheel composition (Amendment 6)

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| FW.inv_cdp_award_authority | compositions/flywheel.qnt:inv_cdp_award_authority | contracts/points-system/src/contracts.rs:2085 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Only CDP mints management points. |
| FW.inv_disco_manager_fee_award_authority | compositions/flywheel.qnt | contracts/points-system/src/contracts.rs:1832 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Only LTV Disco mints manager-fee points. |
| FW.inv_affiliate_fee_award_authority | compositions/flywheel.qnt | contracts/points-system/src/contracts.rs:1748-1762 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Only RevDist / Transmuter / Disco mint affiliate-fee points. |
| FW.inv_emissions_voting_authority | compositions/flywheel.qnt | contracts/points-system/src/contracts.rs:1678 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Only emissions_voting_contract may update PointsMultipliers. |
| FW.inv_mbrn_distribution_under_cap | compositions/flywheel.qnt | (PS.inv_total_mbrn_under_cap) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Cross-cite. |
| FW.inv_management_points_constant | compositions/flywheel.qnt | (PS.inv_management_points_constant) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Cross-cite. |
| FW.inv_discount_priority_stack | compositions/flywheel.qnt | contracts/system_discounts/src/contracts.rs:228-247 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Timed > Static > Linear. |
| FW.inv_discount_consumer_is_cdp | compositions/flywheel.qnt | contracts/cdp/src/rates.rs:1368 (UserDiscount query) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Only CDP/Debt currently consume the discount. |
| FW.inv_user_boost_disabled | compositions/flywheel.qnt | contracts/system_discounts/src/contracts.rs:210 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Structural disable witness. |
| FW.inv_intent_boosts_disabled | compositions/flywheel.qnt | contracts/system_discounts/src/contracts.rs:211 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Structural disable witness. |
| FW.tp_every_management_point_event_is_repay | (temporal, deferred) | contracts/cdp/src/positions.rs:3923 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Apalache property; awaits cdp.qnt thickening. |
| FW.tp_manager_fee_pairing | (temporal, deferred) | contracts/ltv_disco/src/execute.rs:1808,1885 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Apalache property. |
| FW.tp_revenue_eventually_yields_points | (temporal, deferred) | contracts/revenue-distributor/src/reply.rs:64 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Apalache property. |

### Collateral onboarding — PARTIAL (see specs/OPEN_QUESTIONS.md#1)

Core LTV approach (A–E) still unresolved. The **onboarding-window safety belt** below is landed regardless of the LTV choice.

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| COL.inv_instantiate_assets_bypass_onboarding | organs/collateral.qnt:inv_instantiate_assets_bypass_onboarding | contracts/collateral/src/: instantiate-time `AddAsset` sets `onboarding_window_end = 0` | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Structural; enforced by `AddAssetAtInstantiate` precondition. |
| COL.inv_autonomous_windows_in_future_when_created | organs/collateral.qnt:inv_autonomous_windows_in_future_when_created | contracts/collateral/src/:NEW (`AddAssetAutonomous`) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Stamped as `now + config.onboarding_window_duration`. |
| COL.inv_onboarding_end_immutable | organs/collateral.qnt:inv_onboarding_end_immutable | contracts/collateral/src/:UpdateAsset precondition (NEW) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Admin cannot mutate `onboarding_window_end` post-creation. |
| TR.inv_onboarding_exit_gated | organs/transmuter.qnt:inv_onboarding_exit_gated | contracts/transmuter/src/contract.rs:NEW (ExitVault onboarding-gate branch) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | During window, all tranche exits require an intent (junior AND senior). |
| TR.inv_senior_free_withdraw_post_onboarding | organs/transmuter.qnt:inv_senior_free_withdraw_post_onboarding | contracts/transmuter/src/contract.rs:ExitVault | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Outside window, seniors keep free-withdraw. |
| TR.inv_onboarding_mirror_nonneg | organs/transmuter.qnt:inv_onboarding_mirror_nonneg | contracts/transmuter/src/contract.rs:NEW (Collateral query at ExitVault or cached mirror) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Mirror values are non-negative timestamps (0 = no window). |

Still deferred until core LTV choice lands: `CDP.inv_starting_max_ltv_set_without_disco`, `CDP.CurrentMaxLTV` query shape.

## CDP organ trio (Phase B / Amendment 7) — full coverage

### CDP / Positions

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| CDP.inv_position_solvent_before_mint | organs/cdp.qnt:inv_position_solvent_before_mint | contracts/cdp/src/positions.rs:1401-1887 (IncreaseDebt) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | `not insolvent(p_new, prices)` precondition on IncreaseDebt. |
| CDP.inv_credit_minted_nonneg | organs/cdp.qnt:inv_credit_minted_nonneg | contracts/cdp/src/state.rs (basket.credit_minted) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | basket.credit_minted >= 0 always. |
| CDP.inv_pending_revenue_nonneg | organs/cdp.qnt:inv_pending_revenue_nonneg | contracts/cdp/src/revenue.rs (take_revenue) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | basket.pending_revenue >= 0 always. |
| CDP.inv_pending_bad_debt_nonneg | organs/cdp.qnt:inv_pending_bad_debt_nonneg | contracts/cdp/src/contract.rs:336-406 (FulfillBadDebt) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | basket.pending_bad_debt >= 0; FulfillBadDebt monotonically reduces. |
| CDP.inv_max_positions_per_user | organs/cdp.qnt:inv_max_positions_per_user | contracts/cdp/src/positions.rs (Deposit) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | <= MAX_POSITIONS_AMOUNT = 9. |
| CDP.inv_affiliate_limit | organs/cdp.qnt:inv_affiliate_limit | contracts/cdp/src/contract.rs:785-843 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | <= AFFILIATE_LIMIT_PER_POS = 3. |
| CDP.inv_affiliate_fees_capped | organs/cdp.qnt:inv_affiliate_fees_capped | contracts/cdp/src/contract.rs (SetAffiliate) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Each fee <= config.affiliate_fee_max. |
| CDP.inv_liq_fee_capped | organs/cdp.qnt:inv_liq_fee_capped | packages/membrane/src/cdp.rs (Config) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | config.liq_fee <= MAX_LIQ_FEE = 10%. Amendment 4 cap. |
| CDP.inv_max_ltv_capped | organs/cdp.qnt:inv_max_ltv_capped | contracts/cdp/src/contract.rs:510-599 (EditcAsset) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per-asset max_LTV <= MAX_LTV_HARD_CAP = 90%. |
| CDP.inv_supply_caps_respected | organs/cdp.qnt:inv_supply_caps_respected | contracts/cdp/src/risk_engine.rs | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per-asset current <= cap. |
| CDP.inv_liquidation_stats_capped | organs/cdp.qnt:inv_liquidation_stats_capped | contracts/cdp/src/state.rs | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | <= config.liquidation_stat_limit (default 500). |
| CDP.inv_owner_governance_distinct | organs/cdp.qnt:inv_owner_governance_distinct | packages/membrane/src/cdp.rs (Config two-role split) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Amendment 4. |
| CDP.reply.LIQ_QUEUE_REPLY_ID=1 | organs/cdp.qnt:LIQ_QUEUE_REPLY_ID | contracts/cdp/src/positions.rs:41-50 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| CDP.reply.WITHDRAW_REPLY_ID=4 | organs/cdp.qnt:WITHDRAW_REPLY_ID | contracts/cdp/src/positions.rs:41-50 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| CDP.reply.CLOSE_POSITION_REPLY_ID=6 | organs/cdp.qnt | contracts/cdp/src/positions.rs:41-50 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| CDP.reply.SELL_COLLATERAL_REPLY_ID=7 | organs/cdp.qnt | contracts/cdp/src/positions.rs:41-50 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| CDP.reply.DEPLOYABLE_VENUE_REPLY_ID=8 | organs/cdp.qnt | contracts/cdp/src/positions.rs:41-50 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| CDP.reply.BAD_DEBT_REPLY_ID=999999 | organs/cdp.qnt | contracts/cdp/src/contract.rs:1064 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | No-op handler. |

### Liquidation Engine — 9-step reply chain

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| LE.inv_only_one_in_flight | organs/liquidation_engine.qnt:inv_only_one_in_flight | contracts/liquidation-engine/src/state.rs:19-20 (LIQUIDATION_PROPAGATION single Item) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Single in-flight liquidation, structural. |
| LE.inv_outcome_well_formed | organs/liquidation_engine.qnt:inv_outcome_well_formed | contracts/liquidation-engine/src/reply.rs (LiquidationOutcome enum) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | One of 6 outcomes. |
| LE.inv_timer_window_wellformed | organs/liquidation_engine.qnt:inv_timer_window_wellformed | contracts/liquidation-engine/src/state.rs:92 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per-position timer.start_time <= now. |
| LE.inv_stats_capped | organs/liquidation_engine.qnt:inv_stats_capped | contracts/liquidation-engine/src/state.rs:26 (cap 500) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | LIQUIDATION_STATS_CAP = 500 FIFO. |
| LE.inv_liq_fee_capped | organs/liquidation_engine.qnt:inv_liq_fee_capped | packages/membrane/src/liquidation_engine.rs:11-34 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | config.liq_fee <= MAX_LIQ_FEE. |
| LE.inv_liquidation_delay_positive | organs/liquidation_engine.qnt:inv_liquidation_delay_positive | contracts/liquidation-engine/src/contract.rs:52 (default 28800s) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | > 0. |
| LE.inv_owner_governance_distinct | organs/liquidation_engine.qnt:inv_owner_governance_distinct | packages/membrane/src/liquidation_engine.rs (Config) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Amendment 4. |
| LE.inv_fee_cap_safety | organs/liquidation_engine.qnt:apply_fee_cap | contracts/liquidation-engine/src/reply.rs:362-388 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | `caller_fee * repay_value <= collateral_value` post-cap. Lean: `LiqFeeCap.safe`. |
| LE.inv_threshold_semantics | organs/liquidation_engine.qnt:immediate_threshold | contracts/liquidation-engine/src/reply.rs:295-297 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | `immediate = avg_max_ltv * (1 + avg_max_threshold_to_delay)`. Matches docs. |
| LE.inv_chain_complete_structural | (composition: liquidation_flow.qnt) | contracts/liquidation-engine/src/reply.rs (full chain) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Step ordering 1→2→9→3→4→5→6→7→8 enforced. Temporal property at composition level. |
| LE.reply.DEBT_ACCRUE=1 | organs/liquidation_engine.qnt | state.rs:99 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| LE.reply.COLLATERAL_ASSESS=2 | organs/liquidation_engine.qnt | state.rs:101 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| LE.reply.LIQ_QUEUE_TRANSFER=3 | organs/liquidation_engine.qnt | state.rs:103 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| LE.reply.LIQ_QUEUE=4 | organs/liquidation_engine.qnt | state.rs:105 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| LE.reply.SELL_WALL=5 | organs/liquidation_engine.qnt | state.rs:107 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| LE.reply.DEBT_UPDATE=6 | organs/liquidation_engine.qnt | state.rs:109 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| LE.reply.CLAIM_UPDATE=7 | organs/liquidation_engine.qnt | state.rs:111 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| LE.reply.BAD_DEBT=8 | organs/liquidation_engine.qnt | state.rs:113 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | reply_on_error no-op. |
| LE.reply.FEE_TRANSFER=9 | organs/liquidation_engine.qnt | state.rs:115 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |

### Liq Queue — bid auction

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| LQ.inv_max_premium_within_bounds | organs/liq_queue.qnt:inv_max_premium_within_bounds | contracts/liq-queue/src/contract.rs:233 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | 1 <= max_premium <= 50. |
| LQ.inv_bid_threshold_within_bounds | organs/liq_queue.qnt:inv_bid_threshold_within_bounds | contracts/liq-queue/src/contract.rs:272 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | 1M <= bid_threshold <= 10M. |
| LQ.inv_slots_match_max_premium | organs/liq_queue.qnt:inv_slots_match_max_premium | contracts/liq-queue/src/contract.rs:308-322 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | `slots.len() == max_premium + 1`. |
| LQ.inv_bid_id_monotone | organs/liq_queue.qnt:inv_bid_id_monotone | contracts/liq-queue/src/types.rs:111 (Queue.current_bid_id) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | IDs assigned monotonically. |
| LQ.inv_total_bid_amount_matches_active_sum | organs/liq_queue.qnt:inv_total_bid_amount_matches_active_sum | contracts/liq-queue/src/types.rs:200 (PremiumSlot) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Slot.total_bid_amount = Σ active bid.amount. |
| LQ.inv_waiting_bids_capped | organs/liq_queue.qnt:inv_waiting_bids_capped | contracts/liq-queue/src/bid.rs:133-136 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | <= config.maximum_waiting_bids. |
| LQ.inv_active_bid_no_wait_end | organs/liq_queue.qnt:inv_active_bid_no_wait_end | contracts/liq-queue/src/bid.rs:179 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Activation clears wait_end. |
| LQ.inv_waiting_bid_has_wait_end | organs/liq_queue.qnt:inv_waiting_bid_has_wait_end | contracts/liq-queue/src/bid.rs:127 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Waiting state has Some(wait_end). |
| LQ.inv_product_snapshot_positive | organs/liq_queue.qnt:inv_product_snapshot_positive | contracts/liq-queue/src/contract.rs:254,314 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Initialized to 1; only mutated by liquidation math (never zero except at truncation reset). |
| LQ.inv_consumption_lowest_premium_first | organs/liq_queue.qnt:inv_consumption_lowest_premium_first | contracts/liq-queue/src/bid.rs:379 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Loop iterates 0..max_premium ascending. |
| LQ.inv_no_reply_handlers | organs/liq_queue.qnt:inv_no_reply_handlers | contracts/liq-queue/src/contract.rs:29 (comment) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Structural — no reply() entry point. |
| LQ.inv_no_stability_pool | organs/liq_queue.qnt:inv_no_stability_pool | docs/CDP_MODULAR_PARITY.md:163,422 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | SP removed; no references in liq-queue source. |
| LQ.inv_only_positions_liquidates | organs/liq_queue.qnt:inv_only_positions_liquidates | contracts/liq-queue/src/bid.rs:359 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Liquidate auth = positions_contract only. |
| LQ.inv_owner_governance_distinct | organs/liq_queue.qnt:inv_owner_governance_distinct | packages/membrane/src/liq_queue.rs:183 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Amendment 4. |

## Supporting organs (Phase C / Amendment 8) — full coverage

### Acquisition

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| AQ.inv_history_capped | organs/acquisition.qnt:inv_history_capped | contracts/acquisition/src/state.rs:18-33 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | <= 365 entries. |
| AQ.inv_window_id_monotone | organs/acquisition.qnt:inv_window_id_monotone | contracts/acquisition/src/contract.rs:209-288 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Window IDs strictly increase. |
| AQ.inv_pool_under_max | organs/acquisition.qnt:inv_pool_under_max | contracts/acquisition/src/contract.rs:601-900 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | accrued_pool ≤ max_mbrn_emission. |
| AQ.inv_bump_rate_capped | organs/acquisition.qnt:inv_bump_rate_capped | contracts/acquisition/src/contract.rs:707-768 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | bump_rate ≤ max_bump_rate. |
| AQ.inv_intent_ratios_well_formed | organs/acquisition.qnt:inv_intent_ratios_well_formed | contracts/acquisition/src/contract.rs:326 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Σ ratio ≈ 1. |
| AQ.inv_clawback_window_consistency | organs/acquisition.qnt:inv_clawback_window_consistency | contracts/acquisition/src/contract.rs:364 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | vested_at = deposit_time + cliff. |
| AQ.inv_first_deposit_starts_timers | organs/acquisition.qnt:inv_first_deposit_starts_timers | contracts/acquisition/src/contract.rs:339-344 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | window_timers_started ↔ first_deposit_time set. |
| AQ.inv_owner_governance_distinct | organs/acquisition.qnt | packages/membrane/src/acquisition.rs:184-200 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Amendment 4. |

### Governance

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| GOV.inv_proposal_count_monotone | organs/governance.qnt:inv_proposal_count_monotone | contracts/governance/src/state.rs (PROPOSAL_COUNT) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | All keys ≤ count. |
| GOV.inv_pending_below_threshold | organs/governance.qnt:inv_pending_below_threshold | contracts/governance/src/contracts.rs:257-262 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Pending ⇒ aligned < required_stake. |
| GOV.inv_active_above_threshold | organs/governance.qnt:inv_active_above_threshold | contracts/governance/src/contracts.rs:428-435 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Active proposals have ≥ required_stake aligned. |
| GOV.inv_no_self_voting | organs/governance.qnt:inv_no_self_voting | contracts/governance/src/contracts.rs:309-318 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Submitter cannot vote on their own proposal. |
| GOV.inv_executed_no_expired | organs/governance.qnt:inv_executed_no_expired | contracts/governance/src/contracts.rs:585-586 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Cannot execute past expiration. |
| GOV.inv_time_ordering | organs/governance.qnt:inv_time_ordering | contracts/governance/src/contracts.rs:231-236 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | start ≤ end ≤ delayed_end ≤ expiration. |
| GOV.inv_quorum_threshold_bounded | organs/governance.qnt:inv_quorum_threshold_bounded | packages/membrane/src/governance.rs:12,15 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Quorum ≥ 33%, threshold ≥ 51%. |
| GOV.inv_voting_period_bounded | organs/governance.qnt:inv_voting_period_bounded | packages/membrane/src/governance.rs:16 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | 1-14 day window. |
| GOV.inv_required_stake_bounded | organs/governance.qnt:inv_required_stake_bounded | packages/membrane/src/governance.rs:19 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | 1k-5k MBRN. |
| GOV.inv_owner_governance_distinct | organs/governance.qnt | packages/membrane/src/governance.rs:170-199 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Amendment 4. |

### Staking

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| ST.inv_totals_match_staked | organs/staking.qnt:inv_totals_match_staked | contracts/staking/src/state.rs (STAKING_TOTALS) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Σ STAKED.amount = totals.stakers. |
| ST.inv_min_stake_per_deposit | organs/staking.qnt:inv_min_stake_per_deposit | contracts/staking/src/contract.rs:497-549 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per deposit ≥ 1 MBRN. |
| ST.inv_lock_within_ceiling | organs/staking.qnt:inv_lock_within_ceiling | contracts/staking/src/contract.rs:2527 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | locked_until ≤ stake_time + ceiling. |
| ST.inv_unstaking_period_floor | organs/staking.qnt:inv_unstaking_period_floor | packages/membrane/src/staking.rs:206-239 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | ≥ 7 days (Amendment 4). |
| ST.inv_max_commission_capped | organs/staking.qnt:inv_max_commission_capped | contracts/staking/src/contract.rs:131,151 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | ≤ 100%. |
| ST.inv_delegations_bidirectional | organs/staking.qnt:inv_delegations_bidirectional | contracts/staking/src/contract.rs:1018-1116 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | delegated ↔ delegated_to consistency. |
| ST.inv_fee_event_rates_nonneg | organs/staking.qnt:inv_fee_event_rates_nonneg | contracts/staking/src/contract.rs:1568-1698 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per-MBRN rate stored, never negative. |
| ST.inv_owner_governance_distinct | organs/staking.qnt | packages/membrane/src/staking.rs:206-239 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Amendment 4. |

### Vesting

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| VS.inv_total_allocation_under_cap | organs/vesting.qnt:inv_total_allocation_under_cap | contracts/vesting/src/contract.rs:630-793 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Σ allocations ≤ total_allocation. |
| VS.inv_amount_withdrawn_under_amount | organs/vesting.qnt:inv_amount_withdrawn_under_amount | contracts/vesting/src/contract.rs:572-626 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per allocation. |
| VS.inv_schedule_withdrawn_under_total | organs/vesting.qnt:inv_schedule_withdrawn_under_total | contracts/vesting/src/contract.rs:978-1084 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per VestingSchedule. |
| VS.inv_transmutation_balance | organs/vesting.qnt:inv_transmutation_balance | contracts/vesting/src/contract.rs:1124-1150 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | OLD_MBRN - MINTED == TOTAL_OLD. |
| VS.inv_owner_governance_distinct | organs/vesting.qnt | packages/membrane/src/vesting.rs:142-157 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Amendment 4. |

### Revenue Distributor

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| RD.inv_split_sums_to_one | organs/revenue_distributor.qnt:inv_split_sums_to_one | packages/membrane/src/revenue_distributor.rs:30-38 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | disco+junior+senior+reserve = 1. |
| RD.inv_promises_nonneg | organs/revenue_distributor.qnt:inv_promises_nonneg | contracts/revenue-distributor/src/contract.rs:173-250 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | All promise amounts ≥ 0. |
| RD.inv_distribution_prop_subset_of_promises | organs/revenue_distributor.qnt | contracts/revenue-distributor/src/contract.rs:323 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Structural. |
| RD.inv_failed_distributions_nonneg | organs/revenue_distributor.qnt:inv_failed_distributions_nonneg | contracts/revenue-distributor/src/reply.rs:85-86 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| RD.inv_reserve_nonneg | organs/revenue_distributor.qnt:inv_reserve_nonneg | contracts/revenue-distributor/src/contract.rs:369 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| RD.inv_owner_governance_distinct | organs/revenue_distributor.qnt | packages/membrane/src/revenue_distributor.rs:51-77 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Amendment 4. |
| RD.reply.DISTRIBUTION_REPLY_ID=1 | organs/revenue_distributor.qnt | contracts/revenue-distributor/src/reply.rs:18 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| RD.reply.REVENUE_DESTINATION_REPLY_ID=2 | organs/revenue_distributor.qnt | contracts/revenue-distributor/src/reply.rs:99 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| RD.reply.TAKE_REVENUE_REPLY_ID=3 | organs/revenue_distributor.qnt | contracts/revenue-distributor/src/reply.rs:123 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |

### Auction

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| AU.inv_discount_initial_bounded | organs/auction.qnt:inv_discount_initial_bounded | contracts/auction/src/contracts.rs:181-184 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | 1% ≤ initial ≤ 10%. |
| AU.inv_discount_increase_bounded | organs/auction.qnt:inv_discount_increase_bounded | contracts/auction/src/contracts.rs:181-184 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | 1% ≤ increase ≤ 5%. |
| AU.inv_timeframe_bounded | organs/auction.qnt:inv_timeframe_bounded | contracts/auction/src/contracts.rs:195-197 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | 10s..300s. |
| AU.inv_twap_bounded | organs/auction.qnt:inv_twap_bounded | contracts/auction/src/contracts.rs (UpdateConfig) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | 60..480 minutes. |
| AU.inv_no_cdt_in_fee_auctions | organs/auction.qnt:inv_no_cdt_in_fee_auctions | contracts/auction/src/contracts.rs:299-302 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | StartAuction rejects CDT. |
| AU.inv_mbrn_sale_allocations_consistent | organs/auction.qnt:inv_mbrn_sale_allocations_consistent | contracts/auction/src/contracts.rs:967-1024 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | cdt_fulfilled ≤ cdt_bad_debt; mbrn_used ≤ mbrn_allocated. |
| AU.inv_mbrn_sale_summed_consistent | organs/auction.qnt:inv_mbrn_sale_summed_consistent | contracts/auction/src/contracts.rs (StartMBRNSale) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | sale.cdt_fulfilled ≤ sale.max_cdt. |
| AU.inv_owner_governance_distinct | organs/auction.qnt | packages/membrane/src/auction.rs:113-150 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Amendment 4. |

## Infrastructure (Phase D / Amendment 9) — full coverage

### Oracle (multi-source aggregation + Amendment 5 circuit breaker)

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| OR.inv_no_duplicate_basket_per_asset | shared/oracle.qnt:inv_no_duplicate_basket_per_asset | contracts/oracle/src/contracts.rs:312 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | DuplicateOracle guard. |
| OR.inv_staleness_capped | shared/oracle.qnt:inv_staleness_capped | contracts/oracle/src/contracts.rs:666 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | <= MAX_ORACLE_STALENESS_S = 1h. |
| OR.inv_max_deviation_capped | shared/oracle.qnt:inv_max_deviation_capped | contracts/oracle/src/state.rs (Amendment 5) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | <= MAX_PRICE_DEVIATION_PCT. |
| OR.inv_trip_threshold_at_least_max_deviation | shared/oracle.qnt | contracts/oracle/src/state.rs | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Sanity bound. |
| OR.inv_consumers_use_price_safe_structural | shared/oracle.qnt + composition | contracts/cdp/src/, contracts/liquidation-engine/src/, contracts/transmuter/src/ | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Every price read is gated by `price_safe`. |
| OR.aggregation_hierarchy | shared/oracle.qnt:aggregate_price | contracts/oracle/src/contracts.rs:648-984 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Pyth → Osmosis TWAP → median(USD-par TWAP) → asset/USD. |
| OR.inv_owner_governance_distinct | shared/oracle.qnt | packages/membrane/src/oracle.rs:92-108 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Amendment 4. |

### Osmosis Proxy

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| OX.inv_owners_nonempty | infrastructure/osmosis_proxy.qnt:inv_owners_nonempty | contracts/osmosis-proxy/src/state.rs:36 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | At least one owner always. |
| OX.inv_supply_nonneg | infrastructure/osmosis_proxy.qnt:inv_supply_nonneg | contracts/osmosis-proxy/src/state.rs:10-17 (TokenInfo) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | current_supply / burned_supply >= 0. |
| OX.inv_supply_under_cap | infrastructure/osmosis_proxy.qnt:inv_supply_under_cap | contracts/osmosis-proxy/src/contract.rs:779-789 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Mint cap enforced unless debt_auction. |
| OX.inv_supply_history_capped | infrastructure/osmosis_proxy.qnt:inv_supply_history_capped | contracts/osmosis-proxy/src/state.rs:54 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | One snapshot per day, capped. |
| OX.inv_at_least_one_admin_owner | infrastructure/osmosis_proxy.qnt:inv_at_least_one_admin_owner | contracts/osmosis-proxy/src/contract.rs:449 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | At least one owner with non_token_contract_auth. |
| OX.inv_mbrn_mint_restriction | (action precondition: MintTokens) | contracts/osmosis-proxy/src/contract.rs:753-761 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | If restrict_mbrn_mints, only debt_auction may mint MBRN. |
| OX.reply.CREATE_DENOM=1 | infrastructure/osmosis_proxy.qnt | contracts/osmosis-proxy/src/contract.rs (CREATE_DENOM_REPLY_ID) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| OX.reply.SWAP=2 | infrastructure/osmosis_proxy.qnt | contracts/osmosis-proxy/src/contract.rs (SWAP_REPLY_ID) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| OX.reply.USE_BALANCE_SWAP=3 | infrastructure/osmosis_proxy.qnt | contracts/osmosis-proxy/src/contract.rs (USE_BALANCE_SWAP_REPLY_ID) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |

### Neutron Proxy

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| NX.inv_owners_nonempty | infrastructure/neutron_proxy.qnt:inv_owners_nonempty | contracts/neutron-proxy/src/state.rs | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Mirror of OX. |
| NX.inv_supply_nonneg | infrastructure/neutron_proxy.qnt:inv_supply_nonneg | contracts/neutron-proxy/src/state.rs | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| NX.inv_supply_under_cap | infrastructure/neutron_proxy.qnt:inv_supply_under_cap | contracts/neutron-proxy/src/contract.rs (mint cap) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| NX.inv_at_least_one_admin_owner | infrastructure/neutron_proxy.qnt:inv_at_least_one_admin_owner | contracts/neutron-proxy/src/contract.rs (UpdateConfig auth) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | |
| NX.inv_vesting_can_mint | (MintTokens precondition) | contracts/neutron-proxy/src/contract.rs | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | vesting_contract authorized for transmutation mints. |

### Discount Vault

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| DV.inv_amounts_nonneg | infrastructure/discount_vault.qnt:inv_amounts_nonneg | contracts/discount_vault/src/contracts.rs:127-185 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per-deposit amount >= 0. |
| DV.inv_deposit_times_in_past | infrastructure/discount_vault.qnt:inv_deposit_times_in_past | contracts/discount_vault/src/contracts.rs:153 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | deposit_time <= now. |
| DV.inv_user_keys_match_records | infrastructure/discount_vault.qnt:inv_user_keys_match_records | contracts/discount_vault/src/state.rs (USERS map) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Map key = VaultUser.user. |
| DV.inv_deposits_were_accepted | infrastructure/discount_vault.qnt:inv_deposits_were_accepted | contracts/discount_vault/src/contracts.rs:137 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Deposit precondition; structural. |
| DV.inv_no_discount_vault_link | infrastructure/discount_vault.qnt:inv_no_discount_vault_link | (absence of cross-call) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | OPEN_QUESTIONS.md #5 — code/docs gap. |
| DV.inv_owner_governance_distinct | infrastructure/discount_vault.qnt | packages/membrane/src/discount_vault.rs:66-77 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Amendment 4. |

### Tokenfactory (interface contract)

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| TF.inv_authority_consistency | infrastructure/tokenfactory.qnt:inv_authority_consistency | osmosis-std::tokenfactory bindings | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | created_denoms ⇔ authority_for. |
| TF.inv_mint_burn_only_authority | infrastructure/tokenfactory.qnt:inv_mint_burn_only_authority | tokenfactory module behavior | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Structural. |
| TF.inv_no_unauthorized_mint | infrastructure/tokenfactory.qnt:inv_no_unauthorized_mint | tokenfactory module behavior | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Structural via Mint precondition. |

## Cross-organ compositions (Phase E / Amendment 10) — full coverage

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| X.inv_no_unilateral_capital_loss_path | compositions/membrane.qnt | (conjunction of all per-organ rows) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Top-level Amendment 4 invariant. |
| X.all_safety | compositions/membrane.qnt:all_safety | (every organ's `safety`) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Composite safety. |
| X.inv_bad_debt_visible_aggregate >= 0 | compositions/bad_debt_flow.qnt | contracts/cdp/src/liquidations.rs + transmuter + ltv_disco + auction | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Conservation aggregate. |
| X.inv_bad_debt_disco_freeze_implies_pending | compositions/bad_debt_flow.qnt | contracts/ltv_disco/src/state.rs:35 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Pending bad debt freezes unstake. |
| X.inv_clear_freeze_pairing | compositions/bad_debt_flow.qnt | contracts/auction/src/contracts.rs:1011-1018 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Auction emits ClearBadDebtFreeze on completion. |
| X.inv_no_double_count | compositions/bad_debt_flow.qnt | (cross-organ) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | One sink per dispatched msg. |
| X.inv_revenue_visible_aggregate >= 0 | compositions/revenue_flow.qnt | contracts/cdp/src/revenue.rs + revenue-distributor | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Conservation aggregate. |
| X.inv_split_sums_to_one | compositions/revenue_flow.qnt | revenue-distributor `RevenueSplit` | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Already enforced at module level. |
| X.inv_bad_debt_override_routes_to_cdp | compositions/revenue_flow.qnt | revenue-distributor `BAD_DEBT_ACTIVE` branch | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Bad-debt-active routes all CDT to CDP. |
| X.inv_affiliate_points_pairing | compositions/revenue_flow.qnt | revenue-distributor reply.rs:64 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Affiliate pay → Points System call. |
| X.inv_step_in_range | compositions/liquidation_flow.qnt | contracts/liquidation-engine/src/state.rs:98-115 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | 9-step IDs valid. |
| X.inv_propagation_iff_active | compositions/liquidation_flow.qnt | liquidation-engine state.rs:19-20 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Single in-flight invariant. |
| X.inv_chain_terminates | compositions/liquidation_flow.qnt | liquidation-engine reply.rs (Step 7 clears) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Apalache temporal: eventually(no_propagation). |
| X.inv_fee_cap_applied_step_2 | compositions/liquidation_flow.qnt | liquidation-engine reply.rs:362-388 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Lean theorem: LiqFeeCap.safe. |
| X.inv_timer_consistency | compositions/liquidation_flow.qnt | liquidation-engine state.rs:92 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Timer state ↔ TimerStarted outcome. |
| X.inv_bad_debt_recorded_when_unpaid | compositions/liquidation_flow.qnt | liquidation-engine reply.rs:932-941 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Step 7 records residual bad debt. |
| X.inv_acquisition_disco_owner_is_contract | compositions/acquisition_to_disco_flow.qnt | contracts/acquisition/src/contract.rs:1267 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | deposit_owner = acquisition_contract. |
| X.inv_acquisition_disco_manager_is_user | compositions/acquisition_to_disco_flow.qnt | contracts/acquisition/src/contract.rs:1269 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | manager = user. |
| X.inv_acquisition_disco_revenue_dest_is_user | compositions/acquisition_to_disco_flow.qnt | contracts/acquisition/src/contract.rs:1271 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | revenue_destination = user. |
| X.inv_clawback_emits_disco_unstake_and_burn | compositions/acquisition_to_disco_flow.qnt | acquisition contract.rs:487-507 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Pre-cliff withdrawal triggers Disco RequestUnstake + Burn. |
| X.inv_claim_after_cliff_transfers_ownership | compositions/acquisition_to_disco_flow.qnt | acquisition contract.rs:1015-1046 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Cliff passed → UpdateDeposit deposit_owner = user. |
| X.inv_acquisition_total_matches | compositions/acquisition_to_disco_flow.qnt | acquisition contract.rs:1401-1433 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Transmuter ACQ_TOTAL ≈ window total ± 1. |

## Acquisition / Governance / Revenue

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| AQ.inv_emission_window_bounds | organs/acquisition.qnt:inv_emission_window_bounds | contracts/acquisition/src/ | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Adaptive emission bounded by config min/max per block. |
| AQ.inv_disco_clawback_ownership | organs/acquisition.qnt:inv_disco_clawback_ownership | contracts/acquisition/src/ (SubmitDeposit call); docs/organs/ltv-disco/README.md (acquisition section) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Acquisition-originated Disco deposits have `deposit_owner = acquisition_contract`, `manager = user`. |
| AQ.inv_intent_boost_bounded | organs/acquisition.qnt:inv_intent_boost_bounded | docs/README.md:139-144; contracts/acquisition/src/ | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | `boost = min(lock/ceiling, 1) * max_boost`. UserBoost currently zero. |
| ST.inv_unbond_period_elapsed | organs/staking.qnt:inv_unbond_period_elapsed | contracts/staking/src/ | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Unbonding queue FIFO; withdraw blocked until period elapsed. |
| GV.inv_vote_power_sourced | organs/governance.qnt:inv_vote_power_sourced | contracts/governance/src/; docs/organs/governance/ | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Vote weight = Staking + Disco deposits at proposal snapshot. |
| VS.inv_vesting_claim_monotone | organs/vesting.qnt:inv_vesting_claim_monotone | contracts/vesting/src/ | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Claimable only grows with time; no double claim. |
| RD.inv_promise_distribution_exact | organs/revenue_distributor.qnt:inv_promise_distribution_exact | contracts/revenue-distributor/src/ (SetPromises, DistributePromises) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Σ distributed == Σ promised. |
| AU.inv_debt_auction_closes_bad_debt | organs/auction.qnt:inv_debt_auction_closes_bad_debt | contracts/auction/src/ (SendMBRNForSale / ClearBadDebtFreeze) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Auction completion invokes `ClearBadDebtFreeze` on Disco/Transmuter. |

## Shared / Infrastructure

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| OR.inv_price_fresh | shared/oracle.qnt:inv_price_fresh | contracts/oracle/src/; docs/shared/oracle.md | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Quoted prices under configured staleness window. |
| SD.inv_discount_time_curve | shared/system_discounts.qnt:inv_discount_time_curve | contracts/system_discounts/src/; docs/README.md:131-137 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | First-month + remaining piecewise formula. |
| PS.inv_points_ledger_balanced | shared/points_system.qnt:inv_points_ledger_balanced | contracts/points-system/src/ | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Points issuance sums over ledgers match minted total. |
| OX.inv_mint_burn_conservation | infrastructure/osmosis_proxy.qnt:inv_mint_burn_conservation | contracts/osmosis-proxy/src/ | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Per-denom totals match CDP + Transmuter + Disco mint/burn calls. |
| NX.inv_mint_burn_conservation | infrastructure/neutron_proxy.qnt:inv_mint_burn_conservation | contracts/neutron-proxy/src/ | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Same as Osmosis proxy on Neutron chain. |
| DV.inv_lp_deposit_ledger | infrastructure/discount_vault.qnt:inv_lp_deposit_ledger | contracts/discount_vault/src/; docs/README.md (Discount Vault row) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Sum of user balances equals vault holdings per LP denom. |
| TF.inv_denom_namespace | infrastructure/tokenfactory.qnt:inv_denom_namespace | contracts/tokenfactory/src/ | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Denoms created under contract's factory namespace only. |

## Cross-organ compositions

| Invariant ID | Quint location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| X.inv_bad_debt_conserved | compositions/bad_debt_flow.qnt:inv_bad_debt_conserved | contracts/cdp/src/liquidations.rs; contracts/transmuter/src/contract.rs:827-894; contracts/ltv_disco/src/state.rs:35; contracts/auction/src/ | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Bad debt + in-flight msgs + sinks conserved across reply chain. |
| X.inv_revenue_conserved | compositions/revenue_flow.qnt:inv_revenue_conserved | contracts/cdp/src/revenue.rs; contracts/revenue-distributor/src/; contracts/staking/src/; contracts/ltv_disco/src/execute.rs | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | CDP `take_revenue` sums equal RevDist distributed sums equal Staking+Disco receipts. |
| X.inv_cdt_monetary_conservation | compositions/membrane.qnt:inv_cdt_monetary_conservation | contracts/osmosis-proxy/src/; contracts/neutron-proxy/src/; CDP mint paths | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Total CDT minted by CDP equals held balances + burned. |
| X.inv_liquidation_reply_chain_complete | compositions/liquidation_flow.qnt:inv_liquidation_reply_chain_complete | docs/cross-organ/liquidation-flow.md; contracts/liquidation-engine/src/; contracts/liq-queue/src/ | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | 9-step reply chain always reaches a terminal state. |
| X.inv_acquisition_to_disco_binding | compositions/acquisition_to_disco_flow.qnt:inv_acquisition_to_disco_binding | contracts/acquisition/src/; docs/organs/ltv-disco/README.md (acquisition section) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | Deposits routed via acquisition carry `deposit_owner = acquisition_contract`, `manager = user`. |

## Lean arithmetic carve-out (Phase F / Amendment 11) — FULLY PROVED

All seven `sorry` placeholders replaced with concrete proofs. Mathlib pin at `master` (TODO: lock to a commit before first CI run).

| Theorem | Lean location | Rust source (file:lines) | Last synced commit | Notes |
|---|---|---|---|---|
| VTRate.roundtrip_le | lean/MembraneSpec/Arithmetic/VTRate.lean | packages/membrane/src/stability_pool_vault.rs (calculate_vault_tokens / calculate_base_tokens) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | **Proved.** Round-trip never increases base; uses `Nat.div_mul_le_self` + `Nat.mul_div_cancel_left`. |
| VTRate.roundtrip_identity_at_unit_rate | lean/.../VTRate.lean | same | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | **Proved.** Identity when staked = supply. |
| VTRate.mint_empty_pool | lean/.../VTRate.lean | same | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | **Proved.** First-deposit ratio = DEFAULT_VT_PER_BASE. |
| IsqrtMonotone.isqrt_le_isqrt | lean/.../IsqrtMonotone.lean | contracts/ltv_disco/src/ (isqrt usage); docs/README.md:121-127 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | **Proved.** Direct `Nat.sqrt_le_sqrt` application. |
| IsqrtMonotone.isqrt_bounds | lean/.../IsqrtMonotone.lean | same | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | **Proved.** `Nat.sqrt_le` + `Nat.lt_succ_sqrt`. |
| IsqrtMonotone.isqrt_strict_mono_lower_bound | lean/.../IsqrtMonotone.lean | same | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | **Proved.** Riskiest slot's weight is largest. |
| FeeSplit.fee_le_input | lean/.../FeeSplit.lean | contracts/transmuter/src/contract.rs:972-990 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | **Proved.** Helper: fee ≤ input when d ≤ 1. |
| FeeSplit.fee_split_exact | lean/.../FeeSplit.lean | same | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | **Proved.** Floor-rounding residual lives entirely in `fee`. |
| InterestAccrual.accrued_additive | lean/.../InterestAccrual.lean | contracts/cdp/src/rates.rs | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | **Proved.** Floor-rounded sum-of-parts bounded above by floor of sum. |
| InterestAccrual.accrued_zero | lean/.../InterestAccrual.lean | same | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | **Proved.** Identity at zero elapsed. |
| InterestAccrual.accrued_mono_elapsed | lean/.../InterestAccrual.lean | same | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | **Proved.** Monotone in time. |
| TrancheWaterfall.waterfall_bounds | lean/.../TrancheWaterfall.lean | contracts/transmuter/src/contract.rs:859-872 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | **Proved.** Junior + senior loss = amount when amount ≤ j+s. |
| TrancheWaterfall.waterfall_nonneg | lean/.../TrancheWaterfall.lean | same | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | **Proved.** Trivial via `Nat`. |
| LiqFeeCap.liq_fee_cap_safe | lean/.../LiqFeeCap.lean | contracts/liquidation-engine/src/reply.rs:362-388 | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | **Proved.** `caller_fee * repay ≤ collateral` under cap. Both `min` arms covered. |
| DecimalMulRatio.multiplyRatio_eq_div | lean/.../DecimalMulRatio.lean | cosmwasm_std (Decimal::from_ratio) | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | **Proved.** Equality when den > 0. |
| DecimalMulRatio.multiplyRatio_le | lean/.../DecimalMulRatio.lean | same | `c87c2ff70035603998fdb9d7d77d07b68339d8df` | **Proved.** ≤ form covers den = 0 branch. |
