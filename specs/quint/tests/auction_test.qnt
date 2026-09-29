// specs/quint/tests/auction_test.qnt
//
// Auction scenarios — fee auction discount decay, bad-debt MBRN sale,
// ClearBadDebtFreeze emission.
module auction_test {
  import types.*  from "../lib/types"
  import money.*  from "../lib/money"
  import env.*    from "../lib/env"
  import auction.* from "../organs/auction"

  pure val OWNER:    Addr = "owner"
  pure val GOV:      Addr = "governance"
  pure val ORACLE:   Addr = "oracle"
  pure val OSMO_PROXY: Addr = "osmo_proxy"
  pure val POSITIONS: Addr = "positions"
  pure val GOV_C:    Addr = "gov_contract"
  pure val STAKING:  Addr = "staking"
  pure val LTV_DISCO: Addr = "disco"
  pure val DEBT:     Addr = "debt"
  pure val BUYER:    Addr = "buyer"

  pure val CFG: Config = {
    owner: OWNER, governance: GOV,
    oracle_contract: ORACLE,
    osmosis_proxy: OSMO_PROXY,
    mbrn_denom: "mbrn", cdt_denom: "cdt",
    desired_asset: "uosmo",
    positions_contract: POSITIONS,
    governance_contract: GOV_C,
    staking_contract: STAKING,
    twap_timeframe_m: 60,
    initial_discount: 30000000000000000,        // 3%
    discount_increase_timeframe_s: 60,
    discount_increase: 10000000000000000,       // 1%
    send_to_stakers: false,
    delay_window_m: 60,                          // 1-hour delay
    revenue_distributor_contract: None,
    ltv_disco_contract: Some(LTV_DISCO),
    debt_contract: Some(DEBT),
  }

  action runInit: bool = init(CFG)

  action tick(dt: int): bool = all {
    advance_time(dt),
    config' = config,
    fee_auctions' = fee_auctions,
    mbrn_sale' = mbrn_sale,
    ownership_transfer' = ownership_transfer,
  }

  // ----------------------------------------------------------------
  // Scenario 1: fee auction → swap → settlement
  // ----------------------------------------------------------------
  run fee_auction_swap =
    runInit
      .then(StartAuction(POSITIONS, "uatom", 1000000, None))
      .then(tick(3600 + 600))             // delay + 10 min decay
      .then(SwapForFee(BUYER, "uatom", 950000))   // ~95% of asset value at discount

  // ----------------------------------------------------------------
  // Scenario 2: StartAuction rejects CDT
  // ----------------------------------------------------------------
  run reject_cdt_auction =
    runInit
      .then(StartAuction(POSITIONS, "cdt", 1000000, None))   // SHOULD revert

  // ----------------------------------------------------------------
  // Scenario 3: bad-debt MBRN sale lifecycle
  // ----------------------------------------------------------------
  run mbrn_sale_lifecycle =
    runInit
      .then(StartMBRNSale(LTV_DISCO, 5000000, "uatom", 50000000))
      .then(FulfillBadDebt(BUYER, "uatom", 5000000))   // pays full bad debt → emits ClearBadDebtFreeze

  // ----------------------------------------------------------------
  // Scenario 4: BuySuppliedMBRN with leftover recursion
  // ----------------------------------------------------------------
  run buy_supplied_mbrn =
    runInit
      .then(StartMBRNSale(LTV_DISCO, 10000000, "uatom", 100000000))
      .then(StartMBRNSale(LTV_DISCO, 5000000, "uosmo", 50000000))
      .then(BuySuppliedMBRN(BUYER, None, None, 8000000))   // splits across both allocations

  // ----------------------------------------------------------------
  // Scenario 5: governance UpdateConfig validation bounds
  // ----------------------------------------------------------------
  run governance_update =
    runInit
      .then(UpdateConfig(OWNER, { ...CFG,
                                    initial_discount: 50000000000000000  // 5%
                                  }))
}
