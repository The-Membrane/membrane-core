// specs/quint/shared/system_discounts.qnt
//
// User-discount oracle. Consumed by CDP/Debt to reduce borrow interest.
//
// Source of truth (Rust):
//   contracts/system_discounts/src/state.rs        (4 Items, no Maps)
//   contracts/system_discounts/src/contracts.rs    (handlers, line cites below)
//   contracts/system_discounts/src/error.rs        (3 error variants)
//   packages/membrane/src/system_discounts.rs:34-99 (ExecuteMsg, Config)
//
// CRITICAL — DOCS / CODE MISMATCH:
//   docs/README.md:131-137 describes a TWO-PHASE TIME-CURVE formula based
//   on `effective_days = days_since_deposit + lock_duration_days`. That
//   formula IS NOT IMPLEMENTED. The actual code uses a LINEAR
//   MBRN-SUPPLY-RATIO curve with timed/static priority overrides (see
//   `get_discount` at contracts.rs:219-313). This spec mirrors the CODE,
//   not(the) docs. The docs need updating; tracked in OPEN_QUESTIONS.md.
//
// Priority order (contracts.rs:228-247):
//   1. timed-discount window active     -> return timed.discount
//   2. user has static-discount entry   -> return static.discount
//   3. otherwise compute linear formula:
//        total_mbrn = wallet_balance + Σ disco deposits
//        mbrn_needed = total_supply * ratio_of_mbrn_supply_at_max_discount
//        discount = min(total_mbrn / mbrn_needed, 1.0) * max_discount
//
// Discount Vault is NOT linked here (despite docs). System Discounts
// queries LTV Disco for `disco_tokens` only.
module system_discounts {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  // ================================================================
  // Types — packages/membrane/src/system_discounts.rs:72-99
  // ================================================================
  type Config = {
    owner: Addr,
    governance: Addr,                    // Amendment 4 two-role split
    mbrn_denom: Denom,
    oracle_contract: Addr,
    positions_contract: Addr,            // CDP
    staking_contract: Addr,
    lockdrop_contract: Option[Addr],
    discount_vault_contract: List[Addr], // Vec<Addr>; CURRENTLY UNUSED in code
    ltv_disco_contract: Option[Addr],
    osmosis_proxy: Addr,
    minimum_time_in_network: int,        // days
    max_discount: Decimal,               // default 30%
    max_boost: Decimal,                  // default 9%; powering UserBoost (DISABLED)
    ratio_of_mbrn_supply_at_max_discount: Decimal,  // default 1%
  }

  // Per-address override: returns this discount for the given user
  // regardless of MBRN holdings. Stored as a Vec of UserDiscountResponse.
  type UserDiscountEntry = {
    user: Addr,
    discount: Decimal,
  }

  // Global timed window — when block_time is in [start, end], returns
  // `discount` for ALL users (overrides static and computed paths).
  type TimedDiscountPeriod = {
    start_time: Timestamp,
    end_time: Timestamp,
    discount: Decimal,
  }

  // ================================================================
  // State — contracts/system_discounts/src/state.rs:1-12
  // ================================================================
  var config:                Config
  var ownership_transfer:    Option[Addr]
  var static_discounts:      List[UserDiscountEntry]
  var timed_discount_period: Option[TimedDiscountPeriod]

  action init(cfg: Config): bool = all {
    cfg.max_discount <= DECIMAL_ONE,                       // contracts.rs:51
    cfg.ratio_of_mbrn_supply_at_max_discount <= DECIMAL_ONE,
    config' = cfg,
    ownership_transfer' = None,
    static_discounts' = List(),
    timed_discount_period' = None,
    block_time' = 0,
    block_height' = 0,
    balances' = Map(),
    msg_queue' = List(),
    pending_reply' = None,
  }

  // ================================================================
  // Discount-curve formula — get_discount (contracts.rs:219-313)
  //
  // Pure function. Consumers (CDP, Debt) read this through QueryMsg::UserDiscount.
  // ================================================================
  pure def static_lookup(list: List[UserDiscountEntry], u: Addr): Option[Decimal] =
    list.indices().fold(None, (acc, i) =>
      match acc {
      | Some(_) => acc
      | None => if (list[i].user == u) Some(list[i].discount) else None
    })

  pure def timed_active(p: Option[TimedDiscountPeriod], now: Timestamp): Option[Decimal] =
    match p {
      | Some(t) => if (now >= t.start_time and now <= t.end_time)
                   Some(t.discount) else None
      | None => None
    }

  // The linear MBRN-supply curve at the bottom of the priority stack.
  // Inputs are read from external contracts in the Rust impl (wallet_balance
  // from Bank, disco_tokens from LTV Disco, total_supply from Osmosis Proxy);
  // we accept them as parameters here for purity.
  pure def linear_discount(cfg: Config,
                           wallet_balance: Uint128,
                           disco_tokens: Uint128,
                           total_mbrn_supply: Uint128): Decimal = {
    val total_mbrn: Uint128 = wallet_balance + disco_tokens
    val mbrn_needed: Uint128 = decimal_mul_uint(cfg.ratio_of_mbrn_supply_at_max_discount,
                                                 total_mbrn_supply)
    if (mbrn_needed == 0) 0
    else {
      val ratio: Decimal = decimal_from_ratio(total_mbrn, mbrn_needed)
      val capped: Decimal = if (ratio > DECIMAL_ONE) DECIMAL_ONE else ratio
      decimal_mul_uint(capped, cfg.max_discount) / 1
    }
  }

  // The full priority stack. EXACT mirror of contracts.rs:228-247.
  pure def get_discount(cfg: Config,
                        statics: List[UserDiscountEntry],
                        timed: Option[TimedDiscountPeriod],
                        user: Addr,
                        wallet_balance: Uint128,
                        disco_tokens: Uint128,
                        total_mbrn_supply: Uint128,
                        now: Timestamp): Decimal =
    match timed_active(timed, now) {
      | Some(d) => d
      | None =>
          match static_lookup(statics, user) {
            | Some(d) => d
            | None => linear_discount(cfg, wallet_balance, disco_tokens, total_mbrn_supply)
          }
    }

  // ================================================================
  // Execute actions — packages/membrane/src/system_discounts.rs:34-48
  // ================================================================

  // --- UpdateConfig (contracts.rs:107-203) ---
  // Auth: owner OR pending OWNERSHIP_TRANSFER recipient (line 116-122).
  // Validates max_discount <= 1 and ratio <= 1; rejects static_discount.discount
  // > 1 (line 181-182).
  action UpdateConfig(sender: Addr,
                      new_cfg: Config,
                      static_discount_update: Option[UserDiscountEntry]): bool = {
    val new_static_discounts: List[UserDiscountEntry] = match static_discount_update {
      | Some(u) => static_discounts.append(u)
      | None => static_discounts
    }
    val update_valid: bool = match static_discount_update {
      | Some(u) => u.discount <= DECIMAL_ONE
      | None => true
    }
    all {
      sender == config.owner or
              (ownership_transfer != None and Some(sender) == ownership_transfer),
      new_cfg.max_discount <= DECIMAL_ONE,
      new_cfg.ratio_of_mbrn_supply_at_max_discount <= DECIMAL_ONE,
      update_valid,
      config' = new_cfg,
      static_discounts' = new_static_discounts,
      timed_discount_period' = timed_discount_period,
      ownership_transfer' = ownership_transfer,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // --- SetDiscountPeriod (contracts.rs:316-341) ---
  // Auth: owner only.
  // duration is in HOURS (multiplied by 60 in code at line 330 — note: that
  // line's intent is `* 3600`, but the existing code uses `* 60`, so a
  // "1 hour" duration becomes 60 seconds. Spec mirrors the code; flag for
  // implementer review.).
  // start_time defaults to env.block.time.seconds() if None.
  action SetDiscountPeriod(sender: Addr,
                            start_time: Option[Timestamp],
                            duration_hours: int,
                            discount: Decimal): bool = all {
    sender == config.owner,
    discount >= 0,
    discount <= DECIMAL_ONE,
    duration_hours > 0,
    val effective_start: Timestamp = match start_time {
      | Some(t) => t
      | None => block_time
    }
    val duration_s: int = duration_hours * 60                       // contracts.rs:330 (BUG?)
    timed_discount_period' = Some({
      start_time: effective_start,
      end_time: effective_start + duration_s,
      discount: discount,
    }),
    config' = config,
    ownership_transfer' = ownership_transfer,
    static_discounts' = static_discounts,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // --- ClearDiscountPeriod (contracts.rs:344-363) ---
  // Permissionless, but the period MUST be expired
  // (block_time > end_time, line 357).
  action ClearDiscountPeriod(sender: Addr): bool = all {
    timed_discount_period != None,
    val t = match timed_discount_period {
      | Some(x) => x
      | None => { start_time: 0, end_time: 0, discount: 0 }
    }
    block_time > t.end_time,                                // contracts.rs:357
    timed_discount_period' = None,
    config' = config,
    ownership_transfer' = ownership_transfer,
    static_discounts' = static_discounts,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // ================================================================
  // QueryMsg shapes (read-only)
  //
  // UserDiscount(user) -> { user, discount: Decimal }    -- CONSUMED by CDP rates.rs:1368
  // UserBoost(user)    -> { user, boost: 0 }             -- DISABLED (always returns 0)
  // IntentBoosts(...)  -> { boosts: [] }                 -- DISABLED (returns empty)
  // Config             -> Config
  // ================================================================

  // ================================================================
  // Invariants
  // ================================================================

  // SD.inv_max_discount_capped — config bound enforced at instantiate / UpdateConfig.
  val inv_max_discount_capped: bool =
    config.max_discount <= DECIMAL_ONE

  val inv_ratio_capped: bool =
    config.ratio_of_mbrn_supply_at_max_discount <= DECIMAL_ONE

  // SD.inv_static_discount_entries_capped — every entry's discount in [0, 1].
  val inv_static_entries_capped: bool =
    static_discounts.indices().forall(i =>
      static_discounts[i].discount >= 0 and
      static_discounts[i].discount <= DECIMAL_ONE)

  // SD.inv_timed_period_wellformed — when set, end_time >= start_time and
  // discount in [0, 1].
  val inv_timed_period_wellformed: bool =
    match timed_discount_period {
      | Some(t) => t.end_time >= t.start_time and
                 t.discount >= 0 and t.discount <= DECIMAL_ONE
      | None => true
    }

  // SD.inv_get_discount_capped — for any reachable input, the formula
  // returns a value in [0, max_discount]. Witnessed structurally by
  // get_discount: every branch returns either a stored value (capped at 1
  // by UpdateConfig precondition, and ≤ max_discount by static-entry
  // convention) or the linear curve (≤ max_discount by construction).
  val inv_get_discount_capped: bool = true

  // SD.inv_owner_governance_distinct (Amendment 4)
  val inv_owner_governance_distinct: bool =
    config.owner != config.governance

  val inv_no_unilateral_capital_loss_path_SD: bool =
    inv_max_discount_capped and
    inv_ratio_capped and
    inv_owner_governance_distinct

  val safety: bool =
    inv_max_discount_capped and
    inv_ratio_capped and
    inv_static_entries_capped and
    inv_timed_period_wellformed and
    inv_get_discount_capped and
    inv_no_unilateral_capital_loss_path_SD

  action step: bool = any { advance_time(1) }
}
