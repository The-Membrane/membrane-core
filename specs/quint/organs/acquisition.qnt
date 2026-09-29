// specs/quint/organs/acquisition.qnt
//
// Acquisition organ — adaptive MBRN emission window with efficiency
// clamping, bump-rate feedback into CDP, and a Disco-routing path that
// preserves clawback semantics until the cliff.
//
// Source of truth (Rust):
//   contracts/acquisition/src/state.rs
//   contracts/acquisition/src/contract.rs        (handlers; line cites below)
//   contracts/acquisition/src/error.rs
//   packages/membrane/src/acquisition.rs         (Config, ExecuteMsg, types)
module acquisition {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  // ================================================================
  // Types
  // ================================================================
  type AcquisitionWindow = {
    window_id: int,
    start_time: Timestamp,
    deposit_end: Timestamp,
    withdrawal_end: Timestamp,
    deposit_period_days: int,
    total_deposit_amount: Uint128,
    acquisition_budget: Uint128,
  }

  type AcquisitionModelConfig = {
    base_acquisition_rate: Decimal,
    initial_acquisition_amount: Uint128,
    max_mbrn_emission: Uint128,
    target_utilization: Decimal,
    bump_increment: Decimal,
    bump_interval_seconds: int,
    reduction_speed_multiplier: Decimal,
    efficiency_threshold: Decimal,
    max_rate_change_per_mutation: Decimal,
    max_bump_rate: Decimal,
    mutation_cooldown_seconds: int,
    accrual_ramp_seconds: int,
    accrual_ramp_start_fraction: Decimal,
  }

  type AcquisitionModelState = {
    current_acquisition_rate: Decimal,
    accrued_pool: Uint128,
    last_accrual_time: Timestamp,
    pool_maxed: bool,
    bump_rate: Decimal,
    last_bump_time: Timestamp,
    window_timers_started: bool,
    first_deposit_time: Option[Timestamp],
    deposit_event_count: int,
    total_new_deposits: Uint128,
    last_mutation_efficiency: Option[Decimal],
    last_mutation_time: Option[Timestamp],
    efficiency_clamped: bool,
  }

  type AcquisitionDeposit = {
    amount: Uint128,
    deposit_time: Timestamp,
    vested_at: Timestamp,
    sent_to_disco: Option[bool],
    mbrn_claim_amount: Option[Uint128],
    disco_deposit_id: Option[int],
    claimed_mbrn_amount: Option[Uint128],
    disco_asset: Option[Denom],
    disco_slot: Option[int],
  }

  type MbrnIntentType =
    | Intent_Stake
    | Intent_DepositViaMarsMirror({ asset: Denom, slot: int })
    | Intent_SendToAddress({ address: Addr })

  type MbrnIntentOption = {
    intent_type: MbrnIntentType,
    ratio: Decimal,
    lock_duration_days: Option[int],
  }

  type Config = {
    owner: Addr,
    governance: Addr,
    transmuter_contract: Addr,
    neutron_proxy: Addr,
    cdp_contract: Addr,
    deposit_period_days: int,
    withdrawal_period_days: int,
    deposit_token: Denom,
    minimum_deposit: Uint128,
    mbrn_denom: Denom,
    staking_contract: Option[Addr],
    ltv_disco_contract: Option[Addr],
    discounts_contract: Addr,
    cliff_period_seconds: int,
  }

  // ================================================================
  // State
  // ================================================================
  var config:                     Config
  var current_window_id:          int
  var current_acquisition_window: AcquisitionWindow
  var user_acquisition_deposits:  (Addr, int) -> AcquisitionDeposit
  var user_intents:               Addr -> List[MbrnIntentOption]
  var acquisition_config:         AcquisitionModelConfig
  var acquisition_state:          AcquisitionModelState
  var acquisition_history_len:    int

  pure val ACQUISITION_HISTORY_CAP: int = 365

  action init(cfg: Config,
              acq_cfg: AcquisitionModelConfig,
              acq_st: AcquisitionModelState): bool = all {
    config' = cfg,
    current_window_id' = 0,
    current_acquisition_window' = {
      window_id: 0, start_time: 0,
      deposit_end: 0, withdrawal_end: 0,
      deposit_period_days: cfg.deposit_period_days,
      total_deposit_amount: 0, acquisition_budget: 0,
    },
    user_acquisition_deposits' = Map(),
    user_intents' = Map(),
    acquisition_config' = acq_cfg,
    acquisition_state' = acq_st,
    acquisition_history_len' = 0,
    block_time' = 0,
    block_height' = 0,
    balances' = Map(),
    msg_queue' = List(),
    pending_reply' = None,
  }

  // ================================================================
  // Helpers
  // ================================================================
  pure def intent_ratios_sum_to_one(list: List[MbrnIntentOption]): bool = {
    val total = list.indices().fold(0, (acc, i) => acc + list[i].ratio)
    total >= 999000000000000000 and total <= 1001000000000000000
  }

  pure def ramp_factor(time_since_first: int, ramp_s: int,
                       start_fraction: Decimal): Decimal =
    if (time_since_first >= ramp_s) DECIMAL_ONE
    else {
      val progress: Decimal = decimal_from_ratio(time_since_first, ramp_s)
      val span: Decimal = DECIMAL_ONE - start_fraction
      start_fraction + decimal_mul_uint(progress, span) / DECIMAL_FRACTIONAL
    }

  pure def efficiency(total_deposits: Uint128, accrued: Uint128): Decimal =
    if (accrued == 0) 0 else decimal_from_ratio(total_deposits, accrued)

  pure def make_window(id: int, start: Timestamp,
                        dep_end: Timestamp, wd_end: Timestamp,
                        period_days: int, total: Uint128,
                        budget: Uint128): AcquisitionWindow = {
    window_id: id, start_time: start,
    deposit_end: dep_end, withdrawal_end: wd_end,
    deposit_period_days: period_days,
    total_deposit_amount: total,
    acquisition_budget: budget,
  }

  pure def reset_state_for_new_window(s: AcquisitionModelState): AcquisitionModelState = {
    current_acquisition_rate: s.current_acquisition_rate,
    accrued_pool: 0,
    last_accrual_time: s.last_accrual_time,
    pool_maxed: false,
    bump_rate: s.bump_rate,
    last_bump_time: s.last_bump_time,
    window_timers_started: false,
    first_deposit_time: None,
    deposit_event_count: 0,
    total_new_deposits: 0,
    last_mutation_efficiency: s.last_mutation_efficiency,
    last_mutation_time: s.last_mutation_time,
    efficiency_clamped: false,
  }

  pure def update_state_on_deposit(s: AcquisitionModelState, first: bool,
                                     now: Timestamp,
                                     amount: Uint128): AcquisitionModelState = {
    current_acquisition_rate: s.current_acquisition_rate,
    accrued_pool: s.accrued_pool,
    last_accrual_time: now,
    pool_maxed: s.pool_maxed,
    bump_rate: s.bump_rate,
    last_bump_time: s.last_bump_time,
    window_timers_started: true,
    first_deposit_time: if (first) Some(now) else s.first_deposit_time,
    deposit_event_count: s.deposit_event_count + 1,
    total_new_deposits: s.total_new_deposits + amount,
    last_mutation_efficiency: s.last_mutation_efficiency,
    last_mutation_time: s.last_mutation_time,
    efficiency_clamped: s.efficiency_clamped,
  }

  pure def update_window_on_deposit(w: AcquisitionWindow,
                                      dep_end: Timestamp,
                                      wd_end: Timestamp,
                                      add: Uint128): AcquisitionWindow = {
    window_id: w.window_id, start_time: w.start_time,
    deposit_end: dep_end, withdrawal_end: wd_end,
    deposit_period_days: w.deposit_period_days,
    total_deposit_amount: w.total_deposit_amount + add,
    acquisition_budget: w.acquisition_budget,
  }

  pure def update_window_on_withdraw(w: AcquisitionWindow,
                                       sub: Uint128): AcquisitionWindow = {
    window_id: w.window_id, start_time: w.start_time,
    deposit_end: w.deposit_end, withdrawal_end: w.withdrawal_end,
    deposit_period_days: w.deposit_period_days,
    total_deposit_amount: sat_sub(w.total_deposit_amount, sub),
    acquisition_budget: w.acquisition_budget,
  }

  pure def update_deposit_amount(d: AcquisitionDeposit, new_amt: Uint128): AcquisitionDeposit = {
    amount: new_amt, deposit_time: d.deposit_time,
    vested_at: d.vested_at,
    sent_to_disco: d.sent_to_disco,
    mbrn_claim_amount: d.mbrn_claim_amount,
    disco_deposit_id: d.disco_deposit_id,
    claimed_mbrn_amount: d.claimed_mbrn_amount,
    disco_asset: d.disco_asset,
    disco_slot: d.disco_slot,
  }

  pure def mark_sent_to_disco(d: AcquisitionDeposit, asset: Denom, slot: int,
                                boost: Uint128): AcquisitionDeposit = {
    amount: d.amount, deposit_time: d.deposit_time,
    vested_at: d.vested_at,
    sent_to_disco: Some(true),
    mbrn_claim_amount: d.mbrn_claim_amount,
    disco_deposit_id: d.disco_deposit_id,
    claimed_mbrn_amount: Some(boost),
    disco_asset: Some(asset),
    disco_slot: Some(slot),
  }

  pure def update_state_on_accrue(s: AcquisitionModelState,
                                    new_accrued: Uint128,
                                    new_rate: Decimal,
                                    new_bump: Decimal,
                                    pool_max: bool,
                                    bump_changed: bool,
                                    now: Timestamp): AcquisitionModelState = {
    current_acquisition_rate: new_rate,
    accrued_pool: new_accrued,
    last_accrual_time: now,
    pool_maxed: pool_max,
    bump_rate: new_bump,
    last_bump_time: if (bump_changed) now else s.last_bump_time,
    window_timers_started: s.window_timers_started,
    first_deposit_time: s.first_deposit_time,
    deposit_event_count: s.deposit_event_count,
    total_new_deposits: s.total_new_deposits,
    last_mutation_efficiency: s.last_mutation_efficiency,
    last_mutation_time: s.last_mutation_time,
    efficiency_clamped: s.efficiency_clamped,
  }

  // ================================================================
  // Execute actions
  // ================================================================

  // --- StartAcquisitionWindow (contract.rs:209-288) ---
  action StartAcquisitionWindow(sender: Addr): bool = {
    val new_id = current_window_id + 1
    val U64MAX: int = 18446744073709551615
    val new_window: AcquisitionWindow = make_window(new_id, block_time,
                                                       U64MAX, U64MAX,
                                                       config.deposit_period_days,
                                                       0, 0)
    val new_state: AcquisitionModelState = reset_state_for_new_window(acquisition_state)
    all {
      sender == config.owner,
      current_window_id' = new_id,
      current_acquisition_window' = new_window,
      acquisition_state' = new_state,
      config' = config,
      user_acquisition_deposits' = user_acquisition_deposits,
      user_intents' = user_intents,
      acquisition_config' = acquisition_config,
      acquisition_history_len' = acquisition_history_len,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- Deposit (contract.rs:292-416) ---
  action Deposit(sender: Addr,
                  amount: Uint128,
                  intents: Option[List[MbrnIntentOption]],
                  affiliate_address: Option[Addr]): bool = {
    val intents_ok: bool = match intents {
      | Some(l) => intent_ratios_sum_to_one(l)
      | None => true
    }
    val first: bool = not(acquisition_state.window_timers_started)
    val deposit_end_t: Timestamp =
      if (first) block_time + config.deposit_period_days * 86400
      else current_acquisition_window.deposit_end
    val withdrawal_end_t: Timestamp =
      if (first) deposit_end_t + config.withdrawal_period_days * 86400
      else current_acquisition_window.withdrawal_end
    val window_new: AcquisitionWindow =
      update_window_on_deposit(current_acquisition_window, deposit_end_t,
                                withdrawal_end_t, amount)
    val state_new: AcquisitionModelState =
      update_state_on_deposit(acquisition_state, first, block_time, amount)
    val intents_after: Addr -> List[MbrnIntentOption] = match intents {
      | Some(l) => user_intents.put(sender, l)
      | None => user_intents
    }
    val new_dep: AcquisitionDeposit = {
      amount: amount, deposit_time: block_time,
      vested_at: block_time + config.cliff_period_seconds,
      sent_to_disco: None, mbrn_claim_amount: None,
      disco_deposit_id: None, claimed_mbrn_amount: None,
      disco_asset: None, disco_slot: None,
    }
    all {
      amount >= config.minimum_deposit,
      block_time <= current_acquisition_window.deposit_end,
      intents_ok,
      current_acquisition_window' = window_new,
      acquisition_state' = state_new,
      user_acquisition_deposits' =
        user_acquisition_deposits.put((sender, current_window_id), new_dep),
      user_intents' = intents_after,
      msg_queue' = msg_queue.append(Msg_Reply({
        id: 0, data: "acq_rate_assurance"
      })),
      config' = config,
      current_window_id' = current_window_id,
      acquisition_config' = acquisition_config,
      acquisition_history_len' = acquisition_history_len,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- Withdraw (contract.rs:420-572) ---
  action Withdraw(sender: Addr, amount: Uint128): bool = {
    val key: (Addr, int) = (sender, current_window_id)
    val has_deposit = user_acquisition_deposits.keys().contains(key)
    val dep: AcquisitionDeposit = if (has_deposit)
        user_acquisition_deposits.get(key)
      else
        { amount: 0, deposit_time: 0, vested_at: 0,
          sent_to_disco: None, mbrn_claim_amount: None,
          disco_deposit_id: None, claimed_mbrn_amount: None,
          disco_asset: None, disco_slot: None }
    val is_after_withdrawal_period: bool =
      block_time > current_acquisition_window.withdrawal_end
    val before_cliff: bool = block_time < dep.vested_at
    val has_disco: bool = match dep.disco_deposit_id {
      | Some(_) => true
      | None => false
    }
    val should_clawback: bool =
      has_disco and is_after_withdrawal_period and before_cliff
    val deps_after: (Addr, int) -> AcquisitionDeposit =
      if (amount == dep.amount)
        user_acquisition_deposits.keys().exclude(Set(key))
          .mapBy(k => user_acquisition_deposits.get(k))
      else
        user_acquisition_deposits.put(key, update_deposit_amount(dep, dep.amount - amount))
    val window_new: AcquisitionWindow =
      update_window_on_withdraw(current_acquisition_window, amount)
    val msg_queue_after: List[CrossOrganMsg] =
      if (should_clawback)
        msg_queue
          .append(Msg_Reply({ id: 0, data: "disco_request_unstake" }))
          .append(Msg_Tokenfactory_Burn({ denom: config.mbrn_denom, from: SELF, amount: 0 }))
      else msg_queue
    all {
      has_deposit,
      amount <= dep.amount,
      user_acquisition_deposits' = deps_after,
      current_acquisition_window' = window_new,
      msg_queue' = msg_queue_after,
      config' = config, current_window_id' = current_window_id,
      user_intents' = user_intents,
      acquisition_state' = acquisition_state,
      acquisition_config' = acquisition_config,
      acquisition_history_len' = acquisition_history_len,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- AccruePool (contract.rs:576-595) ---
  action AccruePool(sender: Addr,
                     new_accrued: Uint128,
                     new_rate: Decimal,
                     new_bump_rate: Decimal,
                     pool_maxed: bool): bool = {
    val bump_changed: bool = new_bump_rate != acquisition_state.bump_rate
    val state_new: AcquisitionModelState =
      update_state_on_accrue(acquisition_state, new_accrued, new_rate,
                               new_bump_rate, pool_maxed, bump_changed, block_time)
    val history_after: int = if (acquisition_history_len < ACQUISITION_HISTORY_CAP)
        acquisition_history_len + 1
      else ACQUISITION_HISTORY_CAP
    val msg_queue_after: List[CrossOrganMsg] =
      if (bump_changed) msg_queue.append(Msg_Reply({ id: 0, data: "cdp_set_bump_rate" }))
      else msg_queue
    all {
      new_rate <= acquisition_config.max_bump_rate or
              new_rate <= acquisition_state.current_acquisition_rate,
      new_bump_rate <= acquisition_config.max_bump_rate,
      acquisition_state' = state_new,
      acquisition_history_len' = history_after,
      msg_queue' = msg_queue_after,
      config' = config,
      current_window_id' = current_window_id,
      current_acquisition_window' = current_acquisition_window,
      user_acquisition_deposits' = user_acquisition_deposits,
      user_intents' = user_intents,
      acquisition_config' = acquisition_config,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- Claim / ClaimForUser (contract.rs:959-1100) ---
  action Claim(sender: Addr,
                window_id: int,
                user_override: Option[Addr]): bool = {
    val user: Addr = match user_override {
      | Some(u) => u
      | None => sender
    }
    val key: (Addr, int) = (user, window_id)
    val has_deposit = user_acquisition_deposits.keys().contains(key)
    val dep: AcquisitionDeposit = if (has_deposit)
        user_acquisition_deposits.get(key)
      else
        { amount: 0, deposit_time: 0, vested_at: 0,
          sent_to_disco: None, mbrn_claim_amount: None,
          disco_deposit_id: None, claimed_mbrn_amount: None,
          disco_asset: None, disco_slot: None }
    val claim_amount: Uint128 = match dep.mbrn_claim_amount {
      | Some(a) => a
      | None => 0
    }
    val deps_after: (Addr, int) -> AcquisitionDeposit =
      user_acquisition_deposits.keys().exclude(Set(key))
        .mapBy(k => user_acquisition_deposits.get(k))
    all {
      has_deposit,
      block_time >= dep.vested_at,
      user_acquisition_deposits' = deps_after,
      msg_queue' = msg_queue
        .append(Msg_Reply({ id: 0, data: "transmuter_transfer_ownership" }))
        .append(Msg_Tokenfactory_Mint({
          denom: config.mbrn_denom, to: user, amount: claim_amount
        })),
      config' = config, current_window_id' = current_window_id,
      current_acquisition_window' = current_acquisition_window,
      user_intents' = user_intents,
      acquisition_state' = acquisition_state,
      acquisition_config' = acquisition_config,
      acquisition_history_len' = acquisition_history_len,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- SendAcquisitionRewardsToDisco (contract.rs:1104-1296) ---
  action SendAcquisitionRewardsToDisco(sender: Addr,
                                        window_id: int,
                                        asset: Denom,
                                        slot: int,
                                        boosted_amount: Uint128): bool = {
    val key: (Addr, int) = (sender, window_id)
    val has_deposit = user_acquisition_deposits.keys().contains(key)
    val dep: AcquisitionDeposit = if (has_deposit)
        user_acquisition_deposits.get(key)
      else
        { amount: 0, deposit_time: 0, vested_at: 0,
          sent_to_disco: None, mbrn_claim_amount: None,
          disco_deposit_id: None, claimed_mbrn_amount: None,
          disco_asset: None, disco_slot: None }
    val updated_dep: AcquisitionDeposit = mark_sent_to_disco(dep, asset, slot, boosted_amount)
    all {
      has_deposit,
      block_time > current_acquisition_window.withdrawal_end,
      user_acquisition_deposits' =
        user_acquisition_deposits.put(key, updated_dep),
      msg_queue' = msg_queue
        .append(Msg_Tokenfactory_Mint({
          denom: config.mbrn_denom, to: SELF, amount: boosted_amount
        })),
      config' = config, current_window_id' = current_window_id,
      current_acquisition_window' = current_acquisition_window,
      user_intents' = user_intents,
      acquisition_state' = acquisition_state,
      acquisition_config' = acquisition_config,
      acquisition_history_len' = acquisition_history_len,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- UpdateConfig (contract.rs:1300-1397) ---
  action UpdateConfig(sender: Addr, new_cfg: Config): bool = all {
    sender == config.owner,
    config' = new_cfg,
    current_window_id' = current_window_id,
    current_acquisition_window' = current_acquisition_window,
    user_acquisition_deposits' = user_acquisition_deposits,
    user_intents' = user_intents,
    acquisition_state' = acquisition_state,
    acquisition_config' = acquisition_config,
    acquisition_history_len' = acquisition_history_len,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- AcquisitionRateAssurance (contract.rs:1401-1433, self-only) ---
  // Pure invariant check — Rust impl loads CONFIG, queries Transmuter for
  // its acquisition_deposit_total, asserts |t_total - current_window_total|
  // <= 1, and returns Ok(Response). No on-chain state is mutated. The
  // identity update on every var is INTENTIONAL and matches the contract.
  action AcquisitionRateAssurance(sender: Addr,
                                   transmuter_total: Uint128): bool = all {
    sender == SELF,
    transmuter_total >= sat_sub(current_acquisition_window.total_deposit_amount, 1),
    transmuter_total <= current_acquisition_window.total_deposit_amount + 1,
    config' = config,
    current_window_id' = current_window_id,
    current_acquisition_window' = current_acquisition_window,
    user_acquisition_deposits' = user_acquisition_deposits,
    user_intents' = user_intents,
    acquisition_state' = acquisition_state,
    acquisition_config' = acquisition_config,
    acquisition_history_len' = acquisition_history_len,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // ================================================================
  // Invariants
  // ================================================================

  val inv_history_capped: bool =
    acquisition_history_len <= ACQUISITION_HISTORY_CAP

  val inv_window_id_monotone: bool =
    current_acquisition_window.window_id <= current_window_id

  val inv_total_deposits_nonneg: bool =
    current_acquisition_window.total_deposit_amount >= 0

  val inv_pool_under_max: bool =
    acquisition_state.accrued_pool <= acquisition_config.max_mbrn_emission

  val inv_bump_rate_capped: bool =
    acquisition_state.bump_rate <= acquisition_config.max_bump_rate

  val inv_intent_ratios_well_formed: bool =
    user_intents.keys().forall(u => intent_ratios_sum_to_one(user_intents.get(u)))

  val inv_clawback_window_consistency: bool =
    user_acquisition_deposits.keys().forall(k => {
      val dep = user_acquisition_deposits.get(k)
      dep.vested_at == dep.deposit_time + config.cliff_period_seconds
    })

  val inv_first_deposit_starts_timers: bool =
    not(acquisition_state.window_timers_started) or
    acquisition_state.first_deposit_time != None

  val inv_owner_governance_distinct: bool =
    config.owner != config.governance

  val inv_no_unilateral_capital_loss_path_AQ: bool =
    inv_pool_under_max and
    inv_bump_rate_capped and
    inv_owner_governance_distinct

  val safety: bool =
    inv_history_capped and
    inv_window_id_monotone and
    inv_total_deposits_nonneg and
    inv_pool_under_max and
    inv_bump_rate_capped and
    inv_intent_ratios_well_formed and
    inv_clawback_window_consistency and
    inv_first_deposit_starts_timers and
    inv_no_unilateral_capital_loss_path_AQ

  action step: bool = all {
    advance_time(1),
    config' = config,
    current_window_id' = current_window_id,
    current_acquisition_window' = current_acquisition_window,
    user_acquisition_deposits' = user_acquisition_deposits,
    user_intents' = user_intents,
    acquisition_config' = acquisition_config,
    acquisition_state' = acquisition_state,
    acquisition_history_len' = acquisition_history_len,
  }
}
