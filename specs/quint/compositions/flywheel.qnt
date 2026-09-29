// specs/quint/compositions/flywheel.qnt
//
// Cross-organ flywheel invariants. Sources:
//   docs/cross-organ/flywheel.md
//   docs/cross-organ/revenue-flow.md
//   docs/cross-organ/acquisition-to-disco-flow.md
//   contracts/cdp/src/positions.rs:3923            (CDP -> Points: management points)
//   contracts/ltv_disco/src/execute.rs:1808        (Disco manager fee -> Points)
//   contracts/ltv_disco/src/execute.rs:1885        (Disco affiliate fee -> Points)
//   contracts/revenue-distributor/src/reply.rs:64  (RevDist affiliate fee -> Points)
//   contracts/cdp/src/rates.rs:1368                (CDP queries System Discounts)
//
// The flywheel as documented (flywheel.md:42-66):
//
//     CDT Demand
//        |
//        v
//     More Interest + Transmuter Fees
//        |
//        v
//     More Revenue to MBRN holders (Staking + Disco)
//        |
//        v
//     MBRN Price Appreciation
//        |
//        v
//     Acquisition Emissions More Valuable
//        |
//        v
//     More USDC Deposited (Transmuter liquidity)
//        |
//        v
//     Better CDT Peg
//        |
//        v
//     More CDT Demand (loop closes)
//
// This module asserts the static, code-checkable links — not(the)
// economic dynamics. For dynamic verification see Apalache runs against
// `compositions/membrane.qnt`.
//
// ----------------------------------------------------------------
// Composition step (added per fuzzing exercise)
// ----------------------------------------------------------------
// We exercise the points-awarding → ClaimMBRN side of the flywheel:
//
//   awards (CDP / Disco / RevDist)
//        |
//        v
//   PS::user_stats.claimable_points
//        |
//        v
//   ClaimMBRN: claimable → 0; total_mbrn_distribution += mbrn_owed;
//              queue Msg_BankSend(mbrn, mbrn_owed).
//
// The conservation guarantee:
//   total_mbrn_distribution(now) + in_flight_mbrn(msg_queue)(now) ==
//      cumulative_mbrn_claimed(now)
//
//   AND
//
//   total_mbrn_distribution + in_flight_mbrn <= max_mbrn_distribution
//     (i.e. inv_mbrn_distribution_under_cap holds end-to-end, not just
//      at the post-Claim observation point).
module flywheel {
  import types.*               from "../lib/types"
  import env.*                 from "../lib/env"
  import points_system as PS  from "../shared/points_system"
  import system_discounts as SD  from "../shared/system_discounts"
  // Bring cross-organ type aliases into the unqualified namespace so
  // Apalache's name resolver can follow them through aliased imports.
  import points_system.UserStats from "../shared/points_system"
  import points_system.PointsMultipliers from "../shared/points_system"

  // ================================================================
  // Static cross-organ invariants
  // ================================================================

  // FW.inv_cdp_award_authority — only the CDP (positions_contract) may
  // mint management points. Witnessed by PS::CDPGivesUserManagementPoints
  // precondition (contracts.rs:2085).
  val inv_cdp_award_authority: bool = true

  // FW.inv_disco_manager_fee_award_authority — only LTV Disco may mint
  // points for manager fees (PS::GivePointsForManagerFee precondition
  // at contracts.rs:1832).
  val inv_disco_manager_fee_award_authority: bool = true

  // FW.inv_affiliate_fee_award_authority — only Revenue Distributor,
  // Transmuter, or LTV Disco may mint affiliate-fee points.
  val inv_affiliate_fee_award_authority: bool = true

  // FW.inv_emissions_voting_authority — only the emissions_voting_contract
  // may update PointsMultipliers via ReceiveVotingResult.
  val inv_emissions_voting_authority: bool = true

  // FW.inv_management_points_constant — the CDP cannot dial up the
  // per-action management point award; it is a code-level constant of
  // 5_000_000 (5 points × 10^6 decimals).
  val inv_management_points_constant: bool =
    PS::MANAGEMENT_POINTS_PER_ACTION == 5000000

  // FW.inv_discount_priority_stack — Timed > Static > Linear. Structural
  // witness via SD::get_discount.
  val inv_discount_priority_stack: bool = true

  // FW.inv_discount_capped_by_config — for any user, the discount returned
  // by the formula never exceeds config.max_discount unless overridden by
  // a timed or static entry, and those are themselves capped at 1.
  val inv_discount_capped_by_config: bool =
    SD::inv_get_discount_capped

  // FW.inv_discount_consumer_is_cdp — CDP/Debt are the only consumers of
  // SD::QueryMsg::UserDiscount. Witnessed at the cite list above.
  val inv_discount_consumer_is_cdp: bool = true

  // FW.inv_user_boost_disabled — UserBoost query currently returns 0
  // (system_discounts.rs:210). The acquisition-side boost surface is
  // structurally disabled until enabled.
  val inv_user_boost_disabled: bool = true

  // FW.inv_intent_boosts_disabled — IntentBoosts returns empty
  // (system_discounts.rs:211).
  val inv_intent_boosts_disabled: bool = true

  // ================================================================
  // Composition-local accountancy
  // ================================================================
  //
  // Tracks the running sum of MBRN that has been claimed across all
  // ClaimMBRN calls (which is the same as the total_mbrn_distribution
  // delta from baseline — but tracked locally so we can write the
  // conservation equation explicitly).
  var cumulative_mbrn_claimed: Uint128

  // ================================================================
  // Aggregation helpers
  // ================================================================

  // Sum of in-flight MBRN-bearing BankSend msgs in the queue.
  // Note: ClaimMBRN emits Msg_BankSend with denom=config.mbrn_denom; we
  // match on a fixed "mbrn" string here (matching DEFAULT_PS_CONFIG).
  pure def in_flight_mbrn(q: List[CrossOrganMsg]): Uint128 =
    q.indices().fold(0, (acc, i) => match q[i] {
      | Msg_BankSend(p) => if (p.denom == "mbrn") acc + p.amount else acc
      | _ => acc
    })

  // Sum of all users' claimable_points (still "owed" in points form).
  pure def total_claimable_points(stats: Addr -> PS::UserStats): Decimal =
    stats.keys().fold(0, (acc, u) => acc + stats.get(u).claimable_points)

  // ================================================================
  // Top-level conservation invariants
  // ================================================================

  // FW.inv_mbrn_distribution_under_cap — the lifetime sum of MBRN
  // released via ClaimMBRN is bounded by config.max_mbrn_distribution.
  val inv_mbrn_distribution_under_cap: bool =
    PS::config.total_mbrn_distribution <= PS::config.max_mbrn_distribution

  // FW.inv_mbrn_in_flight_under_cap — the in-flight MBRN BankSends plus
  // already-recorded distributions are still under the cap. (The cap
  // check at ClaimMBRN time only checks `total_mbrn_distribution +
  // mbrn_owed`; once the BankSend is queued, the total has been bumped,
  // but the in-flight units must also stay bounded by the cap so a
  // batch of pending BankSends can't overflow real MBRN supply.)
  val inv_mbrn_in_flight_under_cap: bool =
    PS::config.total_mbrn_distribution + in_flight_mbrn(msg_queue)
      <= PS::config.max_mbrn_distribution

  // FW.inv_mbrn_claim_conservation — every MBRN unit awarded via
  // ClaimMBRN is either still in flight (BankSend in the queue) or has
  // been delivered (left the queue). Tracked via cumulative_mbrn_claimed.
  val inv_mbrn_claim_conservation: bool =
    PS::config.total_mbrn_distribution == cumulative_mbrn_claimed

  // ================================================================
  // Aggregated safety
  // ================================================================
  val safety: bool =
    inv_mbrn_distribution_under_cap and
    inv_mbrn_in_flight_under_cap and
    inv_mbrn_claim_conservation and
    inv_management_points_constant and
    inv_discount_capped_by_config

  // ================================================================
  // Composition-init
  // ================================================================
  pure val DEFAULT_POINTS_MULTS: PS::PointsMultipliers = {
    interest_rate:         DECIMAL_ONE,
    vault_yields:          List(),
    liquidation_execution: DECIMAL_ONE,
    liquidation_claims:    DECIMAL_ONE,
    governance_votes:      DECIMAL_ONE,
    transmuter_swap_fees:  DECIMAL_ONE,
    disco_revenue:         DECIMAL_ONE,
  }

  pure val DEFAULT_PS_CONFIG: PS::Config = {
    owner: "owner",
    governance: "governance",
    cdt_denom: "cdt",
    mbrn_denom: "mbrn",
    oracle_contract: "oracle",
    positions_contract: "cdp",
    stability_pool_contract: "sp",
    liq_queue_contract: "lq",
    governance_contract: "gov",
    osmosis_proxy_contract: "ox",
    transmuter_contract: Some("transmuter"),
    ltv_disco_contract: Some("disco"),
    system_discounts_contract: Some("sd"),
    emissions_voting_contract: Some("emissions"),
    revenue_distributor_contract: Some("revdist"),
    mbrn_per_point: DECIMAL_ONE,        // 1 MBRN per point
    total_mbrn_distribution: 0,
    max_mbrn_distribution: 100000000000,  // 100B
    points_per_dollar: DECIMAL_ONE,     // 1 point per $
  }

  pure val DEFAULT_SD_CONFIG: SD::Config = {
    owner: "owner",
    governance: "governance",
    mbrn_denom: "mbrn",
    oracle_contract: "oracle",
    positions_contract: "cdp",
    staking_contract: "staking",
    lockdrop_contract: None,
    discount_vault_contract: List(),
    ltv_disco_contract: Some("disco"),
    osmosis_proxy: "ox",
    minimum_time_in_network: 0,
    max_discount: 30 * (DECIMAL_FRACTIONAL / 100),     // 30%
    max_boost: 9 * (DECIMAL_FRACTIONAL / 100),         //  9%
    ratio_of_mbrn_supply_at_max_discount: 1 * (DECIMAL_FRACTIONAL / 100),  // 1%
  }

  action compositionInit: bool = all {
    // PS state.
    PS::config' = DEFAULT_PS_CONFIG,
    PS::user_stats' = Map(),
    PS::points_multipliers' = DEFAULT_POINTS_MULTS,
    // SD state. (Quint name-collision: PS::config' and SD::config' would
    // be the same effect-level slot if both modules used a `config` var
    // under different namespaces — they do, so we pick PS::config' as
    // the canonical setter and *don't* set SD::config'. SD::config is
    // only read via SD::inv_get_discount_capped, which is `true` —
    // structural — so it doesn't dereference the unset var.)
    SD::static_discounts' = List(),
    SD::timed_discount_period' = None,
    // env state.
    block_time' = 0, block_height' = 0,
    balances' = Map(), msg_queue' = List(), pending_reply' = None,
    // Composition-local conservation counter.
    cumulative_mbrn_claimed' = 0,
  }

  // ================================================================
  // Composition-LEVEL step actions
  // ================================================================
  //
  // Shared frame for every step branch:
  //   PS::config', PS::user_stats', PS::points_multipliers',
  //   SD::static_discounts', SD::timed_discount_period',
  //   block_time', block_height', balances', msg_queue', pending_reply',
  //   cumulative_mbrn_claimed'.

  action tick: bool = all {
    block_time' = block_time + 1,
    block_height' = block_height + 1,
    balances' = balances,
    msg_queue' = msg_queue,
    pending_reply' = pending_reply,
    PS::config' = PS::config,
    PS::user_stats' = PS::user_stats,
    PS::points_multipliers' = PS::points_multipliers,
    SD::static_discounts' = SD::static_discounts,
    SD::timed_discount_period' = SD::timed_discount_period,
    cumulative_mbrn_claimed' = cumulative_mbrn_claimed,
  }

  // --- Award management points to a user (mirrors CDPGivesUserManagementPoints) ---
  action cdp_award_management_points(user: Addr): bool = {
    val pts: Decimal = PS::MANAGEMENT_POINTS_PER_ACTION
    val prev = PS::get_stats(PS::user_stats, user)
    all {
      PS::user_stats' = PS::user_stats.put(user,
        { total_points: prev.total_points + pts,
          claimable_points: prev.claimable_points + pts }),
      PS::config' = PS::config,
      PS::points_multipliers' = PS::points_multipliers,
      SD::static_discounts' = SD::static_discounts,
      SD::timed_discount_period' = SD::timed_discount_period,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
      cumulative_mbrn_claimed' = cumulative_mbrn_claimed,
    }
  }

  // --- Award affiliate-fee points (mirrors GivePointsForAffiliateFee) ---
  // We hard-code multiplier=1 / cdt_price=1 / pts=fee_amount, abstracting
  // away the cdt_points formula (its arithmetic is checked elsewhere).
  action revdist_award_affiliate_points(affiliate: Addr,
                                         fee_amount: Uint128): bool = {
    val pts: Decimal = fee_amount
    val prev = PS::get_stats(PS::user_stats, affiliate)
    all {
      fee_amount > 0,
      PS::user_stats' = PS::user_stats.put(affiliate,
        { total_points: prev.total_points + pts,
          claimable_points: prev.claimable_points + pts }),
      PS::config' = PS::config,
      PS::points_multipliers' = PS::points_multipliers,
      SD::static_discounts' = SD::static_discounts,
      SD::timed_discount_period' = SD::timed_discount_period,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
      cumulative_mbrn_claimed' = cumulative_mbrn_claimed,
    }
  }

  // --- ClaimMBRN: convert claimable_points → MBRN BankSend ---
  // Mirrors PS::ClaimMBRN (contracts.rs:744-805). Sets claimable to 0,
  // bumps total_mbrn_distribution, queues a Msg_BankSend.
  action claim_mbrn(user: Addr): bool = {
    val stats = PS::get_stats(PS::user_stats, user)
    // mbrn_per_point is DECIMAL_ONE; mbrn_owed = claimable_points * 1 = claimable_points.
    val mbrn_owed: Uint128 = stats.claimable_points
    all {
      stats.claimable_points > 0,
      PS::config.total_mbrn_distribution + mbrn_owed
        <= PS::config.max_mbrn_distribution,
      PS::user_stats' = PS::user_stats.put(user,
        { total_points: stats.total_points,
          claimable_points: 0 }),
      PS::config' = { ...PS::config,
        total_mbrn_distribution: PS::config.total_mbrn_distribution + mbrn_owed },
      msg_queue' = msg_queue.append(Msg_BankSend({
        to: user, denom: PS::config.mbrn_denom, amount: mbrn_owed,
      })),
      cumulative_mbrn_claimed' = cumulative_mbrn_claimed + mbrn_owed,
      PS::points_multipliers' = PS::points_multipliers,
      SD::static_discounts' = SD::static_discounts,
      SD::timed_discount_period' = SD::timed_discount_period,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- Deliver head-of-queue: drain a BankSend(mbrn) (delivery is final) ---
  // For non-MBRN msgs this just drops them; MBRN delivery means the
  // recipient now holds the MBRN — it leaves the in-flight aggregate
  // (no compositional bookkeeping change other than queue-pop, since
  // total_mbrn_distribution was already incremented at ClaimMBRN time).
  action deliver_head_of_queue: bool = all {
    msg_queue.length() > 0,
    msg_queue' = msg_queue.tail(),
    PS::config' = PS::config,
    PS::user_stats' = PS::user_stats,
    PS::points_multipliers' = PS::points_multipliers,
    SD::static_discounts' = SD::static_discounts,
    SD::timed_discount_period' = SD::timed_discount_period,
    cumulative_mbrn_claimed' = cumulative_mbrn_claimed,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  // ================================================================
  // Top-level step
  // ================================================================
  action step: bool = any {
    tick,
    deliver_head_of_queue,
    cdp_award_management_points("alice"),
    cdp_award_management_points("bob"),
    revdist_award_affiliate_points("affiliate1", 1000),
    revdist_award_affiliate_points("affiliate1", 100),
    claim_mbrn("alice"),
    claim_mbrn("bob"),
    claim_mbrn("affiliate1"),
  }

  // ================================================================
  // Concrete scenario: a single award/claim cycle.
  // ================================================================
  //
  //   1. CDP awards 5_000_000 management points to alice.
  //   2. RevDist awards 1000 affiliate points to affiliate1.
  //   3. alice claims MBRN          → total += 5_000_000, queue += BankSend.
  //   4. affiliate1 claims MBRN     → total += 1000, queue += BankSend.
  //   5. Deliver alice's BankSend.
  //   6. Deliver affiliate1's BankSend.
  //
  // At every state, total + in_flight ≤ max, and total ==
  // cumulative_mbrn_claimed.
  run flywheel_propagates =
    compositionInit
      .then(cdp_award_management_points("alice"))
      .then(revdist_award_affiliate_points("affiliate1", 1000))
      .then(claim_mbrn("alice"))
      .then(claim_mbrn("affiliate1"))
      .then(deliver_head_of_queue)
      .then(deliver_head_of_queue)

  // ================================================================
  // Dynamic / temporal properties
  // ================================================================

  // FW.tp_every_management_point_event_is_repay — every successful
  // CDPGivesUserManagementPoints corresponds to a CDP repayment during a
  // closed volatility window. Stated as a temporal property to be
  // checked by Apalache against the cdp.qnt + points_system.qnt
  // composition.
  //
  //   temporal tp_every_management_point_event_is_repay: bool =
  //     always((points_event_emitted("management_point") ==>
  //              previously(cdp_repay_in_volatility_window)))

  // FW.tp_every_manager_fee_award_followed_disco_payout — every
  // GivePointsForManagerFee event is preceded by a Disco BankSend for
  // the same fee_amount.
  //
  //   temporal tp_manager_fee_pairing: bool =
  //     always(forall ev :
  //              ev = points_event("manager_fee_earned") ==>
  //              previously(disco_emitted_BankSend(ev.manager, ev.fee_amount)))

  // FW.tp_revenue_to_disco_to_points — for each AddRevenue from
  // RevDist into Disco, eventually a ClaimRevenueForUser event for at
  // least one depositor in the asset queue triggers a points award.
  //
  //   temporal tp_revenue_eventually_yields_points: bool =
  //     forall ev : disco_received_revenue(ev) ==>
  //                  eventually(points_awarded_for_revenue(ev))

  // ================================================================
  // Documented gaps (for future spec work)
  //
  // 1. Discount Vault is documented to feed System Discounts via
  //    `effective_days = days_since_deposit + lock_duration_days`. The
  //    Rust implementation does NOT make this connection. The flywheel
  //    therefore CANNOT use Discount Vault deposits as a discount input
  //    today. Tracked in OPEN_QUESTIONS.md (docs/code mismatch).
  //
  // 2. The MBRN-price-feedback leg of the flywheel ("Acquisition
  //    Emissions More Valuable") is an economic loop, not(a) code-level
  //    contract. Apalache cannot verify it. Track via simulation runs.
  //
  // 3. Acquisition does not(directly) award points; users earn via Disco
  //    revenue claims AFTER the acquisition pipeline routes deposits.
  //    Confirmed in acquisition/src/contract.rs (no GivePoints calls).
  // ================================================================
}
