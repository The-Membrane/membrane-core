// specs/quint/tests/cdp_test.qnt
//
// CDP / Positions test scenarios. Run:
//   quint run --max-steps=200 --max-samples=10000 \
//     --invariant=safety specs/quint/tests/cdp_test.qnt
module cdp_test {
  import types.* from "../lib/types"
  import money.* from "../lib/money"
  import cdp.*   from "../organs/cdp"

  // -- Test fixtures --
  pure val OWNER:    Addr = "owner"
  pure val GOV:      Addr = "governance"
  pure val ALICE:    Addr = "alice"
  pure val BOB:      Addr = "bob"
  pure val LIQUIDATOR: Addr = "liquidator"
  pure val LTV_DISCO: Addr = "disco"
  pure val DEBT_C:   Addr = "debt_contract"
  pure val ORACLE:   Addr = "oracle"
  pure val LIQ_QUEUE: Addr = "liq_queue"
  pure val OSMOSIS_PROXY: Addr = "osmosis_proxy"

  pure val ATOM:     Denom = "atom"
  pure val OSMO:     Denom = "osmo"
  pure val CDT:      Denom = "cdt"

  pure val TEST_BASKET: Basket = {
    basket_id: 1,
    current_position_id: 1,
    collateral_types: List({
      asset_denom: ATOM, asset_amount: 0,
      max_borrow_LTV: 60_0000000000000000,    // 60%
      max_LTV: 70_0000000000000000,             // 70%
      rate_index: DECIMAL_ONE,
      peg_rate_index: DECIMAL_ONE,
      force_redemptions: None,
      max_threshold_to_delay: 15_0000000000000000,  // 15%
    }),
    collateral_supply_caps: List({
      asset_denom: ATOM, cap: 1000000000000, current: 0, debt_total: 0,
    }),
    credit_minted: 0,
    pending_revenue: 0,
    pending_bad_debt: 0,
    credit_price: DECIMAL_ONE,
    oracle_set: true,
    frozen: false,
    distribute_revenue: true,
    liq_queue: Some(LIQ_QUEUE),
  }

  pure val TEST_RATES: Rates = {
    current_adaptive_rate: List(50000000000000000),       // 5%
    peg_current_adaptive_rate: List(0),
    base_interest_rate: 30000000000000000,                  // 3%
    rates_last_accrued: 0,
    credit_last_accrued: 0,
    cpc_margin_of_error: 10000000000000000,                 // 1%
    negative_rates: false,
    acquisition_bump_rate: 0,
  }

  pure val TEST_CONFIG: Config = {
    owner: OWNER,
    governance: GOV,
    staking_contract: None,
    chain_proxy: None,
    debt_auction: None,
    oracle_contract: Some(ORACLE),
    liquidity_contract: None,
    discounts_contract: None,
    ltv_disco: LTV_DISCO,
    liq_fee: 50000000000000000,                              // 5%
    collateral_twap_timeframe: 60,
    credit_twap_timeframe: 60,
    oracle_time_limit: 600,
    cpc_multiplier: DECIMAL_ONE,
    debt_minimum: 1000000,
    base_debt_cap_multiplier: 100,
    rate_slope_multiplier: DECIMAL_ONE,
    affiliate_fee_max: 50000000000000000,                   // 5%
    revenue_distributor: None,
    skip_credit_price_accrual: true,
    liquidation_stat_limit: 500,
    transmuter_addr: None,
    irm_config: { adjustment_speed: 50, min_adaptive_rate: 1000000000000000,
                   max_adaptive_rate: 90000000000000000 },
    points_contract: None,
    acquisition_contract: None,
  }

  action runInit: bool = init(TEST_CONFIG, TEST_BASKET, TEST_RATES)

  // ----------------------------------------------------------------
  // Scenario 1: deposit → borrow → repay → close
  // ----------------------------------------------------------------
  run deposit_borrow_repay =
    runInit
      .then(Deposit(ALICE, None, ALICE, List((ATOM, 10000000))))
      // Increase debt within max_borrow_LTV (60%):
      // collateral = 10 ATOM × $10 = $100; max borrow = $60.
      .then(IncreaseDebt(ALICE, 1, 50000000, None, None, false,
                          Map(ATOM -> 10000000000000000000)))
      .then(Repay(ALICE, 1, ALICE, 50000000, None))

  // ----------------------------------------------------------------
  // Scenario 2: insolvent position → liquidation
  // ----------------------------------------------------------------
  run liquidate_insolvent =
    runInit
      .then(Deposit(ALICE, None, ALICE, List((ATOM, 10000000))))
      .then(IncreaseDebt(ALICE, 1, 70000000, None, None, false,
                          Map(ATOM -> 10000000000000000000)))
      // Price crashes — collateral now insufficient
      .then(Liquidate(LIQUIDATOR, 1, ALICE,
                       Map(ATOM -> 5000000000000000000)))

  // ----------------------------------------------------------------
  // Scenario 3: bad-debt fulfillment
  // ----------------------------------------------------------------
  run bad_debt_fulfillment =
    runInit
      .then(FulfillBadDebt(BOB, 1000000))   // BOB sends CDT to clear bad debt

  // ----------------------------------------------------------------
  // Scenario 4: affiliate registration
  // ----------------------------------------------------------------
  run set_affiliate =
    runInit
      .then(Deposit(ALICE, None, ALICE, List((ATOM, 10000000))))
      .then(SetAffiliate(ALICE, 1, BOB, 30000000000000000, Some("test_referral")))

  // ----------------------------------------------------------------
  // Scenario 5: governance restricts via UpdateConfig (must be governance)
  // ----------------------------------------------------------------
  run governance_update_config =
    runInit
      .then(UpdateConfig(OWNER, { ...TEST_CONFIG, liq_fee: 80000000000000000 }))
}
