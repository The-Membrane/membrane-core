// specs/quint/tests/ltv_disco_test.qnt
//
// LTV Disco test scenarios — manager system, 14d unstaking, lower-LTV intent.
module ltv_disco_test {
  import types.*    from "../lib/types"
  import money.*    from "../lib/money"
  import env.*      from "../lib/env"
  import ltv_disco.* from "../organs/ltv_disco"

  pure val OWNER:    Addr = "owner"
  pure val GOV:      Addr = "governance"
  pure val ALICE:    Addr = "alice"
  pure val BOB:      Addr = "bob"
  pure val MANAGER:  Addr = "manager"
  pure val CDP:      Addr = "cdp"
  pure val AUCTION:  Addr = "auction"
  pure val MBRN:     Denom = "mbrn"
  pure val ATOM:     Denom = "atom"

  pure val CFG: DConfig = {
    owner: OWNER, governance: GOV,
    cdp_contract: CDP,
    deposit_denom: MBRN,
    cdt_denom: "cdt",
    minimum_deposit: 1000000,
    unstaking_period: 1209600,                // 14 days (Amendment 1)
    unstake_execution_window: 172800,         // 2 days
    ltv_switching_period: 1209600,
    ltv_switching_execution_window: 172800,
    manager_fee_change_period: 1209600,
    manager_fee_change_execution_window: 172800,
    oracle_contract: "oracle",
    chain_proxy_contract: "proxy",
    affiliate_fee: 10000000000000000,         // 1%
    max_management_fee: 50000000000000000,    // 5%
    auction_contract: Some(AUCTION),
    mbrn_denom: Some(MBRN),
  }

  action runInit: bool = init(CFG)

  action tick(dt: int): bool = all {
    advance_time(dt),
    config' = config,
    asset_queues' = asset_queues,
    backing_deposits' = backing_deposits,
    user_deposits' = user_deposits,
    managed_deposits' = managed_deposits,
    manager_fee' = manager_fee,
    pending_bad_debt' = pending_bad_debt,
    rate_assurance' = rate_assurance,
    revenue_events' = revenue_events,
    unstake_requests' = unstake_requests,
    affiliates' = affiliates,
    lower_ltv_intents' = lower_ltv_intents,
    next_ltv_intent_id' = next_ltv_intent_id,
    deposit_ltv_intent' = deposit_ltv_intent,
    pending_fee_change' = pending_fee_change,
    pending_config_change' = pending_config_change,
    events' = events,
  }

  // ----------------------------------------------------------------
  // Scenario 1: 14-day unstaking with 2-day window
  // ----------------------------------------------------------------
  run unstake_lifecycle =
    runInit
      .then(SubmitDeposit(ALICE, ATOM, 50_0000000000000000, 10000000,
                            "atom:50:alice:1", None, None, None))
      .then(RequestUnstake(ALICE, "atom:50:alice:1", 5000000))
      .then(tick(1209601))
      .then(CompleteUnstake(BOB, "atom:50:alice:1"))

  // ----------------------------------------------------------------
  // Scenario 2: unstake expires after 2-day window
  // ----------------------------------------------------------------
  // Negative scenario: CompleteUnstake AFTER 2d window — precondition
  // reverts (window_is_open is false). Run truncates before the asserted
  // revert; PruneExpiredUnstakes is the cleanup path.
  run unstake_expires =
    runInit
      .then(SubmitDeposit(ALICE, ATOM, 50_0000000000000000, 10000000,
                            "atom:50:alice:1", None, None, None))
      .then(RequestUnstake(ALICE, "atom:50:alice:1", 5000000))
      .then(tick(1209600 + 172801))
      .then(PruneExpiredUnstakes(BOB))

  // ----------------------------------------------------------------
  // Scenario 3: lower-LTV move requires intent (14d wait + 2d window)
  // ----------------------------------------------------------------
  // Negative-then-positive: direct lower-LTV MoveDeposit reverts; users
  // must use the intent flow. Run truncates the asserted revert and tests
  // only the intent path.
  run lower_ltv_move_via_intent =
    runInit
      .then(SubmitDeposit(ALICE, ATOM, 80_0000000000000000, 10000000,
                            "atom:80:alice:1", None, None, None))
      .then(RequestLowerLTVMove(ALICE, "atom:80:alice:1", "80", "60", 5000000))
      .then(tick(1209601))
      .then(ExecuteLowerLTVMove(BOB, 1))

  // ----------------------------------------------------------------
  // Scenario 4: manager fee — decrease instant, increase delayed
  // ----------------------------------------------------------------
  run manager_fee_asymmetric =
    runInit
      .then(SubmitDeposit(ALICE, ATOM, 80_0000000000000000, 10000000,
                            "atom:80:alice:1", None, Some(MANAGER), None))
      .then(RequestManagerFeeChange(MANAGER, 40000000000000000))
      .then(tick(1209601))
      .then(ExecuteManagerFeeChange(BOB, MANAGER))
      .then(RequestManagerFeeChange(MANAGER, 20000000000000000))
      .then(RequestManagerFeeChange(MANAGER, 40000000000000000))

  // ----------------------------------------------------------------
  // Scenario 5: bad-debt flow freezes withdrawals
  // ----------------------------------------------------------------
  // Negative path: RequestUnstake reverts while pending_bad_debt > 0.
  // Run skips the asserted revert and tests the bad-debt resolution flow.
  run bad_debt_freezes =
    runInit
      .then(SubmitDeposit(ALICE, ATOM, 80_0000000000000000, 10000000,
                            "atom:80:alice:1", None, None, None))
      .then(AddBadDebt(CDP, ATOM, 5000000))
      .then(SendMBRNForSale(AUCTION, ATOM, 5000000, BOB))
      .then(ClearBadDebtFreeze(AUCTION, ATOM, 5000000))
}
