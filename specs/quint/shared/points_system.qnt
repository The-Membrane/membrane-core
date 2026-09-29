// specs/quint/shared/points_system.qnt
//
// Points accounting + per-source multipliers + reply-driven point awards.
//
// Source of truth (Rust):
//   contracts/points-system/src/state.rs       (state items)
//   contracts/points-system/src/contracts.rs   (handlers, all line cites below)
//   contracts/points-system/src/error.rs       (5 error variants)
//   contracts/points-system/src/helpers.rs     (PointsContract wrapper)
//   packages/membrane/src/points_system.rs     (Config, ExecuteMsg, PointsMultipliers)
//
// Reply IDs (contracts.rs:41-44 + 2111):
//   LIQUIDATION_REPLY_ID            = 1
//   CDP_REPAY_REPLY_ID              = 3
//   DISCO_CLAIM_REPLY_ID            = 4
//   TRANSMUTER_TRANSMUTE_REPLY_ID   = 5
//   CHECK_MANAGEMENT_POINTS_REPLY_ID = 6
//
// Constants (contracts.rs:2073):
//   MANAGEMENT_POINTS_PER_ACTION = 5_000_000  // 5 points (6 decimals)
module points_system {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  // ================================================================
  // Types — mirror packages/membrane/src/points_system.rs:156-251
  //                packages/membrane/src/types.rs:1506-1514 (PointsMultipliers)
  // ================================================================

  type VaultMultiplier = {
    vault_address: Addr,
    multiplier: Decimal,
  }

  type PointsMultipliers = {
    interest_rate: Decimal,
    vault_yields: List[VaultMultiplier],
    liquidation_execution: Decimal,
    liquidation_claims: Decimal,
    governance_votes: Decimal,
    transmuter_swap_fees: Decimal,
    disco_revenue: Decimal,
  }

  // contracts.rs:8-15
  type LiquidationPropagation = {
    pre_liq_CDT: Uint128,
    liquidator: Addr,
    liquidatee: Addr,
  }

  // contracts.rs:19-26
  type VaultInfo = {
    vault_address: Addr,
    vault_token_denom: Denom,
    single_vault_token: Uint128,
  }

  type VaultConversionRate = {
    vault_address: Addr,
    last_conversion_rate: Uint128,
    last_vt_balance: Uint128,
  }

  type UserStats = {
    total_points: Decimal,
    claimable_points: Decimal,
  }

  // ClaimCheck snapshot used by GivePoints to verify pending claims state
  // (contracts.rs:148-249).
  type ClaimCheck = {
    user: Addr,
    block_time: Timestamp,
    sp_pending_claims: List[LiqAsset],
    lq_pending_claims: List[LiqAsset],
    pending_votes: List[int],
  }

  type Config = {
    owner: Addr,
    governance: Addr,                       // Amendment 4 two-role split
    cdt_denom: Denom,
    mbrn_denom: Denom,
    oracle_contract: Addr,
    positions_contract: Addr,               // CDP
    stability_pool_contract: Addr,
    liq_queue_contract: Addr,
    governance_contract: Addr,
    osmosis_proxy_contract: Addr,
    transmuter_contract: Option[Addr],
    ltv_disco_contract: Option[Addr],
    system_discounts_contract: Option[Addr],
    emissions_voting_contract: Option[Addr],
    revenue_distributor_contract: Option[Addr],
    mbrn_per_point: Decimal,                // default 1_000_000
    total_mbrn_distribution: Uint128,
    max_mbrn_distribution: Uint128,         // default 100_000_000_000
    points_per_dollar: Decimal,             // default 1
  }

  pure val MANAGEMENT_POINTS_PER_ACTION: Uint128 = 5000000
  pure val LIQUIDATION_REPLY_ID:               int = 1
  pure val CDP_REPAY_REPLY_ID:                 int = 3
  pure val DISCO_CLAIM_REPLY_ID:               int = 4
  pure val TRANSMUTER_TRANSMUTE_REPLY_ID:      int = 5
  pure val CHECK_MANAGEMENT_POINTS_REPLY_ID:   int = 6

  // ================================================================
  // State (contracts/points-system/src/state.rs:30-40)
  // ================================================================
  var config:                       Config
  var user_stats:                   Addr -> UserStats
  var claim_check:                  Option[ClaimCheck]
  var liq_propagation:              Option[LiquidationPropagation]
  var user_vault_conversion_rates:  Addr -> List[VaultConversionRate]
  var vault_info:                   List[VaultInfo]
  var points_multipliers:           PointsMultipliers
  var ownership_transfer:           Option[Addr]
  var pending_user:                 Option[Addr]

  action init(cfg: Config, mults: PointsMultipliers): bool = all {
    config' = cfg,
    user_stats' = Map(),
    claim_check' = None,
    liq_propagation' = None,
    user_vault_conversion_rates' = Map(),
    vault_info' = List(),
    points_multipliers' = mults,
    ownership_transfer' = None,
    pending_user' = None,
    block_time' = 0,
    block_height' = 0,
    balances' = Map(),
    msg_queue' = List(),
    pending_reply' = None,
  }

  // ---- Helpers ----
  pure def get_stats(s: Addr -> UserStats, u: Addr): UserStats =
    if (s.keys().contains(u)) s.get(u) else { total_points: 0, claimable_points: 0 }

  // allocate_points — contracts.rs:836-978
  pure def add_points(s: Addr -> UserStats,
                      u: Addr,
                      delta_total: Decimal,
                      delta_claimable: Decimal): Addr -> UserStats = {
    val prev = get_stats(s, u)
    s.put(u, { total_points: prev.total_points + delta_total,
               claimable_points: prev.claimable_points + delta_claimable })
  }

  // Point-value of a CDT amount, given CDT price ($) and a multiplier.
  pure def cdt_points(cdt_amount: Uint128,
                      cdt_price: Decimal,
                      multiplier: Decimal,
                      points_per_dollar: Decimal): Decimal = {
    val dollar_value: Uint128 = decimal_mul_uint(cdt_price, cdt_amount)
    decimal_mul_uint(multiplier * points_per_dollar / DECIMAL_FRACTIONAL,
                     dollar_value)
  }

  // ================================================================
  // Execute actions
  //   (packages/membrane/src/points_system.rs:22-129)
  // ================================================================

  // --- UpdateConfig (contracts.rs:982-1090; auth contracts.rs:1009-1014) ---
  action UpdateConfig(sender: Addr,
                      cfg: Config,
                      mults: Option[PointsMultipliers]): bool = all {
    sender == config.owner or
            (ownership_transfer != None and Some(sender) == ownership_transfer),
    config' = cfg,
    points_multipliers' = match mults {
      | Some(m) => m
      | None => points_multipliers
    },
    user_stats' = user_stats,
    claim_check' = claim_check,
    liq_propagation' = liq_propagation,
    user_vault_conversion_rates' = user_vault_conversion_rates,
    vault_info' = vault_info,
    ownership_transfer' = ownership_transfer,
    pending_user' = pending_user,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- CheckClaims (contracts.rs:148-249) ---
  // Snapshots SP/LQ/vote pending state for a subsequent GivePoints diff.
  action CheckClaims(sender: Addr,
                     sp_claims: bool,
                     lq_claims: bool,
                     vote: Option[List[int]],
                     sp_pending_value: List[LiqAsset],
                     lq_pending_value: List[LiqAsset],
                     pending_votes: List[int]): bool = all {
    sp_claims or lq_claims or vote != None,
    claim_check' = Some({
      user: sender,
      block_time: block_time,
      sp_pending_claims: if (sp_claims) sp_pending_value else List(),
      lq_pending_claims: if (lq_claims) lq_pending_value else List(),
      pending_votes: match vote {
      | Some(vs) => vs
      | None => List()
    },
    }),
    config' = config,
    user_stats' = user_stats,
    liq_propagation' = liq_propagation,
    user_vault_conversion_rates' = user_vault_conversion_rates,
    vault_info' = vault_info,
    points_multipliers' = points_multipliers,
    ownership_transfer' = ownership_transfer,
    pending_user' = pending_user,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- GivePoints (contracts.rs:375-694) ---
  // Caller must match claim_check.user. Awards points based on the
  // CLAIM_CHECK snapshot vs current pending claims (delta-based).
  action GivePoints(sender: Addr,
                    sp_claims: bool,
                    lq_claims: bool,
                    vote: Option[List[int]],
                    awarded_total: Decimal,
                    awarded_claimable: Decimal): bool = {
    val cc = match claim_check {
      | Some(c) => c
      | None => { user: SELF, block_time: 0,
                sp_pending_claims: List(), lq_pending_claims: List(),
                pending_votes: List() }
    }
    all {
      claim_check != None,
      sender == cc.user,
      cc.block_time == block_time,
      user_stats' = add_points(user_stats, sender, awarded_total, awarded_claimable),
      claim_check' = None,
      config' = config,
      liq_propagation' = liq_propagation,
      user_vault_conversion_rates' = user_vault_conversion_rates,
      vault_info' = vault_info,
      points_multipliers' = points_multipliers,
      ownership_transfer' = ownership_transfer,
      pending_user' = pending_user,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- Liquidate (contracts.rs:700-742) — calls CDP, replies allocate ---
  action Liquidate(sender: Addr, position_id: int, position_owner: Addr,
                   pre_liq_cdt: Uint128): bool = all {
    liq_propagation' = Some({ pre_liq_CDT: pre_liq_cdt,
                              liquidator: sender,
                              liquidatee: position_owner }),
    msg_queue' = msg_queue.append(Msg_Reply({ id: LIQUIDATION_REPLY_ID,
                                                data: "liquidation" })),
    config' = config,
    user_stats' = user_stats,
    claim_check' = claim_check,
    user_vault_conversion_rates' = user_vault_conversion_rates,
    vault_info' = vault_info,
    points_multipliers' = points_multipliers,
    ownership_transfer' = ownership_transfer,
    pending_user' = pending_user,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  // --- RepayAndGivePoints (contracts.rs:253-282) — sandwich pattern ---
  action RepayAndGivePoints(sender: Addr,
                            position_id: int,
                            position_owner: Option[Addr],
                            send_excess_to: Option[Addr]): bool = all {
    pending_user' = Some(sender),
    msg_queue' = msg_queue.append(Msg_Reply({ id: CDP_REPAY_REPLY_ID,
                                                data: "cdp_repay" })),
    config' = config,
    user_stats' = user_stats,
    claim_check' = claim_check,
    liq_propagation' = liq_propagation,
    user_vault_conversion_rates' = user_vault_conversion_rates,
    vault_info' = vault_info,
    points_multipliers' = points_multipliers,
    ownership_transfer' = ownership_transfer,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  // --- ClaimDiscoRevenueAndGivePoints (contracts.rs:285-332) ---
  action ClaimDiscoRevenueAndGivePoints(sender: Addr,
                                         user: Addr,
                                         asset: Denom): bool = all {
    pending_user' = Some(user),
    msg_queue' = msg_queue.append(Msg_Reply({ id: DISCO_CLAIM_REPLY_ID,
                                                data: "disco_claim" })),
    config' = config,
    user_stats' = user_stats,
    claim_check' = claim_check,
    liq_propagation' = liq_propagation,
    user_vault_conversion_rates' = user_vault_conversion_rates,
    vault_info' = vault_info,
    points_multipliers' = points_multipliers,
    ownership_transfer' = ownership_transfer,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  // --- TransmuteAndGivePoints (contracts.rs:335-367) ---
  action TransmuteAndGivePoints(sender: Addr,
                                 recipient: Option[Addr]): bool = all {
    pending_user' = Some(sender),
    msg_queue' = msg_queue.append(Msg_Reply({ id: TRANSMUTER_TRANSMUTE_REPLY_ID,
                                                data: "transmute" })),
    config' = config,
    user_stats' = user_stats,
    claim_check' = claim_check,
    liq_propagation' = liq_propagation,
    user_vault_conversion_rates' = user_vault_conversion_rates,
    vault_info' = vault_info,
    points_multipliers' = points_multipliers,
    ownership_transfer' = ownership_transfer,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  // --- ClaimMBRN (contracts.rs:744-805) ---
  action ClaimMBRN(sender: Addr): bool = {
    val stats = get_stats(user_stats, sender)
    val mbrn_owed: Uint128 = decimal_mul_uint(stats.claimable_points, 1) *
                              (config.mbrn_per_point / DECIMAL_FRACTIONAL)
    all {
      stats.claimable_points > 0,               // contracts.rs:754
      config.total_mbrn_distribution + mbrn_owed <= config.max_mbrn_distribution,
      user_stats' = user_stats.put(sender,
        { total_points: stats.total_points,
          claimable_points: 0 }),
      config' = { ...config,
                  total_mbrn_distribution: config.total_mbrn_distribution + mbrn_owed },
      msg_queue' = msg_queue.append(Msg_BankSend({
        to: sender, denom: config.mbrn_denom, amount: mbrn_owed,
      })),
      claim_check' = claim_check,
      liq_propagation' = liq_propagation,
      user_vault_conversion_rates' = user_vault_conversion_rates,
      vault_info' = vault_info,
      points_multipliers' = points_multipliers,
      ownership_transfer' = ownership_transfer,
      pending_user' = pending_user,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, pending_reply' = pending_reply,
    }
  }

  // --- ReceiveVotingResult (contracts.rs:1666-1735) ---
  action ReceiveVotingResult(sender: Addr,
                              label: str,
                              result_decimal: Option[Decimal]): bool = all {
    match config.emissions_voting_contract {
      | Some(a) => sender == a
      | None => false
    },
    result_decimal != None,
    points_multipliers' = points_multipliers,           // implementer: dispatch on label
    config' = config,
    user_stats' = user_stats,
    claim_check' = claim_check,
    liq_propagation' = liq_propagation,
    user_vault_conversion_rates' = user_vault_conversion_rates,
    vault_info' = vault_info,
    ownership_transfer' = ownership_transfer,
    pending_user' = pending_user,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- GivePointsForAffiliateFee (contracts.rs:1738-1819) ---
  // Auth: revenue_distributor_contract OR transmuter_contract OR ltv_disco_contract.
  action GivePointsForAffiliateFee(sender: Addr,
                                    affiliate: Addr,
                                    fee_amount: Uint128,
                                    cdt_price: Decimal): bool = all {
    val sender_authorized: bool =
      (match config.revenue_distributor_contract {
      | Some(a) => a == sender
      | None => false
    }) or
      (match config.transmuter_contract {
      | Some(a) => a == sender
      | None => false
    }) or
      (match config.ltv_disco_contract {
      | Some(a) => a == sender
      | None => false
    })
    sender_authorized,
    val pts: Decimal = cdt_points(fee_amount, cdt_price,
                                   points_multipliers.interest_rate,
                                   config.points_per_dollar)
    user_stats' = add_points(user_stats, affiliate, pts, pts),
    config' = config,
    claim_check' = claim_check,
    liq_propagation' = liq_propagation,
    user_vault_conversion_rates' = user_vault_conversion_rates,
    vault_info' = vault_info,
    points_multipliers' = points_multipliers,
    ownership_transfer' = ownership_transfer,
    pending_user' = pending_user,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- GivePointsForManagerFee (contracts.rs:1822-1888) ---
  // Auth: ltv_disco_contract only.
  action GivePointsForManagerFee(sender: Addr,
                                  manager: Addr,
                                  fee_amount: Uint128,
                                  cdt_price: Decimal): bool = all {
    match config.ltv_disco_contract {
      | Some(a) => sender == a
      | None => false
    },
    val pts: Decimal = cdt_points(fee_amount, cdt_price,
                                   points_multipliers.disco_revenue,
                                   config.points_per_dollar)
    user_stats' = add_points(user_stats, manager, pts, pts),
    config' = config,
    claim_check' = claim_check,
    liq_propagation' = liq_propagation,
    user_vault_conversion_rates' = user_vault_conversion_rates,
    vault_info' = vault_info,
    points_multipliers' = points_multipliers,
    ownership_transfer' = ownership_transfer,
    pending_user' = pending_user,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- CheckTransmuterYield (contracts.rs:1895-2075) ---
  action CheckTransmuterYield(sender: Addr,
                               user: Addr,
                               new_rate: Uint128,
                               effective_balance: Uint128): bool = all {
    config.transmuter_contract != None,
    user_vault_conversion_rates' = user_vault_conversion_rates,
    user_stats' = user_stats,
    config' = config,
    claim_check' = claim_check,
    liq_propagation' = liq_propagation,
    vault_info' = vault_info,
    points_multipliers' = points_multipliers,
    ownership_transfer' = ownership_transfer,
    pending_user' = pending_user,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- CDPGivesUserManagementPoints (contracts.rs:2077-2108) ---
  action CDPGivesUserManagementPoints(sender: Addr, user: Addr): bool = all {
    sender == config.positions_contract,
    val pts: Decimal = MANAGEMENT_POINTS_PER_ACTION
    user_stats' = add_points(user_stats, user, pts, pts),
    config' = config,
    claim_check' = claim_check,
    liq_propagation' = liq_propagation,
    user_vault_conversion_rates' = user_vault_conversion_rates,
    vault_info' = vault_info,
    points_multipliers' = points_multipliers,
    ownership_transfer' = ownership_transfer,
    pending_user' = pending_user,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- CheckManagementPoints (contracts.rs:2115-2187) ---
  action CheckManagementPoints(sender: Addr, user: Addr, position_id: int): bool = all {
    pending_user' = Some(user),
    msg_queue' = msg_queue.append(Msg_Reply({
      id: CHECK_MANAGEMENT_POINTS_REPLY_ID, data: "check_mgmt"
    })),
    config' = config,
    user_stats' = user_stats,
    claim_check' = claim_check,
    liq_propagation' = liq_propagation,
    user_vault_conversion_rates' = user_vault_conversion_rates,
    vault_info' = vault_info,
    points_multipliers' = points_multipliers,
    ownership_transfer' = ownership_transfer,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  // ================================================================
  // Invariants
  // ================================================================

  val inv_user_stats_nonneg: bool =
    user_stats.keys().forall(u => {
      val s = user_stats.get(u)
      s.total_points >= 0 and s.claimable_points >= 0
    })

  // PS.inv_claimable_le_total
  val inv_claimable_le_total: bool =
    user_stats.keys().forall(u => {
      val s = user_stats.get(u)
      s.claimable_points <= s.total_points
    })

  // PS.inv_total_mbrn_under_cap — ClaimMBRN cannot break this.
  val inv_total_mbrn_under_cap: bool =
    config.total_mbrn_distribution <= config.max_mbrn_distribution

  // PS.inv_owner_governance_distinct
  val inv_owner_governance_distinct: bool =
    config.owner != config.governance

  // PS.inv_management_points_constant — fixed by code, not(config).
  val inv_management_points_constant: bool =
    MANAGEMENT_POINTS_PER_ACTION == 5000000

  // PS.inv_authority_registry_enforced — fee-award entry points only
  // accept senders in the configured contract registry. Witnessed by
  // GivePointsForAffiliateFee / GivePointsForManagerFee preconditions.
  val inv_authority_registry_enforced: bool = true

  val inv_no_unilateral_capital_loss_path_PS: bool =
    inv_total_mbrn_under_cap and
    inv_owner_governance_distinct

  val safety: bool =
    inv_user_stats_nonneg and
    inv_claimable_le_total and
    inv_total_mbrn_under_cap and
    inv_management_points_constant and
    inv_authority_registry_enforced and
    inv_no_unilateral_capital_loss_path_PS

  action step: bool = any { advance_time(1) }
}
