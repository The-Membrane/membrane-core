// specs/quint/organs/auction.qnt
//
// Auction — fee auctions (decay-discount swap to fee buyers) and MBRN
// sales for bad debt (pulls MBRN from LTV Disco, settles via debt
// contract). Debt auctions per se are deprecated.
//
// Source of truth (Rust):
//   contracts/auction/src/state.rs            (5 storage items)
//   contracts/auction/src/contracts.rs        (handlers; line cites below)
//   contracts/auction/src/error.rs            (3 error variants)
//   packages/membrane/src/auction.rs          (Config, ExecuteMsg, types)
//   packages/membrane/src/types.rs:927-975    (DebtAuction, FeeAuction, MBRNSale)
//
// The DebtAuction Item is preserved in storage but unreachable; SwapForMBRN
// reverts at line 729. New bad-debt resolution flows through MBRNSale +
// FulfillBadDebt + BuySuppliedMBRN.
module auction {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  // ================================================================
  // Constants — contracts.rs validation bounds
  // ================================================================
  pure val MIN_DISCOUNT_PCT:        Decimal = 10000000000000000
  pure val MAX_DISCOUNT_PCT:        Decimal = 100000000000000000
  pure val MIN_DISCOUNT_INCREASE:   Decimal = 10000000000000000
  pure val MAX_DISCOUNT_INCREASE:   Decimal = 50000000000000000
  pure val MIN_TIMEFRAME_S:         int = 10
  pure val MAX_TIMEFRAME_S:         int = 300
  pure val MIN_TWAP_TIMEFRAME_M:    int = 60
  pure val MAX_TWAP_TIMEFRAME_M:    int = 480
  pure val DEFAULT_DELAY_WINDOW_M:  int = 60
  pure val MAX_BUY_RECURSION_DEPTH: int = 3

  // ================================================================
  // Types
  // ================================================================
  type AuctionRecipient = {
    address: Addr,
    ratio: Decimal,
  }

  type FeeAuction = {
    auction_asset_denom: Denom,
    auction_asset_amount: Uint128,
    auction_start_time: Timestamp,
    per_asset_distribution: Option[List[(Denom, Uint128)]],
  }

  type AssetBadDebtAllocation = {
    asset: Denom,
    mbrn_allocated: Uint128,
    mbrn_used: Uint128,
    cdt_bad_debt: Uint128,
    cdt_fulfilled: Uint128,
  }

  type MBRNSale = {
    max_cdt: Uint128,
    cdt_fulfilled: Uint128,
    auction_start_time: Timestamp,
    supplier: Addr,
    asset_allocations: List[AssetBadDebtAllocation],
  }

  type Config = {
    owner: Addr,
    governance: Addr,
    oracle_contract: Addr,
    osmosis_proxy: Addr,
    mbrn_denom: Denom,
    cdt_denom: Denom,
    desired_asset: Denom,
    positions_contract: Addr,
    governance_contract: Addr,
    staking_contract: Addr,
    twap_timeframe_m: int,
    initial_discount: Decimal,
    discount_increase_timeframe_s: int,
    discount_increase: Decimal,
    send_to_stakers: bool,
    delay_window_m: int,
    revenue_distributor_contract: Option[Addr],
    ltv_disco_contract: Option[Addr],
    debt_contract: Option[Addr],
  }

  // ================================================================
  // State
  // ================================================================
  var config:               Config
  var fee_auctions:         Denom -> FeeAuction
  var mbrn_sale:            Option[MBRNSale]
  var ownership_transfer:   Option[Addr]

  action init(cfg: Config): bool = all {
    cfg.initial_discount >= MIN_DISCOUNT_PCT and
            cfg.initial_discount <= MAX_DISCOUNT_PCT,
    cfg.discount_increase >= MIN_DISCOUNT_INCREASE and
            cfg.discount_increase <= MAX_DISCOUNT_INCREASE,
    cfg.discount_increase_timeframe_s >= MIN_TIMEFRAME_S and
            cfg.discount_increase_timeframe_s <= MAX_TIMEFRAME_S,
    cfg.twap_timeframe_m >= MIN_TWAP_TIMEFRAME_M and
            cfg.twap_timeframe_m <= MAX_TWAP_TIMEFRAME_M,
    config' = cfg,
    fee_auctions' = Map(),
    mbrn_sale' = None,
    ownership_transfer' = None,
    block_time' = 0,
    block_height' = 0,
    balances' = Map(),
    msg_queue' = List(),
    pending_reply' = None,
  }

  // ================================================================
  // Helpers
  // ================================================================
  pure def get_discount_ratio(auction_start: Timestamp,
                               cfg: Config,
                               now: Timestamp): Decimal = {
    val delay_s: int = cfg.delay_window_m * 60
    val time_post_delay: int = max_int(now - auction_start - delay_s, 0)
    val ticks: int = time_post_delay / cfg.discount_increase_timeframe_s
    val total_discount: Decimal =
      cfg.initial_discount + decimal_mul_uint(cfg.discount_increase, ticks)
    if (total_discount > 990000000000000000) 990000000000000000
    else total_discount
  }

  pure val UNREACHABLE_SALE: MBRNSale =
    { max_cdt: 0, cdt_fulfilled: 0, auction_start_time: 0,
      supplier: SELF, asset_allocations: List() }

  // foldl-based linear scan over allocations: returns first matching index, -1 if none.
  pure def find_alloc_idx(allocs: List[AssetBadDebtAllocation], asset: Denom): int =
    allocs.foldl((-1, 0), (acc, a) => {
      val cur = acc._1
      val pos = acc._2
      if (cur >= 0) (cur, pos + 1)
      else if (a.asset == asset) (pos, pos + 1)
      else (-1, pos + 1)
    })._1

  pure def remove_alloc_at(allocs: List[AssetBadDebtAllocation], idx: int): List[AssetBadDebtAllocation] =
    allocs.foldl((List(), 0), (acc, a) => {
      val pos = acc._2
      if (pos == idx) (acc._1, pos + 1)
      else (acc._1.append(a), pos + 1)
    })._1

  pure def replace_alloc_at(allocs: List[AssetBadDebtAllocation], idx: int,
                             new_alloc: AssetBadDebtAllocation): List[AssetBadDebtAllocation] =
    allocs.foldl((List(), 0), (acc, a) => {
      val pos = acc._2
      if (pos == idx) (acc._1.append(new_alloc), pos + 1)
      else (acc._1.append(a), pos + 1)
    })._1

  pure def filter_active_allocs(allocs: List[AssetBadDebtAllocation]): List[AssetBadDebtAllocation] =
    allocs.foldl(List(), (acc, a) =>
      if (a.cdt_fulfilled < a.cdt_bad_debt and a.mbrn_used < a.mbrn_allocated)
        acc.append(a) else acc)

  // ================================================================
  // Execute actions
  // ================================================================

  // --- UpdateConfig (contracts.rs:111-229) ---
  action UpdateConfig(sender: Addr, new_cfg: Config): bool = all {
    sender == config.owner or
            (ownership_transfer != None and Some(sender) == ownership_transfer),
    new_cfg.initial_discount >= MIN_DISCOUNT_PCT and
            new_cfg.initial_discount <= MAX_DISCOUNT_PCT,
    new_cfg.discount_increase >= MIN_DISCOUNT_INCREASE and
            new_cfg.discount_increase <= MAX_DISCOUNT_INCREASE,
    new_cfg.discount_increase_timeframe_s >= MIN_TIMEFRAME_S and
            new_cfg.discount_increase_timeframe_s <= MAX_TIMEFRAME_S,
    new_cfg.twap_timeframe_m >= MIN_TWAP_TIMEFRAME_M and
            new_cfg.twap_timeframe_m <= MAX_TWAP_TIMEFRAME_M,
    config' = new_cfg,
    fee_auctions' = fee_auctions,
    mbrn_sale' = mbrn_sale,
    ownership_transfer' = ownership_transfer,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- StartAuction (contracts.rs:234-307) ---
  action StartAuction(sender: Addr,
                       auction_asset_denom: Denom,
                       auction_asset_amount: Uint128,
                       per_asset_distribution: Option[List[(Denom, Uint128)]]): bool = {
    val has_existing = fee_auctions.keys().contains(auction_asset_denom)
    val existing: FeeAuction = if (has_existing)
        fee_auctions.get(auction_asset_denom)
      else
        { auction_asset_denom: auction_asset_denom,
          auction_asset_amount: 0, auction_start_time: block_time,
          per_asset_distribution: None }
    val merged: FeeAuction = if (has_existing)
        { auction_asset_denom: existing.auction_asset_denom,
          auction_asset_amount: existing.auction_asset_amount + auction_asset_amount,
          auction_start_time: existing.auction_start_time,
          per_asset_distribution: per_asset_distribution }
      else
        { auction_asset_denom: auction_asset_denom,
          auction_asset_amount: auction_asset_amount,
          auction_start_time: block_time,
          per_asset_distribution: per_asset_distribution }
    all {
      sender == config.owner or
              sender == config.positions_contract or
              sender == config.staking_contract,
      auction_asset_denom != config.cdt_denom,
      fee_auctions' = fee_auctions.put(auction_asset_denom, merged),
      config' = config, mbrn_sale' = mbrn_sale,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- SwapForFee (contracts.rs:437-629) ---
  action SwapForFee(sender: Addr,
                     auction_asset_denom: Denom,
                     funds_amount: Uint128): bool = {
    val has_auction = fee_auctions.keys().contains(auction_asset_denom)
    val auction: FeeAuction = if (has_auction)
        fee_auctions.get(auction_asset_denom)
      else
        { auction_asset_denom: auction_asset_denom,
          auction_asset_amount: 0, auction_start_time: 0,
          per_asset_distribution: None }
    val discount: Decimal = get_discount_ratio(auction.auction_start_time, config, block_time)
    val effective_payout_factor: Decimal = sat_sub(DECIMAL_ONE, discount)
    val payout: Uint128 = min_int(auction.auction_asset_amount,
                                    decimal_mul_uint(effective_payout_factor, funds_amount))
    val remaining: Uint128 = sat_sub(auction.auction_asset_amount, payout)
    val updated_auction: FeeAuction = {
      auction_asset_denom: auction.auction_asset_denom,
      auction_asset_amount: remaining,
      auction_start_time: auction.auction_start_time,
      per_asset_distribution: auction.per_asset_distribution,
    }
    val new_fee_auctions: Denom -> FeeAuction = if (remaining <= 1)
        fee_auctions.keys().exclude(Set(auction_asset_denom))
          .mapBy(k => fee_auctions.get(k))
      else
        fee_auctions.put(auction_asset_denom, updated_auction)
    all {
      has_auction,
      fee_auctions' = new_fee_auctions,
      msg_queue' = msg_queue
        .append(Msg_BankSend({ to: sender, denom: auction_asset_denom, amount: payout }))
        .append(Msg_Reply({ id: 0, data: "auction_proceeds_routing" })),
      config' = config, mbrn_sale' = mbrn_sale,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- StartMBRNSale (contracts.rs:855-932) ---
  action StartMBRNSale(sender: Addr,
                        max_cdt: Uint128,
                        asset: Denom,
                        mbrn_allocation: Uint128): bool = {
    val auth_ok: bool = match config.ltv_disco_contract {
      | Some(a) => sender == a
      | None => false
    }
    val new_alloc: AssetBadDebtAllocation = {
      asset: asset, mbrn_allocated: mbrn_allocation, mbrn_used: 0,
      cdt_bad_debt: max_cdt, cdt_fulfilled: 0,
    }
    val sale_new: MBRNSale = match mbrn_sale {
      | Some(s) => { max_cdt: s.max_cdt + max_cdt,
                    cdt_fulfilled: s.cdt_fulfilled,
                    auction_start_time: s.auction_start_time,
                    supplier: s.supplier,
                    asset_allocations: s.asset_allocations.append(new_alloc) }
      | None => { max_cdt: max_cdt, cdt_fulfilled: 0,
                 auction_start_time: block_time,
                 supplier: sender, asset_allocations: List(new_alloc) }
    }
    all {
      auth_ok,
      mbrn_sale' = Some(sale_new),
      config' = config, fee_auctions' = fee_auctions,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- FulfillBadDebt (contracts.rs:938-1034) ---
  action FulfillBadDebt(sender: Addr, asset: Denom, sent_cdt: Uint128): bool = {
    val sale: MBRNSale = match mbrn_sale {
      | Some(s) => s
      | None => UNREACHABLE_SALE
    }
    val idx = find_alloc_idx(sale.asset_allocations, asset)
    val alloc: AssetBadDebtAllocation = if (idx >= 0)
        sale.asset_allocations[idx]
      else
        { asset: asset, mbrn_allocated: 0, mbrn_used: 0,
          cdt_bad_debt: 0, cdt_fulfilled: 0 }
    val applied: Uint128 = min_int(sent_cdt, sat_sub(alloc.cdt_bad_debt, alloc.cdt_fulfilled))
    val refund: Uint128 = sent_cdt - applied
    val new_alloc: AssetBadDebtAllocation = {
      asset: alloc.asset,
      mbrn_allocated: alloc.mbrn_allocated,
      mbrn_used: alloc.mbrn_used,
      cdt_bad_debt: alloc.cdt_bad_debt,
      cdt_fulfilled: alloc.cdt_fulfilled + applied,
    }
    val asset_done: bool = new_alloc.cdt_fulfilled >= new_alloc.cdt_bad_debt
    val updated_allocs: List[AssetBadDebtAllocation] =
      if (asset_done) remove_alloc_at(sale.asset_allocations, idx)
      else replace_alloc_at(sale.asset_allocations, idx, new_alloc)
    val sale_new: MBRNSale = {
      max_cdt: sale.max_cdt,
      cdt_fulfilled: sale.cdt_fulfilled + applied,
      auction_start_time: sale.auction_start_time,
      supplier: sale.supplier,
      asset_allocations: updated_allocs,
    }
    val notify_debt_msg: CrossOrganMsg = match config.debt_contract {
      | Some(_) => Msg_CDP_FulfillBadDebt({ asset: "cdt", amount: applied })
      | None => Msg_Reply({ id: 0, data: "no_debt_contract" })
    }
    val freeze_clear_msg: CrossOrganMsg = if (asset_done)
        Msg_Disco_ClearBadDebtFreeze({ asset: asset, amount: alloc.cdt_bad_debt })
      else Msg_Reply({ id: 0, data: "noop" })
    val refund_msg: CrossOrganMsg = if (refund > 0)
        Msg_BankSend({ to: sender, denom: config.cdt_denom, amount: refund })
      else Msg_Reply({ id: 0, data: "noop" })
    all {
      mbrn_sale != None,
      idx >= 0,
      mbrn_sale' = if (sale_new.asset_allocations.length() == 0) None
                   else Some(sale_new),
      msg_queue' = msg_queue
        .append(notify_debt_msg)
        .append(freeze_clear_msg)
        .append(refund_msg),
      config' = config, fee_auctions' = fee_auctions,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- BuySuppliedMBRN (contracts.rs:1043-1237) ---
  action BuySuppliedMBRN(sender: Addr,
                          depth: Option[int],
                          buyer_override: Option[Addr],
                          sent_cdt: Uint128): bool = {
    val current_depth: int = match depth {
      | Some(d) => d
      | None => 0
    }
    val recursive_call: bool = match buyer_override {
      | Some(_) => true
      | None => false
    }
    val buyer: Addr = match buyer_override {
      | Some(a) => a
      | None => sender
    }
    val sale: MBRNSale = match mbrn_sale {
      | Some(s) => s
      | None => UNREACHABLE_SALE
    }
    val active_allocs: List[AssetBadDebtAllocation] = filter_active_allocs(sale.asset_allocations)
    val first_active_asset: Denom = if (active_allocs.length() > 0) active_allocs[0].asset else ""
    all {
      mbrn_sale != None,
      current_depth <= MAX_BUY_RECURSION_DEPTH,
      if (recursive_call) sender == SELF else true,
      active_allocs.length() > 0,
      msg_queue' = msg_queue
        .append(Msg_Disco_SendMBRNForSale({ asset: first_active_asset,
                                            amount: 0, buyer: buyer })),
      mbrn_sale' = mbrn_sale,
      config' = config, fee_auctions' = fee_auctions,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- RemoveAuction (contracts.rs:398-417) ---
  // Rust DEBT_AUCTION.remove(deps.storage); -> mbrn_sale := None.
  action RemoveAuction(sender: Addr): bool = all {
    sender == config.owner or sender == config.positions_contract,
    mbrn_sale' = None,
    config' = config, fee_auctions' = fee_auctions,
    ownership_transfer' = ownership_transfer,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // ================================================================
  // Invariants
  // ================================================================

  val inv_discount_initial_bounded: bool =
    config.initial_discount >= MIN_DISCOUNT_PCT and
    config.initial_discount <= MAX_DISCOUNT_PCT

  val inv_discount_increase_bounded: bool =
    config.discount_increase >= MIN_DISCOUNT_INCREASE and
    config.discount_increase <= MAX_DISCOUNT_INCREASE

  val inv_timeframe_bounded: bool =
    config.discount_increase_timeframe_s >= MIN_TIMEFRAME_S and
    config.discount_increase_timeframe_s <= MAX_TIMEFRAME_S

  val inv_twap_bounded: bool =
    config.twap_timeframe_m >= MIN_TWAP_TIMEFRAME_M and
    config.twap_timeframe_m <= MAX_TWAP_TIMEFRAME_M

  val inv_no_cdt_in_fee_auctions: bool =
    not(fee_auctions.keys().contains(config.cdt_denom))

  val inv_mbrn_sale_allocations_consistent: bool =
    match mbrn_sale {
      | Some(s) => s.asset_allocations.indices().forall(i => {
        val a = s.asset_allocations[i]
        a.cdt_fulfilled <= a.cdt_bad_debt and
        a.mbrn_used <= a.mbrn_allocated
      })
      | None => true
    }

  val inv_mbrn_sale_summed_consistent: bool =
    match mbrn_sale {
      | Some(s) => s.cdt_fulfilled <= s.max_cdt
      | None => true
    }

  val inv_owner_governance_distinct: bool =
    config.owner != config.governance

  val inv_no_unilateral_capital_loss_path_AU: bool =
    inv_discount_initial_bounded and
    inv_discount_increase_bounded and
    inv_timeframe_bounded and
    inv_no_cdt_in_fee_auctions and
    inv_owner_governance_distinct

  val safety: bool =
    inv_discount_initial_bounded and
    inv_discount_increase_bounded and
    inv_timeframe_bounded and
    inv_twap_bounded and
    inv_no_cdt_in_fee_auctions and
    inv_mbrn_sale_allocations_consistent and
    inv_mbrn_sale_summed_consistent and
    inv_no_unilateral_capital_loss_path_AU

  action step: bool = all {
    advance_time(1),
    config' = config,
    fee_auctions' = fee_auctions,
    mbrn_sale' = mbrn_sale,
    ownership_transfer' = ownership_transfer,
  }
}
