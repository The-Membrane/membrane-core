#[cfg(test)]
mod tests {

    use crate::helpers::SPContract;

    use membrane::cdp::{PositionResponse, BasketPositionsResponse};
    use membrane::oracle::PriceResponse;
    use membrane::osmosis_proxy::TokenInfoResponse;
    use membrane::stability_pool::{ClaimsResponse, ExecuteMsg, InstantiateMsg, QueryMsg};
    use membrane::types::{Asset, AssetInfo, AssetPool, UserInfo, Deposit, Basket};

    use cosmwasm_std::{
        coin, to_binary, Addr, Binary, Decimal, Empty, Response, StdResult, Uint128,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    const USER: &str = "user";
    const ADMIN: &str = "admin";

    //SP Contract
    pub fn sp_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        );
        Box::new(contract)
    }

    //Mock Osmo Proxy Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Osmo_MockExecuteMsg {
        MintTokens {
            denom: String,
            amount: Uint128,
            mint_to_address: Option<String>,
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Osmo_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Osmo_MockQueryMsg {
        GetTokenInfo {
            denom: String,
        },
    }

    pub fn osmosis_proxy_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Osmo_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Osmo_MockExecuteMsg::MintTokens {
                        denom,
                        mint_to_address,
                        amount,
                    } => {
                        if amount != Uint128::new(10_000u128) && amount != Uint128::new(20_000u128) || denom != "mbrn_denom" || mint_to_address != Some("user".to_string()) {
                            panic!("Params incorrect: {}, {}, {:?}", amount, denom, mint_to_address);
                        }
                        Ok(Response::default())
                    }
                }
            },
            |_, _, _, _: Osmo_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Osmo_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Osmo_MockQueryMsg::GetTokenInfo { denom } => {
                        Ok(to_binary(&TokenInfoResponse {
                            denom,
                            current_supply: Uint128::new(110_000u128),
                            max_supply: Uint128::zero(),
                            burned_supply: Uint128::zero(),
                        })?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    //Mock Staking Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Staking_MockExecuteMsg {    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Staking_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Staking_MockQueryMsg {  }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct MockResponse {}

    pub fn staking_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Staking_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: Staking_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Staking_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_binary(&MockResponse{})?)
            },
        );
        Box::new(contract)
    }

    //Mock Positions Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum CDP_MockExecuteMsg {
        LiqRepay {},
        Accrue { 
            position_owner: Option<String>, 
            position_ids: Vec<Uint128>
        },
        Repay {
            position_id: Uint128,
            position_owner: Option<String>, 
            send_excess_to: Option<String>, 
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct CDP_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum CDP_MockQueryMsg {
        GetBasketPositions {
            user: String,
            limit: Option<u32>,
        }, 
        GetBasket {},
    }

    pub fn cdp_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: CDP_MockExecuteMsg| -> StdResult<Response> {
                    Ok(Response::default())
            },
            |_, _, _, _: CDP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: CDP_MockQueryMsg| -> StdResult<Binary> { 
                match msg {
                    CDP_MockQueryMsg::GetBasketPositions { user, limit } => {
                        Ok(to_binary(&vec![BasketPositionsResponse {
                            user: String::from(USER),
                            positions: vec![PositionResponse { 
                                position_id: Uint128::one(),
                                collateral_assets: vec![],
                                cAsset_ratios: vec![],
                                credit_amount: Uint128::one(),
                                avg_borrow_LTV: Decimal::one(),
                                avg_max_LTV: Decimal::one()}]
                        }])?)
                    },
                    CDP_MockQueryMsg::GetBasket {} => {
                        Ok(to_binary(&Basket {
                            basket_id: Uint128::one(),
                            current_position_id: Uint128::one(),
                            collateral_types: vec![],
                            collateral_supply_caps: vec![],
                            lastest_collateral_rates: vec![],
                            credit_asset: Asset {
                                info: AssetInfo::NativeToken {
                                    denom: String::from("credit_fulldenom"),
                                },
                                amount: Uint128::zero(),
                            },
                            credit_price: PriceResponse { 
                                prices: vec![], 
                                price: Decimal::one(), 
                                decimals: 0
                            },
                            liq_queue: None,
                            base_interest_rate: Decimal::zero(),
                            pending_revenue: Uint128::zero(),
                            negative_rates: true,
                            cpc_margin_of_error: Decimal::zero(),
                            multi_asset_supply_caps: vec![],
                            frozen: false,
                            rev_to_stakers: true,
                            credit_last_accrued: 0,
                            rates_last_accrued: 0,
                            oracle_set: false,
                        })?)
                    }
                }
             },
        );
        Box::new(contract)
    }

    //Mock Oracle Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Oracle_MockExecuteMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Oracle_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Oracle_MockQueryMsg {
        Price {
            asset_info: AssetInfo,
            twap_timeframe: u64,
            basket_id: Option<Uint128>,
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
                    Oracle_MockQueryMsg::Price {
                        asset_info,
                        twap_timeframe,
                        basket_id,
                    } => {
                        Ok(to_binary(&PriceResponse {
                            prices: vec![],
                            price: Decimal::one(),
                            decimals: 0,
                        })?)
                        
                    }
                }
            },
        );
        Box::new(contract)
    }

    //Mock Cw20 Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Cw20_MockExecuteMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Cw20_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Cw20_MockQueryMsg {  }

    pub fn cw20_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Cw20_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: Cw20_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Cw20_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_binary(&MockResponse{})?)
            },
        );
        Box::new(contract)
    }

    fn mock_app() -> App {
        AppBuilder::new().build(|router, _, storage| {
            let bank = BankKeeper::new();

            bank.init_balance(
                storage,
                &Addr::unchecked(USER),
                vec![coin(200_000, "credit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("coin_God"),
                vec![coin(100_000_000, "debit"), coin(100_000_000, "2nddebit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("contract4"),
                vec![coin(100_000, "mbrn_denom")],
            )
            .unwrap();

            router.bank = bank;
        })
    }

    fn proper_instantiate() -> (App, SPContract, Addr, Addr) {
        let mut app = mock_app();

        //Instantiate Cw20
        let cw20_id = app.store_code(cw20_contract());
        let cw20_contract_addr = app
            .instantiate_contract(
                cw20_id,
                Addr::unchecked(ADMIN),
                &Cw20_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate Osmosis Proxy
        let proxy_id = app.store_code(osmosis_proxy_contract());

        let osmosis_proxy_contract_addr = app
            .instantiate_contract(
                proxy_id,
                Addr::unchecked(ADMIN),
                &Osmo_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate Staking Contract
        let staking_id = app.store_code(staking_contract());

        let staking_contract_addr = app
            .instantiate_contract(
                staking_id,
                Addr::unchecked(ADMIN),
                &Staking_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instantiate Positions Contract
        let cdp_id = app.store_code(cdp_contract());

        let cdp_contract_addr = app
            .instantiate_contract(
                cdp_id,
                Addr::unchecked(ADMIN),
                &CDP_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instantiate Oracle Contract
        let oracle_id = app.store_code(oracle_contract());

        let oracle_contract_addr = app
            .instantiate_contract(
                oracle_id,
                Addr::unchecked(ADMIN),
                &Oracle_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instantiate SP contract
        let sp_id = app.store_code(sp_contract());

        let msg = InstantiateMsg {
            owner: None,
            asset_pool: AssetPool {
                credit_asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: "credit".to_string(),
                    },
                    amount: Uint128::zero(),
                },
                liq_premium: Decimal::zero(),
                deposits: vec![],
            },
            osmosis_proxy: osmosis_proxy_contract_addr.to_string(),
            mbrn_denom: String::from("mbrn_denom"),
            incentive_rate: Some(Decimal::percent(10)),
            positions_contract: cdp_contract_addr.to_string(),
            oracle_contract: oracle_contract_addr.to_string(),
            max_incentives: None,
            minimum_deposit_amount: Uint128::new(5),
        };

        let sp_contract_addr = app
            .instantiate_contract(sp_id, Addr::unchecked(ADMIN), &msg, &[], "test", None)
            .unwrap();

        let sp_contract = SPContract(sp_contract_addr);

        (app, sp_contract, cw20_contract_addr, cdp_contract_addr)
    }

    mod stability_pool {

        use super::*;
        use cosmwasm_std::{BlockInfo, Coin};
        use membrane::stability_pool::{Config, UserIncentivesResponse};

        #[test]
        fn cdp_repay() {
            let (mut app, sp_contract, cw20_addr, cdp_contract_addr) = proper_instantiate();

            //Deposit credit to AssetPool
            let deposit_msg = ExecuteMsg::Deposit { user: None };
            let cosmos_msg = sp_contract
                .call(deposit_msg, vec![coin(100_000, "credit")])
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Repay credit to CDP: Error
            let repay_msg = ExecuteMsg::Repay {
                user_info: UserInfo {
                    position_id: Uint128::new(1u128),
                    position_owner: String::from(USER),
                },
                repayment: Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("credit"),
                    },
                    amount: Uint128::new(1u128),
                },
            };
            let cosmos_msg = sp_contract.call(repay_msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Unauthorized"));

            //Repay credit to CDP: Error 
            let repay_msg = ExecuteMsg::Repay {
                user_info: UserInfo {
                    position_id: Uint128::new(1u128),
                    position_owner: String::from(USER),
                },
                repayment: Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("invalid"),
                    },
                    amount: Uint128::new(1u128),
                },
            };
            let cosmos_msg = sp_contract.call(repay_msg, vec![]).unwrap();
            let err = app.execute(cdp_contract_addr.clone(), cosmos_msg).unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Asset pool hasn't been added for this asset yet"));

            //Repay credit to CDP: Error. User no funds in SP
            let repay_msg = ExecuteMsg::Repay {
                user_info: UserInfo {
                    position_id: Uint128::new(1u128),
                    position_owner: String::from("no_funds"),
                },
                repayment: Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("credit"),
                    },
                    amount: Uint128::new(0u128),
                },
            };
            let cosmos_msg = sp_contract.call(repay_msg, vec![]).unwrap();
            let err = app.execute(cdp_contract_addr.clone(), cosmos_msg).unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Invalid withdrawal"));

            //Repay credit to CDP: Error. User no funds in SP
            let repay_msg = ExecuteMsg::Repay {
                user_info: UserInfo {
                    position_id: Uint128::new(1u128),
                    position_owner: String::from(USER),
                },
                repayment: Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("credit"),
                    },
                    amount: Uint128::new(100_001u128),
                },
            };
            let cosmos_msg = sp_contract.call(repay_msg, vec![]).unwrap();
            let err = app.execute(cdp_contract_addr.clone(), cosmos_msg).unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Invalid withdrawal"));

            //Repay credit to CDP: Error. User no funds in SP
            let repay_msg = ExecuteMsg::Repay {
                user_info: UserInfo {
                    position_id: Uint128::new(1u128),
                    position_owner: String::from(USER),
                },
                repayment: Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("credit"),
                    },
                    amount: Uint128::new(100_000u128),
                },
            };
            let cosmos_msg = sp_contract.call(repay_msg, vec![]).unwrap();
            let res = app.execute(cdp_contract_addr.clone(), cosmos_msg).unwrap();
            assert_eq!(app.wrap()
                    .query_all_balances(cdp_contract_addr.clone())
                    .unwrap(),
                vec![coin(100_000, "credit")]);

            //Assert State saved correctly
            //Query AssetPool
            let resp: AssetPool = app
                .wrap()
                .query_wasm_smart(sp_contract.addr(), &QueryMsg::AssetPool { user: None, deposit_limit: None, start_after: None })
                .unwrap();

            assert_eq!(resp.credit_asset.to_string(), "0 credit".to_string());
            assert_eq!(resp.liq_premium.to_string(), "0".to_string());
            assert_eq!(resp.deposits.len().to_string(), "0".to_string());
        }

        #[test]
        fn withdrawal() {
            let (mut app, sp_contract, cw20_addr, cdp_contract_addr) = proper_instantiate();

            //Deposit credit to AssetPool: #1
            let deposit_msg = ExecuteMsg::Deposit { user: None };
            let cosmos_msg = sp_contract
                .call(deposit_msg, vec![coin(100_000, "credit")])
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Deposit credit to AssetPool: #2
            let deposit_msg = ExecuteMsg::Deposit { user: None };
            let cosmos_msg = sp_contract
                .call(deposit_msg, vec![coin(10, "credit")])
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Withdraw: Invalid "Amount too high"
            let withdraw_msg = ExecuteMsg::Withdraw { amount: Uint128::new(100_011u128) };
            let cosmos_msg = sp_contract
                .call(withdraw_msg, vec![])
                .unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Invalid withdrawal")
            );
            //Withdraw all of first, and partial of 2nd Deposit: Success
            //First msg begins unstaking
            let withdraw_msg = ExecuteMsg::Withdraw { amount: Uint128::new(100_005u128) };
            let cosmos_msg = sp_contract
                .call(withdraw_msg, vec![])
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Query to make sure the remaining amount of the 2nd "credit" deposit is still staked
            let resp: AssetPool = app
                .wrap()
                .query_wasm_smart(sp_contract.addr(), &QueryMsg::AssetPool { user: None, deposit_limit: None, start_after: None })
                .unwrap();
            assert_eq!(
                resp.deposits,
                vec![
                    Deposit {
                        user: Addr::unchecked(USER),
                        amount: Decimal::percent(100_000_00),
                        deposit_time: app.block_info().time.seconds(),
                        last_accrued: app.block_info().time.seconds(),
                        unstake_time: Some(app.block_info().time.seconds()),
                    },
                    Deposit {
                        user: Addr::unchecked(USER),
                        amount: Decimal::percent(5_00),
                        deposit_time: app.block_info().time.seconds(),
                        last_accrued: app.block_info().time.seconds(),
                        unstake_time: Some(app.block_info().time.seconds()),
                    },                    
                    Deposit {
                        user: Addr::unchecked(USER),
                        amount: Decimal::percent(5_00),
                        deposit_time: app.block_info().time.seconds(),
                        last_accrued: app.block_info().time.seconds(),
                        unstake_time: None,
                    },
                ]
            );

            //Restake
            let restake_msg = ExecuteMsg::Restake { restake_amount: Decimal::percent(100_005_00) };
            let cosmos_msg = sp_contract
                .call(restake_msg, vec![])
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Assert restake
            let resp: AssetPool = app
                .wrap()
                .query_wasm_smart(sp_contract.addr(), &QueryMsg::AssetPool { user: None, deposit_limit: None, start_after: None })
                .unwrap();
            assert_eq!(
                resp.deposits,
                vec![
                    Deposit {
                        user: Addr::unchecked(USER),
                        amount: Decimal::percent(100_000_00),
                        deposit_time: app.block_info().time.seconds(),
                        last_accrued: app.block_info().time.seconds(),
                        unstake_time: None,
                    },
                    Deposit {
                        user: Addr::unchecked(USER),
                        amount: Decimal::percent(5_00),
                        deposit_time: app.block_info().time.seconds(),
                        last_accrued: app.block_info().time.seconds(),
                        unstake_time: None,
                    },
                    Deposit {
                        user: Addr::unchecked(USER),
                        amount: Decimal::percent(5_00),
                        deposit_time: app.block_info().time.seconds(),
                        last_accrued: app.block_info().time.seconds(),
                        unstake_time: None,
                    },
                ]
            );

            //Reunstake Success
            let withdraw_msg = ExecuteMsg::Withdraw { amount: Uint128::new(100_001u128) };
            let cosmos_msg = sp_contract
                .call(withdraw_msg, vec![])
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Assert unstaking time was set correctly
            let resp: AssetPool = app
                .wrap()
                .query_wasm_smart(sp_contract.addr(), &QueryMsg::AssetPool { user: None, deposit_limit: None, start_after: None })
                .unwrap();
            assert_eq!(
                resp.deposits,
                vec![
                    Deposit {
                        user: Addr::unchecked(USER),
                        amount: Decimal::percent(100_000_00),
                        deposit_time: app.block_info().time.seconds(),
                        last_accrued: app.block_info().time.seconds(),
                        unstake_time: Some(app.block_info().time.seconds()),
                    },
                    Deposit {
                        user: Addr::unchecked(USER),
                        amount: Decimal::percent(5_00),
                        deposit_time: app.block_info().time.seconds(),
                        last_accrued: app.block_info().time.seconds(),
                        unstake_time: Some(app.block_info().time.seconds()),
                    },
                    Deposit {
                        user: Addr::unchecked(USER),
                        amount: Decimal::percent(5_00),
                        deposit_time: app.block_info().time.seconds(),
                        last_accrued: app.block_info().time.seconds(),
                        unstake_time: None,
                    },
                ]
            );

            
            //Test unstaking a new deposit that doesn't stop at the already unstaking deposits
            let withdraw_msg = ExecuteMsg::Withdraw { amount: Uint128::new(5u128) };
            let cosmos_msg = sp_contract
                .call(withdraw_msg, vec![])
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Withdrawl Success, anything between 100_001 & 100_005 will withdraw the first 2 deposits to enforce the minimum
            let withdraw_msg = ExecuteMsg::Withdraw { amount: Uint128::new(100_004u128) };
            let cosmos_msg = sp_contract
                .call(withdraw_msg, vec![])
                .unwrap();
            app.set_block(
                BlockInfo {
                    height: app.block_info().height,
                    time: app.block_info().time.plus_seconds(86400u64), //Added a day
                    chain_id: app.block_info().chain_id,
                }
            );
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Assert success
            //3rd is left
            let resp: AssetPool = app
                .wrap()
                .query_wasm_smart(sp_contract.addr(), &QueryMsg::AssetPool { user: None, deposit_limit: None, start_after: None })
                .unwrap();
            assert_eq!(
                resp.deposits,
                vec![
                    Deposit {
                        user: Addr::unchecked(USER),
                        amount: Decimal::percent(5_00),
                        deposit_time: 1571797419,
                        last_accrued: app.block_info().time.seconds(),
                        unstake_time: Some(1571797419),
                    },
                    ]
            );
        }

        #[test]
        fn accrue_incentives() {
            let (mut app, sp_contract, cw20_addr, cdp_contract_addr) = proper_instantiate();

            //Incentives during withdrawals

            //Deposit credit to AssetPool
            let deposit_msg = ExecuteMsg::Deposit { user: None };
            let cosmos_msg = sp_contract
                .call(deposit_msg, vec![coin(100_000, "credit")])
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });

            //Query Incentives
            let query_msg = QueryMsg::UnclaimedIncentives { user: String::from(USER) };
            let total_incentives: UserIncentivesResponse = app
                .wrap()
                .query_wasm_smart(sp_contract.addr(), &query_msg)
                .unwrap();
            assert_eq!(total_incentives.incentives, Uint128::new(10000));

            //Initial withdrawal to start unstaking
            let withdraw_msg = ExecuteMsg::Withdraw { amount: Uint128::from(100_000u128) };
            let cosmos_msg = sp_contract.call(withdraw_msg.clone(), vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Query and Assert Claimables
            let query_msg = QueryMsg::UserClaims {
                user: String::from(USER),
            };
            let res: ClaimsResponse = app
                .wrap()
                .query_wasm_smart(sp_contract.addr(), &query_msg)
                .unwrap();
            assert_eq!(
                res.claims,
                vec![Coin {
                    denom: String::from("mbrn_denom"),
                    amount: Uint128::new(10_000u128),
                },]
            );

            //Skip unstaking period
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            //Restake
            let restake_msg = ExecuteMsg::Restake { restake_amount: Decimal::percent(100_000_00) };
            let cosmos_msg = sp_contract
                .call(restake_msg, vec![])
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            //Rewithdraw
            let withdraw_msg = ExecuteMsg::Withdraw { amount: Uint128::from(100_000u128) };
            let cosmos_msg = sp_contract.call(withdraw_msg.clone(), vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Query Incentives and assert that there are none after being added to claimables & no retroactive rewards while unstaking
            let query_msg = QueryMsg::UnclaimedIncentives { user: String::from(USER) };
            let total_incentives: UserIncentivesResponse = app
                .wrap()
                .query_wasm_smart(sp_contract.addr(), &query_msg)
                .unwrap();
            assert_eq!(total_incentives.incentives, Uint128::new(0));
            //Query and Assert Claimables
            let query_msg = QueryMsg::UserClaims {
                user: String::from(USER),
            };
            let res: ClaimsResponse = app
                .wrap()
                .query_wasm_smart(sp_contract.addr(), &query_msg)
                .unwrap();
            assert_eq!(
                res.claims,
                vec![Coin {
                    denom: String::from("mbrn_denom"),
                    amount: Uint128::new(10_000u128),
                },]
            );

            //Successful Withdraw
            let cosmos_msg = sp_contract.call(withdraw_msg, vec![]).unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Query and Assert Claimables
            let query_msg = QueryMsg::UserClaims {
                user: String::from(USER),
            };
            let res: ClaimsResponse = app
                .wrap()
                .query_wasm_smart(sp_contract.addr(), &query_msg)
                .unwrap();
            assert_eq!(
                res.claims,
                vec![Coin {
                        denom: String::from("mbrn_denom"),
                        amount: Uint128::new(10_000u128), //This +10k is from the restake->withdraw->1year->withdraw
                },]
            );

            //Incentives during distributions

            //Deposit to AssetPool
            let deposit_msg = ExecuteMsg::Deposit { user: None };
            let cosmos_msg = sp_contract
                .call(deposit_msg, vec![coin(100_000, "credit")])
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            
            //QueryRate
            let query_msg = QueryMsg::Config { };
            let config: Config = app
                .wrap()
                .query_wasm_smart(sp_contract.addr(), &query_msg)
                .unwrap();
            assert_eq!(config.incentive_rate.to_string(), String::from("0.1"));

            //Claim accrued incentives: 20k
            let claim_msg = ExecuteMsg::ClaimRewards { };
            let cosmos_msg = sp_contract.call(claim_msg, vec![]).unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
          

            //Liquidate
            let liq_msg = ExecuteMsg::Liquidate { liq_amount: Decimal::from_ratio(100_000u128, 1u128) };
            let cosmos_msg = sp_contract.call(liq_msg, vec![]).unwrap();
            app.execute(cdp_contract_addr.clone(), cosmos_msg).unwrap();

            //Distribute
            let distribute_msg = ExecuteMsg::Distribute {
                distribution_assets: vec![Asset {
                    info: AssetInfo::NativeToken {
                        denom: "debit".to_string(),
                    },
                    amount: Uint128::new(100u128),
                }],
                distribution_asset_ratios: vec![Decimal::percent(100)],
                distribute_for: Uint128::new(100_000),
            };
            let cosmos_msg = sp_contract
                .call(distribute_msg, vec![coin(100, "debit")])
                .unwrap();
            app.send_tokens(
                Addr::unchecked("coin_God"),
                cdp_contract_addr.clone(),
                &[coin(100, "debit")],
            )
            .unwrap();           
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(cdp_contract_addr, cosmos_msg).unwrap();

            //Query and Assert Claimables
            //Since incentives were claimed earlier, these are only from the most recent timeskip
            let query_msg = QueryMsg::UserClaims {
                user: String::from(USER),
            };
            let res: ClaimsResponse = app
                .wrap()
                .query_wasm_smart(sp_contract.addr(), &query_msg)
                .unwrap();
            assert_eq!(
                res.claims,
                vec![
                    Coin {
                        denom: String::from("debit"),
                        amount: Uint128::new(100u128),
                    },
                    Coin {
                        denom: String::from("mbrn_denom"),
                        amount: Uint128::new(10_000u128),
                    },
                ]
            );

            //Claim 
            let claim_msg = ExecuteMsg::ClaimRewards { };
            let cosmos_msg = sp_contract.call(claim_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Claim but get nothing
            let claim_msg = ExecuteMsg::ClaimRewards { };
            let cosmos_msg = sp_contract.call(claim_msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

        }
    }
}