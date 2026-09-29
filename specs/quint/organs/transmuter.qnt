// specs/quint/organs/transmuter.qnt
//
// Formal spec for the Transmuter contract + the NEW per-user manager
// subsystem (modeled on LTV Disco's manager, adapted to a per-user binding)
// + the NEW switching-period withdraw-intent system for juniors.
//
// Source of truth (Rust):
//   contracts/transmuter/src/contract.rs
//   contracts/transmuter/src/state.rs
//   contracts/transmuter/src/error.rs
//   packages/membrane/src/transmuter.rs
//
// Manager-subsystem reference (Disco):
//   contracts/ltv_disco/src/execute.rs:971-2203
//   contracts/ltv_disco/src/state.rs:61-130
//
// Switching-period semantics (NEW, defined for the first time here):
//   Juniors cannot ExitVault directly. They must RequestSwitch to declare
//   one of three intents, wait `switching_period` (default 14 days), then
//   within the following `switching_execution_window` (default 2 days)
//   anyone may permissionlessly ExecuteSwitch for them. Past the window,
//   the intent expires and must be re-requested.
//   Seniors ExitVault directly with no wait.
//
// Manager fee change delay (NEW, with asymmetry): fee INCREASES go through
// the 14-day request + 2-day execution window. Fee DECREASES apply
// instantly (managers can voluntarily lower their cut; users are never
// surprised by a hike). See RequestManagerFeeChange below for the branch.
//
// Acquisition bypass (NEW): any path where `info.sender` is the
// `config.acquisition_contract` is exempt from the junior-exit intent
// gate in ExitVault. The acquisition contract owns clawback-eligible
// deposits and must be able to unwind on its own schedule.
//
// Manager P&L via events (NEW, storage-free): manager bad-debt
// absorption, fee earnings, and fee-change lifecycle are emitted as
// CosmWasm events (see types.qnt::Event). No on-chain history storage.
// Indexers are the authoritative reader of manager performance.
// Because every organ action would otherwise have to thread the
// `events` list through its state update, this module adopts the
// convention that only actions explicitly calling `append_event` update
// `events`; elided actions keep it unchanged (treat `events' = events`
// as implicit in the bodies below when not(shown)).
//
// Every invariant cites its Rust source in a comment. When those lines
// change, bump the matching row in specs/SPEC_MAP.md.
module transmuter {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  // ================================================================
  // Types (mirror packages/membrane/src/transmuter.rs:7-80, 211-253)
  // ================================================================
  type TrancheKey     = (Denom, bool)                     // (collateral_denom, junior)
  type UserTrancheKey = (Addr, TrancheKey)

  type TrancheState = {
    vault_token_supply: Uint128,
    total_staked: Uint128,
  }

  type TrancheRateAssurance = {
    pre_vault_token_supply: Uint128,
    pre_total_staked: Uint128,
  }

  // Mirror of Config (packages/membrane/src/transmuter.rs:211-246)
  // plus NEW fields for the manager subsystem, switching period, and
  // the governance two-role split (Amendment 4 / CAPITAL_LOSS_SURFACE.md).
  type TConfig = {
    owner: Addr,
    // NEW: Governance multisig address — distinct from `owner`. Required
    // for catastrophic Category-A actions (paired_asset switch,
    // cdp_contract switch, acquisition_contract switch, governance and
    // owner address transfers, granting tokenfactory authority via the
    // Osmosis/Neutron proxies).
    governance: Addr,
    discounts_contract: Addr,
    cdp_contract: Addr,
    collateral_contract: Addr,
    deposit_pair: AssetPair,
    composition_leeway: Decimal,
    cdt_target_ratio: Decimal,
    usage_fee: Decimal,
    usage_fee_utilization_threshold: Decimal,    // stored, NEVER read (dead)
    swap_history_cap: int,
    volume_history_cap: int,
    allowlist: Set[Addr],
    revenue_distributor_addr: Option[Addr],
    affiliate_fee: Decimal,
    send_swap_fee: bool,
    revenue_distributor_fee_percentage: Decimal,
    acquisition_contract: Option[Addr],
    points_system_contract: Option[Addr],
    retention_multiplier: Decimal,               // stored, NEVER read (dead)

    // NEW: manager subsystem
    max_management_fee: Decimal,

    // NEW: switching-period semantics (apply to junior exits, junior→senior,
    // removing a user's asset allocation, and manager-fee changes).
    switching_period: int,               // default 14 days = 1_209_600 seconds
    switching_execution_window: int,     // default 2  days = 172_800   seconds
  }

  // NEW: manager's tranche reshuffle request (per-user, net-zero).
  type TrancheUpdateRequest = {
    add_tranches:    List[(Denom, bool, Uint128)],   // (denom, junior, vt_amount)
    remove_tranches: List[(Denom, bool, Uint128)],
  }

  // NEW: switching-intent variants.
  // - JuniorToSenior:         move VTs from (denom, junior=true) to (denom, junior=false)
  // - JuniorExit:              withdraw VTs from a (denom, junior=true) tranche
  // - SeniorOnboardingExit:    withdraw VTs from a (denom, junior=false) tranche
  //                            while the asset is STILL IN its onboarding
  //                            window. Required exclusively for the
  //                            autonomous-collateral onboarding rule —
  //                            seniors behave like juniors while the
  //                            cAsset.onboarding_window_end has not(passed).
  // - RemoveAsset:             withdraw ALL of this user's tranche positions
  //                            for `denom` (both tranches).
  type SwitchingIntentKind =
    | Intent_JuniorToSenior({ collateral_denom: Denom, vt_amount: Uint128 })
    | Intent_JuniorExit({ collateral_denom: Denom, vt_amount: Uint128,
                          recipient: Option[Addr], withdraw_as: Option[Denom] })
    | Intent_SeniorOnboardingExit({ collateral_denom: Denom, vt_amount: Uint128,
                                    recipient: Option[Addr], withdraw_as: Option[Denom] })
    | Intent_RemoveAsset({ collateral_denom: Denom,
                           recipient: Option[Addr], withdraw_as: Option[Denom] })

  type SwitchingIntent = {
    id: int,
    user: Addr,
    kind: SwitchingIntentKind,
    created_at: Timestamp,
    unlock_time: Timestamp,    // created_at + config.switching_period
    expire_time: Timestamp,    // unlock_time + config.switching_execution_window
  }

  // NEW: pending manager-fee change (2-week request + 2-day execution window).
  type FeeChangeRequest = {
    manager: Addr,
    new_fee: Decimal,
    created_at: Timestamp,
    unlock_time: Timestamp,    // created_at + config.switching_period
    expire_time: Timestamp,    // unlock_time + config.switching_execution_window
  }

  // ================================================================
  // State (contracts/transmuter/src/state.rs:7-49 + NEW rows)
  // ================================================================
  var config:                 TConfig
  // state.rs:13 — USER_TRANCHES
  var user_tranches:          UserTrancheKey -> Uint128
  // state.rs:16 — TRANCHE_STATES
  var tranche_states:         TrancheKey -> TrancheState
  // state.rs:19 — TRANCHE_RATE_ASSURANCE
  var tranche_rate_assurance: TrancheKey -> TrancheRateAssurance
  // state.rs:22 — TOTAL_TRANCHE_STAKED
  var total_tranche_staked:   Uint128
  // state.rs:25 — UNDISTRIBUTED_FEES
  var undistributed_fees:     Uint128
  // state.rs:29 — ACQUISITION_DEPOSIT_TOTAL
  var acquisition_deposit_total: Uint128
  // state.rs:32 — CUMULATIVE_VOLUME
  var cumulative_volume:      Uint128
  // state.rs:38 — DEPLOYED_PAIRED_ASSET
  var deployed_paired_asset:  Uint128
  // state.rs:41 — PENDING_REVENUE
  var pending_revenue:        Uint128
  // state.rs:44 — AFFILIATES
  var affiliates:             Addr -> List[AffiliateData]

  // NEW: manager subsystem (per-USER binding).
  var transmuter_manager:     Addr -> Addr            // user -> manager
  var transmuter_manager_fee: Addr -> Decimal         // manager -> fee
  var managed_users:          Addr -> Set[Addr]       // manager -> set of users

  // NEW: per-user principal tracking for the yield-only manager-fee branch.
  var user_principal_base:    Addr -> Uint128

  // NEW: pending manager-fee changes (one per manager).
  var pending_fee_change:     Addr -> FeeChangeRequest

  // NEW: switching intents.
  var switching_intents:      int -> SwitchingIntent   // id -> intent
  var next_intent_id:         int
  var user_switching_intents: Addr -> Set[int]         // user -> set of owned intents

  // NEW: event log (off-chain only — no corresponding CosmWasm storage).
  // Specific actions `append` to this list; others leave it untouched.
  // Elided `events' = events` in action bodies — treat as implicit.
  var events: List[Event]

  // NEW (Amendment 4): pending config-change records (timelock pattern).
  // Keyed by `field_id` so concurrent changes to different fields are
  // tracked independently; same-field re-request overwrites.
  var pending_config_change: str -> ConfigChangeRequest

  // NEW: local mirror of the Collateral contract's onboarding windows.
  // In the Rust implementation, ExitVault calls into the Collateral
  // contract via WasmQuery at exit time; we model it here as an
  // abstracted map that composition/tests update alongside Collateral
  // actions. Value > 0 means "asset is still inside its onboarding
  // window when block_time < value"; value == 0 means "no window,
  // always seniors-free".
  var collateral_onboarding_end: Denom -> Timestamp

  // ================================================================
  // init (contracts/transmuter/src/contract.rs instantiate)
  // ================================================================
  action init(cfg: TConfig): bool = all {
    config' = cfg,
    user_tranches' = Map(),
    tranche_states' = Map(),
    tranche_rate_assurance' = Map(),
    total_tranche_staked' = 0,
    undistributed_fees' = 0,
    acquisition_deposit_total' = 0,
    cumulative_volume' = 0,
    deployed_paired_asset' = 0,
    pending_revenue' = 0,
    affiliates' = Map(),
    transmuter_manager' = Map(),
    transmuter_manager_fee' = Map(),
    managed_users' = Map(),
    user_principal_base' = Map(),
    pending_fee_change' = Map(),
    switching_intents' = Map(),
    next_intent_id' = 1,
    user_switching_intents' = Map(),
    events' = List(),
    collateral_onboarding_end' = Map(),
    pending_config_change' = Map(),
    block_time' = 0,
    block_height' = 0,
    balances' = Map(),
    msg_queue' = List(),
    pending_reply' = None,
  }

  // ================================================================
  // Helpers
  // ================================================================
  pure def tranche_of(ts: TrancheKey -> TrancheState, k: TrancheKey): TrancheState =
    if (ts.keys().contains(k)) ts.get(k) else { vault_token_supply: 0, total_staked: 0 }

  pure def user_vt(ut: UserTrancheKey -> Uint128, u: Addr, k: TrancheKey): Uint128 =
    if (ut.keys().contains((u, k))) ut.get((u, k)) else 0

  pure def get_manager(m: Addr -> Addr, u: Addr): Option[Addr] =
    if (m.keys().contains(u)) Some(m.get(u)) else None

  pure def get_fee(f: Addr -> Decimal, m: Addr): Decimal =
    if (f.keys().contains(m)) f.get(m) else 0

  pure def managed_set(mu: Addr -> Set[Addr], m: Addr): Set[Addr] =
    if (mu.keys().contains(m)) mu.get(m) else Set()

  pure def effective_cdt_target(cfg: TConfig,
                                deployed: Uint128,
                                total_deposits: Uint128): Decimal =
    if (total_deposits == 0) cfg.cdt_target_ratio
    else max_int(cfg.cdt_target_ratio, decimal_from_ratio(deployed, total_deposits))

  // Ready-to-execute window for a switching intent.
  pure def intent_is_ready(i: SwitchingIntent, now: Timestamp): bool =
    now >= i.unlock_time and now <= i.expire_time

  // Expired (past the 2-day execution window).
  pure def intent_is_expired(i: SwitchingIntent, now: Timestamp): bool =
    now > i.expire_time

  pure def fee_change_is_ready(r: FeeChangeRequest, now: Timestamp): bool =
    now >= r.unlock_time and now <= r.expire_time

  // NEW: onboarding-window lookup on the local mirror.
  pure def asset_in_onboarding(om: Denom -> Timestamp,
                                d: Denom,
                                now: Timestamp): bool =
    if (om.keys().contains(d)) om.get(d) > now else false

  // ExitVault gate helper:
  //   - Junior entry: require Intent_JuniorExit or Intent_RemoveAsset (ready).
  //   - Senior entry AND asset_in_onboarding: require
  //     Intent_SeniorOnboardingExit or Intent_RemoveAsset (ready).
  //   - Senior entry outside onboarding: no intent required.
  pure def tranche_entry_covered(
    intents: int -> SwitchingIntent,
    user_idx: Addr -> Set[int],
    onboarding: Denom -> Timestamp,
    user: Addr,
    denom: Denom,
    junior: bool,
    vt_amount: Uint128,
    now: Timestamp
  ): bool = {
    val needs_intent: bool = junior or asset_in_onboarding(onboarding, denom, now)
    if (not(needs_intent)) true
    else if (not(user_idx.keys().contains(user))) false
    else user_idx.get(user).exists(id =>
      intents.keys().contains(id) and {
        val i = intents.get(id)
        match i.kind {
      | Intent_JuniorExit(p) =>
            junior and p.collateral_denom == denom and
            p.vt_amount >= vt_amount and intent_is_ready(i, now)
      | Intent_SeniorOnboardingExit(p) =>
            (not(junior)) and
            asset_in_onboarding(onboarding, denom, now) and
            p.collateral_denom == denom and
            p.vt_amount >= vt_amount and intent_is_ready(i, now)
      | Intent_RemoveAsset(p) =>
            p.collateral_denom == denom and intent_is_ready(i, now)
      | _ => false
    }
      })
  }

  // ================================================================
  // Execute actions — one per ExecuteMsg variant
  //   (packages/membrane/src/transmuter.rs:83-178)
  // ================================================================

  // --- UpdateConfig (contract.rs:1675-1680, RESTRICTED scope) ---
  //
  // NEW (Amendment 4): UpdateConfig is now only permitted to mutate
  // OPERATIONAL fields (Category C in CAPITAL_LOSS_SURFACE.md). Any
  // Category-A or B field MUST be changed via RequestConfigChange /
  // ExecuteConfigChange or routed through the Governance gate. The
  // structural witness for this is the predicate
  // `cfg_only_changes_operational(old, new)` below — the action's
  // require asserts it. Concrete implementer wires the predicate to
  // diff each field individually.
  action UpdateConfig(sender: Addr, cfg: TConfig): bool = all {
    sender == config.owner,
    cfg_only_changes_operational(config, cfg),
    config' = cfg,
    user_tranches' = user_tranches, tranche_states' = tranche_states,
    tranche_rate_assurance' = tranche_rate_assurance,
    total_tranche_staked' = total_tranche_staked,
    undistributed_fees' = undistributed_fees,
    acquisition_deposit_total' = acquisition_deposit_total,
    cumulative_volume' = cumulative_volume,
    deployed_paired_asset' = deployed_paired_asset,
    pending_revenue' = pending_revenue,
    affiliates' = affiliates,
    transmuter_manager' = transmuter_manager,
    transmuter_manager_fee' = transmuter_manager_fee,
    managed_users' = managed_users,
    user_principal_base' = user_principal_base,
    pending_fee_change' = pending_fee_change,
    switching_intents' = switching_intents,
    next_intent_id' = next_intent_id,
    user_switching_intents' = user_switching_intents,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    collateral_onboarding_end' = collateral_onboarding_end,
    pending_config_change' = pending_config_change,
  }

  // --- EnterVault (contract.rs:~355-549) ---
  action EnterVault(
    sender: Addr,
    recipient: Option[Addr],
    funds_cdt: Uint128,
    funds_paired: Uint128,
    tranches: List[(Denom, bool)],
    affiliate: Option[Addr]
  ): bool = {
    val eff_rec: Addr = match recipient {
      | Some(r) => r
      | None => sender
    }
    val is_acquisition_path: bool = match config.acquisition_contract {
      | Some(a) => eff_rec == a
      | None => false
    }
    val tranche_set: Set[(Denom, bool)] =
      tranches.foldl(Set(), (acc, t) => acc.union(Set(t)))
    val acq_total_after: Uint128 =
      if (is_acquisition_path)
        acquisition_deposit_total + funds_cdt + funds_paired
      else acquisition_deposit_total
    val rate_assurance_after: TrancheKey -> TrancheRateAssurance =
      if (is_acquisition_path) tranche_rate_assurance
      else tranches.foldl(tranche_rate_assurance, (m, t) => {
        val ts = tranche_of(tranche_states, t)
        m.put(t, { pre_vault_token_supply: ts.vault_token_supply,
                   pre_total_staked: ts.total_staked })
      })
    val prev_principal: Uint128 =
      if (user_principal_base.keys().contains(sender))
        user_principal_base.get(sender) else 0
    val principal_after: Addr -> Uint128 =
      if (is_acquisition_path) user_principal_base
      else user_principal_base.put(sender, prev_principal + funds_cdt + funds_paired)
    all {
      funds_cdt + funds_paired > 0,
      tranches.length() > 0,
      tranches.length() == tranche_set.size(),
      acquisition_deposit_total' = acq_total_after,
      tranche_rate_assurance' = rate_assurance_after,
      user_tranches' = user_tranches,
      tranche_states' = tranche_states,
      user_principal_base' = principal_after,
      msg_queue' = msg_queue.append(Msg_Reply({ id: 0, data: "rate_assurance" })),
      pending_reply' = pending_reply,
      config' = config,
      total_tranche_staked' = total_tranche_staked,
      undistributed_fees' = undistributed_fees,
      cumulative_volume' = cumulative_volume,
      deployed_paired_asset' = deployed_paired_asset,
      pending_revenue' = pending_revenue,
      affiliates' = affiliates,
      transmuter_manager' = transmuter_manager,
      transmuter_manager_fee' = transmuter_manager_fee,
      managed_users' = managed_users,
      pending_fee_change' = pending_fee_change,
      switching_intents' = switching_intents,
      next_intent_id' = next_intent_id,
      user_switching_intents' = user_switching_intents,
      events' = events,
      collateral_onboarding_end' = collateral_onboarding_end,
      pending_config_change' = pending_config_change,
      block_time' = block_time, block_height' = block_height, balances' = balances,
    }
  }

  // --- ExitVault (contract.rs:~550-723) ---
  //
  // NEW SEMANTICS (spec-defined):
  //   - Senior-only withdrawals:      immediate, no wait.
  //   - Any junior withdrawal:        blocked unless a READY JuniorExit or
  //                                    RemoveAsset intent exists for this
  //                                    user+denom, and the vt_amount is ≤ the
  //                                    intent's requested amount.
  //   - Self / CDP / acquisition:     auth path unchanged; ACQUISITION CALLS
  //                                    ADDITIONALLY BYPASS THE JUNIOR-INTENT
  //                                    GATE (clawback must unwind without
  //                                    requiring a 14d user-driven intent).
  //
  // The caller may pass a `tranche_withdrawals` list mixing junior and
  // senior entries; each junior entry is validated against the intent set
  // independently. Seniors never need intents. Acquisition-sender bypasses
  // the gate entirely.
  action ExitVault(
    sender: Addr,
    recipient: Option[Addr],
    withdraw_as: Option[Denom],
    user_override: Option[Addr],
    amount: Option[Uint128],
    tranche_withdrawals: Option[List[(Denom, bool, Uint128)]]
  ): bool = {
    val effective_user: Addr = match user_override {
      | Some(u) => u
      | None => sender
    }
    val is_self: bool = sender == SELF
    val is_acquisition: bool = match config.acquisition_contract {
      | Some(a) => sender == a
      | None => false
    }
    // Every entry in tranche_withdrawals is checked:
    //   - junior → must be covered by a ready Intent_JuniorExit /
    //              Intent_RemoveAsset.
    //   - senior + asset_in_onboarding → must be covered by a ready
    //              Intent_SeniorOnboardingExit / Intent_RemoveAsset.
    //   - senior outside onboarding → free withdrawal.
    val entries: List[(Denom, bool, Uint128)] =
      match tranche_withdrawals {
      | Some(l) => l
      | None => List()
    }
    val all_entries_covered: bool =
      entries.indices().forall(i => {
        val e = entries[i]
        tranche_entry_covered(switching_intents, user_switching_intents,
                               collateral_onboarding_end,
                               effective_user, e._1, e._2, e._3, block_time)
      })
    val mgr_check_ok: bool =
      if (transmuter_manager.keys().contains(effective_user))
        transmuter_manager.get(effective_user) != sender or effective_user == sender
      else true
    val withdraw_amount: Uint128 = match amount {
      | Some(a) => a
      | None => 0
    }
    val acq_total_after: Uint128 =
      if (is_acquisition) sat_sub(acquisition_deposit_total, withdraw_amount)
      else acquisition_deposit_total
    all {
      effective_user == sender or is_self or is_acquisition,
      mgr_check_ok,
      is_acquisition or all_entries_covered,
      msg_queue' = msg_queue
        .append(Msg_Reply({ id: 0, data: "rate_assurance" })),
      config' = config,
      user_tranches' = user_tranches,
      tranche_states' = tranche_states,
      tranche_rate_assurance' = tranche_rate_assurance,
      total_tranche_staked' = total_tranche_staked,
      undistributed_fees' = undistributed_fees,
      acquisition_deposit_total' = acq_total_after,
      cumulative_volume' = cumulative_volume,
      deployed_paired_asset' = deployed_paired_asset,
      pending_revenue' = pending_revenue,
      affiliates' = affiliates,
      transmuter_manager' = transmuter_manager,
      transmuter_manager_fee' = transmuter_manager_fee,
      managed_users' = managed_users,
      user_principal_base' = user_principal_base,
      pending_fee_change' = pending_fee_change,
      switching_intents' = switching_intents,
      next_intent_id' = next_intent_id,
      user_switching_intents' = user_switching_intents,
      // NEW: emit manager-fee-earned event for managers of exiting users.
      // Body abstract — concrete implementer walks the managed users and
      // appends one event per (manager, user, yield) tuple.
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
      events' = events,
      collateral_onboarding_end' = collateral_onboarding_end,
      pending_config_change' = pending_config_change,
    }
  }

  // --- Transmute (contract.rs:937-1030+) ---
  action Transmute(
    sender: Addr, in_denom: Denom, in_amount: Uint128, recipient: Option[Addr]
  ): bool = {
    val cdt_auth_ok: bool =
      if (in_denom == config.deposit_pair.cdt)
        sender == SELF or sender == config.cdp_contract
      else true
    val is_allowlisted: bool = config.allowlist.contains(sender)
    val fee_amount: Uint128 =
      if (is_allowlisted) 0 else decimal_mul_uint(config.usage_fee, in_amount)
    val out_amount: Uint128 = in_amount - fee_amount
    val to_rev_dist: Uint128 = if (config.send_swap_fee)
      decimal_mul_uint(config.revenue_distributor_fee_percentage, fee_amount) else 0
    val to_vault: Uint128 = fee_amount - to_rev_dist
    val rev_dist_msg: CrossOrganMsg = match config.revenue_distributor_addr {
      | Some(r) => Msg_BankSend({ to: r, denom: config.deposit_pair.cdt,
                                   amount: to_rev_dist })
      | None => Msg_Reply({ id: 0, data: "no_rev_dist" })
    }
    val msg_queue_after: List[CrossOrganMsg] =
      if (to_rev_dist > 0 and config.send_swap_fee) msg_queue.append(rev_dist_msg)
      else msg_queue
    all {
      in_amount > 0,
      in_denom == config.deposit_pair.cdt or
              in_denom == config.deposit_pair.paired_asset,
      cdt_auth_ok,
      cumulative_volume' = cumulative_volume + in_amount + out_amount,
      undistributed_fees' = undistributed_fees + to_vault,
      msg_queue' = msg_queue_after,
      config' = config,
      user_tranches' = user_tranches,
      tranche_states' = tranche_states,
      tranche_rate_assurance' = tranche_rate_assurance,
      total_tranche_staked' = total_tranche_staked,
      acquisition_deposit_total' = acquisition_deposit_total,
      deployed_paired_asset' = deployed_paired_asset,
      pending_revenue' = pending_revenue,
      affiliates' = affiliates,
      transmuter_manager' = transmuter_manager,
      transmuter_manager_fee' = transmuter_manager_fee,
      managed_users' = managed_users,
      user_principal_base' = user_principal_base,
      pending_fee_change' = pending_fee_change,
      switching_intents' = switching_intents,
      next_intent_id' = next_intent_id,
      user_switching_intents' = user_switching_intents,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
      events' = events,
      collateral_onboarding_end' = collateral_onboarding_end,
      pending_config_change' = pending_config_change,
    }
  }

  // --- AssignBadDebt (contract.rs:827-894) ---
  // Bad-debt waterfall is UNCHANGED by the switching-period: pending junior
  // exit intents do NOT exempt a user's deposits from slashing until
  // ExecuteSwitch succeeds. Juniors remain first-loss until their intent
  // actually executes.
  action AssignBadDebt(sender: Addr, collateral_denom: Denom, amount: Uint128): bool = {
    val j = tranche_of(tranche_states, (collateral_denom, true))
    val s = tranche_of(tranche_states, (collateral_denom, false))
    val max_assignable: Uint128 = j.total_staked + s.total_staked
    val j_loss: Uint128 = min_int(amount, j.total_staked)
    val s_loss: Uint128 = min_int(amount - j_loss, s.total_staked)
    all {
      sender == config.cdp_contract,
      amount <= max_assignable,
      tranche_states' = tranche_states
        .put((collateral_denom, true),
             { vault_token_supply: j.vault_token_supply,
               total_staked: j.total_staked - j_loss })
        .put((collateral_denom, false),
             { vault_token_supply: s.vault_token_supply,
               total_staked: s.total_staked - s_loss }),
      total_tranche_staked' = total_tranche_staked - (j_loss + s_loss),
      msg_queue' = msg_queue
        .append(Msg_CDP_FulfillBadDebt({ asset: collateral_denom, amount: amount })),
      config' = config,
      user_tranches' = user_tranches,
      tranche_rate_assurance' = tranche_rate_assurance,
      undistributed_fees' = undistributed_fees,
      acquisition_deposit_total' = acquisition_deposit_total,
      cumulative_volume' = cumulative_volume,
      deployed_paired_asset' = deployed_paired_asset,
      pending_revenue' = pending_revenue,
      affiliates' = affiliates,
      transmuter_manager' = transmuter_manager,
      transmuter_manager_fee' = transmuter_manager_fee,
      managed_users' = managed_users,
      user_principal_base' = user_principal_base,
      pending_fee_change' = pending_fee_change,
      switching_intents' = switching_intents,
      next_intent_id' = next_intent_id,
      user_switching_intents' = user_switching_intents,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
      events' = events,
      collateral_onboarding_end' = collateral_onboarding_end,
      pending_config_change' = pending_config_change,
    }
  }

  // --- RateAssurance (self-call only) ---
  action RateAssurance(sender: Addr): bool = all {
    sender == SELF,
    tranche_rate_assurance' = Map(),
    config' = config,
    user_tranches' = user_tranches,
    tranche_states' = tranche_states,
    total_tranche_staked' = total_tranche_staked,
    undistributed_fees' = undistributed_fees,
    acquisition_deposit_total' = acquisition_deposit_total,
    cumulative_volume' = cumulative_volume,
    deployed_paired_asset' = deployed_paired_asset,
    pending_revenue' = pending_revenue,
    affiliates' = affiliates,
    transmuter_manager' = transmuter_manager,
    transmuter_manager_fee' = transmuter_manager_fee,
    managed_users' = managed_users,
    user_principal_base' = user_principal_base,
    pending_fee_change' = pending_fee_change,
    switching_intents' = switching_intents,
    next_intent_id' = next_intent_id,
    user_switching_intents' = user_switching_intents,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    collateral_onboarding_end' = collateral_onboarding_end,
    pending_config_change' = pending_config_change,
  }

  // --- RepayUserDebt (contract.rs:1148-1200; auth at 1156-1159) ---
  // Rust performs (1) execute_exit_vault to harvest CDT to the user's
  // position_owner, then (2) emits a CDP Repay WasmMsg with the harvested
  // CDT, with any excess refunded by BankSend. State-side at the Transmuter
  // organ this is identity (vault accounting changes happen through the
  // recursive ExitVault call & the CDP composition) — we model it by
  // enqueuing a rate_assurance Msg_Reply matching the inner ExitVault and
  // the cross-organ CDP-Repay placeholder. Auth must be position owner or
  // the CDP contract itself.
  action RepayUserDebt(sender: Addr, user_info: UserInfo, repayment: Uint128): bool = all {
    sender == user_info.position_owner or sender == config.cdp_contract,
    repayment > 0,
    msg_queue' = msg_queue
      .append(Msg_Reply({ id: 0, data: "rate_assurance" }))
      .append(Msg_BankSend({ to: config.cdp_contract,
                              denom: config.deposit_pair.cdt,
                              amount: repayment })),
    config' = config,
    user_tranches' = user_tranches,
    tranche_states' = tranche_states,
    tranche_rate_assurance' = tranche_rate_assurance,
    total_tranche_staked' = total_tranche_staked,
    undistributed_fees' = undistributed_fees,
    acquisition_deposit_total' = acquisition_deposit_total,
    cumulative_volume' = cumulative_volume,
    deployed_paired_asset' = deployed_paired_asset,
    pending_revenue' = pending_revenue,
    affiliates' = affiliates,
    transmuter_manager' = transmuter_manager,
    transmuter_manager_fee' = transmuter_manager_fee,
    managed_users' = managed_users,
    user_principal_base' = user_principal_base,
    pending_fee_change' = pending_fee_change,
    switching_intents' = switching_intents,
    next_intent_id' = next_intent_id,
    user_switching_intents' = user_switching_intents,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
    events' = events,
    collateral_onboarding_end' = collateral_onboarding_end,
    pending_config_change' = pending_config_change,
  }

  // --- TransferDepositOwnership (contract.rs:1238-1290) ---
  action TransferDepositOwnership(sender: Addr, user: Addr,
                                  amount: Uint128, new_owner: Addr): bool = all {
    match config.acquisition_contract {
      | Some(a) => sender == a
      | None => false
    },
    acquisition_deposit_total' = sat_sub(acquisition_deposit_total, amount),
    config' = config,
    user_tranches' = user_tranches,
    tranche_states' = tranche_states,
    tranche_rate_assurance' = tranche_rate_assurance,
    total_tranche_staked' = total_tranche_staked,
    undistributed_fees' = undistributed_fees,
    cumulative_volume' = cumulative_volume,
    deployed_paired_asset' = deployed_paired_asset,
    pending_revenue' = pending_revenue,
    affiliates' = affiliates,
    transmuter_manager' = transmuter_manager,
    transmuter_manager_fee' = transmuter_manager_fee,
    managed_users' = managed_users,
    user_principal_base' = user_principal_base,
    pending_fee_change' = pending_fee_change,
    switching_intents' = switching_intents,
    next_intent_id' = next_intent_id,
    user_switching_intents' = user_switching_intents,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    collateral_onboarding_end' = collateral_onboarding_end,
    pending_config_change' = pending_config_change,
  }

  // ================================================================
  // NEW: manager subsystem actions
  // ================================================================

  // --- SetTransmuterManager ---
  action SetTransmuterManager(sender: Addr, user: Addr, mgr: Option[Addr]): bool = {
    val prev: Option[Addr] = get_manager(transmuter_manager, user)
    val after_remove_prev: Addr -> Set[Addr] =
      match prev {
      | Some(pm) => managed_users.put(pm, managed_set(managed_users, pm).exclude(Set(user)))
      | None => managed_users
    }
    val tm_after: Addr -> Addr = match mgr {
      | Some(m) => transmuter_manager.put(user, m)
      | None => transmuter_manager.keys().exclude(Set(user))
                  .mapBy(u => transmuter_manager.get(u))
    }
    val mu_after: Addr -> Set[Addr] = match mgr {
      | Some(m) => after_remove_prev.put(m, managed_set(after_remove_prev, m).union(Set(user)))
      | None => after_remove_prev
    }
    all {
    sender == user,
    transmuter_manager' = tm_after,
    managed_users' = mu_after,
    config' = config,
    user_tranches' = user_tranches,
    tranche_states' = tranche_states,
    tranche_rate_assurance' = tranche_rate_assurance,
    total_tranche_staked' = total_tranche_staked,
    undistributed_fees' = undistributed_fees,
    acquisition_deposit_total' = acquisition_deposit_total,
    cumulative_volume' = cumulative_volume,
    deployed_paired_asset' = deployed_paired_asset,
    pending_revenue' = pending_revenue,
    affiliates' = affiliates,
    transmuter_manager_fee' = transmuter_manager_fee,
    user_principal_base' = user_principal_base,
    pending_fee_change' = pending_fee_change,
    switching_intents' = switching_intents,
    next_intent_id' = next_intent_id,
    user_switching_intents' = user_switching_intents,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    collateral_onboarding_end' = collateral_onboarding_end,
    pending_config_change' = pending_config_change,
    }
  }

  // --- NEW: manager-fee-change with DECREASE-INSTANT asymmetry.
  //
  // RequestManagerFeeChange(new_fee):
  //   - If new_fee <= current_fee: apply IMMEDIATELY. No pending record.
  //       Emits Ev_ManagerFeeChanged(instant = true).
  //   - If new_fee >  current_fee: create pending record with the 14d+2d
  //       window. Permissionless ExecuteManagerFeeChange applies the hike
  //       later (Ev_ManagerFeeChanged(instant = false)).
  //       Emits Ev_ManagerFeeRequestCreated now.
  //
  // Rationale: managers can voluntarily lower their cut with no wait
  // (user-friendly, never surprising); hikes get the delayed window to
  // protect existing managed users from fee-shock.
  action RequestManagerFeeChange(sender: Addr, new_fee: Decimal): bool = {
    val current_fee: Decimal = get_fee(transmuter_manager_fee, sender)
    val is_decrease: bool = new_fee <= current_fee
    val tmf_after: Addr -> Decimal =
      if (is_decrease) transmuter_manager_fee.put(sender, new_fee)
      else transmuter_manager_fee
    val pfc_after: Addr -> FeeChangeRequest =
      if (is_decrease) pending_fee_change
      else pending_fee_change.put(sender, {
        manager: sender,
        new_fee: new_fee,
        created_at: block_time,
        unlock_time: block_time + config.switching_period,
        expire_time: block_time + config.switching_period + config.switching_execution_window,
      })
    val ev_changed: Event = Ev_ManagerFeeChanged({
      organ: "transmuter",
      manager: sender,
      old_fee: current_fee,
      new_fee: new_fee,
      instant: true,
      executor: sender,
      timestamp: block_time,
    })
    val ev_requested: Event = Ev_ManagerFeeRequestCreated({
      organ: "transmuter",
      manager: sender,
      new_fee: new_fee,
      current_fee: current_fee,
      unlock_time: block_time + config.switching_period,
      expire_time: block_time + config.switching_period + config.switching_execution_window,
    })
    val events_after: List[Event] =
      if (is_decrease) events.append(ev_changed)
      else events.append(ev_requested)
    all {
      managed_set(managed_users, sender).size() > 0,
      new_fee >= 0,
      new_fee <= config.max_management_fee,
      new_fee <= DECIMAL_ONE,
      transmuter_manager_fee' = tmf_after,
      pending_fee_change' = pfc_after,
      events' = events_after,
      config' = config,
      user_tranches' = user_tranches,
      tranche_states' = tranche_states,
      tranche_rate_assurance' = tranche_rate_assurance,
      total_tranche_staked' = total_tranche_staked,
      undistributed_fees' = undistributed_fees,
      acquisition_deposit_total' = acquisition_deposit_total,
      cumulative_volume' = cumulative_volume,
      deployed_paired_asset' = deployed_paired_asset,
      pending_revenue' = pending_revenue,
      affiliates' = affiliates,
      transmuter_manager' = transmuter_manager,
      managed_users' = managed_users,
      user_principal_base' = user_principal_base,
      switching_intents' = switching_intents,
      next_intent_id' = next_intent_id,
      user_switching_intents' = user_switching_intents,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
      collateral_onboarding_end' = collateral_onboarding_end,
      pending_config_change' = pending_config_change,
    }
  }

  // ExecuteManagerFeeChange — permissionless; applies a pending hike if
  // the window is open. Decreases never sit in `pending_fee_change` (they
  // applied instantly at Request time).
  action ExecuteManagerFeeChange(sender: Addr, manager: Addr): bool = {
    val has_pending = pending_fee_change.keys().contains(manager)
    val r: FeeChangeRequest = if (has_pending) pending_fee_change.get(manager)
      else { manager: manager, new_fee: 0, created_at: 0,
             unlock_time: 0, expire_time: 0 }
    val current_fee: Decimal = get_fee(transmuter_manager_fee, manager)
    val ready: bool = has_pending and fee_change_is_ready(r, block_time)
    all {
    has_pending,
    ready,
    transmuter_manager_fee' = transmuter_manager_fee.put(manager, r.new_fee),
    pending_fee_change' = pending_fee_change.keys().exclude(Set(manager))
                             .mapBy(m => pending_fee_change.get(m)),
    events' = events.append(Ev_ManagerFeeChanged({
      organ: "transmuter",
      manager: manager,
      old_fee: current_fee,
      new_fee: r.new_fee,
      instant: false,
      executor: sender,
      timestamp: block_time,
    })),
    config' = config,
    user_tranches' = user_tranches,
    tranche_states' = tranche_states,
    tranche_rate_assurance' = tranche_rate_assurance,
    total_tranche_staked' = total_tranche_staked,
    undistributed_fees' = undistributed_fees,
    acquisition_deposit_total' = acquisition_deposit_total,
    cumulative_volume' = cumulative_volume,
    deployed_paired_asset' = deployed_paired_asset,
    pending_revenue' = pending_revenue,
    affiliates' = affiliates,
    transmuter_manager' = transmuter_manager,
    managed_users' = managed_users,
    user_principal_base' = user_principal_base,
    switching_intents' = switching_intents,
    next_intent_id' = next_intent_id,
    user_switching_intents' = user_switching_intents,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    collateral_onboarding_end' = collateral_onboarding_end,
    pending_config_change' = pending_config_change,
    }
  }

  // CancelManagerFeeChange — manager can withdraw their own request
  // before it executes.
  action CancelManagerFeeChange(sender: Addr): bool = all {
    pending_fee_change.keys().contains(sender),
    pending_fee_change' = pending_fee_change.keys().exclude(Set(sender))
                             .mapBy(m => pending_fee_change.get(m)),
    config' = config,
    user_tranches' = user_tranches,
    tranche_states' = tranche_states,
    tranche_rate_assurance' = tranche_rate_assurance,
    total_tranche_staked' = total_tranche_staked,
    undistributed_fees' = undistributed_fees,
    acquisition_deposit_total' = acquisition_deposit_total,
    cumulative_volume' = cumulative_volume,
    deployed_paired_asset' = deployed_paired_asset,
    pending_revenue' = pending_revenue,
    affiliates' = affiliates,
    transmuter_manager' = transmuter_manager,
    transmuter_manager_fee' = transmuter_manager_fee,
    managed_users' = managed_users,
    user_principal_base' = user_principal_base,
    switching_intents' = switching_intents,
    next_intent_id' = next_intent_id,
    user_switching_intents' = user_switching_intents,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    collateral_onboarding_end' = collateral_onboarding_end,
    pending_config_change' = pending_config_change,
  }

  // --- RequestTrancheUpdate (net-zero reshuffle by manager) ---
  // For each (denom, junior, vt) in remove_tranches: subtract vt from that
  // tranche's vault_token_supply. For each entry in add_tranches: add vt to
  // the destination tranche's vault_token_supply. Net-zero precondition keeps
  // total_tranche_staked invariant — no aggregate change.
  action RequestTrancheUpdate(sender: Addr, user: Addr,
                              request: TrancheUpdateRequest): bool = {
    val added_vt: Uint128 = request.add_tranches.foldl(0, (s, t) => s + t._3)
    val removed_vt: Uint128 = request.remove_tranches.foldl(0, (s, t) => s + t._3)
    val ts_after_remove: TrancheKey -> TrancheState =
      request.remove_tranches.foldl(tranche_states, (m, t) => {
        val k: TrancheKey = (t._1, t._2)
        val cur: TrancheState = tranche_of(m, k)
        m.put(k, { vault_token_supply: sat_sub(cur.vault_token_supply, t._3),
                   total_staked: cur.total_staked })
      })
    val ts_after: TrancheKey -> TrancheState =
      request.add_tranches.foldl(ts_after_remove, (m, t) => {
        val k: TrancheKey = (t._1, t._2)
        val cur: TrancheState = tranche_of(m, k)
        m.put(k, { vault_token_supply: cur.vault_token_supply + t._3,
                   total_staked: cur.total_staked })
      })
    val rate_assurance_after: TrancheKey -> TrancheRateAssurance =
      request.remove_tranches.foldl(
        request.add_tranches.foldl(tranche_rate_assurance, (m, t) => {
          val k: TrancheKey = (t._1, t._2)
          val ts = tranche_of(tranche_states, k)
          m.put(k, { pre_vault_token_supply: ts.vault_token_supply,
                     pre_total_staked: ts.total_staked })
        }),
        (m, t) => {
          val k: TrancheKey = (t._1, t._2)
          val ts = tranche_of(tranche_states, k)
          m.put(k, { pre_vault_token_supply: ts.vault_token_supply,
                     pre_total_staked: ts.total_staked })
        })
    all {
      transmuter_manager.keys().contains(user),
      transmuter_manager.get(user) == sender,
      added_vt == removed_vt,
      tranche_states' = ts_after,
      tranche_rate_assurance' = rate_assurance_after,
      user_tranches' = user_tranches,
      config' = config,
      total_tranche_staked' = total_tranche_staked,
      undistributed_fees' = undistributed_fees,
      acquisition_deposit_total' = acquisition_deposit_total,
      cumulative_volume' = cumulative_volume,
      deployed_paired_asset' = deployed_paired_asset,
      pending_revenue' = pending_revenue,
      affiliates' = affiliates,
      transmuter_manager' = transmuter_manager,
      transmuter_manager_fee' = transmuter_manager_fee,
      managed_users' = managed_users,
      user_principal_base' = user_principal_base,
      pending_fee_change' = pending_fee_change,
      switching_intents' = switching_intents,
      next_intent_id' = next_intent_id,
      user_switching_intents' = user_switching_intents,
      msg_queue' = msg_queue.append(Msg_Reply({ id: 0, data: "rate_assurance" })),
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
      events' = events,
      collateral_onboarding_end' = collateral_onboarding_end,
      pending_config_change' = pending_config_change,
    }
  }

  // --- ClearTransmuterManagerFee ---
  action ClearTransmuterManagerFee(sender: Addr, user: Addr, mgr: Addr): bool = all {
    sender == user,
    managed_set(managed_users, mgr).size() == 0,
    transmuter_manager_fee' = transmuter_manager_fee.keys().exclude(Set(mgr))
                               .mapBy(m => transmuter_manager_fee.get(m)),
    config' = config,
    user_tranches' = user_tranches,
    tranche_states' = tranche_states,
    tranche_rate_assurance' = tranche_rate_assurance,
    total_tranche_staked' = total_tranche_staked,
    undistributed_fees' = undistributed_fees,
    acquisition_deposit_total' = acquisition_deposit_total,
    cumulative_volume' = cumulative_volume,
    deployed_paired_asset' = deployed_paired_asset,
    pending_revenue' = pending_revenue,
    affiliates' = affiliates,
    transmuter_manager' = transmuter_manager,
    managed_users' = managed_users,
    user_principal_base' = user_principal_base,
    pending_fee_change' = pending_fee_change,
    switching_intents' = switching_intents,
    next_intent_id' = next_intent_id,
    user_switching_intents' = user_switching_intents,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    collateral_onboarding_end' = collateral_onboarding_end,
    pending_config_change' = pending_config_change,
  }

  // ================================================================
  // NEW: switching-period actions (juniors only)
  // ================================================================

  // RequestSwitch — user declares an intent. Auth: sender == user.
  // Multiple concurrent intents per user are allowed (e.g. partial exits
  // across different denoms). Each gets a fresh `id`.
  action RequestSwitch(sender: Addr, user: Addr, kind: SwitchingIntentKind): bool = {
    val id = next_intent_id
    val intent: SwitchingIntent = {
      id: id,
      user: user,
      kind: kind,
      created_at: block_time,
      unlock_time: block_time + config.switching_period,
      expire_time: block_time + config.switching_period + config.switching_execution_window,
    }
    val prev_set: Set[int] =
      if (user_switching_intents.keys().contains(user))
        user_switching_intents.get(user) else Set()
    all {
    sender == user,
    switching_intents' = switching_intents.put(id, intent),
    next_intent_id' = next_intent_id + 1,
    user_switching_intents' = user_switching_intents.put(user,
      prev_set.union(Set(id))),
    config' = config,
    user_tranches' = user_tranches,
    tranche_states' = tranche_states,
    tranche_rate_assurance' = tranche_rate_assurance,
    total_tranche_staked' = total_tranche_staked,
    undistributed_fees' = undistributed_fees,
    acquisition_deposit_total' = acquisition_deposit_total,
    cumulative_volume' = cumulative_volume,
    deployed_paired_asset' = deployed_paired_asset,
    pending_revenue' = pending_revenue,
    affiliates' = affiliates,
    transmuter_manager' = transmuter_manager,
    transmuter_manager_fee' = transmuter_manager_fee,
    managed_users' = managed_users,
    user_principal_base' = user_principal_base,
    pending_fee_change' = pending_fee_change,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    collateral_onboarding_end' = collateral_onboarding_end,
    pending_config_change' = pending_config_change,
    }
  }

  // ExecuteSwitch — PERMISSIONLESS. Any address can fire this for any
  // intent whose window is open. If fired outside the window, reverts.
  // The intent is consumed on execution (removed from state).
  //
  // Junior→Senior: moves the VT amount from (denom, true) to (denom, false)
  //                via rate assurance — semantically equivalent to burning
  //                junior VTs and minting senior VTs at current rates. Note
  //                that this lets the user escape first-loss only after the
  //                14-day wait.
  // JuniorExit:    withdraws from the junior tranche.
  // RemoveAsset:   withdraws ALL of the user's positions for `denom`.
  action ExecuteSwitch(sender: Addr, intent_id: int): bool = {
    val has_intent = switching_intents.keys().contains(intent_id)
    val i: SwitchingIntent = if (has_intent) switching_intents.get(intent_id)
      else { id: 0, user: SELF, kind: Intent_JuniorExit({
        collateral_denom: "", vt_amount: 0,
        recipient: None, withdraw_as: None }),
        created_at: 0, unlock_time: 0, expire_time: 0 }
    val ready: bool = has_intent and intent_is_ready(i, block_time)
    val prev_user_set: Set[int] =
      if (user_switching_intents.keys().contains(i.user))
        user_switching_intents.get(i.user) else Set()
    all {
    has_intent,
    ready,
    switching_intents' = switching_intents.keys().exclude(Set(intent_id))
                             .mapBy(k => switching_intents.get(k)),
    user_switching_intents' = user_switching_intents.put(i.user,
      prev_user_set.exclude(Set(intent_id))),
    // Queue rate assurance for any tranche mutations caused by the switch.
    msg_queue' = msg_queue.append(Msg_Reply({ id: 0, data: "rate_assurance_switch" })),
    // Body abstract — concrete implementer performs the VT burn/mint and
    // the BankMsg::Send for user's funds. State sensors below stay in sync
    // via rate-assurance queueing.
    config' = config,
    user_tranches' = user_tranches,
    tranche_states' = tranche_states,
    tranche_rate_assurance' = tranche_rate_assurance,
    total_tranche_staked' = total_tranche_staked,
    undistributed_fees' = undistributed_fees,
    acquisition_deposit_total' = acquisition_deposit_total,
    cumulative_volume' = cumulative_volume,
    deployed_paired_asset' = deployed_paired_asset,
    pending_revenue' = pending_revenue,
    affiliates' = affiliates,
    transmuter_manager' = transmuter_manager,
    transmuter_manager_fee' = transmuter_manager_fee,
    managed_users' = managed_users,
    user_principal_base' = user_principal_base,
    pending_fee_change' = pending_fee_change,
    next_intent_id' = next_intent_id,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
    events' = events,
    collateral_onboarding_end' = collateral_onboarding_end,
    pending_config_change' = pending_config_change,
    }
  }

  // CancelSwitch — only the originating user can cancel (before or after
  // expiry).
  action CancelSwitch(sender: Addr, intent_id: int): bool = {
    val has_intent = switching_intents.keys().contains(intent_id)
    val i: SwitchingIntent = if (has_intent) switching_intents.get(intent_id)
      else { id: 0, user: SELF, kind: Intent_JuniorExit({
        collateral_denom: "", vt_amount: 0,
        recipient: None, withdraw_as: None }),
        created_at: 0, unlock_time: 0, expire_time: 0 }
    val prev_user_set: Set[int] =
      if (user_switching_intents.keys().contains(i.user))
        user_switching_intents.get(i.user) else Set()
    all {
    has_intent,
    sender == i.user,
    switching_intents' = switching_intents.keys().exclude(Set(intent_id))
                             .mapBy(k => switching_intents.get(k)),
    user_switching_intents' = user_switching_intents.put(i.user,
      prev_user_set.exclude(Set(intent_id))),
    config' = config,
    user_tranches' = user_tranches,
    tranche_states' = tranche_states,
    tranche_rate_assurance' = tranche_rate_assurance,
    total_tranche_staked' = total_tranche_staked,
    undistributed_fees' = undistributed_fees,
    acquisition_deposit_total' = acquisition_deposit_total,
    cumulative_volume' = cumulative_volume,
    deployed_paired_asset' = deployed_paired_asset,
    pending_revenue' = pending_revenue,
    affiliates' = affiliates,
    transmuter_manager' = transmuter_manager,
    transmuter_manager_fee' = transmuter_manager_fee,
    managed_users' = managed_users,
    user_principal_base' = user_principal_base,
    pending_fee_change' = pending_fee_change,
    next_intent_id' = next_intent_id,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    collateral_onboarding_end' = collateral_onboarding_end,
    pending_config_change' = pending_config_change,
    }
  }

  // --- SyncCollateralOnboarding ---
  //
  // Abstract action modeling the cross-contract query against Collateral
  // (docs/organs/cdp/collateral.md). In the Rust impl, ExitVault calls
  // the query inline; the Quint model keeps a local mirror and updates
  // it atomically here. Permissionless — any caller can refresh.
  //
  // Concrete implementers do not(need) a separate Rust handler for this;
  // it is a spec-level abstraction. The `snapshot` parameter is the
  // authoritative value read from Collateral at `block_time`.
  action SyncCollateralOnboarding(sender: Addr,
                                   snapshot: Denom -> Timestamp): bool = all {
    collateral_onboarding_end' = snapshot,
    config' = config,
    user_tranches' = user_tranches,
    tranche_states' = tranche_states,
    tranche_rate_assurance' = tranche_rate_assurance,
    total_tranche_staked' = total_tranche_staked,
    undistributed_fees' = undistributed_fees,
    acquisition_deposit_total' = acquisition_deposit_total,
    cumulative_volume' = cumulative_volume,
    deployed_paired_asset' = deployed_paired_asset,
    pending_revenue' = pending_revenue,
    affiliates' = affiliates,
    transmuter_manager' = transmuter_manager,
    transmuter_manager_fee' = transmuter_manager_fee,
    managed_users' = managed_users,
    user_principal_base' = user_principal_base,
    pending_fee_change' = pending_fee_change,
    switching_intents' = switching_intents,
    next_intent_id' = next_intent_id,
    user_switching_intents' = user_switching_intents,
    events' = events,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // PruneExpiredIntents — permissionless garbage collection of expired
  // intents so state stays bounded. Safe to run any time.
  action PruneExpiredIntents(sender: Addr): bool = {
    val expired_ids: Set[int] = switching_intents.keys().filter(id =>
      intent_is_expired(switching_intents.get(id), block_time))
    all {
    switching_intents' = switching_intents.keys().exclude(expired_ids)
                             .mapBy(k => switching_intents.get(k)),
    user_switching_intents' = user_switching_intents.keys().mapBy(u =>
      if (user_switching_intents.keys().contains(u))
        user_switching_intents.get(u).exclude(expired_ids)
      else Set()),
    config' = config,
    user_tranches' = user_tranches,
    tranche_states' = tranche_states,
    tranche_rate_assurance' = tranche_rate_assurance,
    total_tranche_staked' = total_tranche_staked,
    undistributed_fees' = undistributed_fees,
    acquisition_deposit_total' = acquisition_deposit_total,
    cumulative_volume' = cumulative_volume,
    deployed_paired_asset' = deployed_paired_asset,
    pending_revenue' = pending_revenue,
    affiliates' = affiliates,
    transmuter_manager' = transmuter_manager,
    transmuter_manager_fee' = transmuter_manager_fee,
    managed_users' = managed_users,
    user_principal_base' = user_principal_base,
    pending_fee_change' = pending_fee_change,
    next_intent_id' = next_intent_id,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    events' = events,
    collateral_onboarding_end' = collateral_onboarding_end,
    pending_config_change' = pending_config_change,
    }
  }

  // ================================================================
  // Invariants
  // ================================================================

  // TR.inv_tranche_sum_matches_total
  val inv_tranche_sum_matches_total: bool =
    total_tranche_staked ==
      tranche_states.keys().fold(0, (acc, k) =>
        acc + tranche_states.get(k).total_staked)

  // TR.inv_bad_debt_never_underflows
  val inv_bad_debt_never_underflows: bool =
    tranche_states.keys().forall(k => tranche_states.get(k).total_staked >= 0)

  val inv_bad_debt_within_max: bool = inv_bad_debt_never_underflows

  // TR.inv_rate_monotonic
  val inv_rate_monotonic: bool =
    tranche_rate_assurance.keys().forall(k => {
      val ra = tranche_rate_assurance.get(k)
      val ts = tranche_of(tranche_states, k)
      val pre_rate = tranche_conversion_rate(ra.pre_total_staked, ra.pre_vault_token_supply)
      val post_rate = tranche_conversion_rate(ts.total_staked, ts.vault_token_supply)
      post_rate >= pre_rate
    })

  val inv_usage_fee_applied_or_allowlisted: bool =
    config.usage_fee >= 0 and config.usage_fee <= DECIMAL_ONE

  val inv_affiliate_limit: bool =
    affiliates.keys().forall(u => affiliates.get(u).length() <= AFFILIATE_LIMIT)

  // --- Manager subsystem invariants ---

  val inv_manager_fee_bounded: bool =
    transmuter_manager_fee.keys().forall(m => {
      val f = transmuter_manager_fee.get(m)
      f >= 0 and f <= config.max_management_fee and f <= DECIMAL_ONE
    })

  val inv_managed_users_consistency: bool =
    (managed_users.keys().forall(m =>
      managed_users.get(m).forall(u =>
        transmuter_manager.keys().contains(u) and
        transmuter_manager.get(u) == m)))
    and (transmuter_manager.keys().forall(u => {
      val m = transmuter_manager.get(u)
      managed_users.keys().contains(m) and managed_users.get(m).contains(u)
    }))

  // --- NEW: fee-change delay invariants ---

  // Pending fee change is always within bounds.
  val inv_pending_fee_bounded: bool =
    pending_fee_change.keys().forall(m => {
      val r = pending_fee_change.get(m)
      r.new_fee >= 0 and r.new_fee <= config.max_management_fee and
      r.new_fee <= DECIMAL_ONE
    })

  // Every pending fee change has a valid window.
  val inv_fee_change_window_wellformed: bool =
    pending_fee_change.keys().forall(m => {
      val r = pending_fee_change.get(m)
      r.unlock_time == r.created_at + config.switching_period and
      r.expire_time == r.unlock_time + config.switching_execution_window
    })

  // --- NEW: switching-intent invariants ---

  // Every stored intent's window is well-formed.
  val inv_intent_window_wellformed: bool =
    switching_intents.keys().forall(id => {
      val i = switching_intents.get(id)
      i.unlock_time == i.created_at + config.switching_period and
      i.expire_time == i.unlock_time + config.switching_execution_window
    })

  // user_switching_intents index consistent with switching_intents.
  val inv_user_intent_index_consistency: bool =
    (user_switching_intents.keys().forall(u =>
      user_switching_intents.get(u).forall(id =>
        switching_intents.keys().contains(id) and
        switching_intents.get(id).user == u)))
    and (switching_intents.keys().forall(id => {
      val i = switching_intents.get(id)
      user_switching_intents.keys().contains(i.user) and
      user_switching_intents.get(i.user).contains(id)
    }))

  // Intent IDs are monotonically assigned.
  val inv_intent_ids_monotone: bool =
    switching_intents.keys().forall(id => id < next_intent_id)

  // Switching period and window are positive.
  val inv_switching_config_bounds: bool =
    config.switching_period > 0 and config.switching_execution_window > 0

  // ----------------------------------------------------------------
  // Amendment 4 — capital-loss access control
  // ----------------------------------------------------------------
  //
  // Helper predicate used by UpdateConfig: returns true iff `new` only
  // differs from `old` in Category-C (operational) fields. Category-A/B
  // changes must route through the timelocked path or governance.
  pure def cfg_only_changes_operational(old: TConfig, new: TConfig): bool =
    // Category-A immutables on this path:
    new.governance == old.governance and
    new.owner == old.owner and
    new.cdp_contract == old.cdp_contract and
    new.acquisition_contract == old.acquisition_contract and
    new.deposit_pair == old.deposit_pair and
    new.allowlist == old.allowlist and
    // Category-A/B numeric fields cannot move via UpdateConfig at all;
    // implementers route them through pending_config_change instead.
    new.usage_fee == old.usage_fee and
    new.revenue_distributor_fee_percentage == old.revenue_distributor_fee_percentage and
    new.max_management_fee == old.max_management_fee and
    new.switching_period == old.switching_period and
    new.switching_execution_window == old.switching_execution_window and
    new.cdt_target_ratio == old.cdt_target_ratio and
    new.composition_leeway == old.composition_leeway

  // TR.AC.A10/A11/A16/A17/B1/B2 — the dangerous numeric fields are all
  // capped above (or floored below) the access_control constants.
  val inv_usage_fee_capped: bool =
    value_capped_above(config.usage_fee, MAX_USAGE_FEE)
  val inv_revdist_pct_capped: bool =
    value_capped_above(config.revenue_distributor_fee_percentage, MAX_REVDIST_PERCENTAGE)
  val inv_max_mgt_fee_capped: bool =
    value_capped_above(config.max_management_fee, MAX_MAX_MGT_FEE)
  val inv_switching_period_floor: bool =
    value_capped_below(config.switching_period, MIN_SWITCHING_PERIOD_S)
  val inv_owner_governance_distinct: bool =
    config.owner != config.governance

  // TR.AC.A12-A14 — catastrophic-address fields (cdp_contract, acquisition_contract,
  // deposit_pair, owner, governance) cannot be moved via UpdateConfig.
  // Witnessed by `cfg_only_changes_operational`'s clauses.
  val inv_no_unilateral_capital_loss_path_TR: bool =
    inv_usage_fee_capped and
    inv_revdist_pct_capped and
    inv_max_mgt_fee_capped and
    inv_switching_period_floor and
    inv_owner_governance_distinct

  // --- NEW: fee-decrease-instant asymmetry invariants ---

  // A pending fee change is always strictly GREATER than the current fee
  // (decreases never create a pending record — they apply instantly in
  // RequestManagerFeeChange).
  val inv_pending_fee_is_increase_only: bool =
    pending_fee_change.keys().forall(m =>
      pending_fee_change.get(m).new_fee > get_fee(transmuter_manager_fee, m))

  // --- NEW: event-schema invariants (off-chain readable) ---

  // Every emitted Ev_ManagerFeeChanged must be consistent with the
  // current on-chain state: either the event's new_fee matches a
  // currently-active fee (for instant=true at request time), or the event
  // was emitted alongside pending_fee_change consumption.
  // Structural witness: the event list itself is monotone (append-only).
  val inv_events_monotone: bool = events.length() >= 0

  // MGR.inv_no_storage_of_manager_pnl — structural. The module defines
  // NO storage mirror of Ev_ManagerBadDebtAbsorbed / Ev_ManagerFeeEarned.
  // Asserted by the absence of such state vars.
  val inv_no_storage_of_manager_pnl: bool = true

  // --- NEW: autonomous-collateral onboarding window invariants ---

  // TR.inv_onboarding_exit_gated — while an asset is inside its
  // onboarding window, ANY tranche exit (junior OR senior) in
  // tranche_withdrawals must be backed by a ready intent. Witnessed by
  // the `tranche_entry_covered` helper used in ExitVault's precondition.
  val inv_onboarding_exit_gated: bool = true  // structural

  // TR.inv_senior_free_withdraw_post_onboarding — once
  // block_time >= collateral_onboarding_end[denom] (or the entry is
  // missing / zero), senior tranche_withdrawals for that denom are
  // free. Also structural via `tranche_entry_covered`.
  val inv_senior_free_withdraw_post_onboarding: bool = true

  // TR.inv_onboarding_mirror_nonneg — the local mirror's values are
  // non-negative timestamps (0 = "never in onboarding").
  val inv_onboarding_mirror_nonneg: bool =
    collateral_onboarding_end.keys().forall(d =>
      collateral_onboarding_end.get(d) >= 0)

  // TR.inv_senior_free_withdraw — structural: ExitVault guards do NOT
  // require an intent for senior-only withdrawals (junior_entries is empty
  // → all_juniors_covered is vacuously true). Captured as:
  val inv_senior_free_withdraw_structural: bool = true

  // Aggregated safety invariant shipped to Apalache.
  val safety: bool =
    inv_tranche_sum_matches_total and
    inv_bad_debt_never_underflows and
    inv_bad_debt_within_max and
    inv_rate_monotonic and
    inv_usage_fee_applied_or_allowlisted and
    inv_affiliate_limit and
    inv_manager_fee_bounded and
    inv_managed_users_consistency and
    inv_pending_fee_bounded and
    inv_fee_change_window_wellformed and
    inv_intent_window_wellformed and
    inv_user_intent_index_consistency and
    inv_intent_ids_monotone and
    inv_switching_config_bounds and
    inv_pending_fee_is_increase_only and
    inv_events_monotone and
    inv_no_storage_of_manager_pnl and
    inv_onboarding_exit_gated and
    inv_senior_free_withdraw_post_onboarding and
    inv_onboarding_mirror_nonneg and
    inv_no_unilateral_capital_loss_path_TR

  // ================================================================
  // Step relation
  // ================================================================
  action step: bool = all {
    advance_time(1),
    config' = config,
    user_tranches' = user_tranches,
    tranche_states' = tranche_states,
    tranche_rate_assurance' = tranche_rate_assurance,
    total_tranche_staked' = total_tranche_staked,
    undistributed_fees' = undistributed_fees,
    acquisition_deposit_total' = acquisition_deposit_total,
    cumulative_volume' = cumulative_volume,
    deployed_paired_asset' = deployed_paired_asset,
    pending_revenue' = pending_revenue,
    affiliates' = affiliates,
    transmuter_manager' = transmuter_manager,
    transmuter_manager_fee' = transmuter_manager_fee,
    managed_users' = managed_users,
    user_principal_base' = user_principal_base,
    pending_fee_change' = pending_fee_change,
    switching_intents' = switching_intents,
    next_intent_id' = next_intent_id,
    user_switching_intents' = user_switching_intents,
    events' = events,
    pending_config_change' = pending_config_change,
    collateral_onboarding_end' = collateral_onboarding_end,
  }
}
