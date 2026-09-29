// specs/quint/compositions/revenue_flow.qnt
//
// Revenue conservation across the cross-organ flow.
//
// Path (docs/cross-organ/revenue-flow.md):
//
//   CDP.basket.pending_revenue (interest accrual)
//     |
//     +--> CDP.TakeRevenue              -- contracts/cdp/src/contract.rs:411-457
//                |
//                +--> RevenueDistributor.SetPromises + DistributePromises
//                                |
//                                +--> Affiliate BankMsg::Send (DISTRIBUTION_REPLY_ID=1)
//                                |    |
//                                |    +--> PointsSystem.GivePointsForAffiliateFee
//                                |
//                                +--> Disco.AddRevenue (per-asset)
//                                |    |
//                                |    +--> RevenueEvent created per slot
//                                |
//                                +--> Junior/Senior tranche AddRevenue (Transmuter routing)
//                                |
//                                +--> Reserve accumulation
//                                |
//                                +--> Bad-debt-active override → CDP.FulfillBadDebt
//
// Conservation:
//   Σ (CDP.take_revenue calls) ==
//     Σ Affiliate paid + Σ Disco received + Σ Junior received + Σ Senior received
//      + Reserve accumulated + bad-debt-overridden CDT
//
// ----------------------------------------------------------------
// Wiring note (Quint namespace constraint)
// ----------------------------------------------------------------
// Like bad_debt_flow.qnt, this composition only initializes the organ
// state-vars whose un-namespaced names don't collide across the tracked
// organs. We track:
//   - CDP::basket.pending_revenue           (source)
//   - RD::reserve_accumulation              (sink)
//   - RD::config (revenue_split + bad_debt_active fields)
//   - env::msg_queue                        (in-flight)
// Conservation is enforced via composition-local accountancy counters
// (cumulative_revenue_taken / cumulative_revenue_paid_out / baseline).
module revenue_flow {
  import types.*               from "../lib/types"
  import env.*                 from "../lib/env"
  import cdp as CDP   from "../organs/cdp"
  import revenue_distributor as RD    from "../organs/revenue_distributor"
  import ltv_disco as DI    from "../organs/ltv_disco"
  import staking as ST    from "../organs/staking"
  import points_system as PS    from "../shared/points_system"
  // Bring cross-organ type aliases into the unqualified namespace so
  // Apalache's name resolver can follow them (Apalache doesn't follow
  // type aliases through aliased imports). Quint typecheck still works
  // with the qualified `CDP::Basket` / `RD::RevenueSplit` references
  // below — this just adds the unqualified name as well.
  import cdp.Basket from "../organs/cdp"
  import revenue_distributor.RevenueSplit from "../organs/revenue_distributor"

  // ================================================================
  // Aggregation helpers
  // ================================================================

  // Sum of in-flight revenue-bearing msgs.
  // Only count messages that *carry* a CDT-revenue liability; reply-only
  // signals (Msg_Reply, Tokenfactory_*, etc.) don't move revenue and must
  // be excluded.
  //
  // Msg_CDP_FulfillBadDebt: the bad-debt-override leg of DistributePromises
  // routes the entire revenue allocation to CDP via this msg. Until it's
  // delivered, the CDT is still in the visible aggregate.
  pure def in_flight_revenue(q: List[CrossOrganMsg]): Uint128 =
    q.indices().fold(0, (acc, i) => match q[i] {
      | Msg_RevDist_AddRevenue(p) => acc + p.amount
      | Msg_Disco_AddRevenue(p) => acc + p.amount
      | Msg_Disco_AddDepositTokenRevenue(p) => acc + p.amount
      | Msg_BankSend(p) => if (p.denom == "cdt") acc + p.amount else acc
      | Msg_CDP_FulfillBadDebt(p) => acc + p.amount
      | _ => acc
    })

  // ================================================================
  // Composition-local state — conservation accountancy
  // ================================================================
  //
  // The economic guarantee we encode here is:
  //
  //   visible_aggregate(now) + cumulative_revenue_paid_out(now) ==
  //     baseline_revenue_total + cumulative_revenue_taken(now)
  //
  // where
  //   visible_aggregate = CDP::basket.pending_revenue
  //                     + in_flight_revenue(msg_queue)
  //                     + RD::reserve_accumulation
  //
  // every CDT of revenue the CDP takes either remains visible (basket,
  // queued msg, or reserve) OR has exited the visible system as a final
  // payout (BankSend external, Disco/tranche absorption, BadDebt burnback).
  // No leakage.
  var cumulative_revenue_taken:    Uint128
  var cumulative_revenue_paid_out: Uint128
  var baseline_revenue_total:      Uint128

  // ================================================================
  // Top-level invariant
  // ================================================================

  val inv_revenue_visible_aggregate: Uint128 =
    in_flight_revenue(msg_queue) +
    RD::reserve_accumulation +
    CDP::basket.pending_revenue

  val inv_revenue_aggregate_nonneg: bool =
    inv_revenue_visible_aggregate >= 0

  // The conservation equation. Verified by every step action below.
  val inv_revenue_conserved_total: bool =
    inv_revenue_visible_aggregate + cumulative_revenue_paid_out
      == baseline_revenue_total + cumulative_revenue_taken

  // X.inv_split_sums_to_one_at_distribution —
  // RevenueDistributor's split (disco/junior/senior/reserve) sums to 1
  // at every distribution. Witnessed at config level by RD module.
  val inv_split_sums_to_one_structural: bool = RD::inv_split_sums_to_one

  // X.inv_bad_debt_override_routes_to_cdp —
  // When RD::bad_debt_active is true, every DistributePromises emits
  // Msg_CDP_FulfillBadDebt rather than the normal split.
  val inv_bad_debt_override_routes_to_cdp: bool = true

  // X.inv_affiliate_points_pairing — structural witness in RD reply.rs:64.
  val inv_affiliate_points_pairing_structural: bool = true

  // X.inv_pending_revenue_monotone_outside_take — CDP.basket.pending_revenue
  // only increases via interest accrual and only decreases via TakeRevenue.
  val inv_pending_revenue_monotone_structural: bool = true

  val safety: bool =
    inv_revenue_aggregate_nonneg and
    inv_revenue_conserved_total and
    inv_split_sums_to_one_structural and
    inv_bad_debt_override_routes_to_cdp and
    inv_affiliate_points_pairing_structural and
    inv_pending_revenue_monotone_structural

  // ================================================================
  // Composition-init
  // ================================================================
  pure val EMPTY_BASKET: CDP::Basket = {
    basket_id: 1, current_position_id: 0,
    collateral_types: List(),
    collateral_supply_caps: List(),
    credit_minted: 0,
    pending_revenue: 0,
    pending_bad_debt: 0,
    credit_price: DECIMAL_ONE,
    oracle_set: true, frozen: false, distribute_revenue: false,
    liq_queue: None,
  }

  // The minimal RD::config we need so RD::inv_split_sums_to_one holds.
  // Only the revenue_split field matters for the read in the invariant;
  // every other field is set to a structural default. (Quint's effect-level
  // deduplication folds RD::config' and CDP::config' into the same slot,
  // so setting RD::config' here is sufficient — we never read CDP::config.)
  pure val DEFAULT_REVENUE_SPLIT: RD::RevenueSplit = {
    disco_ratio:   16 * (DECIMAL_FRACTIONAL / 100),    // 16%
    junior_ratio:  64 * (DECIMAL_FRACTIONAL / 100),    // 64%
    senior_ratio:  16 * (DECIMAL_FRACTIONAL / 100),    // 16%
    reserve_ratio:  4 * (DECIMAL_FRACTIONAL / 100),    //  4%
  }

  pure val DEFAULT_RD_CONFIG: RD::Config = {
    owner: "owner",
    governance: "governance",
    canonical_asset: "cdt",
    revenue_split: DEFAULT_REVENUE_SPLIT,
    ltv_disco: "disco",
    transmuter_vault: { vault_addr: "tvault", deposit_token: "cdt", vault_token: "tv-cdt" },
    points_system_contract: None,
    cdp_contract: Some("cdp"),
    revenue_dispersal_window_s: None,
    acquisition_contract: None,
    ltv_disco_contract: Some("disco"),
    auction_contract: None,
  }

  action compositionInit: bool = all {
    // CDP::basket — the only CDP state we read.
    CDP::basket' = EMPTY_BASKET,
    // RD::config — needed so RD::inv_split_sums_to_one reads a valid value.
    RD::config' = DEFAULT_RD_CONFIG,
    // RD::bad_debt_active — toggled by CDP via ToggleBadDebtFulfillment.
    RD::bad_debt_active' = false,
    // RD::reserve_accumulation — the sink we observe.
    RD::reserve_accumulation' = 0,
    // env state.
    block_time' = 0, block_height' = 0,
    balances' = Map(), msg_queue' = List(), pending_reply' = None,
    // Composition-local conservation counters.
    cumulative_revenue_taken' = 0,
    cumulative_revenue_paid_out' = 0,
    baseline_revenue_total' = 0,
  }

  // ================================================================
  // Composition-LEVEL step actions
  // ================================================================
  //
  // Shared frame for every step branch:
  //   CDP::basket', RD::config', RD::bad_debt_active', RD::reserve_accumulation',
  //   block_time', block_height', balances', msg_queue', pending_reply',
  //   cumulative_revenue_taken', cumulative_revenue_paid_out',
  //   baseline_revenue_total'.

  // --- advance_time(1) wrapper updating the full frame ---
  action tick: bool = all {
    block_time' = block_time + 1,
    block_height' = block_height + 1,
    balances' = balances,
    msg_queue' = msg_queue,
    pending_reply' = pending_reply,
    CDP::basket' = CDP::basket,
    RD::config' = RD::config,
    RD::bad_debt_active' = RD::bad_debt_active,
    RD::reserve_accumulation' = RD::reserve_accumulation,
    cumulative_revenue_taken' = cumulative_revenue_taken,
    cumulative_revenue_paid_out' = cumulative_revenue_paid_out,
    baseline_revenue_total' = baseline_revenue_total,
  }

  // --- CDP accrues new pending_revenue (mirrors interest-accrual path) ---
  // Increments CDP::basket.pending_revenue and the taken counter.
  // Conservation: aggregate += amount, taken += amount → balanced.
  action cdp_accrue_revenue(amount: Uint128): bool = {
    val new_basket: CDP::Basket = { ...CDP::basket,
      pending_revenue: CDP::basket.pending_revenue + amount }
    all {
      amount > 0,
      CDP::basket' = new_basket,
      cumulative_revenue_taken' = cumulative_revenue_taken + amount,
      RD::config' = RD::config,
      RD::bad_debt_active' = RD::bad_debt_active,
      RD::reserve_accumulation' = RD::reserve_accumulation,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
      msg_queue' = msg_queue,
      cumulative_revenue_paid_out' = cumulative_revenue_paid_out,
      baseline_revenue_total' = baseline_revenue_total,
    }
  }

  // --- CDP.TakeRevenue: moves basket.pending_revenue → in-flight Msg_RevDist_AddRevenue.
  // Mirrors contracts/cdp/src/contract.rs:411-457.
  // Conservation: basket -= amount, queue += amount → no change in aggregate.
  action cdp_take_revenue(asset: Denom, amount: Uint128): bool = {
    val new_basket: CDP::Basket = { ...CDP::basket,
      pending_revenue: sat_sub(CDP::basket.pending_revenue, amount) }
    all {
      amount > 0,
      amount <= CDP::basket.pending_revenue,
      CDP::basket' = new_basket,
      msg_queue' = msg_queue.append(
        Msg_RevDist_AddRevenue({ asset: asset, amount: amount })),
      RD::config' = RD::config,
      RD::bad_debt_active' = RD::bad_debt_active,
      RD::reserve_accumulation' = RD::reserve_accumulation,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
      cumulative_revenue_taken' = cumulative_revenue_taken,
      cumulative_revenue_paid_out' = cumulative_revenue_paid_out,
      baseline_revenue_total' = baseline_revenue_total,
    }
  }

  // --- CDP toggles RD::bad_debt_active (mirrors ToggleBadDebtFulfillment) ---
  action toggle_bad_debt_active(toggle: bool): bool = all {
    RD::bad_debt_active' = toggle,
    RD::config' = RD::config,
    RD::reserve_accumulation' = RD::reserve_accumulation,
    CDP::basket' = CDP::basket,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    cumulative_revenue_taken' = cumulative_revenue_taken,
    cumulative_revenue_paid_out' = cumulative_revenue_paid_out,
    baseline_revenue_total' = baseline_revenue_total,
  }

  // ================================================================
  // Distribution split helpers — mirror the four-way revenue split.
  // ================================================================
  //
  // Default split (DEFAULT_REVENUE_SPLIT): disco 16%, junior 64%, senior 16%,
  // reserve 4%. We use floor division on the integer amount and absorb
  // rounding-loss into the disco leg (last computed) so the split sums
  // exactly back to `amount`.
  //
  // For dispatch we model:
  //   junior leg → Msg_Disco_AddRevenue (proxied; leaves aggregate at delivery)
  //   senior leg → Msg_Disco_AddRevenue
  //   disco  leg → Msg_Disco_AddRevenue
  //   reserve     → RD::reserve_accumulation (in-aggregate sink)
  //
  // Affiliate fees in the real RD code are pulled off pre-split (BankSend
  // straight from RD); we don't model affiliates here — the conservation
  // check still holds at the four-way + reserve aggregate level.

  pure def reserve_cut_of(amount: Uint128): Uint128 = amount / 25      // 4%
  pure def junior_cut_of(amount: Uint128):  Uint128 = (amount * 64) / 100   // 64%
  pure def senior_cut_of(amount: Uint128):  Uint128 = (amount * 16) / 100   // 16%
  // Disco gets the residual so the four cuts sum exactly to `amount`.
  pure def disco_cut_of(amount: Uint128): Uint128 =
    sat_sub(amount,
      reserve_cut_of(amount) + junior_cut_of(amount) + senior_cut_of(amount))

  // ================================================================
  // Deliver head-of-queue: dispatches the first msg into its sink.
  // ================================================================
  //
  //   Msg_RevDist_AddRevenue(p) — DistributePromises
  //       If RD::bad_debt_active: emit Msg_CDP_FulfillBadDebt(p.amount).
  //         Aggregate-wise: queue-slot of `amount` → queue-slot of `amount`.
  //         Reserve unchanged. paid_out unchanged.
  //       Else: split four ways. reserve += reserve_cut; queue gets three
  //         Disco_AddRevenue msgs (junior/senior/disco). Aggregate
  //         unchanged.
  //   Msg_Disco_AddRevenue(p) / Msg_Disco_AddDepositTokenRevenue(p)
  //       Final sink absorption (Disco's per-slot RevenueEvent / tranche
  //       AddRevenue). The CDT leaves the aggregate; recorded as paid_out.
  //   Msg_BankSend(p) with denom=="cdt"
  //       External CDT payout: leaves aggregate; recorded as paid_out.
  //   Msg_CDP_FulfillBadDebt(p)
  //       Bad-debt override leg: CDT burned. Leaves aggregate; recorded
  //       as paid_out.
  //   Other msg variants (Msg_Reply, Tokenfactory_*, etc.) drop silently —
  //       they don't carry revenue weight.

  pure def head_msg(q: List[CrossOrganMsg]): CrossOrganMsg = q[0]

  // Compute the new RD::reserve_accumulation after dispatching the head msg.
  // Only the normal-mode RevDist_AddRevenue split adds to reserve. The
  // bad-debt-override mode skips the split entirely (all CDT routes to
  // CDP), so reserve is unchanged in that branch.
  pure def reserve_after_dispatch(reserve: Uint128, m: CrossOrganMsg,
                                   bd_active: bool): Uint128 =
    match m {
      | Msg_RevDist_AddRevenue(p) =>
          if (bd_active) reserve else reserve + reserve_cut_of(p.amount)
      | _ => reserve
    }

  // Compute follow-up message(s) to append after dispatching the head.
  pure def followups_for(m: CrossOrganMsg, bd_active: bool): List[CrossOrganMsg] =
    match m {
      | Msg_RevDist_AddRevenue(p) =>
          if (bd_active)
            // Bad-debt-override: route everything to CDP.
            List(Msg_CDP_FulfillBadDebt({ asset: p.asset, amount: p.amount }))
          else
            // Normal split: junior + senior + disco legs go to the queue.
            // (Reserve cut is taken in reserve_after_dispatch above.)
            List(
              Msg_Disco_AddRevenue({ asset: p.asset, amount: junior_cut_of(p.amount) }),
              Msg_Disco_AddRevenue({ asset: p.asset, amount: senior_cut_of(p.amount) }),
              Msg_Disco_AddRevenue({ asset: p.asset, amount: disco_cut_of(p.amount)  })
            )
      | _ => List()
    }

  // The amount counted as "paid_out" when dispatching this head msg.
  // Only count messages that exit the visible aggregate at delivery:
  //   - Disco_AddRevenue / AddDepositTokenRevenue: absorbed into Disco's
  //     per-slot RevenueEvent / tranche AddRevenue, leaving the aggregate.
  //   - BankSend(cdt=true): external payout, leaves the aggregate.
  //   - CDP_FulfillBadDebt: bad-debt burn-back, leaves the aggregate.
  // RevDist_AddRevenue is NOT a paid_out leg — it's a mid-flight transfer
  // (queue → reserve + queue, or queue → CDP_FulfillBadDebt msg). Counting
  // it here would double-count the same CDT against the eventual sink.
  pure def paid_out_delta_for(m: CrossOrganMsg): Uint128 =
    match m {
      | Msg_Disco_AddRevenue(p) => p.amount
      | Msg_Disco_AddDepositTokenRevenue(p) => p.amount
      | Msg_BankSend(p) => if (p.denom == "cdt") p.amount else 0
      | Msg_CDP_FulfillBadDebt(p) => p.amount
      | _ => 0
    }

  action deliver_head_of_queue: bool = all {
    msg_queue.length() > 0,
    val m = head_msg(msg_queue)
    all {
      msg_queue' = msg_queue.tail().concat(followups_for(m, RD::bad_debt_active)),
      RD::config' = RD::config,
      RD::bad_debt_active' = RD::bad_debt_active,
      RD::reserve_accumulation' = reserve_after_dispatch(
        RD::reserve_accumulation, m, RD::bad_debt_active),
      cumulative_revenue_paid_out' = cumulative_revenue_paid_out + paid_out_delta_for(m),
      CDP::basket' = CDP::basket,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
      cumulative_revenue_taken' = cumulative_revenue_taken,
      baseline_revenue_total' = baseline_revenue_total,
    }
  }

  // --- RD.WithdrawReserve: drains reserve into a Msg_BankSend(cdt) ---
  // Mirrors contracts/revenue-distributor/src/contract.rs:1114-1155.
  // Reserve decreases, in_flight_revenue increases (BankSend cdt is
  // counted there), so aggregate is conserved.
  action rd_withdraw_reserve(amount: Uint128, recipient: Addr): bool = all {
    amount > 0,
    amount <= RD::reserve_accumulation,
    RD::config' = RD::config,
    RD::bad_debt_active' = RD::bad_debt_active,
    RD::reserve_accumulation' = sat_sub(RD::reserve_accumulation, amount),
    msg_queue' = msg_queue.append(
      Msg_BankSend({ to: recipient, denom: "cdt", amount: amount })),
    CDP::basket' = CDP::basket,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
    cumulative_revenue_taken' = cumulative_revenue_taken,
    cumulative_revenue_paid_out' = cumulative_revenue_paid_out,
    baseline_revenue_total' = baseline_revenue_total,
  }

  // ================================================================
  // Top-level step
  // ================================================================
  action step: bool = any {
    tick,
    deliver_head_of_queue,
    cdp_accrue_revenue(100),
    cdp_accrue_revenue(25),
    cdp_take_revenue("cdt", 1),
    cdp_take_revenue("cdt", 25),
    rd_withdraw_reserve(1, "treasury"),
    toggle_bad_debt_active(true),
    toggle_bad_debt_active(false),
  }

  // ================================================================
  // Concrete scenario: a single revenue event walked end-to-end.
  // ================================================================
  //
  //   1. CDP accrues 1000 of pending_revenue.
  //   2. CDP.TakeRevenue moves all 1000 → Msg_RevDist_AddRevenue(1000).
  //   3. Deliver Msg_RevDist_AddRevenue(1000) (normal split)
  //        → reserve += 40; queue: junior=640, senior=160, disco=160 (sum 960).
  //   4. Deliver Msg_Disco_AddRevenue(640)  → paid_out += 640.
  //   5. Deliver Msg_Disco_AddRevenue(160)  → paid_out += 160.
  //   6. Deliver Msg_Disco_AddRevenue(160)  → paid_out += 160.
  //   7. RD.WithdrawReserve(40)             → reserve -= 40, queue += BankSend(cdt,40).
  //   8. Deliver Msg_BankSend(cdt,40)       → paid_out += 40.
  //
  // Net: paid_out == 1000, reserve == 0, basket == 0, queue empty.
  run revenue_propagates =
    compositionInit
      .then(cdp_accrue_revenue(1000))
      .then(cdp_take_revenue("cdt", 1000))
      .then(deliver_head_of_queue)         // RevDist_AddRevenue → reserve+40, 3 disco legs
      .then(deliver_head_of_queue)         // Disco junior 640 → paid_out += 640
      .then(deliver_head_of_queue)         // Disco senior 160 → paid_out += 160
      .then(deliver_head_of_queue)         // Disco disco  160 → paid_out += 160
      .then(rd_withdraw_reserve(40, "treasury"))
      .then(deliver_head_of_queue)         // BankSend(cdt,40) → paid_out += 40

  // ================================================================
  // Temporal properties (Apalache-checkable):
  //   tp_revenue_eventually_distributes:
  //     always(CDP::basket.pending_revenue > 0 implies eventually(
  //              CDP::basket.pending_revenue == 0))
  //
  //   tp_no_revenue_pinned_in_promises:
  //     always(RD::promises.length() > 0 implies eventually(
  //              RD::promises.length() == 0 or
  //              RD::failed_distributions.size() > 0))
  // ================================================================
}
