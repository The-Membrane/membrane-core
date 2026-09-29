// specs/quint/lib/access_control.qnt
//
// Access-control taxonomy + the top-level no-unilateral-capital-loss
// invariant. Used by every organ module via `import access_control.* from
// "../lib/access_control"`.
//
// See specs/CAPITAL_LOSS_SURFACE.md for the full enumeration of which
// actions fall under which category and which mitigation applies.
//
// The invariant `inv_no_unilateral_capital_loss_path` is a *meta*
// invariant: it holds iff every dangerous action across the protocol is
// guarded by at least one of the four mitigations (TL/GOV/CAP/WIN).
// Per-organ modules contribute their share via the structural witnesses
// listed in CAPITAL_LOSS_SURFACE.md.
module access_control {
  import types.* from "./types"

  // -- Mitigation tags ---------------------------------------------------
  // A change that is gated by ONE of these is considered safe under the
  // top-level invariant.
  type Mitigation =
    | Mit_Timelock                  // 14d request + 2d permissionless execute
    | Mit_Governance                // info.sender == config.governance (multisig)
    | Mit_HardCap                   // value bounded by code-level constant
    | Mit_Window                    // gated behind onboarding/switching window
    | Mit_None                      // no mitigation -- VIOLATING

  // -- Severity ---------------------------------------------------------
  type LossCategory =
    | Cat_DirectLoss                // Category A in CAPITAL_LOSS_SURFACE.md
    | Cat_IndirectLoss              // Category B
    | Cat_Operational               // Category C (no mitigation needed)

  // -- Generic timelocked-config-change record --------------------------
  // Mirrors the FeeChangeRequest pattern from Amendment 2 but typed for
  // arbitrary config fields. Concrete organs typically narrow this with
  // a sum type listing the specific fields they support.
  type ConfigChangeRequest = {
    field_id: str,                  // e.g. "transmuter.usage_fee", organ-prefixed
    new_value: str,                 // serialized; opaque to this module
    proposer: Addr,
    created_at: Timestamp,
    unlock_time: Timestamp,
    expire_time: Timestamp,
  }

  // -- Hard caps (CAP mitigation) ---------------------------------------
  // These are code-level constants; the spec asserts that admin actions
  // cannot push the corresponding param above (or below) these values.
  // Numeric values are in the Decimal raw scale (10^18 = 100%).
  pure val MAX_LTV_HARD_CAP: Decimal           = 900000000000000000   // 90%
  pure val MAX_USAGE_FEE: Decimal              = 50000000000000000    //  5%
  pure val MAX_REVDIST_PERCENTAGE: Decimal     = 500000000000000000   // 50%
  pure val MAX_MAX_MGT_FEE: Decimal            = 250000000000000000   // 25%
  pure val MAX_LIQ_FEE: Decimal                = 100000000000000000   // 10%

  // Amendment 5 (2026-04-30): the normal-rate ceiling and the
  // comparative-rate ceiling are both raised to 100%.
  // - MAX_INTEREST_RATE_BPS — ceiling on the CDP base interest rate
  //   (per-year). 100% means borrowed CDT can at most double in a year
  //   from interest accrual; admin can never exceed this even via
  //   governance.
  // - MAX_COMPARATIVE_RATE — ceiling on the rate used in dynamic
  //   rate comparisons (e.g. cross-asset spread accruals). Same 100%
  //   ceiling as the normal rate.
  pure val MAX_INTEREST_RATE_BPS: Decimal      = 1000000000000000000  // 100%/yr
  pure val MAX_COMPARATIVE_RATE: Decimal       = 1000000000000000000  // 100%/yr

  pure val MAX_AFFILIATE_FEE: Decimal          = 50000000000000000    //  5%
  pure val MIN_SWITCHING_PERIOD_S: int         = 604800               //  7 days
  pure val MIN_UNSTAKING_PERIOD_S: int         = 604800               //  7 days
  pure val MIN_UNBONDING_PERIOD_S: int         = 604800               //  7 days
  pure val MAX_ORACLE_STALENESS_S: int         = 3600                 //  1 hour
  pure val MAX_PRICE_DEVIATION_PCT: Decimal    = 100000000000000000   // 10%/update

  // Amendment 5: static borrow flow limit. Per 24h window, no more than
  // MAX_DAILY_BORROW_PCT of total outstanding CDT borrow can be newly
  // minted. Code-level constant; admin cannot raise it.
  pure val MAX_DAILY_BORROW_PCT: Decimal       = 100000000000000000   // 10%/day
  pure val DAILY_WINDOW_S: int                 = 86400                //  24 hours

  // -- Predicates -------------------------------------------------------

  // Auth shape for a Category-A action. Either the sender IS the
  // governance multisig, OR the action is being applied via a timelocked
  // execute that has been pending past its unlock_time.
  pure def gov_or_timelock_satisfied(
    sender: Addr,
    governance_addr: Addr,
    pending: Option[ConfigChangeRequest],
    now: Timestamp
  ): bool =
    sender == governance_addr or {
      match pending {
      | Some(r) => now >= r.unlock_time and now <= r.expire_time
      | None    => false
    }
    }

  // For Category-B actions: timelock alone is sufficient.
  pure def timelock_satisfied(pending: Option[ConfigChangeRequest], now: Timestamp): bool =
    match pending {
      | Some(r) => now >= r.unlock_time and now <= r.expire_time
      | None    => false
    }

  // Cap predicates — instantiated per-field by organ modules.
  pure def value_capped_above(v: Decimal, cap: Decimal): bool = v <= cap
  pure def value_capped_below(v: int, floor_v: int): bool      = v >= floor_v

  // Window predicate — used for the autonomous-onboarding case (A4).
  pure def in_active_window(window_end: Timestamp, now: Timestamp): bool =
    window_end > now

  // -- Top-level invariant skeleton -------------------------------------
  // Each per-organ module exports a list of (action_id, satisfied) pairs;
  // the membrane composition takes the conjunction. We declare the shape
  // here so all organs share it.
  type ActionWitness = { action_id: str, satisfied: bool, category: LossCategory }

  pure def witness_ok(w: ActionWitness): bool =
    match w.category {
      | Cat_Operational => true
      | _ => w.satisfied
    }

  pure def all_dangerous_actions_mitigated(witnesses: List[ActionWitness]): bool =
    witnesses.indices().forall(i => witness_ok(witnesses[i]))
}
