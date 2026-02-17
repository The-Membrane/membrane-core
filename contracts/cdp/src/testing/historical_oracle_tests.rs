use std::str::FromStr;

use cosmwasm_std::{
    coin, to_json_binary, Addr, Binary, Decimal, Empty, Response, StdResult,
    Uint128,
};
use cw_multi_test::{App, AppBuilder, Contract, ContractWrapper, Executor};
use cosmwasm_schema::cw_serde;

use membrane::cdp::{ExecuteMsg, InstantiateMsg, QueryMsg, CreateBasket, HistoricalOraclePricesResponse};
use membrane::oracle::{AssetResponse, PriceResponse};
use membrane::types::{
    cAsset, Asset, AssetInfo, AssetOracleInfo,
};

const USER: &str = "user";
const ADMIN: &str = "admin";

//CDP Contract
pub fn cdp_contract() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new_with_empty(
        crate::contract::execute,
        crate::contract::instantiate,
        crate::contract::query,
    )
    .with_reply(crate::contract::reply);
    Box::new(contract)
}

//Mock Oracle Contract
#[cw_serde]
pub enum Oracle_MockExecuteMsg {
    AddAsset {
        asset_info: AssetInfo,
        oracle_info: Vec<AssetOracleInfo>,
    },
    EditAsset {
        asset_info: AssetInfo,
        oracle_info: Option<Vec<AssetOracleInfo>>,
        remove: bool,
    },
}

#[cw_serde]
pub struct Oracle_MockInstantiateMsg {}

#[cw_serde]
pub enum Oracle_MockQueryMsg {
    Prices {
        asset_infos: Vec<AssetInfo>,
        twap_timeframe: u64,
        oracle_time_limit: u64,
    },
    Assets {
        asset_infos: Vec<AssetInfo>,
    },
}

pub fn oracle_contract() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(
        |deps, _, info, msg: Oracle_MockExecuteMsg| -> StdResult<Response> {
            Ok(Response::default())
        },
        |_, _, _, _: Oracle_MockInstantiateMsg| -> StdResult<Response> {
            Ok(Response::default())
        },
        |_, _, msg: Oracle_MockQueryMsg| -> StdResult<Binary> {
            match msg {
                Oracle_MockQueryMsg::Prices {
                    asset_infos,
                    twap_timeframe: _,
                    oracle_time_limit: _,
                } => {
                    let mut prices = vec![];
                    for asset_info in asset_infos.iter() {
                        if asset_info.to_string() == String::from("credit_fulldenom") {
                            prices.push(PriceResponse {
                                prices: vec![],
                                price: Decimal::percent(98),
                                decimals: 6,
                            });
                        } else if asset_info.to_string() == String::from("lp_denom") {
                            prices.push(PriceResponse {
                                prices: vec![],
                                price: Decimal::from_ratio(2u128, 1u128),
                                decimals: 18,
                            });
                        } else {
                            prices.push(PriceResponse {
                                prices: vec![],
                                price: Decimal::one(),
                                decimals: 6,
                            });
                        }
                    }
                    
                    Ok(to_json_binary(&prices)?)                        
                }
                Oracle_MockQueryMsg::Assets { asset_infos } => {
                    let mut assets = vec![];
                    for asset_info in asset_infos.iter() {
                        assets.push(AssetResponse {
                            asset_info: asset_info.clone(),
                            oracle_info: vec![AssetOracleInfo {
                                basket_id: Uint128::new(1),
                                pyth_price_feed_id: None,
                                pools_for_osmo_twap: vec![],
                                is_usd_par: false,
                                lp_pool_info: None,
                                vault_info: None,
                                decimals: 6,
                            }],
                        });
                    }
                    Ok(to_json_binary(&assets)?)
                }
            }
        },
    );
    Box::new(contract)
}

//Mock LTV Disco Contract
#[cw_serde]
pub enum LTVDisco_MockExecuteMsg {}

#[cw_serde]
pub struct LTVDisco_MockInstantiateMsg {}

#[cw_serde]
pub enum LTVDisco_MockQueryMsg {
    GetAverageLTVs { assets: Vec<String> }
}

pub fn ltv_disco_contract() -> Box<dyn Contract<Empty>> {
    let contract = ContractWrapper::new(
        |deps, _, info, msg: LTVDisco_MockExecuteMsg| -> StdResult<Response> {
            Ok(Response::default())
        },
        |_, _, _, _: LTVDisco_MockInstantiateMsg| -> StdResult<Response> {
            Ok(Response::default())
        },
        |_, _, msg: LTVDisco_MockQueryMsg| -> StdResult<Binary> {
            match msg {
                LTVDisco_MockQueryMsg::GetAverageLTVs { assets: _ } => {
                    Ok(to_json_binary(&membrane::ltv_disco::AverageLTVsResponse {
                        average_max_ltv: Decimal::percent(80),
                        average_max_borrow_ltv: Decimal::percent(70),
                    })?)
                }
            }
        },
    );
    Box::new(contract)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_app() -> App {
        AppBuilder::new().build(|router, api, storage| {
            // Pre-fund bigger_bank (like in integration_tests.rs)
            router
                .bank
                .init_balance(
                    storage,
                    &api.addr_make("bigger_bank"),
                    vec![
                        coin(100_000_000_000, "denom1"),
                        coin(100_000_000_000, "denom2"),
                        coin(100_000_000_000, "lp_denom"),
                        coin(100_000_000_000, "credit_fulldenom"),
                    ],
                )
                .unwrap();
        })
    }

    fn instantiate_cdp_contract(app: &mut App, oracle_addr: Addr, admin_addr: Addr) -> Addr {
        let cdp_code_id = app.store_code(cdp_contract());
        let ltv_disco_code_id = app.store_code(ltv_disco_contract());
        
        let ltv_disco_addr = app
            .instantiate_contract(
                ltv_disco_code_id,
                admin_addr.clone(),
                &LTVDisco_MockInstantiateMsg {},
                &[],
                "ltv_disco",
                None,
            )
            .unwrap();

        let msg = InstantiateMsg {
            liq_fee: Decimal::percent(5),
            ltv_disco: ltv_disco_addr.to_string(),
            oracle_time_limit: 300,
            rate_slope_multiplier: Decimal::one(),
            debt_minimum: Uint128::new(1000),
            base_debt_cap_multiplier: Uint128::new(1),
            collateral_twap_timeframe: 300,
            credit_twap_timeframe: 300,
            owner: Some(admin_addr.to_string()),
            staking_contract: None,
            oracle_contract: Some(oracle_addr.to_string()),
            chain_proxy: None,
            debt_auction: None,
            liquidity_contract: None,
            discounts_contract: None,
            create_basket: CreateBasket {
                basket_id: Uint128::new(1),
                collateral_types: vec![
                    cAsset {
                        asset: Asset {
                            info: AssetInfo::NativeToken { denom: "denom1".to_string() },
                            amount: Uint128::zero(),
                        },
                        max_borrow_LTV: Decimal::percent(70),
                        max_LTV: Decimal::percent(80),
                        pool_info: None,
                        rate_index: Decimal::one(),
                        peg_rate_index: Decimal::one(),
                        force_redemptions: None,
                    },
                    cAsset {
                        asset: Asset {
                            info: AssetInfo::NativeToken { denom: "denom2".to_string() },
                            amount: Uint128::zero(),
                        },
                        max_borrow_LTV: Decimal::percent(70),
                        max_LTV: Decimal::percent(80),
                        pool_info: None,
                        rate_index: Decimal::one(),
                        peg_rate_index: Decimal::one(),
                        force_redemptions: None,
                    },
                ],
                credit_asset: Asset {
                    info: AssetInfo::NativeToken { denom: "credit_fulldenom".to_string() },
                    amount: Uint128::zero(),
                },
                credit_price: Decimal::percent(98),
                base_interest_rate: Some(Decimal::percent(2)),
                credit_pool_infos: vec![],
                liq_queue: None,
            },
        };

        app.instantiate_contract(
            cdp_code_id,
            admin_addr,
            &msg,
            &[],
            "cdp",
            None,
        )
        .unwrap()
    }

    #[test]
    fn test_historical_oracle_updates_on_deposit() {
        let mut app = setup_test_app();
        let admin_addr = app.api().addr_make(ADMIN);
        let user_addr = app.api().addr_make(USER);
        
        // Fund user from bigger_bank (per debugging guide)
        app.send_tokens(
            app.api().addr_make("bigger_bank"),
            user_addr.clone(),
            &[
                coin(100_000_000_000, "denom1"),
                coin(100_000_000_000, "denom2"),
            ],
        )
        .unwrap();
        
        let oracle_code_id = app.store_code(oracle_contract());
        let oracle_addr = app
            .instantiate_contract(
                oracle_code_id,
                admin_addr.clone(),
                &Oracle_MockInstantiateMsg {},
                &[],
                "oracle",
                None,
            )
            .unwrap();

        let cdp_addr = instantiate_cdp_contract(&mut app, oracle_addr.clone(), admin_addr.clone());

        // Check initial state - no historical prices
        let historical_prices: HistoricalOraclePricesResponse = app
            .wrap()
            .query_wasm_smart(
                &cdp_addr,
                &QueryMsg::GetHistoricalOraclePrices { 
                    asset: "denom1".to_string() 
                },
            )
            .unwrap();
        
        assert_eq!(historical_prices.prices.len(), 0);

        // Perform deposit operation
        let deposit_msg = ExecuteMsg::Deposit {
            position_owner: Some(user_addr.to_string()),
            position_id: None,
            affiliate_address: None,
        };

        let result = app.execute_contract(
            user_addr.clone(),
            cdp_addr.clone(),
            &deposit_msg,
            &[coin(1000, "denom1"), coin(1000, "denom2")],
        );

        // Verify deposit succeeded
        assert!(result.is_ok());
        
        // Check that historical prices were stored
        let historical_prices: HistoricalOraclePricesResponse = app
            .wrap()
            .query_wasm_smart(
                &cdp_addr,
                &QueryMsg::GetHistoricalOraclePrices { 
                    asset: "denom1".to_string() 
                },
            )
            .unwrap();
        
        // Should have 1 price entry for denom1
        assert_eq!(historical_prices.prices.len(), 1);
        assert_eq!(historical_prices.prices[0].price, "1"); // Price from oracle_contract
        
        // Check denom2 as well
        let historical_prices_denom2: HistoricalOraclePricesResponse = app
            .wrap()
            .query_wasm_smart(
                &cdp_addr,
                &QueryMsg::GetHistoricalOraclePrices { 
                    asset: "denom2".to_string() 
                },
            )
            .unwrap();
        
        assert_eq!(historical_prices_denom2.prices.len(), 1);
        assert_eq!(historical_prices_denom2.prices[0].price, "1");
    }

    #[test]
    fn test_historical_oracle_only_collateral_assets() {
        // This test verifies that only collateral assets are stored, not credit assets
        let mut app = setup_test_app();
        let admin_addr = app.api().addr_make(ADMIN);
        let user_addr = app.api().addr_make(USER);
        
        // Fund user from bigger_bank (per debugging guide)
        app.send_tokens(
            app.api().addr_make("bigger_bank"),
            user_addr.clone(),
            &[
                coin(100_000_000_000, "denom1"),
                coin(100_000_000_000, "denom2"),
            ],
        )
        .unwrap();
        
        let oracle_code_id = app.store_code(oracle_contract());
        let oracle_addr = app
            .instantiate_contract(
                oracle_code_id,
                admin_addr.clone(),
                &Oracle_MockInstantiateMsg {},
                &[],
                "oracle",
                None,
            )
            .unwrap();

        let cdp_addr = instantiate_cdp_contract(&mut app, oracle_addr.clone(), admin_addr.clone());

        // Deposit operation should only store collateral asset prices, not credit asset prices
        let deposit_msg = ExecuteMsg::Deposit {
            position_owner: Some(user_addr.to_string()),
            position_id: None,
            affiliate_address: None,
        };

        app.execute_contract(
            user_addr.clone(),
            cdp_addr.clone(),
            &deposit_msg,
            &[coin(1000, "denom1"), coin(1000, "denom2")],
        ).unwrap();

        // Check that collateral asset prices were stored
        let historical_prices_denom1: HistoricalOraclePricesResponse = app
            .wrap()
            .query_wasm_smart(
                &cdp_addr,
                &QueryMsg::GetHistoricalOraclePrices { 
                    asset: "denom1".to_string() 
                },
            )
            .unwrap();
        
        assert_eq!(historical_prices_denom1.prices.len(), 1);
        
        // Check that credit asset prices were NOT stored
        let historical_prices_credit: HistoricalOraclePricesResponse = app
            .wrap()
            .query_wasm_smart(
                &cdp_addr,
                &QueryMsg::GetHistoricalOraclePrices { 
                    asset: "credit_fulldenom".to_string() 
                },
            )
            .unwrap();
        
        // Credit asset should not be stored in historical oracle
        assert_eq!(historical_prices_credit.prices.len(), 0);
    }

    #[test]
    fn test_historical_oracle_only_stores_fresh_prices() {
        let mut app = setup_test_app();
        let admin_addr = app.api().addr_make(ADMIN);
        let user_addr = app.api().addr_make(USER);
        
        // Fund user from bigger_bank (per debugging guide)
        app.send_tokens(
            app.api().addr_make("bigger_bank"),
            user_addr.clone(),
            &[
                coin(100_000_000_000, "denom1"),
                coin(100_000_000_000, "denom2"),
            ],
        )
        .unwrap();
        
        let oracle_code_id = app.store_code(oracle_contract());
        let oracle_addr = app
            .instantiate_contract(
                oracle_code_id,
                admin_addr.clone(),
                &Oracle_MockInstantiateMsg {},
                &[],
                "oracle",
                None,
            )
            .unwrap();

        let cdp_addr = instantiate_cdp_contract(&mut app, oracle_addr.clone(), admin_addr.clone());

        // First deposit - should store fresh prices
        let deposit_msg = ExecuteMsg::Deposit {
            position_owner: Some(user_addr.to_string()),
            position_id: None,
            affiliate_address: None,
        };

        app.execute_contract(
            user_addr.clone(),
            cdp_addr.clone(),
            &deposit_msg,
            &[coin(1000, "denom1"), coin(1000, "denom2")],
        ).unwrap();

        // Check that prices were stored
        let historical_prices: HistoricalOraclePricesResponse = app
            .wrap()
            .query_wasm_smart(
                &cdp_addr,
                &QueryMsg::GetHistoricalOraclePrices { 
                    asset: "denom1".to_string() 
                },
            )
            .unwrap();
        
        assert_eq!(historical_prices.prices.len(), 1);

        // Second deposit immediately after - should use cached prices, not store new ones
        // Advance time to ensure we're within oracle_time_limit (300 seconds)
        app.update_block(|block| {
            block.time = block.time.plus_seconds(100); // Within 300 second limit
        });

        app.execute_contract(
            user_addr.clone(),
            cdp_addr.clone(),
            &deposit_msg,
            &[coin(500, "denom1")],
        ).unwrap();

        // Check that no new prices were stored (still only 1)
        let historical_prices: HistoricalOraclePricesResponse = app
            .wrap()
            .query_wasm_smart(
                &cdp_addr,
                &QueryMsg::GetHistoricalOraclePrices { 
                    asset: "denom1".to_string() 
                },
            )
            .unwrap();
        
        assert_eq!(historical_prices.prices.len(), 1);
    }
}