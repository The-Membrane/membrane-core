#[cfg(test)]
mod tests {

    use crate::helpers::SPContract;

    use cw20::BalanceResponse;
    use membrane::osmosis_proxy::{GetDenomResponse, TokenInfoResponse};
    use membrane::stability_pool::{ClaimsResponse, ExecuteMsg, InstantiateMsg, QueryMsg};
    use membrane::staking::RewardsResponse;
    use membrane::types::{Asset, AssetInfo, AssetPool, LiqAsset};

    use cosmwasm_std::{
        attr, coin, to_binary, Addr, Binary, Decimal, Empty, Response, StdResult, Uint128,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use osmo_bindings::{ArithmeticTwapToNowResponse, PoolStateResponse, SpotPriceResponse};
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
    pub enum Osmo_MockExecuteMsg { }

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
                Ok(Response::default())
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
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct CDP_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum CDP_MockQueryMsg {}

    pub fn cdp_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: CDP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    CDP_MockExecuteMsg::LiqRepay {} => Ok(Response::default()),
                }
            },
            |_, _, _, _: CDP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, _: CDP_MockQueryMsg| -> StdResult<Binary> { to_binary(&MockResponse {}) },
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
                vec![coin(100_000, "credit")],
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

        //Instantiate SP contract
        let sp_id = app.store_code(sp_contract());

        let msg = InstantiateMsg {
            owner: None,
            asset_pool: Some(AssetPool {
                credit_asset: Asset {
                    info: AssetInfo::NativeToken {
                        denom: "credit".to_string(),
                    },
                    amount: Uint128::zero(),
                },
                liq_premium: Decimal::zero(),
                deposits: vec![],
            }),
            dex_router: Some(String::from("router_addr")),
            max_spread: Some(Decimal::percent(10)),
            desired_ratio_of_total_credit_supply: Some(Decimal::percent(80)),
            osmosis_proxy: osmosis_proxy_contract_addr.to_string(),
            mbrn_denom: String::from("mbrn_denom"),
            incentive_rate: Some(Decimal::percent(10)),
            positions_contract: cdp_contract_addr.to_string(),
            max_incentives: None,
        };

        let sp_contract_addr = app
            .instantiate_contract(sp_id, Addr::unchecked(ADMIN), &msg, &[], "test", None)
            .unwrap();

        let sp_contract = SPContract(sp_contract_addr);

        (app, sp_contract, cw20_contract_addr, cdp_contract_addr)
    }

    mod stability_pool {

        use super::*;
        use cosmwasm_std::BlockInfo;
        use membrane::stability_pool::DepositResponse;

        #[test]
        fn accrue_incentives() {
            let (mut app, sp_contract, cw20_addr, cdp_contract_addr) = proper_instantiate();

            //Incentives during withdrawals

            //Deposit credit to AssetPool
            let deposit_msg = ExecuteMsg::Deposit {
                user: None,
                assets: vec![AssetInfo::NativeToken {
                    denom: "credit".to_string(),
                }],
            };
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
            let query_msg = QueryMsg::UnclaimedIncentives {
                user: String::from(USER),
                asset_info: AssetInfo::NativeToken { denom: String::from("credit") }
            };
            let total_incentives: Uint128 = app
                .wrap()
                .query_wasm_smart(sp_contract.addr(), &query_msg)
                .unwrap();
            assert_eq!(total_incentives, Uint128::new(8800));

            //Initial withdrawal to start unstaking
            let assets: Vec<Asset> = vec![Asset {
                info: AssetInfo::NativeToken {
                    denom: "credit".to_string(),
                },
                amount: Uint128::from(100_000u128),
            }];

            let withdraw_msg = ExecuteMsg::Withdraw { assets };

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
                vec![Asset {
                    info: AssetInfo::NativeToken {
                        denom: String::from("mbrn_denom")
                    },
                    amount: Uint128::new(8_800u128),
                },]
            );

            //Query Incentives and assert that there are none after being added to claimables
            let query_msg = QueryMsg::UnclaimedIncentives {
                user: String::from(USER),
                asset_info: AssetInfo::NativeToken { denom: String::from("credit") }
            };
            let total_incentives: Uint128 = app
                .wrap()
                .query_wasm_smart(sp_contract.addr(), &query_msg)
                .unwrap();
            assert_eq!(total_incentives, Uint128::new(0));

            //Successful Withdraw
            let cosmos_msg = sp_contract.call(withdraw_msg, vec![]).unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();           

            //Incentives during distributions

            //Deposit to AssetPool
            let deposit_msg = ExecuteMsg::Deposit {
                user: None,
                assets: vec![AssetInfo::NativeToken {
                    denom: "credit".to_string(),
                }],
            };
            let cosmos_msg = sp_contract
                .call(deposit_msg, vec![coin(100_000, "credit")])
                .unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            
            //QueryRate
            let query_msg = QueryMsg::Rate {
                asset_info: AssetInfo::NativeToken { denom: String::from("credit") }
            };
            let rate: Decimal = app
                .wrap()
                .query_wasm_smart(sp_contract.addr(), &query_msg)
                .unwrap();
            assert_eq!(rate.to_string(), String::from("0.088"));

            //Claim accrued incentives 
            let claim_msg = ExecuteMsg::Claim {
                claim_as_native: None,
                claim_as_cw20: None,
                deposit_to: None,
            };
            let cosmos_msg = sp_contract.call(claim_msg, vec![]).unwrap();
            app.set_block(BlockInfo {
                height: app.block_info().height,
                time: app.block_info().time.plus_seconds(31536000u64), //Added a year
                chain_id: app.block_info().chain_id,
            });
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
          

            //Liquidate
            let liq_msg = ExecuteMsg::Liquidate {
                credit_asset: LiqAsset {
                    info: AssetInfo::NativeToken {
                        denom: "credit".to_string(),
                    },
                    amount: Decimal::from_ratio(100_000u128, 1u128),
                },
            };
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
                credit_asset: AssetInfo::NativeToken {
                    denom: "credit".to_string(),
                },
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
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: String::from("mbrn_denom")
                        },
                        amount: Uint128::new(10_000u128),
                    },
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: String::from("debit")
                        },
                        amount: Uint128::new(100u128),
                    },
                ]
            );

            //Claim 
            let claim_msg = ExecuteMsg::Claim {
                claim_as_native: None,
                claim_as_cw20: None,
                deposit_to: None,
            };
            let cosmos_msg = sp_contract.call(claim_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Claim but get nothing
            let claim_msg = ExecuteMsg::Claim {
                claim_as_native: None,
                claim_as_cw20: None,
                deposit_to: None,
            };
            let cosmos_msg = sp_contract.call(claim_msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

        }
    }
}
