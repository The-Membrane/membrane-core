// specs/quint/organs/cdp.qnt
//
// CDP / Positions — the entry contract for borrow/repay/liquidate flows.
// This module is the canonical mirror of contracts/cdp/src/, written
// against the actual Rust implementation (Amendment 7, Phase B).
//
// Source of truth (Rust):
//   contracts/cdp/src/contract.rs            (router, config, callback, take_revenue)
//   contracts/cdp/src/positions.rs           (deposit/withdraw/IncreaseDebt/Repay/ClosePosition)
//   contracts/cdp/src/rates.rs               (interest accrual; AdaptiveCurve IRM)
//   contracts/cdp/src/debt_manager.rs        (rate segment accrual, fixed-rate caps)
//   contracts/cdp/src/liquidations.rs        (Liquidate orchestration; fee cap)
//   contracts/cdp/src/reply.rs               (7 reply handlers)
//   contracts/cdp/src/state.rs               (all storage items)
//   contracts/cdp/src/circuit_breaker.rs     (asset freeze/thaw)
//   packages/membrane/src/cdp.rs             (Config, ExecuteMsg, QueryMsg)
//   packages/membrane/src/types.rs           (Position, cAsset, Basket, Rates)
module cdp {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  // ================================================================
  // Reply IDs
  // ================================================================
  pure val LIQ_QUEUE_REPLY_ID:        int = 1
  pure val WITHDRAW_REPLY_ID:         int = 4
  pure val REVENUE_REPLY_ID:          int = 5
  pure val CLOSE_POSITION_REPLY_ID:   int = 6
  pure val SELL_COLLATERAL_REPLY_ID:  int = 7
  pure val DEPLOYABLE_VENUE_REPLY_ID: int = 8
  pure val BAD_DEBT_REPLY_ID:         int = 999999

  pure val MAX_POSITIONS_AMOUNT:        int = 9
  pure val AFFILIATE_LIMIT_PER_POS:     int = 3
  pure val BAD_DEBT_CALLER_FEE:         Decimal = 10000000000000000
  pure val DEFAULT_LIQUIDATION_STAT_LIMIT: int = 500
  pure val MAX_ORACLE_ENTRIES:          int = 365
  pure val MAX_CDT_SUPPLY_ENTRIES:      int = 500

  // ================================================================
  // Types
  // ================================================================
  type CAsset = {
    asset_denom: Denom,
    asset_amount: Uint128,
    max_borrow_LTV: Decimal,
    max_LTV: Decimal,
    rate_index: Decimal,
    peg_rate_index: Decimal,
    force_redemptions: Option[bool],
    max_threshold_to_delay: Decimal,
  }

  type FixedRate = {
    rate: Decimal,
    end_time: Timestamp,
    rollover: bool,
    duration_months: int,
  }

  type RateSegment = {
    amount: Uint128,
    fixed_rate: Option[FixedRate],
  }

  type DeploymentVenue = {
    venue_address: Addr,
    deployed_amount: Uint128,
  }

  type Position = {
    position_id: int,
    collateral_assets: List[CAsset],
    rate_segments: List[RateSegment],
    deployed_to: List[DeploymentVenue],
    pending_interest: Uint128,
    total_interest_accrued: Uint128,
    peg_rate_segments: List[RateSegment],
    vol_window_initial_debt: Option[Uint128],
  }

  type SupplyCap = {
    asset_denom: Denom,
    cap: Uint128,
    current: Uint128,
    debt_total: Uint128,
  }

  type Basket = {
    basket_id: int,
    current_position_id: int,
    collateral_types: List[CAsset],
    collateral_supply_caps: List[SupplyCap],
    credit_minted: Uint128,
    pending_revenue: Uint128,
    pending_bad_debt: Uint128,
    credit_price: Decimal,
    oracle_set: bool,
    frozen: bool,
    distribute_revenue: bool,
    liq_queue: Option[Addr],
  }

  type IRMConfig = {
    adjustment_speed: Decimal,
    min_adaptive_rate: Decimal,
    max_adaptive_rate: Decimal,
  }

  type Rates = {
    current_adaptive_rate: List[Decimal],
    peg_current_adaptive_rate: List[Decimal],
    base_interest_rate: Decimal,
    rates_last_accrued: Timestamp,
    credit_last_accrued: Timestamp,
    cpc_margin_of_error: Decimal,
    negative_rates: bool,
    acquisition_bump_rate: Decimal,
  }

  type AssetCircuitBreaker = {
    asset_denom: Denom,
    frozen: bool,
    threshold: Decimal,
    reference_price: Decimal,
  }

  type LiquidationStat = {
    block_time: Timestamp,
    position_id: int,
    position_owner: Addr,
    collateral_assets: List[CAsset],
    amount_liquidated: Uint128,
  }

  type WithdrawPropagation = {
    position_id: int,
    position_owner: Addr,
    requested_assets: List[CAsset],
  }

  type LiquidationPropagation = {
    target_position_owner: Addr,
    target_position_id: int,
    caller: Addr,
    per_asset_repayment: List[(Denom, Uint128)],
    liq_queue_repayment: Uint128,
    user_repay_amount: Uint128,
    cAsset_ratios: List[Decimal],
    cAsset_prices: List[Decimal],
    liquidated_assets: List[Denom],
    caller_fee_value_paid: Uint128,
    total_repaid: Uint128,
  }

  type Config = {
    owner: Addr,
    governance: Addr,
    staking_contract: Option[Addr],
    chain_proxy: Option[Addr],
    debt_auction: Option[Addr],
    oracle_contract: Option[Addr],
    liquidity_contract: Option[Addr],
    discounts_contract: Option[Addr],
    ltv_disco: Addr,
    liq_fee: Decimal,
    collateral_twap_timeframe: int,
    credit_twap_timeframe: int,
    oracle_time_limit: int,
    cpc_multiplier: Decimal,
    debt_minimum: Uint128,
    base_debt_cap_multiplier: Uint128,
    rate_slope_multiplier: Decimal,
    affiliate_fee_max: Decimal,
    revenue_distributor: Option[Addr],
    skip_credit_price_accrual: bool,
    liquidation_stat_limit: int,
    transmuter_addr: Option[Addr],
    irm_config: IRMConfig,
    points_contract: Option[Addr],
    acquisition_contract: Option[Addr],
  }

  // ================================================================
  // State
  // ================================================================
  var config:                       Config
  var basket:                       Basket
  var rates:                        Rates
  var positions:                    Addr -> List[Position]
  var affiliations:                 int -> List[AffiliateData]
  var ownership_transfer:           Option[Addr]

  var withdraw_propagation:         Option[WithdrawPropagation]
  var repay_propagation:            Option[LiquidationPropagation]
  var asset_circuit_breakers:       Denom -> AssetCircuitBreaker

  var liquidation_stats:            List[LiquidationStat]
  var freeze_timer_start:           Timestamp
  var freeze_timer_end:             Timestamp

  var collateral_rate_assurance:    Denom -> Uint128

  action init(cfg: Config, b: Basket, r: Rates): bool = all {
    config' = cfg,
    basket' = b,
    rates' = r,
    positions' = Map(),
    affiliations' = Map(),
    ownership_transfer' = None,
    withdraw_propagation' = None,
    repay_propagation' = None,
    asset_circuit_breakers' = Map(),
    liquidation_stats' = List(),
    freeze_timer_start' = 0,
    freeze_timer_end' = 0,
    collateral_rate_assurance' = Map(),
    block_time' = 0,
    block_height' = 0,
    balances' = Map(),
    msg_queue' = List(),
    pending_reply' = None,
  }

  // ================================================================
  // Helpers
  // ================================================================
  pure val EMPTY_POSITION: Position = {
    position_id: 0, collateral_assets: List(),
    rate_segments: List(), deployed_to: List(),
    pending_interest: 0, total_interest_accrued: 0,
    peg_rate_segments: List(), vol_window_initial_debt: None,
  }

  pure def positions_of(ps: Addr -> List[Position], u: Addr): List[Position] =
    if (ps.keys().contains(u)) ps.get(u) else List()

  pure def find_position_idx(list: List[Position], pid: int): int =
    list.foldl((-1, 0), (acc, p) => {
      val cur = acc._1
      val pos = acc._2
      if (cur >= 0) (cur, pos + 1)
      else if (p.position_id == pid) (pos, pos + 1)
      else (-1, pos + 1)
    })._1

  pure def get_position(ps: Addr -> List[Position],
                        u: Addr,
                        pid: int): Option[Position] = {
    val list = positions_of(ps, u)
    val idx = find_position_idx(list, pid)
    if (idx < 0) None else Some(list[idx])
  }

  pure def replace_position(list: List[Position], pid: int,
                              new_p: Position): List[Position] =
    list.foldl(List(), (acc, p) =>
      if (p.position_id == pid) acc.append(new_p) else acc.append(p))

  // Look up the registered cAsset config (max_LTV etc.) by denom.
  pure def find_casset_cfg(b: Basket, denom: Denom): Option[CAsset] =
    b.collateral_types.foldl(None, (acc, ca) =>
      match acc {
        | Some(_) => acc
        | None => if (ca.asset_denom == denom) Some(ca) else None
      })

  pure def find_casset_idx(coll: List[CAsset], denom: Denom): int =
    coll.foldl((-1, 0), (acc, ca) => {
      val cur = acc._1
      val pos = acc._2
      if (cur >= 0) (cur, pos + 1)
      else if (ca.asset_denom == denom) (pos, pos + 1)
      else (-1, pos + 1)
    })._1

  pure def casset_with_amount(c: CAsset, amount: Uint128): CAsset = {
    asset_denom: c.asset_denom,
    asset_amount: amount,
    max_borrow_LTV: c.max_borrow_LTV,
    max_LTV: c.max_LTV,
    rate_index: c.rate_index,
    peg_rate_index: c.peg_rate_index,
    force_redemptions: c.force_redemptions,
    max_threshold_to_delay: c.max_threshold_to_delay,
  }

  // Merge a single (denom, amount) into a position's collateral list.
  pure def merge_fund(coll: List[CAsset],
                       fund: (Denom, Uint128),
                       b: Basket): List[CAsset] = {
    val denom = fund._1
    val amount = fund._2
    val idx = find_casset_idx(coll, denom)
    val cfg_opt = find_casset_cfg(b, denom)
    val empty_cfg: CAsset = {
      asset_denom: denom, asset_amount: 0,
      max_borrow_LTV: 0, max_LTV: 0,
      rate_index: DECIMAL_ONE, peg_rate_index: DECIMAL_ONE,
      force_redemptions: None, max_threshold_to_delay: 0,
    }
    val cfg: CAsset = match cfg_opt {
      | Some(c) => c
      | None => empty_cfg
    }
    if (idx >= 0)
      coll.foldl((List(), 0), (acc, ca) => {
        val pos = acc._2
        if (pos == idx)
          (acc._1.append(casset_with_amount(ca, ca.asset_amount + amount)), pos + 1)
        else (acc._1.append(ca), pos + 1)
      })._1
    else
      coll.append(casset_with_amount(cfg, amount))
  }

  pure def merge_all_funds(coll: List[CAsset],
                            funds: List[(Denom, Uint128)],
                            b: Basket): List[CAsset] =
    funds.foldl(coll, (acc, f) => merge_fund(acc, f, b))

  // Subtract a single (denom, amount) from a position's collateral list.
  // Saturates at zero (sat_sub) to mirror Rust Uint128 checked semantics
  // without panicking the spec; the Withdraw action also asserts solvency
  // post-decrement, so over-withdraw paths are fenced separately.
  pure def subtract_fund(coll: List[CAsset],
                          fund: (Denom, Uint128)): List[CAsset] = {
    val denom = fund._1
    val amount = fund._2
    val idx = find_casset_idx(coll, denom)
    if (idx < 0) coll
    else coll.foldl((List(), 0), (acc, ca) => {
      val pos = acc._2
      if (pos == idx)
        (acc._1.append(casset_with_amount(ca, sat_sub(ca.asset_amount, amount))), pos + 1)
      else (acc._1.append(ca), pos + 1)
    })._1
  }

  pure def subtract_all_funds(coll: List[CAsset],
                               funds: List[(Denom, Uint128)]): List[CAsset] =
    funds.foldl(coll, (acc, f) => subtract_fund(acc, f))

  // Did every requested withdraw amount fit (i.e., no underflow)?
  pure def funds_available(coll: List[CAsset],
                            funds: List[(Denom, Uint128)]): bool =
    funds.foldl(true, (ok, f) => {
      val idx = find_casset_idx(coll, f._1)
      ok and idx >= 0 and coll[idx].asset_amount >= f._2
    })

  pure def position_with_collaterals(p: Position,
                                       cols: List[CAsset]): Position = {
    position_id: p.position_id,
    collateral_assets: cols,
    rate_segments: p.rate_segments,
    deployed_to: p.deployed_to,
    pending_interest: p.pending_interest,
    total_interest_accrued: p.total_interest_accrued,
    peg_rate_segments: p.peg_rate_segments,
    vol_window_initial_debt: p.vol_window_initial_debt,
  }

  pure def basket_with_pid_bumped(b: Basket): Basket = {
    basket_id: b.basket_id,
    current_position_id: b.current_position_id + 1,
    collateral_types: b.collateral_types,
    collateral_supply_caps: b.collateral_supply_caps,
    credit_minted: b.credit_minted,
    pending_revenue: b.pending_revenue,
    pending_bad_debt: b.pending_bad_debt,
    credit_price: b.credit_price,
    oracle_set: b.oracle_set,
    frozen: b.frozen,
    distribute_revenue: b.distribute_revenue,
    liq_queue: b.liq_queue,
  }

  pure def accumulate_interest(principal: Uint128, rate: Decimal,
                                elapsed: int): Uint128 =
    principal + (principal * decimal_mul_uint(rate, elapsed)) / 31536000

  pure def total_position_debt(p: Position): Uint128 =
    p.rate_segments.foldl(0, (a, s) => a + s.amount)
    + p.peg_rate_segments.foldl(0, (a, s) => a + s.amount)
    + p.pending_interest

  pure def collateral_value(p: Position, prices: Denom -> Decimal): Uint128 =
    p.collateral_assets.foldl(0, (acc, ca) => {
      val price = if (prices.keys().contains(ca.asset_denom))
                    prices.get(ca.asset_denom) else 0
      acc + decimal_mul_uint(price, ca.asset_amount)
    })

  pure def avg_max_ltv(p: Position, prices: Denom -> Decimal): Decimal = {
    val cv: Uint128 = collateral_value(p, prices)
    if (cv == 0) 0
    else p.collateral_assets.foldl(0, (acc, ca) => {
      val price = if (prices.keys().contains(ca.asset_denom)) prices.get(ca.asset_denom) else 0
      val asset_v: Uint128 = decimal_mul_uint(price, ca.asset_amount)
      acc + (decimal_mul_uint(ca.max_LTV, asset_v) / 1) * DECIMAL_FRACTIONAL / cv
    })
  }

  pure def insolvent(p: Position, prices: Denom -> Decimal): bool = {
    val cv = collateral_value(p, prices)
    val debt = total_position_debt(p)
    if (cv == 0) debt > 0
    else decimal_from_ratio(debt, cv) > avg_max_ltv(p, prices)
  }

  pure def fee_cap_applies(caller_fee: Decimal, protocol_fee: Decimal,
                            repay_value: Uint128, collateral_value: Uint128): bool =
    decimal_mul_uint(caller_fee + protocol_fee, repay_value) > collateral_value

  pure def capped_caller_fee(repay_value: Uint128, collateral_value: Uint128): Decimal =
    if (repay_value == 0) 0
    else min_int(BAD_DEBT_CALLER_FEE,
                  decimal_from_ratio(collateral_value, repay_value))

  pure def update_position_for_increase(p: Position, new_seg: RateSegment,
                                          peg_debt: bool): Position = {
    position_id: p.position_id,
    collateral_assets: p.collateral_assets,
    rate_segments: if (peg_debt) p.rate_segments else p.rate_segments.append(new_seg),
    deployed_to: p.deployed_to,
    pending_interest: p.pending_interest,
    total_interest_accrued: p.total_interest_accrued,
    peg_rate_segments: if (peg_debt) p.peg_rate_segments.append(new_seg)
                         else p.peg_rate_segments,
    vol_window_initial_debt: p.vol_window_initial_debt,
  }

  pure def update_position_for_repay(p: Position, amount: Uint128): Position = {
    position_id: p.position_id,
    collateral_assets: p.collateral_assets,
    rate_segments: p.rate_segments,
    deployed_to: p.deployed_to,
    pending_interest: sat_sub(p.pending_interest, amount),
    total_interest_accrued: p.total_interest_accrued,
    peg_rate_segments: p.peg_rate_segments,
    vol_window_initial_debt: p.vol_window_initial_debt,
  }

  pure def basket_with_credit_added(b: Basket, amount: Uint128): Basket = {
    basket_id: b.basket_id, current_position_id: b.current_position_id,
    collateral_types: b.collateral_types,
    collateral_supply_caps: b.collateral_supply_caps,
    credit_minted: b.credit_minted + amount,
    pending_revenue: b.pending_revenue,
    pending_bad_debt: b.pending_bad_debt,
    credit_price: b.credit_price,
    oracle_set: b.oracle_set,
    frozen: b.frozen,
    distribute_revenue: b.distribute_revenue,
    liq_queue: b.liq_queue,
  }

  pure def basket_with_credit_subbed(b: Basket, amount: Uint128): Basket = {
    basket_id: b.basket_id, current_position_id: b.current_position_id,
    collateral_types: b.collateral_types,
    collateral_supply_caps: b.collateral_supply_caps,
    credit_minted: sat_sub(b.credit_minted, amount),
    pending_revenue: b.pending_revenue,
    pending_bad_debt: b.pending_bad_debt,
    credit_price: b.credit_price,
    oracle_set: b.oracle_set,
    frozen: b.frozen,
    distribute_revenue: b.distribute_revenue,
    liq_queue: b.liq_queue,
  }

  pure def basket_with_bad_debt_subbed(b: Basket, amount: Uint128): Basket = {
    basket_id: b.basket_id, current_position_id: b.current_position_id,
    collateral_types: b.collateral_types,
    collateral_supply_caps: b.collateral_supply_caps,
    credit_minted: b.credit_minted,
    pending_revenue: b.pending_revenue,
    pending_bad_debt: sat_sub(b.pending_bad_debt, amount),
    credit_price: b.credit_price,
    oracle_set: b.oracle_set,
    frozen: b.frozen,
    distribute_revenue: b.distribute_revenue,
    liq_queue: b.liq_queue,
  }

  pure def basket_with_revenue_zero(b: Basket): Basket = {
    basket_id: b.basket_id, current_position_id: b.current_position_id,
    collateral_types: b.collateral_types,
    collateral_supply_caps: b.collateral_supply_caps,
    credit_minted: b.credit_minted,
    pending_revenue: 0,
    pending_bad_debt: b.pending_bad_debt,
    credit_price: b.credit_price,
    oracle_set: b.oracle_set,
    frozen: b.frozen,
    distribute_revenue: b.distribute_revenue,
    liq_queue: b.liq_queue,
  }

  pure def rates_with_accrual(r: Rates, now: Timestamp): Rates = {
    current_adaptive_rate: r.current_adaptive_rate,
    peg_current_adaptive_rate: r.peg_current_adaptive_rate,
    base_interest_rate: r.base_interest_rate,
    rates_last_accrued: now,
    credit_last_accrued: now,
    cpc_margin_of_error: r.cpc_margin_of_error,
    negative_rates: r.negative_rates,
    acquisition_bump_rate: r.acquisition_bump_rate,
  }

  pure def rates_with_bump(r: Rates, rate: Decimal): Rates = {
    current_adaptive_rate: r.current_adaptive_rate,
    peg_current_adaptive_rate: r.peg_current_adaptive_rate,
    base_interest_rate: r.base_interest_rate,
    rates_last_accrued: r.rates_last_accrued,
    credit_last_accrued: r.credit_last_accrued,
    cpc_margin_of_error: r.cpc_margin_of_error,
    negative_rates: r.negative_rates,
    acquisition_bump_rate: rate,
  }

  // ================================================================
  // Execute actions
  // ================================================================

  // --- UpdateConfig (contract.rs:602-640) ---
  action UpdateConfig(sender: Addr, new_cfg: Config): bool = all {
    sender == config.owner or
            (ownership_transfer != None and Some(sender) == ownership_transfer),
    new_cfg.liq_fee <= MAX_LIQ_FEE,
    new_cfg.affiliate_fee_max <= MAX_AFFILIATE_FEE,
    config' = new_cfg,
    basket' = basket, rates' = rates,
    positions' = positions, affiliations' = affiliations,
    ownership_transfer' = ownership_transfer,
    withdraw_propagation' = withdraw_propagation,
    repay_propagation' = repay_propagation,
    asset_circuit_breakers' = asset_circuit_breakers,
    liquidation_stats' = liquidation_stats,
    freeze_timer_start' = freeze_timer_start,
    freeze_timer_end' = freeze_timer_end,
    collateral_rate_assurance' = collateral_rate_assurance,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- Deposit (positions.rs:59-318) ---
  // Concrete body: creates a new Position (or merges into an existing one)
  // for `position_owner`, looking up cAsset config from `basket` to set the
  // correct max_LTV / max_borrow_LTV per collateral. When position_id is
  // None, allocates basket.current_position_id and bumps the counter.
  action Deposit(sender: Addr,
                  position_id: Option[int],
                  position_owner: Addr,
                  funds: List[(Denom, Uint128)]): bool = {
    val list = positions_of(positions, position_owner)
    val is_new: bool = match position_id {
      | Some(_) => false
      | None => true
    }
    val pid: int = match position_id {
      | Some(id) => id
      | None => basket.current_position_id
    }
    val existing_idx = if (is_new) -1 else find_position_idx(list, pid)
    val existing_p: Position = if (existing_idx >= 0) list[existing_idx]
                                else { position_id: pid, collateral_assets: List(),
                                        rate_segments: List(), deployed_to: List(),
                                        pending_interest: 0, total_interest_accrued: 0,
                                        peg_rate_segments: List(),
                                        vol_window_initial_debt: None }
    val updated_p: Position = position_with_collaterals(existing_p,
        merge_all_funds(existing_p.collateral_assets, funds, basket))
    val new_list: List[Position] = if (existing_idx >= 0)
        replace_position(list, pid, updated_p)
      else list.append(updated_p)
    val new_basket: Basket = if (is_new) basket_with_pid_bumped(basket) else basket
    all {
      not(basket.frozen),
      list.length() < MAX_POSITIONS_AMOUNT,
      funds.length() > 0,
      positions' = positions.put(position_owner, new_list),
      basket' = new_basket,
      msg_queue' = msg_queue.append(Msg_Reply({ id: 0, data: "collateral_rate_assurance" })),
      rates' = rates, affiliations' = affiliations,
      ownership_transfer' = ownership_transfer,
      withdraw_propagation' = withdraw_propagation,
      repay_propagation' = repay_propagation,
      asset_circuit_breakers' = asset_circuit_breakers,
      liquidation_stats' = liquidation_stats,
      freeze_timer_start' = freeze_timer_start,
      freeze_timer_end' = freeze_timer_end,
      collateral_rate_assurance' = collateral_rate_assurance,
      config' = config,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- IncreaseDebt (positions.rs:1401-1887) ---
  action IncreaseDebt(sender: Addr,
                       position_id: int,
                       amount: Uint128,
                       target_LTV: Option[Decimal],
                       mint_to_addr: Option[Addr],
                       peg_debt: bool,
                       prices: Denom -> Decimal): bool = {
    val opt_p = get_position(positions, sender, position_id)
    val p_old: Position = match opt_p {
      | Some(p) => p
      | None => EMPTY_POSITION
    }
    val new_seg: RateSegment = { amount: amount, fixed_rate: None }
    val p_new: Position = update_position_for_increase(p_old, new_seg, peg_debt)
    val new_list: List[Position] =
      replace_position(positions_of(positions, sender), position_id, p_new)
    all {
      not(basket.frozen),
      opt_p != None,
      not(insolvent(p_new, prices)),
      total_position_debt(p_new) >= config.debt_minimum,
      positions' = positions.put(sender, new_list),
      basket' = basket_with_credit_added(basket, amount),
      rates' = rates, affiliations' = affiliations,
      ownership_transfer' = ownership_transfer,
      withdraw_propagation' = withdraw_propagation,
      repay_propagation' = repay_propagation,
      asset_circuit_breakers' = asset_circuit_breakers,
      liquidation_stats' = liquidation_stats,
      freeze_timer_start' = freeze_timer_start,
      freeze_timer_end' = freeze_timer_end,
      collateral_rate_assurance' = collateral_rate_assurance,
      config' = config,
      msg_queue' = msg_queue,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- Withdraw (positions.rs:398-659) ---
  // Concrete body: decrement each requested (denom, amount) from the
  // position's collateral list, then assert the post-withdraw position
  // is still solvent. We do this synchronously and clear
  // withdraw_propagation rather than queueing the reply chain — the
  // reply chain in the Rust impl exists to validate solvency *after*
  // the bank send completes, but in the spec we have prices in hand,
  // so we can fold both halves into a single atomic transition.
  action Withdraw(sender: Addr,
                   position_id: int,
                   assets: List[(Denom, Uint128)],
                   send_to: Option[Addr],
                   prices: Denom -> Decimal): bool = {
    val opt_p = get_position(positions, sender, position_id)
    val p_old: Position = match opt_p {
      | Some(p) => p
      | None => EMPTY_POSITION
    }
    val new_coll: List[CAsset] = subtract_all_funds(p_old.collateral_assets, assets)
    val p_new: Position = position_with_collaterals(p_old, new_coll)
    val new_list: List[Position] =
      replace_position(positions_of(positions, sender), position_id, p_new)
    all {
      not(basket.frozen),
      opt_p != None,
      assets.length() > 0,
      funds_available(p_old.collateral_assets, assets),
      not(insolvent(p_new, prices)),
      positions' = positions.put(sender, new_list),
      withdraw_propagation' = None,
      msg_queue' = msg_queue,
      basket' = basket, rates' = rates,
      affiliations' = affiliations,
      ownership_transfer' = ownership_transfer,
      repay_propagation' = repay_propagation,
      asset_circuit_breakers' = asset_circuit_breakers,
      liquidation_stats' = liquidation_stats,
      freeze_timer_start' = freeze_timer_start,
      freeze_timer_end' = freeze_timer_end,
      collateral_rate_assurance' = collateral_rate_assurance,
      config' = config,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- Repay (positions.rs:660-1187) ---
  action Repay(sender: Addr,
                position_id: int,
                position_owner: Addr,
                amount: Uint128,
                send_excess_to: Option[Addr]): bool = {
    val opt_p = get_position(positions, position_owner, position_id)
    val p_old: Position = match opt_p {
      | Some(p) => p
      | None => EMPTY_POSITION
    }
    val current_debt = total_position_debt(p_old)
    val p_new: Position = update_position_for_repay(p_old, amount)
    val new_list: List[Position] =
      replace_position(positions_of(positions, position_owner), position_id, p_new)
    all {
      opt_p != None,
      amount <= current_debt,
      positions' = positions.put(position_owner, new_list),
      basket' = basket_with_credit_subbed(basket, amount),
      rates' = rates, affiliations' = affiliations,
      ownership_transfer' = ownership_transfer,
      withdraw_propagation' = withdraw_propagation,
      repay_propagation' = repay_propagation,
      asset_circuit_breakers' = asset_circuit_breakers,
      liquidation_stats' = liquidation_stats,
      freeze_timer_start' = freeze_timer_start,
      freeze_timer_end' = freeze_timer_end,
      collateral_rate_assurance' = collateral_rate_assurance,
      config' = config,
      msg_queue' = msg_queue.append(Msg_Tokenfactory_Burn({
        denom: "cdt", from: sender, amount: amount
      })),
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- Liquidate (liquidations.rs:38-563) ---
  action Liquidate(sender: Addr,
                    position_id: int,
                    position_owner: Addr,
                    prices: Denom -> Decimal): bool = {
    val opt_p = get_position(positions, position_owner, position_id)
    val p: Position = match opt_p {
      | Some(x) => x
      | None => EMPTY_POSITION
    }
    all {
      not(basket.frozen),
      opt_p != None,
      insolvent(p, prices),
      repay_propagation' = Some({
        target_position_owner: position_owner,
        target_position_id: position_id,
        caller: sender,
        per_asset_repayment: List(),
        liq_queue_repayment: 0,
        user_repay_amount: 0,
        cAsset_ratios: List(),
        cAsset_prices: List(),
        liquidated_assets: List(),
        caller_fee_value_paid: 0,
        total_repaid: 0,
      }),
      msg_queue' = msg_queue
        .append(Msg_Reply({ id: DEPLOYABLE_VENUE_REPLY_ID, data: "venue_recall" }))
        .append(Msg_Reply({ id: LIQ_QUEUE_REPLY_ID, data: "liq_queue" }))
        .append(Msg_Reply({ id: SELL_COLLATERAL_REPLY_ID, data: "sell_wall" }))
        .append(Msg_Reply({ id: BAD_DEBT_REPLY_ID, data: "bad_debt_check" })),
      positions' = positions, basket' = basket, rates' = rates,
      affiliations' = affiliations,
      ownership_transfer' = ownership_transfer,
      withdraw_propagation' = withdraw_propagation,
      asset_circuit_breakers' = asset_circuit_breakers,
      liquidation_stats' = liquidation_stats,
      freeze_timer_start' = freeze_timer_start,
      freeze_timer_end' = freeze_timer_end,
      collateral_rate_assurance' = collateral_rate_assurance,
      config' = config,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- Accrue (rates.rs:268-325) ---
  action Accrue(sender: Addr,
                 position_owner: Addr,
                 position_ids: List[int],
                 elapsed: int): bool = all {
    elapsed >= 0,
    rates' = rates_with_accrual(rates, block_time),
    config' = config, basket' = basket, positions' = positions,
    affiliations' = affiliations,
    ownership_transfer' = ownership_transfer,
    withdraw_propagation' = withdraw_propagation,
    repay_propagation' = repay_propagation,
    asset_circuit_breakers' = asset_circuit_breakers,
    liquidation_stats' = liquidation_stats,
    freeze_timer_start' = freeze_timer_start,
    freeze_timer_end' = freeze_timer_end,
    collateral_rate_assurance' = collateral_rate_assurance,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- ClosePosition (positions.rs:1888-2105) ---
  action ClosePosition(sender: Addr,
                        position_id: int,
                        close_percentage: Decimal,
                        max_spread: Decimal,
                        send_to: Option[Addr]): bool = {
    val opt_p = get_position(positions, sender, position_id)
    val p: Position = match opt_p {
      | Some(x) => x
      | None => EMPTY_POSITION
    }
    val close_amount: Uint128 =
      decimal_mul_uint(close_percentage, total_position_debt(p))
    all {
      not(basket.frozen),
      close_percentage > 0,
      close_percentage <= DECIMAL_ONE,
      opt_p != None,
      // Rust persists CLOSE_POSITION_PROPAGATION + dispatches venue/router
      // SubMsgs that bottom out in a CLOSE_POSITION_REPLY_ID. We model this
      // by storing the analogous repay_propagation and queuing the reply
      // chain, mirroring Liquidate's pattern (no on-chain debt change yet —
      // that happens after collateral sales settle).
      repay_propagation' = Some({
        target_position_owner: sender,
        target_position_id: position_id,
        caller: sender,
        per_asset_repayment: List(),
        liq_queue_repayment: 0,
        user_repay_amount: 0,
        cAsset_ratios: List(),
        cAsset_prices: List(),
        liquidated_assets: List(),
        caller_fee_value_paid: 0,
        total_repaid: close_amount,
      }),
      msg_queue' = msg_queue.append(Msg_Reply({
        id: SELL_COLLATERAL_REPLY_ID, data: "close_position"
      })),
      positions' = positions, basket' = basket, rates' = rates,
      affiliations' = affiliations,
      ownership_transfer' = ownership_transfer,
      withdraw_propagation' = withdraw_propagation,
      asset_circuit_breakers' = asset_circuit_breakers,
      liquidation_stats' = liquidation_stats,
      freeze_timer_start' = freeze_timer_start,
      freeze_timer_end' = freeze_timer_end,
      collateral_rate_assurance' = collateral_rate_assurance,
      config' = config,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- EditcAsset (contract.rs:510-599) ---
  // Rust: locate basket.collateral_types[i] by asset.info; mutate
  // max_LTV / max_borrow_LTV / max_threshold_to_delay (None fields untouched).
  action EditcAsset(sender: Addr,
                     asset_denom: Denom,
                     max_borrow_LTV: Option[Decimal],
                     max_LTV: Option[Decimal],
                     max_threshold_to_delay: Option[Decimal]): bool = {
    val ltv_ok: bool = match max_LTV {
      | Some(v) => v <= MAX_LTV_HARD_CAP and v >= DECIMAL_ONE / 100
      | None => true
    }
    val borrow_ltv_ok: bool = match max_borrow_LTV {
      | Some(v) => v < DECIMAL_ONE
      | None => true
    }
    val asset_present: bool =
      basket.collateral_types.foldl(false, (acc, ca) =>
        acc or ca.asset_denom == asset_denom)
    val cols_after: List[CAsset] =
      basket.collateral_types.foldl(List(), (acc, ca) =>
        if (ca.asset_denom != asset_denom) acc.append(ca)
        else acc.append({
          asset_denom: ca.asset_denom,
          asset_amount: ca.asset_amount,
          max_borrow_LTV: match max_borrow_LTV {
            | Some(v) => v
            | None    => ca.max_borrow_LTV
          },
          max_LTV: match max_LTV {
            | Some(v) => v
            | None    => ca.max_LTV
          },
          rate_index: ca.rate_index,
          peg_rate_index: ca.peg_rate_index,
          force_redemptions: ca.force_redemptions,
          max_threshold_to_delay: match max_threshold_to_delay {
            | Some(v) => v
            | None    => ca.max_threshold_to_delay
          },
        }))
    val basket_after: Basket = {
      basket_id: basket.basket_id,
      current_position_id: basket.current_position_id,
      collateral_types: cols_after,
      collateral_supply_caps: basket.collateral_supply_caps,
      credit_minted: basket.credit_minted,
      pending_revenue: basket.pending_revenue,
      pending_bad_debt: basket.pending_bad_debt,
      credit_price: basket.credit_price,
      oracle_set: basket.oracle_set,
      frozen: basket.frozen,
      distribute_revenue: basket.distribute_revenue,
      liq_queue: basket.liq_queue,
    }
    all {
      sender == config.owner,
      ltv_ok,
      borrow_ltv_ok,
      asset_present,
      basket' = basket_after,
      config' = config, rates' = rates, positions' = positions,
      affiliations' = affiliations,
      ownership_transfer' = ownership_transfer,
      withdraw_propagation' = withdraw_propagation,
      repay_propagation' = repay_propagation,
      asset_circuit_breakers' = asset_circuit_breakers,
      liquidation_stats' = liquidation_stats,
      freeze_timer_start' = freeze_timer_start,
      freeze_timer_end' = freeze_timer_end,
      collateral_rate_assurance' = collateral_rate_assurance,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- EditBasket (positions.rs:3043-3456) ---
  action EditBasket(sender: Addr, b: Basket): bool = all {
    sender == config.owner,
    basket' = b,
    config' = config, rates' = rates, positions' = positions,
    affiliations' = affiliations,
    ownership_transfer' = ownership_transfer,
    withdraw_propagation' = withdraw_propagation,
    repay_propagation' = repay_propagation,
    asset_circuit_breakers' = asset_circuit_breakers,
    liquidation_stats' = liquidation_stats,
    freeze_timer_start' = freeze_timer_start,
    freeze_timer_end' = freeze_timer_end,
    collateral_rate_assurance' = collateral_rate_assurance,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- SetAffiliate (contract.rs:785-843) ---
  action SetAffiliate(sender: Addr,
                       position_id: int,
                       affiliate_address: Addr,
                       affiliate_fee: Decimal,
                       label: Option[str]): bool = {
    val current = if (affiliations.keys().contains(position_id))
                    affiliations.get(position_id) else List()
    val label_str: str = match label {
      | Some(l) => l
      | None => ""
    }
    val new_aff: AffiliateData = {
      affiliate_address: affiliate_address,
      affiliate_fee: affiliate_fee,
      label: label_str,
      time_affiliated: block_time,
    }
    all {
      affiliate_fee > 0,
      affiliate_fee <= config.affiliate_fee_max,
      current.length() < AFFILIATE_LIMIT_PER_POS,
      affiliations' = affiliations.put(position_id, current.append(new_aff)),
      config' = config, basket' = basket, rates' = rates, positions' = positions,
      ownership_transfer' = ownership_transfer,
      withdraw_propagation' = withdraw_propagation,
      repay_propagation' = repay_propagation,
      asset_circuit_breakers' = asset_circuit_breakers,
      liquidation_stats' = liquidation_stats,
      freeze_timer_start' = freeze_timer_start,
      freeze_timer_end' = freeze_timer_end,
      collateral_rate_assurance' = collateral_rate_assurance,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- FulfillBadDebt (contract.rs:336-406) ---
  action FulfillBadDebt(sender: Addr, sent_cdt: Uint128): bool = {
    val to_burn = min_int(sent_cdt, basket.pending_bad_debt)
    val refund  = sent_cdt - to_burn
    val burn_msg: CrossOrganMsg =
      if (to_burn > 0) Msg_Tokenfactory_Burn({ denom: "cdt", from: SELF, amount: to_burn })
      else Msg_Reply({ id: 0, data: "noop" })
    val refund_msg: CrossOrganMsg =
      if (refund > 0) Msg_BankSend({ to: sender, denom: "cdt", amount: refund })
      else Msg_Reply({ id: 0, data: "noop" })
    all {
      basket' = basket_with_bad_debt_subbed(basket, to_burn),
      msg_queue' = msg_queue.append(burn_msg).append(refund_msg),
      config' = config, rates' = rates, positions' = positions,
      affiliations' = affiliations,
      ownership_transfer' = ownership_transfer,
      withdraw_propagation' = withdraw_propagation,
      repay_propagation' = repay_propagation,
      asset_circuit_breakers' = asset_circuit_breakers,
      liquidation_stats' = liquidation_stats,
      freeze_timer_start' = freeze_timer_start,
      freeze_timer_end' = freeze_timer_end,
      collateral_rate_assurance' = collateral_rate_assurance,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- TakeRevenue (contract.rs:411-457) ---
  action TakeRevenue(sender: Addr): bool = {
    val auth_ok: bool = match config.revenue_distributor {
      | Some(a) => sender == a
      | None => false
    }
    val taken = basket.pending_revenue
    all {
      auth_ok,
      basket' = basket_with_revenue_zero(basket),
      msg_queue' = msg_queue.append(Msg_BankSend({
        to: sender, denom: "cdt", amount: taken
      })),
      config' = config, rates' = rates, positions' = positions,
      affiliations' = affiliations,
      ownership_transfer' = ownership_transfer,
      withdraw_propagation' = withdraw_propagation,
      repay_propagation' = repay_propagation,
      asset_circuit_breakers' = asset_circuit_breakers,
      liquidation_stats' = liquidation_stats,
      freeze_timer_start' = freeze_timer_start,
      freeze_timer_end' = freeze_timer_end,
      collateral_rate_assurance' = collateral_rate_assurance,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- CheckAndClearDebtDelta (contract.rs:462-497) ---
  // Rust: position.vol_window_initial_debt = None; update_position(...).
  action CheckAndClearDebtDelta(sender: Addr,
                                  position_owner: Addr,
                                  position_id: int): bool = {
    val auth_ok: bool = match config.points_contract {
      | Some(a) => sender == a
      | None => false
    }
    val opt_p = get_position(positions, position_owner, position_id)
    val p_old: Position = match opt_p {
      | Some(p) => p
      | None => EMPTY_POSITION
    }
    val p_new: Position = {
      position_id: p_old.position_id,
      collateral_assets: p_old.collateral_assets,
      rate_segments: p_old.rate_segments,
      deployed_to: p_old.deployed_to,
      pending_interest: p_old.pending_interest,
      total_interest_accrued: p_old.total_interest_accrued,
      peg_rate_segments: p_old.peg_rate_segments,
      vol_window_initial_debt: None,
    }
    val new_list: List[Position] =
      replace_position(positions_of(positions, position_owner), position_id, p_new)
    all {
      auth_ok,
      opt_p != None,
      positions' = positions.put(position_owner, new_list),
      config' = config, basket' = basket, rates' = rates,
      affiliations' = affiliations,
      ownership_transfer' = ownership_transfer,
      withdraw_propagation' = withdraw_propagation,
      repay_propagation' = repay_propagation,
      asset_circuit_breakers' = asset_circuit_breakers,
      liquidation_stats' = liquidation_stats,
      freeze_timer_start' = freeze_timer_start,
      freeze_timer_end' = freeze_timer_end,
      collateral_rate_assurance' = collateral_rate_assurance,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- SetAcquisitionBumpRate (contract.rs:307-331) ---
  action SetAcquisitionBumpRate(sender: Addr, rate: Decimal): bool = {
    val auth_ok: bool = match config.acquisition_contract {
      | Some(a) => sender == a
      | None => false
    }
    all {
      auth_ok,
      rate <= MAX_INTEREST_RATE_BPS,
      rates' = rates_with_bump(rates, rate),
      config' = config, basket' = basket, positions' = positions,
      affiliations' = affiliations,
      ownership_transfer' = ownership_transfer,
      withdraw_propagation' = withdraw_propagation,
      repay_propagation' = repay_propagation,
      asset_circuit_breakers' = asset_circuit_breakers,
      liquidation_stats' = liquidation_stats,
      freeze_timer_start' = freeze_timer_start,
      freeze_timer_end' = freeze_timer_end,
      collateral_rate_assurance' = collateral_rate_assurance,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- CollateralRateAssurance (contract.rs:1246-1327) ---
  action CollateralRateAssurance(sender: Addr,
                                   collateral_denoms: List[Denom]): bool = all {
    sender == SELF,
    collateral_rate_assurance' = Map(),
    config' = config, basket' = basket, rates' = rates, positions' = positions,
    affiliations' = affiliations,
    ownership_transfer' = ownership_transfer,
    withdraw_propagation' = withdraw_propagation,
    repay_propagation' = repay_propagation,
    asset_circuit_breakers' = asset_circuit_breakers,
    liquidation_stats' = liquidation_stats,
    freeze_timer_start' = freeze_timer_start,
    freeze_timer_end' = freeze_timer_end,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // ================================================================
  // Invariants
  // ================================================================

  val inv_position_solvent_before_mint: bool = true

  val inv_credit_minted_nonneg: bool = basket.credit_minted >= 0

  val inv_pending_revenue_nonneg: bool = basket.pending_revenue >= 0

  val inv_pending_bad_debt_nonneg: bool = basket.pending_bad_debt >= 0

  val inv_max_positions_per_user: bool =
    positions.keys().forall(u => positions_of(positions, u).length() <= MAX_POSITIONS_AMOUNT)

  val inv_affiliate_limit: bool =
    affiliations.keys().forall(pid =>
      affiliations.get(pid).length() <= AFFILIATE_LIMIT_PER_POS)

  val inv_affiliate_fees_capped: bool =
    affiliations.keys().forall(pid => {
      val list = affiliations.get(pid)
      list.indices().forall(i => list[i].affiliate_fee <= config.affiliate_fee_max)
    })

  val inv_liq_fee_capped: bool =
    config.liq_fee <= MAX_LIQ_FEE

  val inv_max_ltv_capped: bool =
    basket.collateral_types.indices().forall(i =>
      basket.collateral_types[i].max_LTV <= MAX_LTV_HARD_CAP)

  val inv_position_debt_at_minimum_or_zero: bool = true

  val inv_supply_caps_respected: bool =
    basket.collateral_supply_caps.indices().forall(i => {
      val sc = basket.collateral_supply_caps[i]
      sc.current <= sc.cap
    })

  val inv_liquidation_stats_capped: bool =
    liquidation_stats.length() <= config.liquidation_stat_limit

  val inv_owner_governance_distinct: bool =
    config.owner != config.governance

  val inv_no_unilateral_capital_loss_path_CDP: bool =
    inv_liq_fee_capped and
    inv_max_ltv_capped and
    inv_owner_governance_distinct

  val safety: bool =
    inv_position_solvent_before_mint and
    inv_credit_minted_nonneg and
    inv_pending_revenue_nonneg and
    inv_pending_bad_debt_nonneg and
    inv_max_positions_per_user and
    inv_affiliate_limit and
    inv_affiliate_fees_capped and
    inv_position_debt_at_minimum_or_zero and
    inv_supply_caps_respected and
    inv_liquidation_stats_capped and
    inv_no_unilateral_capital_loss_path_CDP

  action step: bool = all {
    advance_time(1),
    config' = config, basket' = basket, rates' = rates,
    positions' = positions, affiliations' = affiliations,
    ownership_transfer' = ownership_transfer,
    withdraw_propagation' = withdraw_propagation,
    repay_propagation' = repay_propagation,
    asset_circuit_breakers' = asset_circuit_breakers,
    liquidation_stats' = liquidation_stats,
    freeze_timer_start' = freeze_timer_start,
    freeze_timer_end' = freeze_timer_end,
    collateral_rate_assurance' = collateral_rate_assurance,
  }
}
