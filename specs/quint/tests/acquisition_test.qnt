// specs/quint/tests/acquisition_test.qnt
//
// Acquisition scenarios — adaptive emission, Disco routing with clawback,
// claim ownership transfer.
module acquisition_test {
  import types.*       from "../lib/types"
  import money.*       from "../lib/money"
  import env.*         from "../lib/env"
  import acquisition.* from "../organs/acquisition"

  pure val OWNER: Addr = "owner"
  pure val GOV:   Addr = "governance"
  pure val ALICE: Addr = "alice"
  pure val BOB:   Addr = "bob"
  pure val TR:    Addr = "transmuter"
  pure val NX:    Addr = "neutron_proxy"
  pure val CDP:   Addr = "cdp"
  pure val DI:    Addr = "disco"

  pure val CFG: Config = {
    owner: OWNER, governance: GOV,
    transmuter_contract: TR, neutron_proxy: NX, cdp_contract: CDP,
    deposit_period_days: 30,
    withdrawal_period_days: 7,
    deposit_token: "usdc",
    minimum_deposit: 1000000,
    mbrn_denom: "mbrn",
    staking_contract: None,
    ltv_disco_contract: Some(DI),
    discounts_contract: "discounts",
    cliff_period_seconds: 7776000,                   // 90 days
  }

  pure val ACQ_CFG: AcquisitionModelConfig = {
    base_acquisition_rate: 1000000000000,
    initial_acquisition_amount: 0,
    max_mbrn_emission: 100000000000,
    target_utilization: 800000000000000000,
    bump_increment: 1000000000000000,
    bump_interval_seconds: 86400,
    reduction_speed_multiplier: 2,
    efficiency_threshold: 100000000000000000,
    max_rate_change_per_mutation: 50000000000000000,
    max_bump_rate: 100000000000000000,
    mutation_cooldown_seconds: 3600,
    accrual_ramp_seconds: 86400,
    accrual_ramp_start_fraction: 100000000000000000,
  }

  pure val ACQ_ST: AcquisitionModelState = {
    current_acquisition_rate: 1000000000000,
    accrued_pool: 0,
    last_accrual_time: 0,
    pool_maxed: false,
    bump_rate: 0,
    last_bump_time: 0,
    window_timers_started: false,
    first_deposit_time: None,
    deposit_event_count: 0,
    total_new_deposits: 0,
    last_mutation_efficiency: None,
    last_mutation_time: None,
    efficiency_clamped: false,
  }

  action runInit: bool = init(CFG, ACQ_CFG, ACQ_ST)

  action tick(dt: int): bool = all {
    advance_time(dt),
    config' = config,
    current_window_id' = current_window_id,
    current_acquisition_window' = current_acquisition_window,
    user_acquisition_deposits' = user_acquisition_deposits,
    user_intents' = user_intents,
    acquisition_config' = acquisition_config,
    acquisition_state' = acquisition_state,
    acquisition_history_len' = acquisition_history_len,
  }

  // ----------------------------------------------------------------
  // Scenario 1: window lifecycle — start, deposit, accrue, claim
  // ----------------------------------------------------------------
  run happy_path =
    runInit
      .then(StartAcquisitionWindow(OWNER))
      .then(Deposit(ALICE, 50000000, None, None))
      .then(tick(86400 * 30))                  // deposit period ends
      .then(AccruePool(BOB, 1000000000, ACQ_CFG.base_acquisition_rate, 0, false))
      .then(tick(86400 * 7))                   // withdrawal period ends
      .then(tick(7776001))                      // cliff passes
      .then(Claim(ALICE, 1, None))

  // ----------------------------------------------------------------
  // Scenario 2: Disco routing with clawback BEFORE cliff
  // ----------------------------------------------------------------
  run disco_routing_clawback =
    runInit
      .then(StartAcquisitionWindow(OWNER))
      .then(Deposit(ALICE, 50000000, None, None))
      .then(tick(86400 * 37 + 1))              // post-withdrawal
      .then(SendAcquisitionRewardsToDisco(ALICE, 1, "atom", 50, 1000000))
      // Withdraw before cliff → clawback
      .then(Withdraw(ALICE, 50000000))

  // ----------------------------------------------------------------
  // Scenario 3: rate assurance enforces transmuter total parity
  // ----------------------------------------------------------------
  run rate_assurance =
    runInit
      .then(StartAcquisitionWindow(OWNER))
      .then(Deposit(ALICE, 50000000, None, None))
      .then(AcquisitionRateAssurance(SELF, 50000000))  // matches; passes

  // ----------------------------------------------------------------
  // Scenario 4: rate assurance reverts on mismatch
  // ----------------------------------------------------------------
  run rate_assurance_mismatch =
    runInit
      .then(StartAcquisitionWindow(OWNER))
      .then(Deposit(ALICE, 50000000, None, None))
      .then(AcquisitionRateAssurance(SELF, 100000000))  // mismatch — SHOULD revert
}
