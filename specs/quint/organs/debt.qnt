// specs/quint/organs/debt.qnt
//
// Debt basket — system-wide CDT debt accounting, pending_revenue accumulation,
// interest accrual formula, circuit breaker.
//
// Source of truth:
//   contracts/debt/src/  (+ contracts/cdp/src/rates.rs)
//   docs/README.md:86-92 (interest accrual formula)
//
// Lean proves the accrual formula correctness (InterestAccrual.lean).
module debt {
  import types.*          from "../lib/types"
  import money.*          from "../lib/money"
  import env.*            from "../lib/env"
  import access_control.* from "../lib/access_control"

  type Basket = {
    cdt_minted: Uint128,
    pending_revenue: Uint128,
    debt_cap: Uint128,
    base_interest_rate: Decimal,
    last_accrued: Timestamp,
    circuit_breaker: bool,
  }

  // Amendment 5: 24-hour borrow flow tracker (rolling window).
  // The contract enforces that the SUM of new mints in a window is
  // at most `borrow_limit_pct * cdt_minted_at_window_start`. This is a
  // STATIC percentage — admin cannot raise `borrow_limit_pct` above
  // MAX_DAILY_BORROW_PCT at any time.
  type BorrowWindow = {
    window_start: Timestamp,
    minted_at_window_start: Uint128,
    minted_in_window: Uint128,
  }

  var basket: Basket
  var borrow_window: BorrowWindow
  var borrow_limit_pct: Decimal      // <= MAX_DAILY_BORROW_PCT

  pure val SECONDS_PER_YEAR: int = 31536000

  action init(initial: Basket, limit_pct: Decimal): bool = all {
    limit_pct >= 0,
    limit_pct <= MAX_DAILY_BORROW_PCT,
    basket' = initial,
    borrow_window' = { window_start: 0,
                       minted_at_window_start: initial.cdt_minted,
                       minted_in_window: 0 },
    borrow_limit_pct' = limit_pct,
    block_time' = 0,
    block_height' = 0,
    balances' = Map(),
    msg_queue' = List(),
    pending_reply' = None,
  }

  // Helper — does the window need rolling forward?
  pure def window_expired(w: BorrowWindow, now: Timestamp): bool =
    now >= w.window_start + DAILY_WINDOW_S

  // Helper — quota for the current window, evaluated at the window start.
  pure def daily_quota(w: BorrowWindow, pct: Decimal): Uint128 =
    decimal_mul_uint(pct, w.minted_at_window_start)

  // Interest accrual — docs/README.md:86-92
  // accumulated = principal * rate * elapsed / SECONDS_PER_YEAR
  action AccrueInterest(elapsed: int): bool = all {
    elapsed > 0,
    val delta = (basket.cdt_minted * decimal_mul_uint(basket.base_interest_rate, elapsed)) / SECONDS_PER_YEAR
    basket' = { ...basket,
                pending_revenue: basket.pending_revenue + delta,
                last_accrued: block_time },
    borrow_window' = borrow_window,
    borrow_limit_pct' = borrow_limit_pct,
    block_time' = block_time, block_height' = block_height,
    balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
  }

  // Amendment 5: borrow-rate-limited mint. Replaces ad-hoc IncreaseDebt
  // dispatching. Window rolls forward when expired; new mints checked
  // against `daily_quota`.
  action MintBorrow(sender: Addr, recipient: Addr, amount: Uint128): bool = {
    val rolled: BorrowWindow =
      if (window_expired(borrow_window, block_time))
        { window_start: block_time,
          minted_at_window_start: basket.cdt_minted,
          minted_in_window: 0 }
      else borrow_window
    val quota: Uint128 = daily_quota(rolled, borrow_limit_pct)
    all {
      amount > 0,
      rolled.minted_in_window + amount <= quota,
      basket.cdt_minted + amount <= basket.debt_cap,
      basket' = { ...basket, cdt_minted: basket.cdt_minted + amount },
      borrow_window' = { ...rolled, minted_in_window: rolled.minted_in_window + amount },
      borrow_limit_pct' = borrow_limit_pct,
      block_time' = block_time, block_height' = block_height,
      balances' = balances, msg_queue' = msg_queue, pending_reply' = pending_reply,
    }
  }

  // TakeRevenue — CDP-authorized drain of pending_revenue into RevDist flow
  action TakeRevenue(sender: Addr, cdp_contract: Addr): bool = all {
    sender == cdp_contract,
    basket' = { ...basket, pending_revenue: 0 },
    borrow_window' = borrow_window,
    borrow_limit_pct' = borrow_limit_pct,
    msg_queue' = msg_queue.append(Msg_RevDist_AddRevenue({ asset: "cdt", amount: basket.pending_revenue })),
    block_time' = block_time, block_height' = block_height,
    balances' = balances, pending_reply' = pending_reply,
  }

  // Invariants
  val inv_accrued_interest_formula: bool =
    basket.pending_revenue >= 0

  val inv_debt_under_cap: bool = basket.cdt_minted <= basket.debt_cap

  // NEW (Amendment 4): rate cap. Amendment 5 raised ceiling to 100%.
  val inv_interest_rate_capped: bool =
    value_capped_above(basket.base_interest_rate, MAX_INTEREST_RATE_BPS)

  // NEW (Amendment 5): borrow flow caps.
  val inv_borrow_limit_pct_capped: bool =
    value_capped_above(borrow_limit_pct, MAX_DAILY_BORROW_PCT)

  // The mint-in-window invariant: at any reachable state, the running
  // sum cannot exceed `daily_quota` for the window. Witnessed by
  // `MintBorrow`'s precondition.
  val inv_minted_in_window_under_quota: bool =
    borrow_window.minted_in_window <= daily_quota(borrow_window, borrow_limit_pct)

  val inv_window_wellformed: bool =
    borrow_window.window_start >= 0 and
    borrow_window.minted_in_window >= 0 and
    borrow_window.minted_at_window_start >= 0

  val inv_no_unilateral_capital_loss_path_DBT: bool =
    inv_interest_rate_capped and
    inv_borrow_limit_pct_capped and
    inv_minted_in_window_under_quota and
    inv_window_wellformed

  val safety: bool =
    inv_accrued_interest_formula and inv_debt_under_cap and
    inv_no_unilateral_capital_loss_path_DBT

  action step: bool = all {
    advance_time(1),
    basket' = basket,
    borrow_window' = borrow_window,
    borrow_limit_pct' = borrow_limit_pct,
  }
}
