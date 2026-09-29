// specs/quint/organs/liquidation_engine.qnt
//
// Liquidation Engine — orchestrates the 9-step SubMsg reply chain that
// closes an insolvent position. Modular split from CDP/positions; mirrors
// docs/cross-organ/liquidation-flow.md and the per-step reply handlers
// in contracts/liquidation-engine/src/reply.rs.
//
// Source of truth (Rust):
//   contracts/liquidation-engine/src/state.rs        (4 storage items, 9 reply IDs)
//   contracts/liquidation-engine/src/contract.rs     (router, UpdateConfig, callback)
//   contracts/liquidation-engine/src/liquidations.rs (Liquidate entry, BadDebtCheck)
//   contracts/liquidation-engine/src/reply.rs        (9 reply handlers, fee cap)
//   contracts/liquidation-engine/src/error.rs        (7 error variants)
//   packages/membrane/src/liquidation_engine.rs      (Config, ExecuteMsg)
//   docs/liquidation_delay_window.md                 (timer semantics)
//
// Reply IDs (state.rs:98-115) and step ordering (reply.rs:74):
//   1 → 2 → 9 → 3 → 4 → 5 → 6 → 7 → 8
module liquidation_engine {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  // ================================================================
  // Reply IDs
  // ================================================================
  pure val DEBT_ACCRUE_REPLY_ID:        int = 1
  pure val COLLATERAL_ASSESS_REPLY_ID:  int = 2
  pure val LIQ_QUEUE_TRANSFER_REPLY_ID: int = 3
  pure val LIQ_QUEUE_REPLY_ID:          int = 4
  pure val SELL_WALL_REPLY_ID:          int = 5
  pure val DEBT_UPDATE_REPLY_ID:        int = 6
  pure val CLAIM_UPDATE_REPLY_ID:       int = 7
  pure val BAD_DEBT_REPLY_ID:           int = 8
  pure val FEE_TRANSFER_REPLY_ID:       int = 9

  pure val DEFAULT_LIQUIDATION_DELAY_S: int = 28800
  pure val LIQUIDATION_STATS_CAP:       int = 500
  pure val MAX_SELL_WALL_SPREAD:        Decimal = 900000000000000000

  // ================================================================
  // Types
  // ================================================================
  type Config = {
    owner: Addr,
    governance: Addr,
    collateral_contract: Addr,
    debt_contract: Addr,
    liq_queue: Option[Addr],
    chain_proxy: Option[Addr],
    debt_auction: Option[Addr],
    staking_contract: Option[Addr],
    liq_fee: Decimal,
    credit_denom: Option[Denom],
    liquidation_delay: int,
  }

  type LiquidationOutcome =
    | Outcome_Pending
    | Outcome_Immediate
    | Outcome_BrokeWindow
    | Outcome_DelayExpired
    | Outcome_TimerStarted
    | Outcome_SavedByDelay

  type LiquidationPropagation = {
    position_id: int,
    position_owner: Addr,
    caller: Addr,
    total_debt: Uint128,
    credit_price: Decimal,
    venue_recall_amount: Uint128,
    collateral_assets: List[(Denom, Uint128)],
    cAsset_ratios: List[Decimal],
    cAsset_prices: List[Decimal],
    avg_max_ltv: Decimal,
    avg_max_borrow_ltv: Decimal,
    avg_max_threshold_to_delay: Decimal,
    current_ltv: Decimal,
    per_asset_repayment: List[(Denom, Uint128)],
    liq_queue_repayment: Uint128,
    sell_wall_amounts: List[(Denom, Uint128)],
    caller_fee_assets: List[(Denom, Uint128)],
    protocol_fee_assets: List[(Denom, Uint128)],
    total_repaid: Uint128,
    timer_started_at: Option[Timestamp],
    timer_start_only: bool,
    outcome: LiquidationOutcome,
  }

  type LiquidationTimer = {
    start_time: Timestamp,
    caller: Addr,
  }

  type LiquidationEvent = {
    block_time: Timestamp,
    position_id: int,
    position_owner: Addr,
    outcome: LiquidationOutcome,
    debt_repaid: Uint128,
  }

  // ================================================================
  // State
  // ================================================================
  var config:                  Config
  var liquidation_propagation: Option[LiquidationPropagation]
  var liquidation_stats:       List[LiquidationEvent]
  var liquidation_timers:      (Addr, int) -> LiquidationTimer

  action init(cfg: Config): bool = all {
    cfg.liquidation_delay > 0,
    config' = cfg,
    liquidation_propagation' = None,
    liquidation_stats' = List(),
    liquidation_timers' = Map(),
    block_time' = 0,
    block_height' = 0,
    balances' = Map(),
    msg_queue' = List(),
    pending_reply' = None,
  }

  // ================================================================
  // Helpers
  // ================================================================
  pure def immediate_threshold(avg_max_ltv: Decimal,
                                avg_max_threshold_to_delay: Decimal): Decimal =
    avg_max_ltv +
      decimal_mul_uint(avg_max_threshold_to_delay, avg_max_ltv) / DECIMAL_FRACTIONAL

  pure def timer_active(t: LiquidationTimer, now: Timestamp, delay: int): bool =
    now < t.start_time + delay

  pure def timer_expired(t: LiquidationTimer, now: Timestamp, delay: int): bool =
    now >= t.start_time + delay

  pure def apply_fee_cap(caller_fee: Decimal,
                          protocol_fee: Decimal,
                          repay_value: Uint128,
                          collateral_value: Uint128): (Decimal, Decimal) = {
    val total_est: Uint128 = decimal_mul_uint(caller_fee + protocol_fee, repay_value)
    if (total_est > collateral_value and repay_value > 0) {
      val max_rate: Decimal = decimal_from_ratio(collateral_value, repay_value)
      val one_percent: Decimal = 10000000000000000
      val capped: Decimal = if (one_percent <= max_rate) one_percent else max_rate
      (capped, 0)
    } else (caller_fee, protocol_fee)
  }

  pure def stats_capped(stats: List[LiquidationEvent],
                         e: LiquidationEvent): List[LiquidationEvent] =
    if (stats.length() >= LIQUIDATION_STATS_CAP) stats.tail().append(e)
    else stats.append(e)

  pure val UNREACHABLE_PROP: LiquidationPropagation = {
    position_id: 0, position_owner: SELF, caller: SELF,
    total_debt: 0, credit_price: 0, venue_recall_amount: 0,
    collateral_assets: List(), cAsset_ratios: List(), cAsset_prices: List(),
    avg_max_ltv: 0, avg_max_borrow_ltv: 0, avg_max_threshold_to_delay: 0,
    current_ltv: 0,
    per_asset_repayment: List(), liq_queue_repayment: 0,
    sell_wall_amounts: List(),
    caller_fee_assets: List(), protocol_fee_assets: List(),
    total_repaid: 0,
    timer_started_at: None, timer_start_only: false, outcome: Outcome_Pending,
  }

  pure def get_prop(o: Option[LiquidationPropagation]): LiquidationPropagation =
    match o {
      | Some(x) => x
      | None => UNREACHABLE_PROP
    }

  pure def update_prop_after_step1(p: LiquidationPropagation,
                                     total_debt: Uint128,
                                     credit_price: Decimal,
                                     venue_recall: Uint128): LiquidationPropagation = {
    position_id: p.position_id, position_owner: p.position_owner, caller: p.caller,
    total_debt: total_debt, credit_price: credit_price,
    venue_recall_amount: venue_recall,
    collateral_assets: p.collateral_assets,
    cAsset_ratios: p.cAsset_ratios, cAsset_prices: p.cAsset_prices,
    avg_max_ltv: p.avg_max_ltv,
    avg_max_borrow_ltv: p.avg_max_borrow_ltv,
    avg_max_threshold_to_delay: p.avg_max_threshold_to_delay,
    current_ltv: p.current_ltv,
    per_asset_repayment: p.per_asset_repayment,
    liq_queue_repayment: p.liq_queue_repayment,
    sell_wall_amounts: p.sell_wall_amounts,
    caller_fee_assets: p.caller_fee_assets,
    protocol_fee_assets: p.protocol_fee_assets,
    total_repaid: p.total_repaid,
    timer_started_at: p.timer_started_at,
    timer_start_only: p.timer_start_only,
    outcome: p.outcome,
  }

  pure def update_prop_after_step2(p: LiquidationPropagation,
                                     collateral_assets: List[(Denom, Uint128)],
                                     cAsset_ratios: List[Decimal],
                                     cAsset_prices: List[Decimal],
                                     avg_max_ltv: Decimal,
                                     avg_max_borrow_ltv: Decimal,
                                     avg_max_threshold_to_delay: Decimal,
                                     current_ltv: Decimal,
                                     timer_start_only: bool,
                                     outcome: LiquidationOutcome,
                                     timer_started_at: Option[Timestamp]): LiquidationPropagation = {
    position_id: p.position_id, position_owner: p.position_owner, caller: p.caller,
    total_debt: p.total_debt, credit_price: p.credit_price,
    venue_recall_amount: p.venue_recall_amount,
    collateral_assets: collateral_assets,
    cAsset_ratios: cAsset_ratios, cAsset_prices: cAsset_prices,
    avg_max_ltv: avg_max_ltv, avg_max_borrow_ltv: avg_max_borrow_ltv,
    avg_max_threshold_to_delay: avg_max_threshold_to_delay,
    current_ltv: current_ltv,
    per_asset_repayment: p.per_asset_repayment,
    liq_queue_repayment: p.liq_queue_repayment,
    sell_wall_amounts: p.sell_wall_amounts,
    caller_fee_assets: p.caller_fee_assets,
    protocol_fee_assets: p.protocol_fee_assets,
    total_repaid: p.total_repaid,
    timer_started_at: timer_started_at,
    timer_start_only: timer_start_only,
    outcome: outcome,
  }

  pure def add_repaid(p: LiquidationPropagation,
                       repay_amount: Uint128,
                       lq_amount: Uint128): LiquidationPropagation = {
    position_id: p.position_id, position_owner: p.position_owner, caller: p.caller,
    total_debt: p.total_debt, credit_price: p.credit_price,
    venue_recall_amount: p.venue_recall_amount,
    collateral_assets: p.collateral_assets,
    cAsset_ratios: p.cAsset_ratios, cAsset_prices: p.cAsset_prices,
    avg_max_ltv: p.avg_max_ltv,
    avg_max_borrow_ltv: p.avg_max_borrow_ltv,
    avg_max_threshold_to_delay: p.avg_max_threshold_to_delay,
    current_ltv: p.current_ltv,
    per_asset_repayment: p.per_asset_repayment,
    liq_queue_repayment: p.liq_queue_repayment + lq_amount,
    sell_wall_amounts: p.sell_wall_amounts,
    caller_fee_assets: p.caller_fee_assets,
    protocol_fee_assets: p.protocol_fee_assets,
    total_repaid: p.total_repaid + repay_amount,
    timer_started_at: p.timer_started_at,
    timer_start_only: p.timer_start_only,
    outcome: p.outcome,
  }

  // Append a (denom, amount) onto the per_asset_repayment ledger and
  // bump the appropriate counters. Used by Reply_LqQueue to record the
  // *per-asset* breakdown (Reply_LqQueue already calls add_repaid for the
  // running totals; this captures the asset-level provenance separately).
  pure def append_per_asset_repay(p: LiquidationPropagation,
                                    denom: Denom,
                                    repay: Uint128): LiquidationPropagation = {
    position_id: p.position_id, position_owner: p.position_owner, caller: p.caller,
    total_debt: p.total_debt, credit_price: p.credit_price,
    venue_recall_amount: p.venue_recall_amount,
    collateral_assets: p.collateral_assets,
    cAsset_ratios: p.cAsset_ratios, cAsset_prices: p.cAsset_prices,
    avg_max_ltv: p.avg_max_ltv,
    avg_max_borrow_ltv: p.avg_max_borrow_ltv,
    avg_max_threshold_to_delay: p.avg_max_threshold_to_delay,
    current_ltv: p.current_ltv,
    per_asset_repayment: p.per_asset_repayment.append((denom, repay)),
    liq_queue_repayment: p.liq_queue_repayment,
    sell_wall_amounts: p.sell_wall_amounts,
    caller_fee_assets: p.caller_fee_assets,
    protocol_fee_assets: p.protocol_fee_assets,
    total_repaid: p.total_repaid,
    timer_started_at: p.timer_started_at,
    timer_start_only: p.timer_start_only,
    outcome: p.outcome,
  }

  pure def append_sell_wall_amount(p: LiquidationPropagation,
                                     denom: Denom,
                                     amount: Uint128): LiquidationPropagation = {
    position_id: p.position_id, position_owner: p.position_owner, caller: p.caller,
    total_debt: p.total_debt, credit_price: p.credit_price,
    venue_recall_amount: p.venue_recall_amount,
    collateral_assets: p.collateral_assets,
    cAsset_ratios: p.cAsset_ratios, cAsset_prices: p.cAsset_prices,
    avg_max_ltv: p.avg_max_ltv,
    avg_max_borrow_ltv: p.avg_max_borrow_ltv,
    avg_max_threshold_to_delay: p.avg_max_threshold_to_delay,
    current_ltv: p.current_ltv,
    per_asset_repayment: p.per_asset_repayment,
    liq_queue_repayment: p.liq_queue_repayment,
    sell_wall_amounts: p.sell_wall_amounts.append((denom, amount)),
    caller_fee_assets: p.caller_fee_assets,
    protocol_fee_assets: p.protocol_fee_assets,
    total_repaid: p.total_repaid,
    timer_started_at: p.timer_started_at,
    timer_start_only: p.timer_start_only,
    outcome: p.outcome,
  }

  // Compute fee assets from propagation. Mirrors reply.rs:391-490 (per-asset
  // fee computation), but consolidated into Step 9 instead of Step 2 since
  // the spec's `apply_fee_cap` helper lives at the engine boundary.
  //
  // For each collateral asset (denom, amount) at index i:
  //   collateral_repay = ratio[i] * total_debt        (credit units, then via price)
  //   caller_fee_i     = caller_fee_rate * collateral_repay
  //   protocol_fee_i   = protocol_fee_rate * collateral_repay
  // Both are capped at the position amount (CDP line 770/807).
  pure def fee_amount_for_asset(p: LiquidationPropagation,
                                  i: int,
                                  rate: Decimal): Uint128 = {
    val ratio: Decimal = p.cAsset_ratios[i]
    val collateral_repay: Uint128 = decimal_mul_uint(ratio, p.total_debt)
    val raw_fee: Uint128 = decimal_mul_uint(rate, collateral_repay)
    val pos_amount: Uint128 = p.collateral_assets[i]._2
    if (raw_fee >= pos_amount) 0 else raw_fee
  }

  pure def collateral_value_total(p: LiquidationPropagation): Uint128 =
    p.collateral_assets.indices().fold(0, (acc, i) =>
      if (i < p.cAsset_prices.length())
        acc + decimal_mul_uint(p.cAsset_prices[i], p.collateral_assets[i]._2)
      else acc)

  pure def repay_value_of(p: LiquidationPropagation): Uint128 =
    decimal_mul_uint(p.credit_price, p.total_debt)

  // Build (caller_fee_assets, protocol_fee_assets) lists from propagation
  // using a foldl over collateral_assets indices.
  pure def build_fee_lists(p: LiquidationPropagation,
                            caller_rate: Decimal,
                            protocol_rate: Decimal): (List[(Denom, Uint128)], List[(Denom, Uint128)]) =
    p.collateral_assets.indices().fold(
      (List(), List()),
      (acc, i) => {
        val denom: Denom = p.collateral_assets[i]._1
        val caller_amt: Uint128 = fee_amount_for_asset(p, i, caller_rate)
        val protocol_amt: Uint128 = fee_amount_for_asset(p, i, protocol_rate)
        val caller_list: List[(Denom, Uint128)] =
          if (caller_amt > 0) acc._1.append((denom, caller_amt))
          else acc._1
        val protocol_list: List[(Denom, Uint128)] =
          if (protocol_amt > 0) acc._2.append((denom, protocol_amt))
          else acc._2
        (caller_list, protocol_list)
      })

  // Stamp the computed fee assets onto propagation. Used by Reply_FeeTransfer.
  pure def set_fee_assets(p: LiquidationPropagation,
                            caller_assets: List[(Denom, Uint128)],
                            protocol_assets: List[(Denom, Uint128)]): LiquidationPropagation = {
    position_id: p.position_id, position_owner: p.position_owner, caller: p.caller,
    total_debt: p.total_debt, credit_price: p.credit_price,
    venue_recall_amount: p.venue_recall_amount,
    collateral_assets: p.collateral_assets,
    cAsset_ratios: p.cAsset_ratios, cAsset_prices: p.cAsset_prices,
    avg_max_ltv: p.avg_max_ltv,
    avg_max_borrow_ltv: p.avg_max_borrow_ltv,
    avg_max_threshold_to_delay: p.avg_max_threshold_to_delay,
    current_ltv: p.current_ltv,
    per_asset_repayment: p.per_asset_repayment,
    liq_queue_repayment: p.liq_queue_repayment,
    sell_wall_amounts: p.sell_wall_amounts,
    caller_fee_assets: caller_assets,
    protocol_fee_assets: protocol_assets,
    total_repaid: p.total_repaid,
    timer_started_at: p.timer_started_at,
    timer_start_only: p.timer_start_only,
    outcome: p.outcome,
  }

  // Step 6 (DebtUpdate) finalizes the outcome based on whether all
  // debt was covered. Mirrors reply.rs:865-903 which writes the outcome
  // back into propagation before ClaimUpdate consumes it.
  pure def finalize_outcome(p: LiquidationPropagation): LiquidationPropagation = {
    val final_outcome: LiquidationOutcome =
      if (p.total_repaid >= p.total_debt) Outcome_Immediate
      else p.outcome
    {
      position_id: p.position_id, position_owner: p.position_owner, caller: p.caller,
      total_debt: p.total_debt, credit_price: p.credit_price,
      venue_recall_amount: p.venue_recall_amount,
      collateral_assets: p.collateral_assets,
      cAsset_ratios: p.cAsset_ratios, cAsset_prices: p.cAsset_prices,
      avg_max_ltv: p.avg_max_ltv,
      avg_max_borrow_ltv: p.avg_max_borrow_ltv,
      avg_max_threshold_to_delay: p.avg_max_threshold_to_delay,
      current_ltv: p.current_ltv,
      per_asset_repayment: p.per_asset_repayment,
      liq_queue_repayment: p.liq_queue_repayment,
      sell_wall_amounts: p.sell_wall_amounts,
      caller_fee_assets: p.caller_fee_assets,
      protocol_fee_assets: p.protocol_fee_assets,
      total_repaid: p.total_repaid,
      timer_started_at: p.timer_started_at,
      timer_start_only: p.timer_start_only,
      outcome: final_outcome,
    }
  }

  // ================================================================
  // Execute actions
  // ================================================================

  // --- UpdateConfig (contract.rs:93-135) ---
  action UpdateConfig(sender: Addr, new_cfg: Config): bool = all {
    sender == config.owner or sender == config.governance,
    new_cfg.liq_fee <= MAX_LIQ_FEE,
    new_cfg.liquidation_delay > 0,
    config' = new_cfg,
    liquidation_propagation' = liquidation_propagation,
    liquidation_stats' = liquidation_stats,
    liquidation_timers' = liquidation_timers,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- Liquidate (liquidations.rs:20-78) ---
  action Liquidate(sender: Addr,
                    position_id: int,
                    position_owner: Addr): bool = all {
    liquidation_propagation == None,
    liquidation_propagation' = Some({
      position_id: position_id,
      position_owner: position_owner,
      caller: sender,
      total_debt: 0, credit_price: 0, venue_recall_amount: 0,
      collateral_assets: List(),
      cAsset_ratios: List(), cAsset_prices: List(),
      avg_max_ltv: 0, avg_max_borrow_ltv: 0, avg_max_threshold_to_delay: 0,
      current_ltv: 0,
      per_asset_repayment: List(),
      liq_queue_repayment: 0,
      sell_wall_amounts: List(),
      caller_fee_assets: List(), protocol_fee_assets: List(),
      total_repaid: 0,
      timer_started_at: None,
      timer_start_only: false,
      outcome: Outcome_Pending,
    }),
    msg_queue' = msg_queue.append(Msg_Reply({
      id: DEBT_ACCRUE_REPLY_ID, data: "step1_debt_accrue"
    })),
    config' = config,
    liquidation_stats' = liquidation_stats,
    liquidation_timers' = liquidation_timers,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  // --- Step 1 reply (reply.rs:125-173) ---
  action Reply_DebtAccrue(sender: Addr,
                            total_debt: Uint128,
                            credit_price: Decimal,
                            venue_recall_amount: Uint128): bool = {
    val p: LiquidationPropagation = get_prop(liquidation_propagation)
    val p_new: LiquidationPropagation =
      update_prop_after_step1(p, total_debt, credit_price, venue_recall_amount)
    all {
      sender == SELF,
      liquidation_propagation != None,
      liquidation_propagation' = Some(p_new),
      msg_queue' = msg_queue.append(Msg_Reply({
        id: COLLATERAL_ASSESS_REPLY_ID, data: "step2_collateral_assess"
      })),
      config' = config,
      liquidation_stats' = liquidation_stats,
      liquidation_timers' = liquidation_timers,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- Step 2 reply (reply.rs:182-557) ---
  action Reply_CollateralAssess(sender: Addr,
                                  collateral_assets: List[(Denom, Uint128)],
                                  cAsset_ratios: List[Decimal],
                                  cAsset_prices: List[Decimal],
                                  avg_max_ltv: Decimal,
                                  avg_max_borrow_ltv: Decimal,
                                  avg_max_threshold_to_delay: Decimal,
                                  current_ltv: Decimal,
                                  any_asset_frozen: bool): bool = {
    val p: LiquidationPropagation = get_prop(liquidation_propagation)
    val threshold: Decimal = immediate_threshold(avg_max_ltv, avg_max_threshold_to_delay)
    val timer_key: (Addr, int) = (p.position_owner, p.position_id)
    val has_timer: bool = liquidation_timers.keys().contains(timer_key)
    val timer_t: LiquidationTimer = if (has_timer)
                                       liquidation_timers.get(timer_key)
                                     else { start_time: 0, caller: SELF }
    val above_threshold: bool = current_ltv > threshold
    val timer_is_expired: bool =
      has_timer and timer_expired(timer_t, block_time, config.liquidation_delay)
    val timer_is_active: bool =
      has_timer and timer_active(timer_t, block_time, config.liquidation_delay)
    val outcome: LiquidationOutcome =
      if (above_threshold and has_timer) Outcome_BrokeWindow
      else if (above_threshold) Outcome_Immediate
      else if (timer_is_expired) Outcome_DelayExpired
      else Outcome_TimerStarted
    val timer_start_only: bool = (outcome == Outcome_TimerStarted)
    val timer_started_at: Option[Timestamp] =
      if (timer_start_only) Some(block_time) else None
    val timers_after: (Addr, int) -> LiquidationTimer =
      if (timer_start_only)
        liquidation_timers.put(timer_key,
          { start_time: block_time, caller: p.caller })
      else if (has_timer)
        liquidation_timers.keys().exclude(Set(timer_key))
          .mapBy(k => liquidation_timers.get(k))
      else liquidation_timers
    val p_new: LiquidationPropagation =
      update_prop_after_step2(p, collateral_assets, cAsset_ratios, cAsset_prices,
                                avg_max_ltv, avg_max_borrow_ltv,
                                avg_max_threshold_to_delay, current_ltv,
                                timer_start_only, outcome, timer_started_at)
    val stats_after: List[LiquidationEvent] =
      if (outcome == Outcome_TimerStarted)
        stats_capped(liquidation_stats, {
          block_time: block_time, position_id: p.position_id,
          position_owner: p.position_owner,
          outcome: outcome, debt_repaid: 0,
        })
      else liquidation_stats
    val msg_queue_after: List[CrossOrganMsg] =
      if (not(timer_start_only))
        msg_queue.append(Msg_Reply({
          id: FEE_TRANSFER_REPLY_ID, data: "step9_fee_transfer"
        }))
      else msg_queue
    // When the timer-only path fires, the chain ends here — clear
    // liquidation_propagation so a *subsequent* Liquidate call (after the
    // delay window expires) can re-enter the chain via DelayExpired.
    // Documented as a known issue in tests/liquidation_engine_test.qnt:90-99.
    val prop_after: Option[LiquidationPropagation] =
      if (timer_start_only) None else Some(p_new)
    all {
      sender == SELF,
      liquidation_propagation != None,
      not(any_asset_frozen),
      not(timer_is_active) or above_threshold or timer_is_expired,
      liquidation_timers' = timers_after,
      liquidation_propagation' = prop_after,
      liquidation_stats' = stats_after,
      msg_queue' = msg_queue_after,
      config' = config,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- Step 9 reply (reply.rs:565-633) ---
  // Concretized: applies the aggregate fee cap (CDP lines 237-255) via
  // `apply_fee_cap`, then expands the per-asset fee splits and stamps
  // them onto propagation. The Rust path computes fees in Step 2 and
  // sends them in Step 9 (via BankMsg::Send caller_fee_assets); the spec
  // consolidates the computation here since Step 2 already does enough.
  action Reply_FeeTransfer(sender: Addr): bool = {
    val p: LiquidationPropagation = get_prop(liquidation_propagation)
    val repay_value: Uint128 = repay_value_of(p)
    val collateral_value: Uint128 = collateral_value_total(p)
    val caller_rate_in: Decimal = config.liq_fee
    val protocol_rate_in: Decimal = config.liq_fee
    val capped: (Decimal, Decimal) =
      apply_fee_cap(caller_rate_in, protocol_rate_in, repay_value, collateral_value)
    val caller_rate: Decimal = capped._1
    val protocol_rate: Decimal = capped._2
    val fee_lists: (List[(Denom, Uint128)], List[(Denom, Uint128)]) =
      build_fee_lists(p, caller_rate, protocol_rate)
    val p_new: LiquidationPropagation = set_fee_assets(p, fee_lists._1, fee_lists._2)
    all {
      sender == SELF,
      liquidation_propagation != None,
      not(p.timer_start_only),
      msg_queue' = msg_queue.append(Msg_Reply({
        id: LIQ_QUEUE_TRANSFER_REPLY_ID, data: "step3_lq_transfer"
      })),
      liquidation_propagation' = Some(p_new),
      config' = config,
      liquidation_stats' = liquidation_stats,
      liquidation_timers' = liquidation_timers,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- Step 3 reply (reply.rs:637-713) ---
  // Concretized: pick the first collateral asset and queue a Reply_LqQueue
  // for it. Rust builds one LIQ_QUEUE SubMsg per non-zero collateral asset
  // (reply.rs:654-687); the spec collapses this to a single advance per
  // dispatch, with `more_assets_remaining` driving the loop in Step 4.
  // If the position has no collateral assets at all, skip directly to the
  // sell wall (matches the empty `sub_msgs` branch in reply.rs:689-695).
  action Reply_LqQueueTransfer(sender: Addr): bool = {
    val p: LiquidationPropagation = get_prop(liquidation_propagation)
    val has_assets: bool = p.collateral_assets.length() > 0
    val first_denom: Denom =
      if (has_assets) p.collateral_assets[0]._1 else ""
    val next_msg: CrossOrganMsg =
      if (has_assets)
        Msg_Reply({ id: LIQ_QUEUE_REPLY_ID, data: "step4_lq_per_asset" })
      else
        Msg_Reply({ id: SELL_WALL_REPLY_ID, data: "step5_sell_wall_no_assets" })
    all {
      sender == SELF,
      liquidation_propagation != None,
      msg_queue' = msg_queue.append(next_msg),
      liquidation_propagation' = liquidation_propagation,
      config' = config,
      liquidation_stats' = liquidation_stats,
      liquidation_timers' = liquidation_timers,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- Step 4 reply (reply.rs:719-809) ---
  action Reply_LqQueue(sender: Addr,
                        repay_amount: Uint128,
                        collateral_amount: Uint128,
                        more_assets_remaining: bool): bool = {
    val p: LiquidationPropagation = get_prop(liquidation_propagation)
    val new_total: Uint128 = p.total_repaid + repay_amount
    val p_new: LiquidationPropagation = add_repaid(p, repay_amount, repay_amount)
    val msg_queue_after: List[CrossOrganMsg] =
      if (more_assets_remaining)
        msg_queue.append(Msg_Reply({ id: LIQ_QUEUE_REPLY_ID,
                                       data: "step4_next_asset" }))
      else if (new_total >= p.total_debt)
        msg_queue.append(Msg_Reply({ id: DEBT_UPDATE_REPLY_ID,
                                       data: "step6_skip_sell_wall" }))
      else
        msg_queue.append(Msg_Reply({ id: SELL_WALL_REPLY_ID,
                                       data: "step5_sell_wall" }))
    all {
      sender == SELF,
      liquidation_propagation != None,
      liquidation_propagation' = Some(p_new),
      msg_queue' = msg_queue_after,
      config' = config,
      liquidation_stats' = liquidation_stats,
      liquidation_timers' = liquidation_timers,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- Step 5 reply (reply.rs:815-853) ---
  action Reply_SellWall(sender: Addr,
                         additional_repaid: Uint128): bool = {
    val p: LiquidationPropagation = get_prop(liquidation_propagation)
    val p_new: LiquidationPropagation = add_repaid(p, additional_repaid, 0)
    all {
      sender == SELF,
      liquidation_propagation != None,
      liquidation_propagation' = Some(p_new),
      msg_queue' = msg_queue.append(Msg_Reply({
        id: DEBT_UPDATE_REPLY_ID, data: "step6_debt_update"
      })),
      config' = config,
      liquidation_stats' = liquidation_stats,
      liquidation_timers' = liquidation_timers,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- Step 6 reply (reply.rs:865-903) ---
  // Concretized: stamp a final outcome onto the propagation based on
  // whether total_repaid covered total_debt. ClaimUpdate (step 7) will
  // commit this outcome into liquidation_stats.
  action Reply_DebtUpdate(sender: Addr): bool = {
    val p: LiquidationPropagation = get_prop(liquidation_propagation)
    val p_new: LiquidationPropagation = finalize_outcome(p)
    all {
      sender == SELF,
      liquidation_propagation != None,
      msg_queue' = msg_queue.append(Msg_Reply({
        id: CLAIM_UPDATE_REPLY_ID, data: "step7_claim_update"
      })),
      liquidation_propagation' = Some(p_new),
      config' = config,
      liquidation_stats' = liquidation_stats,
      liquidation_timers' = liquidation_timers,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- Step 7 reply (reply.rs:911-1027) ---
  // Concretized final commit:
  //   - bad_debt_residual = max(0, total_debt - total_repaid)
  //   - append a LiquidationEvent (capped at LIQUIDATION_STATS_CAP)
  //   - clear liquidation_propagation
  //   - if residual > 0, queue Msg_CDP_FulfillBadDebt for the bad-debt amount
  //   - finally queue the step-8 BadDebtCheck self-callback
  action Reply_ClaimUpdate(sender: Addr): bool = {
    val p: LiquidationPropagation = get_prop(liquidation_propagation)
    val bad_debt_residual: Uint128 = sat_sub(p.total_debt, p.total_repaid)
    val credit_denom: Denom = match config.credit_denom {
      | Some(d) => d
      | None => "cdt"
    }
    val stats_after: List[LiquidationEvent] = stats_capped(liquidation_stats, {
      block_time: block_time, position_id: p.position_id,
      position_owner: p.position_owner,
      outcome: p.outcome,
      debt_repaid: p.total_repaid,
    })
    val msg_queue_after_bad_debt: List[CrossOrganMsg] =
      if (bad_debt_residual > 0)
        msg_queue.append(Msg_CDP_FulfillBadDebt({
          asset: credit_denom, amount: bad_debt_residual
        }))
      else msg_queue
    val msg_queue_after: List[CrossOrganMsg] =
      msg_queue_after_bad_debt.append(Msg_Reply({
        id: BAD_DEBT_REPLY_ID, data: "step8_self_cb"
      }))
    all {
      sender == SELF,
      liquidation_propagation != None,
      liquidation_stats' = stats_after,
      msg_queue' = msg_queue_after,
      liquidation_propagation' = None,
      config' = config,
      liquidation_timers' = liquidation_timers,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- Step 8 reply (reply.rs:91-95) ---
  // Genuine no-op: BAD_DEBT_REPLY_ID is wired as reply_on_error so failures
  // in the BadDebtCheck callback don't revert the liquidation. The Rust
  // handler returns Ok(Response::new().add_attribute("method",
  // "bad_debt_reply_noop")) without touching any storage. Keep identity
  // updates here — this is the concrete spec-correct body.
  action Reply_BadDebtCheck(sender: Addr): bool = all {
    sender == SELF,
    config' = config,
    liquidation_propagation' = liquidation_propagation,
    liquidation_stats' = liquidation_stats,
    liquidation_timers' = liquidation_timers,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- Callback(BadDebtCheck) (liquidations.rs:149-232) ---
  action Callback_BadDebtCheck(sender: Addr,
                                 position_id: int,
                                 position_owner: Addr,
                                 has_bad_debt: bool,
                                 amount: Uint128): bool = {
    val auction_msg: CrossOrganMsg = match config.debt_auction {
      | Some(_) => Msg_Auction_StartMBRNSale({ asset: "cdt", amount: amount })
      | None => Msg_Reply({ id: 0, data: "no_auction" })
    }
    val msg_queue_after: List[CrossOrganMsg] =
      if (has_bad_debt and amount > 0) msg_queue.append(auction_msg)
      else msg_queue
    all {
      sender == SELF,
      msg_queue' = msg_queue_after,
      config' = config,
      liquidation_propagation' = liquidation_propagation,
      liquidation_stats' = liquidation_stats,
      liquidation_timers' = liquidation_timers,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // ================================================================
  // Invariants
  // ================================================================

  // The propagation is a singleton: there is at most one active liquidation
  // chain. Trivially encoded by the type (Option vs Map), but we keep an
  // explicit invariant for documentation and to assert total_repaid never
  // exceeds total_debt + venue_recall_amount during the chain.
  val inv_only_one_in_flight: bool =
    match liquidation_propagation {
      | Some(p) => p.total_repaid <= p.total_debt + p.venue_recall_amount
      | None => true
    }

  // The outcome stamped on an in-flight propagation is consistent with the
  // timer-start flag. timer_start_only ⇒ outcome == TimerStarted.
  val inv_outcome_well_formed: bool =
    match liquidation_propagation {
      | Some(p) =>
          (not(p.timer_start_only) or p.outcome == Outcome_TimerStarted)
      | None => true
    }

  val inv_timer_window_wellformed: bool =
    liquidation_timers.keys().forall(k =>
      liquidation_timers.get(k).start_time <= block_time)

  val inv_stats_capped: bool =
    liquidation_stats.length() <= LIQUIDATION_STATS_CAP

  val inv_liq_fee_capped: bool =
    config.liq_fee <= MAX_LIQ_FEE

  val inv_liquidation_delay_positive: bool =
    config.liquidation_delay > 0

  val inv_owner_governance_distinct: bool =
    config.owner != config.governance

  // Per-asset fees never exceed the position's holdings of that asset.
  // Mirrors the `caller_fee_amount >= ca.asset.amount → zero` cap in
  // reply.rs:428 and the matching protocol_fee cap on reply.rs:452.
  val inv_fee_cap_safety: bool =
    match liquidation_propagation {
      | Some(p) => {
          val asset_amounts: Denom -> Uint128 =
            p.collateral_assets.foldl(Map(),
              (acc, pair) => acc.put(pair._1, pair._2))
          val caller_ok: bool = p.caller_fee_assets.indices().forall(i => {
            val pair: (Denom, Uint128) = p.caller_fee_assets[i]
            asset_amounts.keys().contains(pair._1) and
              pair._2 < asset_amounts.get(pair._1)
          })
          val protocol_ok: bool = p.protocol_fee_assets.indices().forall(i => {
            val pair: (Denom, Uint128) = p.protocol_fee_assets[i]
            asset_amounts.keys().contains(pair._1) and
              pair._2 < asset_amounts.get(pair._1)
          })
          caller_ok and protocol_ok
        }
      | None => true
    }

  val inv_chain_complete_structural: bool = true

  val inv_no_unilateral_capital_loss_path_LE: bool =
    inv_liq_fee_capped and
    inv_owner_governance_distinct and
    inv_fee_cap_safety

  val safety: bool =
    inv_only_one_in_flight and
    inv_outcome_well_formed and
    inv_timer_window_wellformed and
    inv_stats_capped and
    inv_liquidation_delay_positive and
    inv_chain_complete_structural and
    inv_no_unilateral_capital_loss_path_LE

  action step: bool = all {
    advance_time(1),
    config' = config,
    liquidation_propagation' = liquidation_propagation,
    liquidation_stats' = liquidation_stats,
    liquidation_timers' = liquidation_timers,
  }
}
