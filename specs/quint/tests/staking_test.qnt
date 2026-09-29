// specs/quint/tests/staking_test.qnt
//
// Staking scenarios — stake/unstake/restake, fee deposit + claim,
// per-MBRN-rate trick, lock + RefreshLock.
module staking_test {
  import types.*   from "../lib/types"
  import money.*   from "../lib/money"
  import env.*     from "../lib/env"
  import staking.* from "../organs/staking"

  pure val OWNER:    Addr = "owner"
  pure val GOV:      Addr = "governance"
  pure val ALICE:    Addr = "alice"
  pure val BOB:      Addr = "bob"
  pure val POSITIONS: Addr = "positions"
  pure val AUCTION:  Addr = "auction"

  pure val CFG: Config = {
    owner: OWNER, governance: GOV,
    mbrn_denom: "mbrn",
    incentive_schedule: {
      rate: 90000000000000000,                    // 9%
      duration_s: 20736000,                        // 240 days
      start_time: 0,
    },
    lock_duration_ceiling_s: 31536000,             // 365 days
    unstaking_period_s: 1209600,                   // 14 days
    max_commission_rate: 100000000000000000,       // 10%
    keep_raw_cdt: true,
    vesting_rev_multiplier: 200000000000000000,    // 20%
    positions_contract: Some(POSITIONS),
    auction_contract: Some(AUCTION),
    vesting_contract: None,
    governance_contract: None,
    osmosis_proxy: None,
    emissions_voting_contract: None,
  }

  action runInit: bool = init(CFG, CFG.incentive_schedule)

  action tick(dt: int): bool = all {
    advance_time(dt),
    config' = config,
    staking_totals' = staking_totals,
    staked' = staked,
    delegations' = delegations,
    delegate_claims' = delegate_claims,
    fee_events' = fee_events,
    incentive_scheduling' = incentive_scheduling,
    buyback_and_burn' = buyback_and_burn,
    ownership_transfer' = ownership_transfer,
  }

  // ----------------------------------------------------------------
  // Scenario 1: stake → unstake (2-step) → withdraw after period
  // ----------------------------------------------------------------
  run stake_unstake_lifecycle =
    runInit
      .then(Stake(ALICE, None, 10000000, None))
      .then(Unstake(ALICE, Some(5000000), false))
      .then(tick(1209601))
      .then(Unstake(ALICE, Some(5000000), false))

  // ----------------------------------------------------------------
  // Scenario 2: governance lock blocks Unstake
  // ----------------------------------------------------------------
  run gov_lock_blocks_unstake =
    runInit
      .then(Stake(ALICE, None, 10000000, None))
      .then(Unstake(ALICE, Some(5000000), true))

  // ----------------------------------------------------------------
  // Scenario 3: DepositFee → claim per-MBRN rate
  // ----------------------------------------------------------------
  run fee_distribution =
    runInit
      .then(Stake(ALICE, None, 10000000, None))
      .then(Stake(BOB, None, 10000000, None))
      .then(DepositFee(POSITIONS, "cdt", 1000000))
      .then(tick(60))
      .then(ClaimRewards(ALICE, None, false))
      .then(ClaimRewards(BOB, None, false))

  // ----------------------------------------------------------------
  // Scenario 4: Lock + RefreshLock with perpetual_lock
  // ----------------------------------------------------------------
  run lock_lifecycle =
    runInit
      .then(Stake(ALICE, None, 10000000, None))
      .then(Lock(ALICE, {
        locked_until: 31535999,
        perpetual_lock_days: Some(180),
        intended_lock_days: Some(180),
      }, 5000000))
      .then(tick(86400 * 90))
      .then(RefreshLock(BOB, Some(ALICE), Some(0)))

  // ----------------------------------------------------------------
  // Scenario 5: Restake reverses unstaking
  // ----------------------------------------------------------------
  run restake_reverses =
    runInit
      .then(Stake(ALICE, None, 10000000, None))
      .then(Unstake(ALICE, Some(5000000), false))
      .then(tick(86400 * 7))
      .then(Restake(ALICE, 5000000))
}
