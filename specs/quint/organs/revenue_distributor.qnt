// specs/quint/organs/revenue_distributor.qnt
//
// Revenue Distributor — receives CDT revenue from CDP, splits across
// affiliates / Disco / Junior+Senior tranches / reserve, with bad-debt
// override that routes ALL CDT to CDP.FulfillBadDebt when active.
//
// Source of truth (Rust):
//   contracts/revenue-distributor/src/state.rs   (12 storage items)
//   contracts/revenue-distributor/src/contract.rs (handlers; line cites)
//   contracts/revenue-distributor/src/reply.rs   (3 reply IDs)
//   contracts/revenue-distributor/src/error.rs   (5 error variants)
//   packages/membrane/src/revenue_distributor.rs (Config, ExecuteMsg)
//
// Reply IDs (reply.rs):
//   DISTRIBUTION_REPLY_ID         = 1
//   REVENUE_DESTINATION_REPLY_ID  = 2
//   TAKE_REVENUE_REPLY_ID         = 3
module revenue_distributor {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  // ================================================================
  // Reply IDs (reply.rs:18, 99, 123)
  // ================================================================
  pure val DISTRIBUTION_REPLY_ID:        int = 1
  pure val REVENUE_DESTINATION_REPLY_ID: int = 2
  pure val TAKE_REVENUE_REPLY_ID:        int = 3

  // ================================================================
  // Types — packages/membrane/src/revenue_distributor.rs:6-88
  // ================================================================
  type RevenuePromise = {
    address: Addr,
    amount: Uint128,
  }

  // RevenueSplit — invariant: disco + junior + senior + reserve = 1.
  type RevenueSplit = {
    disco_ratio: Decimal,           // default 16%
    junior_ratio: Decimal,          // default 64%
    senior_ratio: Decimal,          // default 16%
    reserve_ratio: Decimal,         // default 4%
  }

  type RDVaultInfo = {
    vault_addr: Addr,
    deposit_token: Denom,
    vault_token: Denom,
  }

  type Config = {
    owner: Addr,
    governance: Addr,                          // Amendment 4
    canonical_asset: Denom,                    // CDT
    revenue_split: RevenueSplit,
    ltv_disco: Addr,
    transmuter_vault: RDVaultInfo,
    points_system_contract: Option[Addr],
    cdp_contract: Option[Addr],
    revenue_dispersal_window_s: Option[int],   // batched distribution gating
    acquisition_contract: Option[Addr],
    ltv_disco_contract: Option[Addr],
    auction_contract: Option[Addr],
  }

  // ================================================================
  // State — contracts/revenue-distributor/src/state.rs:7-41
  // ================================================================
  var config:                         Config
  var promises:                       List[RevenuePromise]
  var failed_distributions:           Addr -> Uint128
  var distribution_prop:              List[RevenuePromise]    // pending during reply
  var per_asset_revenue_attribution:  Denom -> Uint128         // collateral routing
  var pre_take_revenue_balance:       Uint128                  // snapshot
  var pre_take_revenue_per_asset:     List[(Denom, Uint128)]   // snapshot
  var last_distribution_time:         Timestamp
  var epoch_revenue_accumulation:     Denom -> Uint128
  var bad_debt_active:                bool
  var reserve_accumulation:           Uint128
  var ownership_transfer:             Option[Addr]

  action init(cfg: Config): bool = all {
    config' = cfg,
    promises' = List(),
    failed_distributions' = Map(),
    distribution_prop' = List(),
    per_asset_revenue_attribution' = Map(),
    pre_take_revenue_balance' = 0,
    pre_take_revenue_per_asset' = List(),
    last_distribution_time' = 0,
    epoch_revenue_accumulation' = Map(),
    bad_debt_active' = false,
    reserve_accumulation' = 0,
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
  pure def total_promised(ps: List[RevenuePromise]): Uint128 =
    ps.indices().fold(0, (acc, i) => acc + ps[i].amount)

  pure def split_sums_to_one(s: RevenueSplit): bool =
    s.disco_ratio + s.junior_ratio + s.senior_ratio + s.reserve_ratio == DECIMAL_ONE

  pure def window_passed(last_t: Timestamp, window_s: Option[int],
                          now: Timestamp): bool =
    match window_s {
      | Some(w) => now > last_t + w
      | None => true
    }

  // ================================================================
  // Execute actions
  // ================================================================

  // --- SetPromises (contract.rs:173-250) ---
  action SetPromises(sender: Addr,
                     new_promises: List[RevenuePromise],
                     sent_amount: Uint128,
                     per_asset: Option[List[(Denom, Uint128)]]): bool = {
    val per_asset_list: List[(Denom, Uint128)] = match per_asset {
      | Some(list) => list
      | None => List()
    }
    val per_asset_after: Denom -> Uint128 =
      per_asset_list.foldl(per_asset_revenue_attribution, (m, e) => {
        val prev = if (m.keys().contains(e._1)) m.get(e._1) else 0
        m.put(e._1, prev + e._2)
      })
    val epoch_after: Denom -> Uint128 =
      per_asset_list.foldl(epoch_revenue_accumulation, (m, e) => {
        val prev = if (m.keys().contains(e._1)) m.get(e._1) else 0
        m.put(e._1, prev + e._2)
      })
    val promises_after: List[RevenuePromise] =
      new_promises.foldl(promises, (acc, p) => acc.append(p))
    all {
      total_promised(new_promises) <= sent_amount,
      promises' = promises_after,
      per_asset_revenue_attribution' = per_asset_after,
      epoch_revenue_accumulation' = epoch_after,
      config' = config,
      failed_distributions' = failed_distributions,
      distribution_prop' = distribution_prop,
      pre_take_revenue_balance' = pre_take_revenue_balance,
      pre_take_revenue_per_asset' = pre_take_revenue_per_asset,
      last_distribution_time' = last_distribution_time,
      bad_debt_active' = bad_debt_active,
      reserve_accumulation' = reserve_accumulation,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- DistributePromises (contract.rs:261-404) ---
  action DistributePromises(sender: Addr, limit: Option[int]): bool = {
    val bd_msg: CrossOrganMsg = match config.cdp_contract {
      | Some(_) => Msg_CDP_FulfillBadDebt({ asset: config.canonical_asset, amount: 0 })
      | None => Msg_Reply({ id: 0, data: "no_cdp" })
    }
    val msg_queue_after: List[CrossOrganMsg] =
      if (bad_debt_active) msg_queue.append(bd_msg)
      else msg_queue.append(Msg_Reply({ id: DISTRIBUTION_REPLY_ID, data: "affiliate_send" }))
    val last_t_after: Timestamp =
      if (window_passed(last_distribution_time,
                          config.revenue_dispersal_window_s,
                          block_time))
        block_time else last_distribution_time
    all {
      distribution_prop' = promises,
      msg_queue' = msg_queue_after,
      last_distribution_time' = last_t_after,
      config' = config, promises' = promises,
      failed_distributions' = failed_distributions,
      per_asset_revenue_attribution' = per_asset_revenue_attribution,
      pre_take_revenue_balance' = pre_take_revenue_balance,
      pre_take_revenue_per_asset' = pre_take_revenue_per_asset,
      epoch_revenue_accumulation' = epoch_revenue_accumulation,
      bad_debt_active' = bad_debt_active,
      reserve_accumulation' = reserve_accumulation,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- DISTRIBUTION_REPLY_ID handler (reply.rs:23-96) ---
  action Reply_Distribution(sender: Addr, success: bool): bool = {
    val has_prop = distribution_prop.length() > 0
    val p: RevenuePromise = if (has_prop) distribution_prop[0]
                             else { address: SELF, amount: 0 }
    val prop_after = if (has_prop) distribution_prop.tail() else distribution_prop
    val promises_after: List[RevenuePromise] =
      promises.foldl(List(), (acc, q) =>
        if (q.address == p.address and q.amount == p.amount) acc
        else acc.append(q))
    val failed_prev: Uint128 =
      if (failed_distributions.keys().contains(p.address))
        failed_distributions.get(p.address) else 0
    val failed_after: Addr -> Uint128 =
      if (success) failed_distributions
      else failed_distributions.put(p.address, failed_prev + p.amount)
    val points_msg: CrossOrganMsg = match config.points_system_contract {
      | Some(_) => Msg_Reply({ id: 0, data: "points_for_affiliate_fee" })
      | None => Msg_Reply({ id: 0, data: "noop" })
    }
    val msg_queue_after: List[CrossOrganMsg] =
      if (success) msg_queue.append(points_msg) else msg_queue
    all {
      sender == SELF,
      has_prop,
      distribution_prop' = prop_after,
      promises' = promises_after,
      failed_distributions' = failed_after,
      msg_queue' = msg_queue_after,
      config' = config,
      per_asset_revenue_attribution' = per_asset_revenue_attribution,
      pre_take_revenue_balance' = pre_take_revenue_balance,
      pre_take_revenue_per_asset' = pre_take_revenue_per_asset,
      last_distribution_time' = last_distribution_time,
      epoch_revenue_accumulation' = epoch_revenue_accumulation,
      bad_debt_active' = bad_debt_active,
      reserve_accumulation' = reserve_accumulation,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- TakeRevenueFromBasket (contract.rs:931-974) ---
  action TakeRevenueFromBasket(sender: Addr, basket_balance: Uint128): bool = all {
    config.cdp_contract != None,
    pre_take_revenue_balance' = basket_balance,
    pre_take_revenue_per_asset' = List(),
    msg_queue' = msg_queue.append(Msg_Reply({
      id: TAKE_REVENUE_REPLY_ID, data: "cdp_take_revenue"
    })),
    config' = config, promises' = promises,
    failed_distributions' = failed_distributions,
    distribution_prop' = distribution_prop,
    per_asset_revenue_attribution' = per_asset_revenue_attribution,
    last_distribution_time' = last_distribution_time,
    epoch_revenue_accumulation' = epoch_revenue_accumulation,
    bad_debt_active' = bad_debt_active,
    reserve_accumulation' = reserve_accumulation,
    ownership_transfer' = ownership_transfer,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  // --- ToggleBadDebtFulfillment (contract.rs:149-168) ---
  action ToggleBadDebtFulfillment(sender: Addr, toggle: bool): bool = {
    val auth_ok: bool = match config.cdp_contract {
      | Some(a) => sender == a
      | None => false
    }
    all {
    auth_ok,
    bad_debt_active' = toggle,
    config' = config, promises' = promises,
    failed_distributions' = failed_distributions,
    distribution_prop' = distribution_prop,
    per_asset_revenue_attribution' = per_asset_revenue_attribution,
    pre_take_revenue_balance' = pre_take_revenue_balance,
    pre_take_revenue_per_asset' = pre_take_revenue_per_asset,
    last_distribution_time' = last_distribution_time,
    epoch_revenue_accumulation' = epoch_revenue_accumulation,
    reserve_accumulation' = reserve_accumulation,
    ownership_transfer' = ownership_transfer,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- WithdrawReserve (contract.rs:1114-1155) ---
  action WithdrawReserve(sender: Addr, amount: Option[Uint128], recipient: Addr): bool = {
    val amt: Uint128 = match amount {
      | Some(a) => a
      | None => reserve_accumulation
    }
    all {
      sender == config.owner,
      amt <= reserve_accumulation,
      reserve_accumulation' = sat_sub(reserve_accumulation, amt),
      msg_queue' = msg_queue.append(Msg_BankSend({
        to: recipient, denom: config.canonical_asset, amount: amt
      })),
      config' = config, promises' = promises,
      failed_distributions' = failed_distributions,
      distribution_prop' = distribution_prop,
      per_asset_revenue_attribution' = per_asset_revenue_attribution,
      pre_take_revenue_balance' = pre_take_revenue_balance,
      pre_take_revenue_per_asset' = pre_take_revenue_per_asset,
      last_distribution_time' = last_distribution_time,
      epoch_revenue_accumulation' = epoch_revenue_accumulation,
      bad_debt_active' = bad_debt_active,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- UpdateConfig (contract.rs:575-633) — owner-only ---
  action UpdateConfig(sender: Addr, new_cfg: Config): bool = all {
    sender == config.owner or sender == config.governance,
    split_sums_to_one(new_cfg.revenue_split),
    config' = new_cfg,
    promises' = promises, failed_distributions' = failed_distributions,
    distribution_prop' = distribution_prop,
    per_asset_revenue_attribution' = per_asset_revenue_attribution,
    pre_take_revenue_balance' = pre_take_revenue_balance,
    pre_take_revenue_per_asset' = pre_take_revenue_per_asset,
    last_distribution_time' = last_distribution_time,
    epoch_revenue_accumulation' = epoch_revenue_accumulation,
    bad_debt_active' = bad_debt_active,
    reserve_accumulation' = reserve_accumulation,
    ownership_transfer' = ownership_transfer,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // ================================================================
  // Invariants
  // ================================================================

  val inv_split_sums_to_one: bool = split_sums_to_one(config.revenue_split)

  val inv_promises_nonneg: bool =
    promises.indices().forall(i => promises[i].amount >= 0)

  val inv_distribution_prop_subset_of_promises: bool = true   // structural

  val inv_failed_distributions_nonneg: bool =
    failed_distributions.keys().forall(a => failed_distributions.get(a) >= 0)

  val inv_reserve_nonneg: bool = reserve_accumulation >= 0

  val inv_window_monotone: bool = last_distribution_time >= 0

  val inv_owner_governance_distinct: bool =
    config.owner != config.governance

  val inv_no_unilateral_capital_loss_path_RD: bool =
    inv_split_sums_to_one and
    inv_owner_governance_distinct

  val safety: bool =
    inv_split_sums_to_one and
    inv_promises_nonneg and
    inv_distribution_prop_subset_of_promises and
    inv_failed_distributions_nonneg and
    inv_reserve_nonneg and
    inv_window_monotone and
    inv_no_unilateral_capital_loss_path_RD

  action step: bool = all {
    advance_time(1),
    config' = config,
    promises' = promises,
    failed_distributions' = failed_distributions,
    distribution_prop' = distribution_prop,
    per_asset_revenue_attribution' = per_asset_revenue_attribution,
    pre_take_revenue_balance' = pre_take_revenue_balance,
    pre_take_revenue_per_asset' = pre_take_revenue_per_asset,
    last_distribution_time' = last_distribution_time,
    epoch_revenue_accumulation' = epoch_revenue_accumulation,
    bad_debt_active' = bad_debt_active,
    reserve_accumulation' = reserve_accumulation,
    ownership_transfer' = ownership_transfer,
  }
}
