# CDP → Modular Contract Parity Map

Side-by-side logic tree showing where every piece of CDP logic moved to in the modular split.

**Source:** `contracts/cdp/` (monolith)
**Targets:** `contracts/collateral/` + `contracts/debt/` + `contracts/liquidation-engine/`

---

## 1. Execute Messages

| CDP ExecuteMsg | Modular Contract | Modular ExecuteMsg | Notes |
|---|---|---|---|
| `UpdateConfig` | Collateral + Debt + Liquidation | `UpdateConfig` (each) | Split across 3 configs |
| `Deposit` | **Collateral** | `Deposit` | Collateral notifies Debt via `NotifyDeposit` |
| `Withdraw` | **Collateral** → Debt | `Withdraw` → `AccrueAndValidate` | Collateral initiates, Debt validates solvency |
| `IncreaseDebt` | **Debt** → Collateral | `IncreaseDebt` → `RefreshAndAssess` | Debt initiates, Collateral provides assessment |
| `Repay` | **Debt** | `Repay` | Fully in Debt contract |
| `Accrue` | **Debt** | `Accrue` | Fully in Debt contract |
| `EditcAsset` | **Collateral** | `EditcAsset` | LTV parameters live in Collateral basket |
| `EditBasket` | Collateral + Debt | `EditBasket` (each) | Collateral-side params vs debt-side params |
| `Liquidate` | **Liquidation** | `Liquidate` | Orchestrates across Debt + Collateral |
| `ClosePosition` | **Debt** | `ClosePosition` | Debt orchestrates sell + repay flow |
| `SetUserIntents` | **Debt** | `SetUserIntents` | Deployment intents are debt-side |
| `FulfillIntents` | **Debt** | `FulfillIntents` | Minting intents are debt-side |
| `SetDeploymentVenue` | **Debt** | `SetDeploymentVenue` | Venues are debt-side |
| `UpdateBasketLTVs` | **Collateral** | `UpdateBasketLTVs` | LTV dynamics live with collateral |
| `SetAffiliate` | **Debt** | `SetAffiliate` | Affiliates are debt-side |
| `FulfillBadDebt` | **Debt** | `FulfillBadDebt` | Bad debt accounting is debt-side |
| `TakeRevenue` | **Debt** | `TakeRevenue` | Revenue is debt-side |
| `CollateralRateAssurance` | **Collateral** | `CollateralRateAssurance` | Rate assurance is collateral-side |
| `CheckAndClearDebtDelta` | **Debt** | `CheckAndClearDebtDelta` | Points/volatility is debt-side |
| `Callback::BadDebtCheck` | **Liquidation** | `Callback::BadDebtCheck` | Post-liquidation callback |
| `Callback::ClosePositionCallback` | **Debt** | `Callback::ClosePositionCallback` | Post-venue closure |

### New Execute Messages (not in CDP)

| Contract | ExecuteMsg | Purpose |
|---|---|---|
| Collateral | `SetFrozen` | Syncs frozen state to Debt |
| Collateral | `RefreshAndAssess` | Returns collateral assessment (Debt/Liquidation only) |
| Collateral | `AuthorizedWithdraw` | Execute withdrawal after Debt validates |
| Collateral | `AuthorizedSellCollateral` | Sell collateral via Chain Proxy |
| Collateral | `ActivateSupplyCaps` | Toggle supply cap tracking (Debt only) |
| Collateral | `DeactivateSupplyCaps` | Toggle supply cap tracking (Debt only) |
| Collateral | `LiquidationClaimUpdate` | Update collateral after liquidation |
| Collateral | `AuthorizedTransfer` | Transfer collateral to liq queue |
| Collateral | `Callback::WithdrawCallback` | Post-withdrawal supply cap update |
| Debt | `SetFrozen` | Syncs frozen state to Collateral |
| Debt | `NotifyDeposit` | Collateral notifies of new deposit |
| Debt | `AccrueAndValidate` | Accrue + validate solvency for withdrawal |
| Debt | `AccrueAndReturnDebt` | Return debt data for liquidation |
| Debt | `LiquidationDebtUpdate` | Update debt after liquidation repayment |
| Debt | `RecordBadDebt` | Record bad debt from liquidation |
| Liquidation | `Callback::SellWallCallback` | Process sell wall results (TODO stub) |
| Liquidation | `Callback::LiqQueueCallback` | Process liq queue results (TODO stub) |

---

## 2. Query Messages

| CDP QueryMsg | Modular Contract | Modular QueryMsg | Notes |
|---|---|---|---|
| `Config` | Collateral + Debt + Liquidation | `Config` (each) | Split across 3 configs |
| `GetBasketPositions` | **Debt** (partial) | `GetDebtPositions` | Debt portion only; full positions = TODO |
| `GetBasket` | Collateral + Debt | `GetBasket` (each) | Split: `BasketCollateral` vs `BasketDebt` |
| `GetCreditRate` | **Debt** | `GetCreditRate` | Interest rates are debt-side |
| `GetCollateralInterest` | **Debt** | `GetCollateralInterest` | TODO stub |
| `SimulateMint` | **Debt** | `SimulateMint` | TODO stub |
| `GetUserIntent` | **Debt** | `GetUserIntent` | Intents are debt-side |
| `GetAffiliates` | **Debt** | `GetAffiliates` | Affiliates are debt-side |
| `GetLiquidationStats` | **Debt** | `GetLiquidationStats` | Stats stored in Debt |
| `GetActiveDeploymentVenues` | **Debt** | `GetActiveDeploymentVenues` | Venues are debt-side |
| `GetHistoricalOraclePrices` | **Collateral** | `GetHistoricalOraclePrices` | Prices are collateral-side |
| `GetHistoricalInterestRates` | **Debt** | `GetHistoricalInterestRates` | Rates are debt-side |
| `GetRates` | **Debt** | `GetRates` | Rates are debt-side |
| `SimulateLiquidation` | **Liquidation** (+ Collateral) | `SimulateLiquidation` (Liq) / `SimulateLiquidation` (Coll) | Liq TODO; Collateral has sales sim |
| `CheckVolatilityWindow` | **Collateral** | `CheckVolatilityWindow` | Volatility is collateral-side |
| `GetHistoricalLTV` | **Collateral** | `GetHistoricalLTV` | LTV history is collateral-side |
| `GetLTVShiftInfo` | **Collateral** | `GetLTVShiftInfo` | LTV trackers are collateral-side |

### New Query Messages (not in CDP)

| Contract | QueryMsg | Purpose |
|---|---|---|
| Collateral | `GetCircuitBreakerStatus` | Query circuit breaker state per asset |
| Collateral | `GetPositionValue` | Get position collateral value + avg LTVs |
| Collateral | `GetCollateralPositions` | Query collateral-only positions |

---

## 3. Reply Handlers

| CDP Reply ID | CDP Handler | Modular Contract | Modular Reply ID | Modular Handler |
|---|---|---|---|---|
| 1 `LIQ_QUEUE_REPLY_ID` | `handle_liq_queue_reply` | **Liquidation** | 4 `LIQ_QUEUE_REPLY_ID` | `handle_liq_queue_reply` |
| 4 `WITHDRAW_REPLY_ID` | `handle_withdraw_reply` | **Collateral** | 4 `WITHDRAW_REPLY_ID` | `handle_withdraw_reply` |
| 5 `REVENUE_REPLY_ID` | `handle_revenue_reply` | **Debt** | 6 `REVENUE_REPLY_ID` | `handle_revenue_reply` |
| 6 `CLOSE_POSITION_REPLY_ID` | `handle_close_position_reply` | **Debt** | 2+3 `CLOSE_POSITION_SELL/WITHDRAW_REPLY_ID` | Split into sell + withdraw steps |
| 7 `SELL_COLLATERAL_REPLY_ID` | `handle_sell_collateral_reply` | **Collateral** | 7 `SELL_COLLATERAL_REPLY_ID` | `handle_sell_collateral_reply` |
| 8 `DEPLOYABLE_VENUE_REPLY_ID` | `handle_deployable_venue_reply` | **Debt** | 5 `DEPLOYABLE_VENUE_REPLY_ID` | `handle_deployable_venue_reply` |
| 999999 `BAD_DEBT_REPLY_ID` | no-op | **Liquidation** | 1 `DEBT_ACCRUE_REPLY_ID` (reused for error-tolerant callback) | `handle_claim_update_reply` fires BadDebtCheck |

### New Reply Handlers (Liquidation Engine SubMsg Chain)

The CDP liquidation was a single function with multiple SubMsgs. The modular Liquidation Engine splits this into a 7-step reply chain across contracts:

| Step | Liquidation Reply ID | Handler | Target Contract |
|---|---|---|---|
| 1 | `DEBT_ACCRUE_REPLY_ID (1)` | `handle_debt_accrue_reply` | Debt.AccrueAndReturnDebt |
| 2 | `COLLATERAL_ASSESS_REPLY_ID (2)` | `handle_collateral_assess_reply` | Collateral.RefreshAndAssess |
| 3 | `LIQ_QUEUE_TRANSFER_REPLY_ID (3)` | `handle_liq_queue_transfer_reply` | Collateral.AuthorizedTransfer |
| 4 | `LIQ_QUEUE_REPLY_ID (4)` | `handle_liq_queue_reply` | LiqQueue.Liquidate |
| 5 | `SELL_WALL_REPLY_ID (5)` | `handle_sell_wall_reply` | Collateral.AuthorizedSellCollateral |
| 6 | `DEBT_UPDATE_REPLY_ID (6)` | `handle_debt_update_reply` | Debt.LiquidationDebtUpdate |
| 7 | `CLAIM_UPDATE_REPLY_ID (7)` | `handle_claim_update_reply` | Collateral.LiquidationClaimUpdate |

### New Reply Handlers (Debt Contract)

| Reply ID | Handler | Purpose |
|---|---|---|
| 1 `COLLATERAL_REFRESH_REPLY_ID` | `handle_collateral_refresh_reply` | After Collateral.RefreshAndAssess for IncreaseDebt |

### New Reply Handlers (Collateral Contract)

| Reply ID | Handler | Purpose |
|---|---|---|
| 10 `DEBT_ACCRUE_VALIDATE_REPLY_ID` | `handle_debt_accrue_validate_reply` | After Debt.AccrueAndValidate for Withdraw |
| 11 `REFRESH_ASSESS_REPLY_ID` | `handle_refresh_assess_reply` | Internal oracle refresh |

---

## 4. State Migration

### Positions

| CDP State | Modular Location | Type Change |
|---|---|---|
| `POSITIONS: Map<Addr, Vec<Position>>` | **Collateral** `POSITIONS: Map<Addr, Vec<CollateralPosition>>` | Position split into CollateralPosition (collateral_assets) |
| _(same)_ | **Debt** `POSITIONS: Map<Addr, Vec<DebtPosition>>` | + DebtPosition (rate_segments, peg_rate_segments) |

The CDP `Position` (which held both collateral AND debt) is now split:
- `CollateralPosition` = position_id + collateral_assets (Vec<cAsset>)
- `DebtPosition` = position_id + rate_segments + peg_rate_segments + last_accrued + deployment_venues

### Basket

| CDP State | Modular Location | Type Change |
|---|---|---|
| `BASKET: Item<Basket>` | **Collateral** `BASKET: Item<BasketCollateral>` | Collateral params: collateral_types, supply_caps, multi_asset_caps, position_id_counter |
| _(same)_ | **Debt** `BASKET: Item<BasketDebt>` | Debt params: credit_asset, credit_price, pending_revenue, pending_bad_debt, liq_queue, oracle_set, distribute_revenue |

### Config

| CDP Config Field | Modular Location | Notes |
|---|---|---|
| `owner` | All 3 | Each contract has own owner |
| `oracle_contract` | Collateral | Price queries are collateral-side |
| `osmosis_proxy` | Collateral | Chain interactions are collateral-side |
| `liq_queue` | Debt basket + Liquidation config | LQ address shared |
| `debt_auction` | Debt + Liquidation | Bad debt routing |
| `staking_contract` | Debt + Liquidation | Fee routing |
| `stability_pool` | _(removed)_ | Replaced by Disco/deployment venues |
| `credit_price_oracle_set` | Debt basket | Oracle tracking is debt-side |
| `collateral_twap_timeframe` | Collateral | TWAP settings are collateral-side |
| `base_interest_rate` | Debt rates | Rates are debt-side |
| `revenue_distributor` | Debt | Revenue routing is debt-side |
| `chain_proxy` | Collateral + Liquidation | Swap routing for sell wall |
| `ltv_disco` | Collateral | Dynamic LTV source |

### Core State Items

| CDP State | Modular Contract | Modular State |
|---|---|---|
| `CONFIG` | All 3 | `CONFIG` (specialized per contract) |
| `BASKET` | Collateral + Debt | `BASKET` (split types) |
| `POSITIONS` | Collateral + Debt | `POSITIONS` (split types) |
| `RATES` | **Debt** | `RATES` |
| `CDT_SUPPLY` | **Debt** (as Vec<SupplyTimestamp> or Uint128) | `CDT_SUPPLY` |
| `AFFILIATES` | **Debt** | `AFFILIATES` |
| `VOLATILITY` | **Collateral** | `VOLATILITY` |
| `STORED_PRICES` | **Collateral** | `STORED_PRICES` |
| `HISTORICAL_ORACLE_PRICES` | **Collateral** | `HISTORICAL_ORACLE_PRICES` |
| `HISTORICAL_INTEREST_RATES` | **Debt** | `HISTORICAL_INTEREST_RATES` |
| `LTV_UPDATE_TRACKERS` | **Collateral** | `LTV_UPDATE_TRACKERS` |
| `LTV_HISTORY` | **Collateral** | `LTV_HISTORY` |
| `ASSET_CIRCUIT_BREAKERS` | **Collateral** | `ASSET_CIRCUIT_BREAKERS` |
| `COLLATERAL_RATE_ASSURANCE` | **Collateral** | `COLLATERAL_RATE_ASSURANCE` |
| `FREEZE_TIMER` | **Collateral** | `FREEZE_TIMER` |
| `USER_INTENTS` | **Debt** | `USER_INTENTS` |
| `ACTIVE_DEPLOYMENT_VENUES` | **Debt** | `ACTIVE_DEPLOYMENT_VENUES` |
| `LIQUIDATION_STATS` | **Debt** | `LIQUIDATION_STATS` |
| `REDEMPTION_OPT_IN` | _(removed)_ | Feature removed |
| `OWNERSHIP_TRANSFER` | _(removed)_ | Simplified to direct update |
| `CONTRACT` (version) | _(removed)_ | Standard CW2 not used |

### Propagation State

| CDP Propagation | Modular Contract | Modular Propagation |
|---|---|---|
| `WITHDRAW` (WithdrawPropagation) | **Collateral** | `WITHDRAW_PROPAGATION` |
| `CLOSE_POSITION` (ClosePositionPropagation) | **Collateral** + **Debt** | Split: Collateral has `CLOSE_POSITION_PROPAGATION`, Debt has `CLOSE_POSITION_PROPAGATION` |
| `SELL_COLLATERAL` (SellCollateralPropagation) | **Collateral** | `SELL_COLLATERAL_PROPAGATION` |
| `LIQUIDATION` (LiquidationPropagation) | **Liquidation** | `LIQUIDATION_PROPAGATION` |
| `DEPLOYABLE_VENUE` (DeployableVenuePropagation) | **Debt** | `DEPLOYABLE_VENUE_PROPAGATION` |
| _(new)_ | **Debt** | `ACCRUE_VALIDATE_PROPAGATION` (for cross-contract withdraw validation) |
| _(new)_ | **Debt** | `INCREASE_DEBT_PROPAGATION` (for cross-contract debt minting) |

---

## 5. Function-Level Mapping

### positions.rs

| CDP Function | Modular Contract | Modular Function | File |
|---|---|---|---|
| `deposit()` | **Collateral** | `deposit()` | positions.rs |
| `withdraw()` | **Collateral** | `withdraw()` → Debt.AccrueAndValidate → `authorized_withdraw()` | positions.rs |
| `increase_debt()` | **Debt** | `increase_debt()` → Collateral.RefreshAndAssess → mint | positions.rs |
| `repay()` | **Debt** | `repay()` | positions.rs |
| `close_position()` | **Debt** | `close_position()` | positions.rs |
| `create_basket()` | Collateral + Debt | Split into `instantiate()` of each | contract.rs |
| `edit_basket()` | Collateral + Debt | `edit_basket()` in each (different params) | contract.rs |
| `edit_cAsset()` | **Collateral** | `edit_casset()` | contract.rs |
| `set_intents()` | **Debt** | `set_user_intents()` | positions.rs |
| `fulfill_intents()` | **Debt** | `fulfill_intents()` | positions.rs |
| `set_deployment_venue()` | **Debt** | `set_deployment_venue()` | positions.rs |
| `set_affiliate()` | **Debt** | `set_affiliate()` | positions.rs |
| `check_and_clear_debt_delta()` | **Debt** | `check_and_clear_debt_delta()` | positions.rs |

### liquidations.rs

| CDP Function | Modular Contract | Modular Function | File |
|---|---|---|---|
| `liquidate()` | **Liquidation** | `liquidate()` (orchestrator) | liquidations.rs |
| `get_repay_quantities()` | **Liquidation** | `get_repay_quantities()` | liquidations.rs |
| `per_asset_fulfillments()` | **Liquidation** | `per_asset_fulfillments()` (TODO stub) | liquidations.rs |
| `get_deployable_venues_user_repay_amount()` | _(TODO)_ | Not yet ported | - |
| Insolvency check (inline) | **Liquidation** reply handler | Inline in `handle_collateral_assess_reply` | reply.rs |
| Fee calculation (inline) | **Liquidation** | Via `get_repay_quantities` | liquidations.rs |
| LQ routing (inline) | **Liquidation** | `handle_liq_queue_transfer_reply` + `handle_liq_queue_reply` | reply.rs |
| Sell wall (inline) | **Liquidation** | `handle_sell_wall_reply` → Collateral.AuthorizedSellCollateral | reply.rs |
| Bad debt callback | **Liquidation** | `handle_claim_update_reply` → `BadDebtCheck` callback | reply.rs |

### rates.rs

| CDP Function | Modular Contract | Modular Function | File |
|---|---|---|---|
| `adaptive_smooth_rate()` | **Debt** | `adaptive_smooth_rate()` | rates.rs |
| `taylor_exp()` | **Debt** | `taylor_exp()` | rates.rs |
| `signed_taylor_exp()` | **Debt** | `signed_taylor_exp()` | rates.rs |
| `accrue()` | **Debt** | `accrue()` / `accrue_position()` | rates.rs |
| `external_accrue_call()` | **Debt** | `accrue()` execute handler | positions.rs |

### debt_manager.rs

| CDP Function | Modular Contract | Modular Function | File |
|---|---|---|---|
| `accrue_segments()` | **Debt** | `accrue_segments()` | debt_manager.rs |
| `get_total_from_segments()` | **Debt** | `get_total_from_segments()` | debt_manager.rs |
| `repay_segments_default_order()` | **Debt** | `repay_segments_default_order()` | debt_manager.rs |
| `repay_segments_targeted()` | **Debt** | `repay_segments_targeted()` | debt_manager.rs |
| `create_segments_from_split()` | **Debt** | `create_segments_from_split()` | debt_manager.rs |
| `apply_delta_to_regular_debt()` | **Debt** | `apply_delta_to_regular_debt()` | debt_manager.rs |
| `apply_delta_to_peg_debt()` | **Debt** | `apply_delta_to_peg_debt()` | debt_manager.rs |
| `apply_delta_to_caps()` | **Debt** | `apply_delta_to_caps()` | debt_manager.rs |

### risk_engine.rs

| CDP Function | Modular Contract | Modular Function | File |
|---|---|---|---|
| `update_basket_tally()` | **Collateral** | `update_basket_tally()` / `activate_supply_caps()` / `deactivate_supply_caps()` | risk_engine.rs |
| `assert_basket_assets()` | **Collateral** | `assert_basket_assets()` | risk_engine.rs |
| `transform_caps_based_on_volatility()` | **Collateral** | (inline in risk_engine.rs) | risk_engine.rs |

### ltv_updater.rs

| CDP Function | Modular Contract | Modular Function | File |
|---|---|---|---|
| `update_basket_ltvs()` | **Collateral** | `update_basket_ltvs()` | ltv_updater.rs |
| `update_asset_ltv()` | **Collateral** | `update_asset_ltv()` | ltv_updater.rs |

### circuit_breaker.rs

| CDP Function | Modular Contract | Modular Function | File |
|---|---|---|---|
| `check_and_update_circuit_breaker()` | **Collateral** | `check_and_update_circuit_breaker()` | circuit_breaker.rs |
| `check_assets_not_frozen()` | **Collateral** | `check_assets_not_frozen()` | circuit_breaker.rs |

### query.rs

| CDP Function | Modular Contract | Modular Function | File |
|---|---|---|---|
| `query_basket_positions()` | **Debt** | `query_debt_positions()` | query.rs |
| `query_basket_credit_interest()` | **Debt** | `query_credit_rate()` | query.rs |
| `query_collateral_rates()` | **Debt** | `query_collateral_interest()` (TODO) | query.rs |
| `query_user_intent_state()` | **Debt** | `query_user_intents()` | query.rs |
| `query_liquidation_stats()` | **Debt** | `query_liquidation_stats()` | query.rs |
| `query_active_deployment_venues()` | **Debt** | `query_active_deployment_venues()` | query.rs |
| `query_historical_oracle_prices()` | **Collateral** | `query_historical_oracle_prices()` | query.rs |
| `query_historical_interest_rates()` | **Debt** | `query_historical_interest_rates()` | query.rs |
| `query_volatility_window()` | **Collateral** | `query_volatility_window()` | query.rs |
| `query_simulate_liquidation()` | **Collateral** + **Liquidation** | Collateral has sell sim, Liquidation has full sim (TODO) | query.rs |
| `query_historical_ltv()` | **Collateral** | `query_historical_ltv()` | query.rs |
| `query_ltv_shift_info()` | **Collateral** | `query_ltv_shift_info()` | query.rs |
| `insolvency_check()` | **Liquidation** (inline) | Inline in `handle_collateral_assess_reply` | reply.rs |
| `get_cAsset_ratios()` | **Collateral** | `refresh_and_assess()` (execute) + `get_casset_ratios_and_prices()` (internal) | query.rs / risk_engine.rs |
| `simulate_LTV_mint()` | **Debt** | `query_simulate_mint()` (TODO) | query.rs |

### reply.rs

| CDP Function | Modular Contract | Modular Function | File |
|---|---|---|---|
| `handle_liq_queue_reply()` | **Liquidation** | `handle_liq_queue_reply()` | reply.rs |
| `handle_withdraw_reply()` | **Collateral** | `handle_withdraw_reply()` | reply.rs |
| `handle_revenue_reply()` | **Debt** | `handle_revenue_reply()` | reply.rs |
| `handle_close_position_reply()` | **Debt** | `handle_close_position_sell_reply()` + `handle_close_position_withdraw_reply()` | reply.rs |
| `handle_sell_collateral_reply()` | **Collateral** | `handle_sell_collateral_reply()` | reply.rs |
| `handle_deployable_venue_reply()` | **Debt** | `handle_deployable_venue_reply()` | reply.rs |

### callbacks (contract.rs)

| CDP Callback | Modular Contract | Modular Handler | File |
|---|---|---|---|
| `BadDebtCheck` | **Liquidation** | `bad_debt_check()` (TODO stub) | liquidations.rs |
| `ClosePositionCallback` | **Debt** | `close_position_callback()` | positions.rs |

---

## 6. Shared Pure Logic (packages/membrane)

| Module | Purpose | Used By |
|---|---|---|
| `membrane::risk` | `compute_ltv`, `check_solvency`, `can_borrow`, `compute_weighted_avg_ltv`, `compute_liquidation_fee` | Collateral, Debt, Liquidation |
| `membrane::math` | `Decimal256`, `Uint256`, `decimal_multiplication`, `decimal_division` | All (via PriceResponse) |
| `membrane::oracle::PriceResponse` | `get_value(amount)`, `get_amount(value)` with decimal normalization | Collateral, Liquidation |
| `membrane::types` | `cAsset`, `Asset`, `AssetInfo`, `Basket`, `Position`, `Rates`, `SupplyCap`, etc. | All |

---

## 7. Cross-Contract Interaction Flows

### Deposit Flow
```
User → Collateral.Deposit
         ├── Validate assets in basket
         ├── Create/update CollateralPosition
         ├── Update supply caps
         └── Debt.NotifyDeposit (create DebtPosition if new)
               └── Set up accrual, affiliate
```

### Withdraw Flow
```
User → Collateral.Withdraw
         └── SubMsg → Debt.AccrueAndValidate
                        ├── Accrue interest
                        ├── Validate solvency post-withdrawal
                        └── Reply → Collateral.AuthorizedWithdraw
                                      ├── Execute bank send
                                      └── Update supply caps
```

### Increase Debt Flow
```
User → Debt.IncreaseDebt
         └── SubMsg → Collateral.RefreshAndAssess
                        ├── Query oracle prices
                        ├── Update circuit breakers
                        └── Reply → Debt (handle_collateral_refresh_reply)
                                      ├── Validate LTV
                                      ├── Create debt segments
                                      └── Mint credit tokens
```

### Liquidation Flow
```
Anyone → Liquidation.Liquidate
           └── SubMsg → Debt.AccrueAndReturnDebt
                          └── Reply (step 1) → Collateral.RefreshAndAssess
                                                 └── Reply (step 2) → Insolvency check + fee calc
                                                                        └── Collateral.AuthorizedTransfer (to LQ)
                                                                             └── Reply (step 3) → LiqQueue.Liquidate
                                                                                                    └── Reply (step 4) → Accumulate repayment
                                                                                                                           └── Collateral.AuthorizedSellCollateral
                                                                                                                                └── Reply (step 5) → Debt.LiquidationDebtUpdate
                                                                                                                                                       └── Reply (step 6) → Collateral.LiquidationClaimUpdate
                                                                                                                                                                              └── Reply (step 7) → Bad debt check + cleanup
```

### Close Position Flow
```
User → Debt.ClosePosition
         └── SubMsg → Collateral.AuthorizedSellCollateral
                        └── Reply (sell) → Debt (handle_close_position_sell_reply)
                                             └── SubMsg → Collateral.AuthorizedWithdraw
                                                            └── Reply (withdraw) → Debt (handle_close_position_withdraw_reply)
                                                                                     └── Repay debt + cleanup
```

---

## 8. Known Gaps / TODOs

### 8.1 Summary Table

| # | Item | Contract | Severity | Status | CDP Equivalent |
|---|------|----------|----------|--------|----------------|
| 1 | **Caller fee not sent** | Liquidation | **BUG** | Coins computed, never sent | CDP sends via `BankMsg::Send` to fee_recipient |
| 2 | **BadDebtCheck reply ID collision** | Liquidation | **BUG** | Uses `DEBT_ACCRUE_REPLY_ID` (1) | CDP uses `BAD_DEBT_REPLY_ID` (999999) |
| 3 | `per_asset_fulfillments()` | Liquidation | Dead code | Returns empty vec | CDP has full implementation (714-1080 lines) |
| 4 | `bad_debt_check()` callback | Liquidation | Stub | Returns Ok with attrs | CDP has `check_and_fulfill_bad_debt` (145 lines) |
| 5 | `sell_wall_callback()` | Liquidation | Dead code | Returns Ok with attrs | CDP handles inline in reply |
| 6 | `liq_queue_callback()` | Liquidation | Dead code | Returns Ok with attrs | CDP handles inline in reply |
| 7 | `query_simulate_liquidation()` | Liquidation | Stub | Returns hardcoded zeros | CDP has `simulate_liquidation_sales` |
| 8 | `query_simulate_mint()` | Debt | Stub | Returns zeros | CDP has `simulate_LTV_mint` |
| 9 | `query_collateral_interest()` | Debt | Stub | Returns empty | CDP has `query_collateral_rates` |
| 10 | `query_full_positions()` | Debt | Stub | Returns empty | CDP returns both in `query_basket_positions` |
| 11 | No liquidation stats recording | Liquidation | Feature gap | Not stored | CDP records `LiquidationEvent` history |
| 12 | No deployable venue liq routing | Liquidation | Feature gap | Not ported | CDP has `get_deployable_venues_user_repay_amount` |
| 13 | Redemption/Redeemability | _(removed)_ | Removed | Feature removed | CDP had `RedeemCollateral`, `EditRedeemability` |
| 14 | Stability Pool routing | _(removed)_ | Replaced | Replaced by Disco | CDP had `SP_REPLY_ID`, stability pool routing |

### 8.2 Bug Details

#### BUG #1: Caller fee computed but never sent

**Location:** `liquidation-engine/src/reply.rs` lines 576-602 (`handle_claim_update_reply`)

**What happens:** The contract correctly computes `caller_coins` — a `Vec<Coin>` containing the liquidator's fee for each collateral asset, proportional to `cAsset_ratios` and capped at position holdings. But lines 596-602 are an empty block with a comment. No `BankMsg::Send` is ever created.

**Root cause:** The liquidator's address (`info.sender` from the original `liquidate()` call) is not stored in `LiquidationPropagation`. By the time `handle_claim_update_reply` runs (step 7), the caller's address is lost.

**CDP reference:** `cdp/liquidations.rs` lines 1041-1047:
```rust
let msg = CosmosMsg::Bank(BankMsg::Send {
    to_address: fee_recipient.clone(),
    amount: caller_coins.clone(),
});
```

**Fix options:**
1. Add `caller: Addr` field to `LiquidationPropagation` (set in `liquidate()` from `info.sender`)
2. Send caller fee in the initial `liquidate()` response before the SubMsg chain
3. Route fee through the Debt contract's `LiquidationDebtUpdate` response

**Test:** `reply_handler_tests::claim_update_caller_fee_computed_but_not_sent`

---

#### BUG #2: BadDebtCheck callback reuses DEBT_ACCRUE_REPLY_ID

**Location:** `liquidation-engine/src/reply.rs` line 616

```rust
let bad_debt_sub = SubMsg::reply_on_error(bad_debt_callback, DEBT_ACCRUE_REPLY_ID);
```

**What happens:** The BadDebtCheck self-callback uses `DEBT_ACCRUE_REPLY_ID` (1) for its `reply_on_error`. If the callback fails, the error reply routes to `handle_debt_accrue_reply` — which expects an `AccrueDebtResponse`, not a BadDebtCheck error.

**Impact:** Since it's `reply_on_error` (only fires on failure) and `handle_debt_accrue_reply` checks `SubMsgResult::Ok` first, the error branch would propagate the error string. But:
- The error message will say "Debt accrual failed during liquidation" instead of "BadDebtCheck failed"
- The reply ID is semantically wrong and masks the real failure source
- If future changes to `handle_debt_accrue_reply`'s error branch have side effects, they'd fire unexpectedly

**CDP reference:** Uses `BAD_DEBT_REPLY_ID: u64 = 999999u64` (a dedicated ID)

**Fix:** Add `BAD_DEBT_REPLY_ID: u64 = 8u64` to `state.rs`, handle it in `handle_reply()` dispatcher (can be a no-op handler since it's reply_on_error and failures should just propagate).

**Test:** `reply_handler_tests::claim_update_bad_debt_callback_uses_wrong_reply_id`

### 8.3 Stub Function Details

#### `per_asset_fulfillments()` — Dead Code

**Location:** `liquidation-engine/src/liquidations.rs` lines 129-137

This function exists but is **never called** by any code path. In the CDP, it's a 366-line function handling caller fee extraction, liq queue processing, sell wall creation, and fee distribution. In the modular design, this logic is split across the 7-step reply chain:
- Caller fee → `handle_collateral_assess_reply` (step 2)
- Liq queue → `handle_liq_queue_transfer_reply` + `handle_liq_queue_reply` (steps 3-4)
- Sell wall → `handle_sell_wall_reply` (step 5)
- Fee distribution → `handle_claim_update_reply` (step 7, but broken — see Bug #1)

**Recommendation:** Remove the function or keep as reference. Do not port — logic already lives in reply handlers.

#### `bad_debt_check()` callback — Incomplete Stub

**Location:** `liquidation-engine/src/liquidations.rs` lines 142-157

Called as a self-callback from `handle_claim_update_reply` (step 7). Currently returns `Ok` with basic attributes. Does not:
- Query Debt contract for remaining position debt
- Query Collateral contract for remaining collateral value
- Check if collateral value < $1 (the bad debt threshold)
- Use basket revenue to cover bad debt
- Route to LTV Disco for per-asset bad debt handling
- Send to debt auction for MBRN auction

**CDP reference:** `cdp/contract.rs` `check_and_fulfill_bad_debt` (lines 811-955, ~145 lines)

**Note:** `handle_claim_update_reply` already does a basic bad debt check inline (lines 551-571): if `total_repaid < total_debt`, it sends `RecordBadDebt` to the Debt contract. The callback is meant for a more thorough post-liquidation check, but currently does nothing beyond the inline check.

#### `sell_wall_callback()` and `liq_queue_callback()` — Dead Code

**Location:** `liquidation-engine/src/liquidations.rs` lines 162-192

These are never called. Their logic lives in reply handlers (steps 4-5). **Recommend removal.**

#### `query_simulate_liquidation()` — Hardcoded Zeros

**Location:** `liquidation-engine/src/query.rs` lines 12-33

Returns `SimulateLiquidationResponse` with all zero fields. The CDP version queries the chain proxy for swap simulations. Used by the frontend to estimate liquidation costs before execution.

**To port:** Needs cross-contract queries to Debt (for position debt) and Collateral (for collateral assessment), then runs `get_repay_quantities()` and estimates sell wall slippage.

### 8.4 Feature Gaps

#### No liquidation stats recording

The Debt contract has `LIQUIDATION_STATS: Item<Vec<LiquidationEvent>>` and the Collateral contract has historical data maps, but the Liquidation Engine itself doesn't record any liquidation history. After a liquidation completes, there's no queryable record of the event in the engine contract.

**Test:** `reply_handler_tests::claim_update_no_liquidation_stats_recorded`

#### No deployable venue liquidation routing

The CDP has `get_deployable_venues_user_repay_amount()` which checks if collateral is deployed in venues (e.g., lending protocols) and includes withdrawal as part of the liquidation flow. The modular design doesn't handle this — if collateral is deployed, the liquidation may not access it.

**Impact:** Positions with deployed collateral may be under-liquidated.

---

## 9. Architectural Differences

| Aspect | CDP (Monolith) | Modular Split |
|---|---|---|
| **Liquidation** | Single function with inline SubMsgs | 7-step reply chain across 3 contracts |
| **Position data** | Single `Position` struct | Split `CollateralPosition` + `DebtPosition` |
| **Basket** | Single `Basket` | Split `BasketCollateral` + `BasketDebt` |
| **Solvency check** | Inline in `liquidate()` / `withdraw()` / `increase_debt()` | Cross-contract via `RefreshAndAssess` + `AccrueAndValidate` |
| **Oracle queries** | Direct from any function | Centralized in Collateral contract |
| **Supply cap updates** | `update_basket_tally()` inline | Cross-contract via `ActivateSupplyCaps` / `DeactivateSupplyCaps` |
| **Rate accrual** | Inline in position operations | Cross-contract via `AccrueAndValidate` / `AccrueAndReturnDebt` |
| **LTV dynamics** | Same contract, same storage | Collateral-only, Disco integration |
| **Circuit breaker** | Same contract | Collateral-only, assessment includes `any_frozen` flag |
| **Frozen state** | Single `FROZEN` item | Synced between Collateral and Debt via `SetFrozen` |

---

## 10. Test Coverage Summary

| Contract | Unit Tests | Proptest | Total |
|---|---|---|---|
| CDP | ~200+ (integration) | No | ~200+ |
| **Collateral** | **171** | 12 strategies x 256 cases | **171 + 3,072 effective** |
| **Debt** | **314** | 12 strategies x 256 cases | **314 + 3,072 effective** |
| **Liquidation** | **~140** | 12 strategies x 256 cases | **~140 + 3,072 effective** |
| **Total modular** | **~625** | 36 strategies | **~625 + 9,216 effective** |

**Liquidation Engine Test Breakdown:**
- `integration_tests.rs` — 31 tests (instantiation, config, auth, queries, entry)
- `financial_tests.rs` — 29 tests (fee calculation, reply parsing, simulate, edge cases)
- `reply_handler_tests.rs` — 47 tests (all 7 reply handlers + 4 gap-documenting tests)
- `propagation_tests.rs` — 10 tests (state lifecycle)
- `stub_function_tests.rs` — 14 tests (8 current behavior + 6 `#[ignore]` intended behavior)
- `proptest_tests.rs` — 12 property strategies (256+ cases each)

**Gap-documenting tests (pass but document broken/missing logic):**
- `claim_update_caller_fee_computed_but_not_sent` — BUG #1
- `claim_update_bad_debt_callback_uses_wrong_reply_id` — BUG #2
- `claim_update_no_liquidation_stats_recorded` — Feature gap
- All `_stub_` tests — Document current no-op behavior

**Ignored tests (will fail until stubs are implemented):**
- `per_asset_fulfillments_should_return_proportional_amounts`
- `bad_debt_check_should_query_debt_and_route_to_auction`
- `sell_wall_callback_should_build_debt_update_submsg`
- `liq_queue_callback_should_update_propagation_and_continue`
- `simulate_liquidation_should_return_realistic_estimates`
