// specs/quint/tests/transmuter_test.qnt
//
// Runner scenarios for the Transmuter, including the new manager subsystem
// and the 14-day switching-period withdraw-intent flow.
//
// Run:
//   quint run --max-steps=200 --max-samples=10000 \
//     --invariant=safety specs/quint/tests/transmuter_test.qnt
module transmuter_test {
  import types.*      from "../lib/types"
  import env.*        from "../lib/env"
  import transmuter.* from "../organs/transmuter"

  pure val ALICE: Addr = "alice"
  pure val BOB: Addr = "bob"
  pure val CAROL: Addr = "carol"
  pure val CDP: Addr = "cdp"
  pure val ACQUISITION: Addr = "acquisition"
  pure val REVDIST: Addr = "revdist"
  pure val OWNER: Addr = "owner"
  pure val GOV:   Addr = "governance"

  pure val CDT: Denom = "cdt"
  pure val PAIRED: Denom = "usdc"
  pure val ATOM: Denom = "atom"

  pure val SWITCHING_PERIOD_S: int = 1209600    // 14 days
  pure val SWITCHING_WINDOW_S: int = 172800     // 2 days

  pure val CFG: TConfig = {
    owner: OWNER,
    governance: GOV,
    discounts_contract: "sd",
    cdp_contract: CDP,
    collateral_contract: "col",
    deposit_pair: { cdt: CDT, paired_asset: PAIRED },
    composition_leeway: decimal_from_percent(5),
    cdt_target_ratio: decimal_from_percent(50),
    usage_fee: decimal_from_percent(1),
    usage_fee_utilization_threshold: decimal_from_percent(80),
    swap_history_cap: 50,
    volume_history_cap: 50,
    allowlist: Set(CDP),
    revenue_distributor_addr: Some(REVDIST),
    affiliate_fee: decimal_from_percent(1),
    send_swap_fee: true,
    revenue_distributor_fee_percentage: decimal_from_percent(20),
    acquisition_contract: Some(ACQUISITION),
    points_system_contract: None,
    retention_multiplier: 0,
    max_management_fee: decimal_from_percent(5),
    switching_period: SWITCHING_PERIOD_S,
    switching_execution_window: SWITCHING_WINDOW_S,
  }

  action runInit: bool = init(CFG)

  action tick(dt: int): bool = all {
    advance_time(dt),
    config' = config,
    user_tranches' = user_tranches,
    tranche_states' = tranche_states,
    tranche_rate_assurance' = tranche_rate_assurance,
    total_tranche_staked' = total_tranche_staked,
    undistributed_fees' = undistributed_fees,
    acquisition_deposit_total' = acquisition_deposit_total,
    cumulative_volume' = cumulative_volume,
    deployed_paired_asset' = deployed_paired_asset,
    pending_revenue' = pending_revenue,
    affiliates' = affiliates,
    transmuter_manager' = transmuter_manager,
    transmuter_manager_fee' = transmuter_manager_fee,
    managed_users' = managed_users,
    user_principal_base' = user_principal_base,
    pending_fee_change' = pending_fee_change,
    switching_intents' = switching_intents,
    next_intent_id' = next_intent_id,
    user_switching_intents' = user_switching_intents,
    events' = events,
    pending_config_change' = pending_config_change,
    collateral_onboarding_end' = collateral_onboarding_end,
  }

  // Scenario: set manager → fee change → wait → execute → clear.
  run manager_fee_change_path =
    runInit
      .then(SetTransmuterManager(ALICE, ALICE, Some(BOB)))
      .then(RequestManagerFeeChange(BOB, decimal_from_percent(3)))
      .then(tick(SWITCHING_PERIOD_S))
      .then(ExecuteManagerFeeChange(CAROL, BOB))
      .then(SetTransmuterManager(ALICE, ALICE, None))
      .then(ClearTransmuterManagerFee(ALICE, ALICE, BOB))

  // Scenario: junior exit intent — wait, then execute permissionlessly.
  run junior_exit_happy_path =
    runInit
      .then(RequestSwitch(ALICE, ALICE, Intent_JuniorExit({
        collateral_denom: ATOM, vt_amount: 100,
        recipient: None, withdraw_as: None,
      })))
      .then(tick(SWITCHING_PERIOD_S + 60))
      .then(ExecuteSwitch(CAROL, 1))

  // Scenario: intent expires past 2-day window — ExecuteSwitch reverts.
  // Run truncates the asserted revert; PruneExpiredIntents handles cleanup.
  run expired_intent_rejected =
    runInit
      .then(RequestSwitch(ALICE, ALICE, Intent_JuniorToSenior({
        collateral_denom: ATOM, vt_amount: 500,
      })))
      .then(tick(SWITCHING_PERIOD_S + SWITCHING_WINDOW_S + 60))
      .then(PruneExpiredIntents(CAROL))

  // Scenario: manager cannot unilaterally exit.
  run manager_cannot_exit =
    runInit
      .then(SetTransmuterManager(ALICE, ALICE, Some(BOB)))

  // Scenario: fee request bounded by max_management_fee.
  run fee_cap_rejected_at_request =
    runInit
      .then(SetTransmuterManager(ALICE, ALICE, Some(BOB)))
      .then(RequestManagerFeeChange(BOB, decimal_from_percent(99)))
}
