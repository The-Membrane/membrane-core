// specs/quint/tests/regression_corpus.qnt
//
// REGRESSION CORPUS — bugs the Rust implementation had, encoded as
// scenarios whose post-fix execution preserves the relevant Quint
// safety invariant. Each scenario documents:
//   - The Rust commit hash + one-line description.
//   - The CDP-organ invariant (or, for cross-organ fixes, the
//     companion organ's invariant) that catches a regression.
//
// This file is the "pre-port artifact" for the Solidity implementer:
// every bug below is one the Rust author has already paid for. If
// the Solidity port reintroduces the same logic mistake, the cited
// Quint invariant flags it before the contract ships.
//
// Run a single scenario:
//   cd specs && quint run --backend=typescript \
//       --init=<run_name> --step=step \
//       --max-steps=10 --max-samples=50 \
//       --invariant=safety quint/tests/regression_corpus.qnt
//
// ----------------------------------------------------------------
// Catalogue
// ----------------------------------------------------------------
// (1) 4e139eadd  "Fix supply cap ratio in error"
//        Rust:    contracts/cdp/src/risk_engine.rs:135-138
//        Bug:     post-mutation `basket.collateral_supply_caps[i]` was
//                 read for the bound, masking an OOB-vs-cap mismatch.
//        Caught:  CDP.inv_supply_caps_respected
//        Run:     supply_cap_ratio_post_fix
//
// (2) 91d192d0c  "0 out fees if they are more than collateral amount"
//        Rust:    contracts/cdp/src/liquidations.rs:597-640
//        Bug:     unchecked `caller_fee_in_collateral_amount` could
//                 exceed position.collateral, then checked_sub would
//                 underflow on the position-update step.
//        Caught:  CDP.inv_pending_bad_debt_nonneg + collateral
//                 monotonicity (within Liquidate's repay_propagation).
//        Run:     fee_clamp_post_fix
//
// (3) 4450841f3  "Fix bad debt liquidations" (fee min-cap)
//        Rust:    contracts/cdp/src/liquidations.rs:597-635
//        Bug:     pre-cap version multiplied
//                 `pre_user_repay_collateral_repay_amount * caller_fee`
//                 with no `min(.., collateral)` clamp.
//        Caught:  CDP.inv_pending_bad_debt_nonneg (same surface as #2;
//                 this is the predecessor commit to 91d192d0c).
//        Run:     bad_debt_liquidation_post_fix
//
// (4) 1f2680ac4  "close position reply: don't full-withdraw with debt"
//        Rust:    contracts/cdp/src/reply.rs:361-368
//        Bug:     `if assets_to_withdraw.len() > 0` triggered a full
//                 Withdraw even when `target_position.credit_amount`
//                 was non-zero (close-percentage < 100%).
//        Caught:  CDP.inv_position_solvent_before_mint (a partial
//                 close that strips collateral while leaving debt
//                 immediately violates solvency precondition for the
//                 next IncreaseDebt).
//        Run:     partial_close_keeps_collateral_post_fix
//
// (5) 3764f3b03  "remove staking-delegation indices after the loop"
//        Rust:    contracts/staking/src/contract.rs:579-647 +
//                 contracts/cdp/src/contract.rs:577-602 (migrate strip)
//        Bug:     `staker_delegation_info.delegated_to.remove(i)`
//                 inside the iteration shifted indices, corrupting
//                 the staker<->delegate symmetry. Also: a hardcoded
//                 migrate() that mutated a single user's CDP position.
//        Caught:  ST.inv_delegations_bidirectional (staking_test.qnt)
//                 and CDP.inv_credit_minted_nonneg (post-migrate)
//        Run:     migrate_no_position_mutation_post_fix
//
// (6) d11febebb  "delegation commission rate: cap total ratio at 1"
//        Rust:    contracts/staking/src/contract.rs:2261-2266
//        Bug:     `Decimal::from_ratio(total_delegated_to,
//                  total_rewarding_stake)` with no min(.., 1) gave
//                 ratios > 100% when delegations exceeded rewarding
//                 stake, then commission_subtraction overshot fees.
//        Caught:  ST.inv_max_commission_capped (staking_test.qnt)
//        Run:     (cross-organ; documented only)
//
// (7) a4ffb9b4b  "fix price parsing in Auction & System Discounts"
//        Rust:    contracts/auction/src/contracts.rs:145, 390-446
//        Bug:     Decoded `PriceResponse` instead of the canonical
//                 `Vec<PriceResponse>` returned by oracle.Price; all
//                 downstream `get_value` calls would panic on bytes
//                 decode error.
//        Caught:  OR.aggregation_hierarchy (shared/oracle.qnt) +
//                 AU.inv_no_cdt_in_fee_auctions  (structural shape:
//                 oracle returns `List[PriceResponse]`, not scalar).
//        Run:     (cross-organ; documented only)
//
// (8) 6cd7baa75  "decimal placement when decimals are incongruent"
//        Rust:    contracts/oracle/src/contracts.rs:669-714
//        Bug:     hard-coded `oracle_info.decimals - 6` assumed every
//                 trailing quote asset had OSMO's 6-decimal
//                 precision, mispricing 18-decimal vault tokens.
//        Caught:  OR.aggregation_hierarchy / OR.inv_no_duplicate_basket_per_asset
//                 (shared/oracle.qnt). Solidity port: a Chainlink
//                 quote feed with 8 decimals against a 18-decimal
//                 base would hit the same bug.
//        Run:     (cross-organ; documented only)
//
module regression_corpus {
  import types.* from "../lib/types"
  import money.* from "../lib/money"
  import cdp.*   from "../organs/cdp"

  // -- Test fixtures (mirror cdp_test.qnt) --
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
      max_borrow_LTV: 60_0000000000000000,
      max_LTV: 70_0000000000000000,
      rate_index: DECIMAL_ONE,
      peg_rate_index: DECIMAL_ONE,
      force_redemptions: None,
      max_threshold_to_delay: 15_0000000000000000,
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
    current_adaptive_rate: List(50000000000000000),
    peg_current_adaptive_rate: List(0),
    base_interest_rate: 30000000000000000,
    rates_last_accrued: 0,
    credit_last_accrued: 0,
    cpc_margin_of_error: 10000000000000000,
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
    liq_fee: 50000000000000000,
    collateral_twap_timeframe: 60,
    credit_twap_timeframe: 60,
    oracle_time_limit: 600,
    cpc_multiplier: DECIMAL_ONE,
    debt_minimum: 1000000,
    base_debt_cap_multiplier: 100,
    rate_slope_multiplier: DECIMAL_ONE,
    affiliate_fee_max: 50000000000000000,
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
  // (1) 4e139eadd  "Fix supply cap ratio in error"
  //
  // Forward witness: a deposit that respects the supply cap leaves
  // basket.collateral_supply_caps[0].current <= cap (== 1e12).
  // If the pre-fix body used `basket.collateral_supply_caps[i]`
  // (read after mutation) instead of the freshly-built `supply_caps`,
  // the comparison error would not panic — but
  // CDP.inv_supply_caps_respected enforces the per-asset bound on
  // every state, catching any path that lets `current` overshoot
  // `cap` due to the off-by-source-vector confusion.
  // ----------------------------------------------------------------
  run supply_cap_ratio_post_fix =
    runInit
      .then(Deposit(ALICE, None, ALICE, List((ATOM, 10000000))))
      .then(IncreaseDebt(ALICE, 1, 50000000, None, None, false,
                          Map(ATOM -> 10000000000000000000)))

  // ----------------------------------------------------------------
  // (2) 91d192d0c  "0 out fees if they are more than collateral"
  //
  // A liquidation against a small collateral position used to apply
  // `caller_fee_in_collateral_amount = pre_user_repay * caller_fee`
  // unconditionally. When the caller fee multiplier exceeded the
  // remaining collateral, the subsequent `checked_sub` underflowed
  // and the position's collateral got blown out — tripping
  // CDP.inv_pending_bad_debt_nonneg downstream once the bad-debt
  // path picked up the residue. Forward witness: insolvent position
  // is liquidated and post-state still satisfies safety.
  // ----------------------------------------------------------------
  run fee_clamp_post_fix =
    runInit
      .then(Deposit(ALICE, None, ALICE, List((ATOM, 10000000))))
      .then(IncreaseDebt(ALICE, 1, 70000000, None, None, false,
                          Map(ATOM -> 10000000000000000000)))
      .then(Liquidate(LIQUIDATOR, 1, ALICE,
                       Map(ATOM -> 5000000000000000000)))

  // ----------------------------------------------------------------
  // (3) 4450841f3  "Fix bad debt liquidations" (predecessor of #2)
  //
  // Same surface as (2): without the `min(fee_amt, collateral)` clamp
  // a tiny-collateral, large-debt liquidation would attempt to send
  // more collateral than the position holds. The post-fix scenario
  // is a low-collateral / high-debt liquidation that no longer pulls
  // unbacked fee value into the caller's bag; safety holds.
  // ----------------------------------------------------------------
  run bad_debt_liquidation_post_fix =
    runInit
      .then(Deposit(BOB, None, BOB, List((ATOM, 2000000))))
      .then(IncreaseDebt(BOB, 1, 1200000, None, None, false,
                          Map(ATOM -> 1000000000000000000)))
      .then(Liquidate(LIQUIDATOR, 1, BOB,
                       Map(ATOM -> 500000000000000000)))

  // ----------------------------------------------------------------
  // (4) 1f2680ac4  "close position reply: don't full-withdraw with debt"
  //
  // ClosePosition with `close_percentage < 100%` should retain the
  // residual collateral until the residual debt is also cleared.
  // The pre-fix reply handler would emit the WithdrawMsg whenever
  // assets_to_withdraw was non-empty, regardless of credit_amount.
  // Forward witness: a 50% close leaves the position open; safety
  // (in particular CDP.inv_position_solvent_before_mint when paired
  // with the next IncreaseDebt) is preserved.
  // ----------------------------------------------------------------
  run partial_close_keeps_collateral_post_fix =
    runInit
      .then(Deposit(ALICE, None, ALICE, List((ATOM, 10000000))))
      .then(IncreaseDebt(ALICE, 1, 30000000, None, None, false,
                          Map(ATOM -> 10000000000000000000)))
      .then(ClosePosition(ALICE, 1,
                           500000000000000000,           // 50%
                           50000000000000000,            // 5% spread
                           None))

  // ----------------------------------------------------------------
  // (5) 3764f3b03  "staking: remove indices after the loop"
  //                + drop the hardcoded CDP migrate() patch
  //
  // The CDP half of the commit deletes a migrate() body that
  // unilaterally subtracted collateral and debt from a hardcoded
  // user position. Post-fix: migrate is a no-op, so basket
  // credit_minted does not change across migrations.
  //
  // Forward witness: the post-migration state must satisfy
  // CDP.inv_credit_minted_nonneg and CDP.inv_pending_bad_debt_nonneg
  // even after a Deposit + IncreaseDebt + UpdateConfig roundtrip
  // (UpdateConfig is the closest available proxy for the migration
  // entry point in the spec).
  //
  // The staking half is exercised in staking_test.qnt — see
  // ST.inv_delegations_bidirectional and ST.inv_totals_match_staked.
  // ----------------------------------------------------------------
  run migrate_no_position_mutation_post_fix =
    runInit
      .then(Deposit(ALICE, None, ALICE, List((ATOM, 10000000))))
      .then(IncreaseDebt(ALICE, 1, 25000000, None, None, false,
                          Map(ATOM -> 10000000000000000000)))
      .then(UpdateConfig(OWNER, TEST_CONFIG))
}
