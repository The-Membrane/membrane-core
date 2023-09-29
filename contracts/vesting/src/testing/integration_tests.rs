#[cfg(test)]
mod tests {

    use crate::helpers::BVContract;

    use membrane::vesting::{ExecuteMsg, InstantiateMsg, QueryMsg, RecipientResponse};
    use membrane::staking::{ StakerResponse, RewardsResponse};
    use membrane::types::{Asset, AssetInfo, VestingPeriod};

    use cosmwasm_std::{
        coin, to_binary, Addr, Binary, Empty, Response, StdResult, Uint128
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
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
        UserRewards { user: String },
        UserStake { staker: String },
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
                    Staking_MockQueryMsg::UserRewards { user } => {
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
                    },
                    Staking_MockQueryMsg::UserStake { staker } => {
                        Ok(to_binary(&StakerResponse {
                            staker,
                            total_staked: Uint128::new(30_000_000_000_000),
                            deposit_list: vec![],
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
                vec![coin(60_000_000_000_000, "mbrn_denom")],
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
            pre_launch_contributors: String::from("labs"),
        };

        let bv_contract_addr = app
            .instantiate_contract(bv_id, Addr::unchecked(ADMIN), &msg, &[], "test", None)
            .unwrap();

        let builders_contract = BVContract(bv_contract_addr);

        let msg = ExecuteMsg::UpdateConfig { 
            owner: None, 
            mbrn_denom: None,
            osmosis_proxy: None,
            staking_contract: None,
            additional_allocation: Some( Uint128::new(20_000_000_000_000) ),
        };
        let cosmos_msg = builders_contract.call(msg, vec![]).unwrap();
        app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

        (app, builders_contract, cw20_contract_addr)
    }

    mod builders {
        use membrane::vesting::Config;

        use super::*;

        #[test]
        fn claim_fees() {
            let (mut app, bv_contract, cw20_addr) = proper_instantiate();

            //Add 2 Recipients
            let msg = ExecuteMsg::AddRecipient {
                recipient: String::from("recipient1"),
            };
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            let msg = ExecuteMsg::AddRecipient {
                recipient: String::from("recipient2"),
            };
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Allocate to 2 Recipients
            let msg = ExecuteMsg::AddAllocation {
                recipient: String::from("recipient1"),
                allocation: Uint128::new(10_000_000_000_000u128),
                vesting_period: Some(VestingPeriod {
                    cliff: 365u64,
                    linear: 365u64,
                }),
            };
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //----
            let msg = ExecuteMsg::AddAllocation {
                recipient: String::from("recipient2"),
                allocation: Uint128::new(7_500_000_000_000u128),
                vesting_period: Some(VestingPeriod {
                    cliff: 365u64,
                    linear: 365u64,
                }),
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
            let query_msg = QueryMsg::Recipient {
                recipient: String::from("recipient1"),
            };
            let res: RecipientResponse = app
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
            let query_msg = QueryMsg::Recipient {
                recipient: String::from("recipient2"),
            };
            let res: RecipientResponse = app
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

            //Invalid recipient for ClaimFeesforRecipient
            let msg = ExecuteMsg::ClaimFeesforRecipient {};
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("not_a_recipient"), cosmos_msg)
                .unwrap_err();

            //Claim for each recipient
            let msg = ExecuteMsg::ClaimFeesforRecipient {};
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("recipient1"), cosmos_msg)
                .unwrap();

            let msg = ExecuteMsg::ClaimFeesforRecipient {};
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("recipient2"), cosmos_msg)
                .unwrap();

            //Query and Assert new balances
            assert_eq!(
                app.wrap()
                    .query_all_balances(Addr::unchecked("recipient1"))
                    .unwrap(),
                vec![coin(666_666, "2nddebit"), coin(666_666, "debit")]
            );
            assert_eq!(
                app.wrap()
                    .query_all_balances(Addr::unchecked("recipient2"))
                    .unwrap(),
                vec![coin(500_000, "2nddebit"), coin(500_000, "debit")]
            );

            //Assert there is nothing left to claim
            let msg = ExecuteMsg::ClaimFeesforRecipient {};
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("recipient1"), cosmos_msg)
                .unwrap_err();

            let msg = ExecuteMsg::ClaimFeesforRecipient {};
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("recipient2"), cosmos_msg)
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
                additional_allocation: None,
            };
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("not_owner"), cosmos_msg).unwrap_err();

            //Update Config: Success
            let msg = ExecuteMsg::UpdateConfig { 
                owner: Some( String::from("new_owner")), 
                mbrn_denom: Some( String::from("new_denom") ), 
                osmosis_proxy: Some( cw20_addr.to_string() ), 
                staking_contract: Some( cw20_addr.to_string() ), 
                additional_allocation: Some( Uint128::one() ),
            };
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            
            //Query and Assert
            let query_msg = QueryMsg::Config {  };
            let res: Config = app
                .wrap()
                .query_wasm_smart(bv_contract.addr(), &query_msg)
                .unwrap();
            assert_eq!(
                res,
                Config { 
                    owner: Addr::unchecked(ADMIN), 
                    total_allocation: Uint128::new(50_000_000_000_001), 
                    mbrn_denom: String::from("new_denom"), 
                    osmosis_proxy: cw20_addr.clone(), 
                    staking_contract: cw20_addr.clone(), 
                }
            );

            //Update Config: Ownership transfer
            let msg = ExecuteMsg::UpdateConfig { 
                owner: None,
                mbrn_denom: None,
                osmosis_proxy: None,
                staking_contract: None,
                additional_allocation: None,
            };
            let cosmos_msg = bv_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("new_owner"), cosmos_msg).unwrap();
            
            //Query and Assert transfer
            let query_msg = QueryMsg::Config {  };
            let res: Config = app
                .wrap()
                .query_wasm_smart(bv_contract.addr(), &query_msg)
                .unwrap();
            assert_eq!(
                res,
                Config { 
                    owner: Addr::unchecked("new_owner"), 
                    total_allocation: Uint128::new(50_000_000_000_001), 
                    mbrn_denom: String::from("new_denom"), 
                    osmosis_proxy: cw20_addr.clone(), 
                    staking_contract: cw20_addr, 
                }
            );
        }
    }
}
