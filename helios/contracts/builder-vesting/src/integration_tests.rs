#[cfg(test)]
#[allow(unused_variables)]
mod tests {

    use crate::helpers::BVContract;

    use cw20::BalanceResponse;
    use membrane::builder_vesting::{ExecuteMsg, InstantiateMsg, QueryMsg, ReceiverResponse};
    use membrane::osmosis_proxy::GetDenomResponse;
    use membrane::staking::RewardsResponse;
    use membrane::types::{Asset, AssetInfo, VestingPeriod};

    use cosmwasm_std::{
        attr, coin, to_binary, Addr, Binary, Decimal, Empty, Response, StdResult, Uint128,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use osmo_bindings::{ArithmeticTwapToNowResponse, PoolStateResponse, SpotPriceResponse};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    const ADMIN: &str = "admin";

    //Builder's Contract
    pub fn builders_contract() -> Box<dyn Contract<Empty>> {
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
            mint_to_address: String,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Osmo_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Osmo_MockQueryMsg { }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct MockResponse {}

    pub fn osmosis_proxy_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Osmo_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Osmo_MockExecuteMsg::MintTokens {
                        denom,
                        amount,
                        mint_to_address,
                    } => Ok(Response::new()),
                }
            },
            |_, _, _, _: Osmo_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Osmo_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_binary(&MockResponse {})?)
            },
        );
        Box::new(contract)
    }

    //Mock Staking Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Staking_MockExecuteMsg {
        ClaimRewards {
            claim_as_cw20: Option<String>,
            claim_as_native: Option<String>,
            send_to: Option<String>,
            restake: bool,
        },
        Stake {
            user: Option<String>,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Staking_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Staking_MockQueryMsg {
        StakerRewards { staker: String },
    }

    pub fn staking_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Staking_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Staking_MockExecuteMsg::ClaimRewards {
                        claim_as_cw20,
                        claim_as_native,
                        send_to,
                        restake,
                    } => Ok(Response::default()),
                    Staking_MockExecuteMsg::Stake { user } => Ok(Response::default()),
                }
            },
            |_, _, _, _: Staking_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Staking_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Staking_MockQueryMsg::StakerRewards { staker } => {
                        Ok(to_binary(&RewardsResponse {
                            claimables: vec![
                                Asset {
                                    info: AssetInfo::NativeToken {
                                        denom: String::from("debit"),
                                    },
                                    amount: Uint128::new(1_000_000u128),
                                },
                                Asset {
                                    info: AssetInfo::NativeToken {
                                        denom: String::from("2nddebit"),
                                    },
                                    amount: Uint128::new(1_000_000u128),
                                },
                            ],
                            accrued_interest: Uint128::zero(),
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
    pub enum Cw20_MockExecuteMsg { }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Cw20_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Cw20_MockQueryMsg { }

    pub fn cw20_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Cw20_MockExecuteMsg| -> StdResult<Response> {
                 Ok(Response::default())
            },
            |_, _, _, _: Cw20_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Cw20_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_binary(&MockResponse {})?)
            },
        );
        Box::new(contract)
    }

    fn mock_app() -> App {
        AppBuilder::new().build(|router, _, storage| {
            let bank = BankKeeper::new();

            bank.init_balance(
                storage,
                &Addr::unchecked("contract3"),
                vec![coin(30_000_000_000_000, "mbrn_denom")],
            )
            .unwrap(); //contract3 = Builders contract
            bank.init_balance(
                storage,
                &Addr::unchecked("coin_God"),
                vec![coin(100_000_000, "debit"), coin(100_000_000, "2nddebit")],
            )
            .unwrap();

            router.bank = bank;
        })
    }

    fn proper_instantiate() -> (App, BVContract, Addr) {
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

        //Instantiate Builders contract
        let bv_id = app.store_code(builders_contract());

        let msg = InstantiateMsg {
            owner: None,
            initial_allocation: Uint128::new(30_000_000_000_000u128),
            mbrn_denom: String::from("mbrn_denom"),
            osmosis_proxy: osmosis_proxy_contract_addr.to_string(),
            staking_contract: staking_contract_addr.to_string(),
        };

        let bv_contract_addr = app
            .instantiate_contract(bv_id, Addr::unchecked(ADMIN), &msg, &[], "test", None)
            .unwrap();

        let builders_contract = BVContract(bv_contract_addr);

        (app, builders_contract, cw20_contract_addr)
    }

    mod builders {

        use membrane::builder_vesting::ConfigResponse;

        use super::*;

        #[test]
        fn claim_fees() {
            let (mut app, bv_contract, cw20_addr) = proper_instantiate();

            //Add 2 Receivers
            let msg = ExecuteMsg::AddReceiver {
                receiver: String::from("receiver1"),
            };
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            let msg = ExecuteMsg::AddReceiver {
                receiver: String::from("receiver2"),
            };
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Allocate to 2 Receivers
            let msg = ExecuteMsg::AddAllocation {
                receiver: String::from("receiver1"),
                allocation: Uint128::new(10_000_000_000_000u128),
                vesting_period: VestingPeriod {
                    cliff: 365u64,
                    linear: 365u64,
                },
            };
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //----
            let msg = ExecuteMsg::AddAllocation {
                receiver: String::from("receiver2"),
                allocation: Uint128::new(7_500_000_000_000u128),
                vesting_period: VestingPeriod {
                    cliff: 365u64,
                    linear: 365u64,
                },
            };
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Claim Fees from Staking
            let msg = ExecuteMsg::ClaimFeesforContract {};
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            //Send the Claimed Fees to mimic a send from the Staking Contract
            app.send_tokens(
                Addr::unchecked("coin_God"),
                Addr::unchecked(bv_contract.addr()),
                &[coin(1_000_000, "debit"), coin(1_000_000, "2nddebit")],
            )
            .unwrap();

            //Claim Fees again from Staking
            let msg = ExecuteMsg::ClaimFeesforContract {};
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            //Send the Claimed Fees to mimic a send from the Staking Contract
            app.send_tokens(
                Addr::unchecked("coin_God"),
                Addr::unchecked(bv_contract.addr()),
                &[coin(1_000_000, "debit"), coin(1_000_000, "2nddebit")],
            )
            .unwrap();

            //Query and Assert Claimables
            let query_msg = QueryMsg::Receiver {
                receiver: String::from("receiver1"),
            };
            let res: ReceiverResponse = app
                .wrap()
                .query_wasm_smart(bv_contract.addr(), &query_msg)
                .unwrap();
            assert_eq!(
                res.claimables,
                vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: String::from("debit")
                        },
                        amount: Uint128::new(666_666u128),
                    },
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: String::from("2nddebit")
                        },
                        amount: Uint128::new(666_666u128),
                    },
                ]
            );

            //Query and Assert Claimables
            let query_msg = QueryMsg::Receiver {
                receiver: String::from("receiver2"),
            };
            let res: ReceiverResponse = app
                .wrap()
                .query_wasm_smart(bv_contract.addr(), &query_msg)
                .unwrap();
            assert_eq!(
                res.claimables,
                vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: String::from("debit")
                        },
                        amount: Uint128::new(500_000u128),
                    },
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: String::from("2nddebit")
                        },
                        amount: Uint128::new(500_000u128),
                    },
                ]
            );

            //Invalid receiver for ClaimFeesforReceiver
            let msg = ExecuteMsg::ClaimFeesforReceiver {};
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("not_a_receiver"), cosmos_msg)
                .unwrap_err();

            //Claim for each receiver
            let msg = ExecuteMsg::ClaimFeesforReceiver {};
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("receiver1"), cosmos_msg)
                .unwrap();

            let msg = ExecuteMsg::ClaimFeesforReceiver {};
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("receiver2"), cosmos_msg)
                .unwrap();

            //Query and Assert new balances
            assert_eq!(
                app.wrap()
                    .query_all_balances(Addr::unchecked("receiver1"))
                    .unwrap(),
                vec![coin(666_666, "2nddebit"), coin(666_666, "debit")]
            );
            assert_eq!(
                app.wrap()
                    .query_all_balances(Addr::unchecked("receiver2"))
                    .unwrap(),
                vec![coin(500_000, "2nddebit"), coin(500_000, "debit")]
            );

            //Assert there is nothing left to claim
            let msg = ExecuteMsg::ClaimFeesforReceiver {};
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("receiver1"), cosmos_msg)
                .unwrap_err();

            let msg = ExecuteMsg::ClaimFeesforReceiver {};
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("receiver2"), cosmos_msg)
                .unwrap_err();

        }

        #[test]
        fn update_config(){
            let (mut app, bv_contract, cw20_addr) = proper_instantiate();

            //Update Config: Error Unauthorized
            let msg = ExecuteMsg::UpdateConfig { 
                owner: None, 
                mbrn_denom: None,
                osmosis_proxy: None,
                staking_contract: None,
            };
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("not_owner"), cosmos_msg).unwrap_err();

            //Update Config: Success
            let msg = ExecuteMsg::UpdateConfig { 
                owner: Some( String::from("new_owner")), 
                mbrn_denom: Some( String::from("new_denom") ), 
                osmosis_proxy: Some( cw20_addr.to_string() ), 
                staking_contract: Some( cw20_addr.to_string() ), 
            };
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            
            //Query and Assert Claimables
            let query_msg = QueryMsg::Config {  };
            let res: ConfigResponse = app
                .wrap()
                .query_wasm_smart(bv_contract.addr(), &query_msg)
                .unwrap();
            assert_eq!(
                res,
                ConfigResponse { 
                    owner: String::from("new_owner"), 
                    initial_allocation: String::from("30000000000000"), 
                    mbrn_denom: String::from("new_denom"), 
                    osmosis_proxy: cw20_addr.to_string(), 
                    staking_contract: cw20_addr.to_string(), 
                }
            );
        }
    }
}
