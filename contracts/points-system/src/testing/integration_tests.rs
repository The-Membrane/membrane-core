#[cfg(test)]
#[allow(unused_variables)]
mod tests {

    use std::os;
    use std::str::FromStr;

    use crate::helpers::PointsContract;


    use membrane::governance::{Proposal, ProposalStatus};
    use membrane::liq_queue::ClaimsResponse as LQ_ClaimsResponse;
    use membrane::math::Uint256;
    use membrane::oracle::{AssetResponse, PriceResponse};
    use membrane::osmosis_proxy::{GetDenomResponse, TokenInfoResponse, OwnerResponse};
    use membrane::points_system::{ExecuteMsg, InstantiateMsg, QueryMsg};
    use membrane::stability_pool::ClaimsResponse as SP_ClaimsResponse;
    use membrane::staking::Config as Staking_Config;
    use membrane::types::{
        cAsset, Asset, AssetInfo, AssetOracleInfo, AssetPool, Basket, DebtCap, Deposit, LiquidityInfo, MultiAssetSupplyCap, Owner, PoolStateResponse, PoolType, StakeDistribution, TWAPPoolInfo, UserInfo
    };
    use membrane::liquidity_check::LiquidityResponse;

    use cosmwasm_std::{
        attr, coin, to_binary, Addr, Binary, Coin, Decimal, Empty, Response, StdError, StdResult,
        Uint128, Uint64,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use cosmwasm_schema::cw_serde;

    const USER: &str = "user";
    const ADMIN: &str = "admin";

    //Points Contract
    pub fn points_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contracts::execute,
            crate::contracts::instantiate,
            crate::contracts::query,
        )
        .with_reply(crate::contracts::reply);
        Box::new(contract)
    }

    
    //Mock Positions Contract
    #[cw_serde]
    pub enum CDP_MockExecuteMsg {
        Liquidate {
            position_id: Uint128,
            position_owner: String,
        },
    }

    #[cw_serde]
    pub struct CDP_MockInstantiateMsg {}

    #[cw_serde]
    pub enum CDP_MockQueryMsg {
        GetBasket {},
    }

    pub fn cdp_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: CDP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    CDP_MockExecuteMsg::Liquidate {
                        position_id,
                        position_owner,
                    } => Ok(
                        Response::new()
                    ),
                }
            },
            |_, _, _, _: CDP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: CDP_MockQueryMsg| -> StdResult<Binary> { 
                match msg {
                    CDP_MockQueryMsg::GetBasket { } => {
                        Ok(to_binary(&Basket {
                            basket_id: Uint128::zero(),
                            current_position_id: Uint128::zero(),
                            collateral_types: vec![],
                            collateral_supply_caps: vec![],
                            lastest_collateral_rates: vec![],
                            credit_asset: Asset { 
                                info: AssetInfo::NativeToken { denom: String::from("credit") },
                                amount: Uint128::new(100_000_000) 
                            },
                            credit_price: PriceResponse { 
                                prices: vec![], 
                                price: Decimal::zero(), 
                                decimals: 6
                            },
                            liq_queue: None,
                            base_interest_rate: Decimal::zero(),
                            pending_revenue: Uint128::new(1_000_000),
                            negative_rates: false,
                            cpc_margin_of_error: Decimal::zero(),
                            multi_asset_supply_caps: vec![],
                            frozen: false,
                            rev_to_stakers: true,
                            credit_last_accrued: 0,
                            rates_last_accrued: 0,
                            oracle_set: true,
                        })?)
                    },
                }
            },
        );
        Box::new(contract)
    }

    
    pub fn post_cdp_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: CDP_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    CDP_MockExecuteMsg::Liquidate {
                        position_id,
                        position_owner,
                    } => Ok(
                        Response::new()
                    ),
                }
            },
            |_, _, _, _: CDP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: CDP_MockQueryMsg| -> StdResult<Binary> { 
                match msg {
                    CDP_MockQueryMsg::GetBasket { } => {
                        Ok(to_binary(&Basket {
                            basket_id: Uint128::zero(),
                            current_position_id: Uint128::zero(),
                            collateral_types: vec![],
                            collateral_supply_caps: vec![],
                            lastest_collateral_rates: vec![],
                            credit_asset: Asset { 
                                info: AssetInfo::NativeToken { denom: String::from("credit") },
                                amount: Uint128::new(90_000_000) 
                            },
                            credit_price: PriceResponse { 
                                prices: vec![], 
                                price: Decimal::zero(), 
                                decimals: 6
                            },
                            liq_queue: None,
                            base_interest_rate: Decimal::zero(),
                            pending_revenue: Uint128::new(900_000),
                            negative_rates: false,
                            cpc_margin_of_error: Decimal::zero(),
                            multi_asset_supply_caps: vec![],
                            frozen: false,
                            rev_to_stakers: true,
                            credit_last_accrued: 0,
                            rates_last_accrued: 0,
                            oracle_set: true,
                        })?)
                    },
                }
            },
        );
        Box::new(contract)
    }

    //Mock Governance Contract
    #[cw_serde]
    pub enum Gov_MockExecuteMsg { }

    #[cw_serde]
    pub struct Gov_MockInstantiateMsg {}

    #[cw_serde]
    pub enum Gov_MockQueryMsg {
        Proposal {
            proposal_id: u64,
        },
    }

    pub fn gov_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Gov_MockExecuteMsg| -> StdResult<Response> {
                match msg {}
            },
            |_, _, _, _: Gov_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Gov_MockQueryMsg| -> StdResult<Binary> { 
                match msg {
                    Gov_MockQueryMsg::Proposal { proposal_id } => {
                        if proposal_id == 3u64 {
                            return Ok(to_binary(&Proposal {
                                voting_power: vec![],
                                proposal_id: Uint64::new(proposal_id),
                                submitter: Addr::unchecked(""),
                                status: ProposalStatus::Passed,
                                aligned_power: Uint128::zero(),
                                for_power: Uint128::zero(),
                                against_power: Uint128::zero(),
                                amendment_power: Uint128::zero(),
                                removal_power: Uint128::zero(),
                                aligned_voters: vec![],
                                for_voters: vec![ Addr::unchecked(USER) ],
                                against_voters: vec![],
                                amendment_voters: vec![],
                                removal_voters: vec![],
                                start_block: 1,
                                start_time: 1,
                                end_block: 1,
                                delayed_end_block: 1,
                                expiration_block: 1,
                                title: String::from(""),
                                description: String::from(""),
                                link: None,
                                messages: None,
                            })?)
                        }

                        Ok(to_binary(&Proposal {
                            voting_power: vec![],
                            proposal_id: Uint64::new(proposal_id),
                            submitter: Addr::unchecked(""),
                            status: ProposalStatus::Passed,
                            aligned_power: Uint128::zero(),
                            for_power: Uint128::zero(),
                            against_power: Uint128::zero(),
                            amendment_power: Uint128::zero(),
                            removal_power: Uint128::zero(),
                            aligned_voters: vec![],
                            for_voters: vec![],
                            against_voters: vec![],
                            amendment_voters: vec![],
                            removal_voters: vec![],
                            start_block: 1,
                            start_time: 1,
                            end_block: 1,
                            delayed_end_block: 1,
                            expiration_block: 1,
                            title: String::from(""),
                            description: String::from(""),
                            link: None,
                            messages: None,
                        })?)
                    },
                }
            },
        );
        Box::new(contract)
    }

    pub fn post_gov_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Gov_MockExecuteMsg| -> StdResult<Response> {
                match msg {}
            },
            |_, _, _, _: Gov_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Gov_MockQueryMsg| -> StdResult<Binary> { 
                match msg {
                    Gov_MockQueryMsg::Proposal { proposal_id } => {
                        Ok(to_binary(&Proposal {
                            voting_power: vec![],
                            proposal_id: Uint64::new(proposal_id),
                            submitter: Addr::unchecked(""),
                            status: ProposalStatus::Passed,
                            aligned_power: Uint128::zero(),
                            for_power: Uint128::zero(),
                            against_power: Uint128::zero(),
                            amendment_power: Uint128::zero(),
                            removal_power: Uint128::zero(),
                            aligned_voters: vec![],
                            for_voters: vec![ Addr::unchecked(USER) ],
                            against_voters: vec![],
                            amendment_voters: vec![],
                            removal_voters: vec![],
                            start_block: 1,
                            start_time: 1,
                            end_block: 1,
                            delayed_end_block: 1,
                            expiration_block: 1,
                            title: String::from(""),
                            description: String::from(""),
                            link: None,
                            messages: None,
                        })?)
                    },
                }
            },
        );
        Box::new(contract)
    }

    //Mock LQ Contract
    #[cw_serde]
    pub enum LQ_MockExecuteMsg {}
    
    #[cw_serde]
    pub struct LQ_MockInstantiateMsg {}
    
    #[cw_serde]
    pub enum LQ_MockQueryMsg {
        UserClaims {
            user: String,
        }
    }

    pub fn liq_queue_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: LQ_MockExecuteMsg| -> StdResult<Response> {
                match msg {}
            },
            |_, _, _, _: LQ_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: LQ_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    LQ_MockQueryMsg::UserClaims {
                        user: _,
                    } => 
                        Ok(to_binary(&vec![LQ_ClaimsResponse {
                            bid_for: String::from("cdp"),
                            pending_liquidated_collateral: Uint256::from(100_000_000u128),
                        }, LQ_ClaimsResponse {
                            bid_for: String::from("second_cdp"),
                            pending_liquidated_collateral: Uint256::from(10_000_000u128),
                        }])?)
                    
                }
            },
        );
        Box::new(contract)
    }
    pub fn post_liq_queue_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: LQ_MockExecuteMsg| -> StdResult<Response> {
                match msg {}
            },
            |_, _, _, _: LQ_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: LQ_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    LQ_MockQueryMsg::UserClaims {
                        user: _,
                    } => 
                        Ok(to_binary(&vec![LQ_ClaimsResponse {
                            bid_for: String::from("cdp"),
                            pending_liquidated_collateral: Uint256::from(90_000_000u128),
                        }])?)
                    
                }
            },
        );
        Box::new(contract)
    }

    //Mock SP Contract    
    #[cw_serde]
    pub enum SP_MockExecuteMsg {}
    
    #[cw_serde]
    pub struct SP_MockInstantiateMsg {}
    
    #[cw_serde]
    pub enum SP_MockQueryMsg {
        UserClaims {
            user: String,
        }        
    }

    pub fn stability_pool_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: SP_MockExecuteMsg| -> StdResult<Response> {
                match msg {}
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::UserClaims { user: _ } => {
                        Ok(to_binary(&SP_ClaimsResponse {
                            claims: vec![
                                Coin::new(100_000_000u128, "claim1"),
                                Coin::new(10_000_000u128, "claim2"),
                            ],
                        })?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    pub fn post_stability_pool_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: SP_MockExecuteMsg| -> StdResult<Response> {
                match msg {}
            },
            |_, _, _, _: SP_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: SP_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    SP_MockQueryMsg::UserClaims { user: _ } => {
                        Ok(to_binary(&SP_ClaimsResponse {
                            claims: vec![
                                Coin::new(90_000_000u128, "claim1"),
                            ],
                        })?)
                    }
                }
            },
        );
        Box::new(contract)
    }

    //Mock Osmo Proxy Contract    
    #[cw_serde]
    pub enum Osmo_MockExecuteMsg {
        MintTokens {
            denom: String,
            amount: Uint128,
            mint_to_address: String,
        }
    }

    
    #[cw_serde]
    pub struct Osmo_MockInstantiateMsg {}

    
    #[cw_serde]
    pub enum Osmo_MockQueryMsg {}

    pub fn osmosis_proxy_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |_, _, _, msg: Osmo_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Osmo_MockExecuteMsg::MintTokens {
                        denom,
                        amount,
                        mint_to_address,
                    } => {
                        Ok(Response::new())
                    }
                }
            },
            |_, _, _, _: Osmo_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Osmo_MockQueryMsg| -> StdResult<Binary> {
                match msg {}
            },
        );
        Box::new(contract)
    }

    //Mock Oracle Contract
     #[cw_serde]    
    pub enum Oracle_MockExecuteMsg {}

     #[cw_serde]    
    pub struct Oracle_MockInstantiateMsg {}

     #[cw_serde]    
    pub enum Oracle_MockQueryMsg {
        Prices {
            asset_infos: Vec<AssetInfo>,
            twap_timeframe: u64,
            oracle_time_limit: u64,
        },
    }

    pub fn oracle_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Oracle_MockExecuteMsg| -> StdResult<Response> {
                match msg {}
            },
            |_, _, _, _: Oracle_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Oracle_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Oracle_MockQueryMsg::Prices {
                        asset_infos,
                        twap_timeframe,
                        oracle_time_limit,
                    } => 
                        {
                            let mut resp = vec![];
                            for asset in asset_infos {
                                resp.push(PriceResponse {
                                    prices: vec![],
                                    price: Decimal::one(),
                                    decimals: 6,
                                });
                            }
                            Ok(to_binary(&resp)?) 
                    }
            }
        }
        );
        Box::new(contract)
    }

    fn mock_app() -> App {
        AppBuilder::new().build(|router, _, storage| {
            let bank = BankKeeper::new();

            bank.init_balance(
                storage,
                &Addr::unchecked(USER),
                vec![coin(100_000_000_000, "debit"), coin(100_000_000_000, "2nddebit")],
            )
            .unwrap();
            bank.init_balance(
                storage,
                &Addr::unchecked("contract10"),
                vec![coin(1_000_000, "debit"), coin(1_000_000, "2nddebit")],
            )
            .unwrap();

            router.bank = bank;
        })
    }

    pub fn proper_instantiate() -> (App, PointsContract, Vec<Addr>) {
        let mut app = mock_app();

        //Instantiate Governance
        let gov_id: u64 = app.store_code(gov_contract());

        let gov_contract_addr = app
            .instantiate_contract(
                gov_id,
                Addr::unchecked(ADMIN),
                &Gov_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Post Governance Contract
        let post_gov_id: u64 = app.store_code(post_gov_contract());

        let post_gov_contract_addr = app
            .instantiate_contract(
                post_gov_id,
                Addr::unchecked(ADMIN),
                &Gov_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instanitate SP
        let sp_id: u64 = app.store_code(stability_pool_contract());        

        let sp_contract_addr = app
            .instantiate_contract(
                sp_id,
                Addr::unchecked(ADMIN),
                &SP_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Post SP Contract
        let post_sp_id: u64 = app.store_code(post_stability_pool_contract());

        let post_sp_contract_addr = app
            .instantiate_contract(
                post_sp_id,
                Addr::unchecked(ADMIN),
                &SP_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate LQ
        let lq_id: u64 = app.store_code(liq_queue_contract());

        let lq_contract_addr = app
            .instantiate_contract(
                lq_id,
                Addr::unchecked(ADMIN),
                &LQ_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Post LQ Contract
        let post_lq_id: u64 = app.store_code(post_liq_queue_contract());

        let post_lq_contract_addr = app
            .instantiate_contract(
                post_lq_id,
                Addr::unchecked(ADMIN),
                &LQ_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instaniate Osmosis Proxy
        let proxy_id: u64 = app.store_code(osmosis_proxy_contract());

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

        //Instaniate Oracle Contract
        let oracle_id: u64= app.store_code(oracle_contract());

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

        //Instantiate CDP contract
        let cdp_id = app.store_code(cdp_contract());

        let cdp_contract_addr = app
            .instantiate_contract(
                cdp_id,
                Addr::unchecked(ADMIN),
                &Oracle_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instantiate Post CDP contract
        let post_cdp_id = app.store_code(post_cdp_contract());

        let post_cdp_contract_addr = app
            .instantiate_contract(
                post_cdp_id,
                Addr::unchecked(ADMIN),
                &Oracle_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instantiate Points Contract
        let points_id = app.store_code(points_contract());
        let msg = InstantiateMsg { 
            cdt_denom: String::from("cdt"), 
            oracle_contract: oracle_contract_addr.clone().to_string(), 
            positions_contract: cdp_contract_addr.clone().to_string(),
            stability_pool_contract: sp_contract_addr.clone().to_string(),
            liq_queue_contract: lq_contract_addr.clone().to_string(),
            governance_contract: gov_contract_addr.clone().to_string(),
            osmosis_proxy_contract: osmosis_proxy_contract_addr.clone().to_string(),
        };
        let points_contract_addr = app
            .instantiate_contract(points_id, Addr::unchecked(ADMIN), &msg, &[], "test", None)
            .unwrap();

        let points_contract = PointsContract(points_contract_addr);

        (app, points_contract, vec![post_cdp_contract_addr, post_gov_contract_addr, post_lq_contract_addr, post_sp_contract_addr])
    }

    mod points {

        use cosmwasm_std::coins;
        use membrane::points_system::{ClaimCheck, Config, UserStats, UserStatsResponse};

        use super::*;

        //Can't test liquidated CDT bc we can't switch contracts before the reply
        #[test]
        fn liquidation_points() {
            let (mut app, points_contract, post_action_contracts) = proper_instantiate();

            //Liquidate
            let msg = ExecuteMsg::Liquidate { position_id: Uint128::one(), position_owner: String::from("fefesfaefe") };
            let cosmos_msg = points_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            
            //Query contract to assert points were given
            let stats: Vec<UserStatsResponse> = app
                .wrap()
                .query_wasm_smart(
                    points_contract.addr(),
                    &QueryMsg::UserStats { user: Some(String::from(USER)), limit: None, start_after: None },
                )
                .unwrap();
            assert_eq!(stats[0], UserStatsResponse { 
                user: Addr::unchecked(USER), 
                stats: UserStats { 
                    total_points: Decimal::from_ratio(2u128, 1u128), 
                    claimable_points: Decimal::from_ratio(2u128, 1u128),
                }
             });
        }

        #[test]
        fn points_flow() {
            let (mut app, points_contract, post_action_contracts) = proper_instantiate();

            //CheckClaims
            let msg = ExecuteMsg::CheckClaims { 
                cdp_repayment: true, 
                sp_claims: true, 
                lq_claims: true, 
                vote: Some(vec![1u64, 21u64, 3u64])
            };
            let cosmos_msg = points_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            
            //Query contract to assert claims were saved
            let pending_claims: ClaimCheck = app
                .wrap()
                .query_wasm_smart(
                    points_contract.addr(),
                    &QueryMsg::ClaimCheck {  },
                )
                .unwrap();
            assert_eq!(pending_claims, ClaimCheck { 
                user: Addr::unchecked(USER), 
                cdp_pending_revenue: Uint128::new(1_000_000),
                sp_pending_claims: vec![coin(100_000_000, "claim1"), coin(10_000_000, "claim2")],
                lq_pending_claims: vec![LQ_ClaimsResponse {
                    bid_for: String::from("cdp"),
                    pending_liquidated_collateral: Uint256::from(100_000_000u128),
                }, LQ_ClaimsResponse {
                    bid_for: String::from("second_cdp"),
                    pending_liquidated_collateral: Uint256::from(10_000_000u128),
                }],
                vote_pending: vec![1u64, 21u64], //Doesn't add 3 bc USER voted in it
             });

             //Update config to use post action contracts
                let msg = ExecuteMsg::UpdateConfig { 
                    owner: None,
                    cdt_denom: None,
                    oracle_contract: None,
                    positions_contract: Some(post_action_contracts[0].clone().to_string()),
                    stability_pool_contract: Some(post_action_contracts[3].clone().to_string()),
                    liq_queue_contract: Some(post_action_contracts[2].clone().to_string()),
                    governance_contract: Some(post_action_contracts[1].clone().to_string()),
                    osmosis_proxy_contract: None,
                    mbrn_per_point: None,
                    max_mbrn_distribution: None,
                    points_per_dollar: None,
                };
                let cosmos_msg = points_contract.call(msg, vec![]).unwrap();
                app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

                //GivePoints
                let msg = ExecuteMsg::GivePoints { 
                    cdp_repayment: true, 
                    sp_claims: true, 
                    lq_claims: true, 
                    vote: Some(vec![1u64, 21u64, 3u64]) //Should still only give 2 points bc the 3rd Prop was already voted in 
                };                
                let cosmos_msg = points_contract.call(msg, vec![]).unwrap();
                //Call from someone who isnt the owner of the claim check errors
                app.execute(Addr::unchecked(ADMIN), cosmos_msg.clone()).unwrap_err();
                //Success
                app.execute(Addr::unchecked(USER), cosmos_msg.clone()).unwrap();
                //A 2nd call errors bc claim check is empty
                app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

                //Query contract to assert points were given
                let stats: Vec<UserStatsResponse> = app
                    .wrap()
                    .query_wasm_smart(
                        points_contract.addr(),
                        &QueryMsg::UserStats { user: Some(String::from(USER)), limit: None, start_after: None },
                    )
                    .unwrap();
                assert_eq!(stats[0], UserStatsResponse {
                    user: Addr::unchecked(USER), 
                    stats: UserStats { 
                        total_points: Decimal::from_ratio(42u128, 1u128), 
                        claimable_points: Decimal::from_ratio(42u128, 1u128),
                    }
                });
                //20 from each liquidation contract claim & 1 from each vote

                
                //Claim MBRN: Not a user
                let msg = ExecuteMsg::ClaimMBRN {};
                let cosmos_msg = points_contract.call(msg, vec![]).unwrap();
                app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap_err();

                //Claim MBRN
                let msg = ExecuteMsg::ClaimMBRN {};
                let cosmos_msg = points_contract.call(msg, vec![]).unwrap();
                app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
                
                //Claim MBRN: Error none to claim
                let msg = ExecuteMsg::ClaimMBRN {};
                let cosmos_msg = points_contract.call(msg, vec![]).unwrap();
                app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();

                //Query contract to assert points were given
                let stats: Vec<UserStatsResponse> = app
                    .wrap()
                    .query_wasm_smart(
                        points_contract.addr(),
                        &QueryMsg::UserStats { user: Some(String::from(USER)), limit: None, start_after: None },
                    )
                    .unwrap();
                assert_eq!(stats[0], UserStatsResponse {
                    user: Addr::unchecked(USER), 
                    stats: UserStats { 
                        total_points: Decimal::from_ratio(42u128, 1u128), 
                        claimable_points: Decimal::from_ratio(0u128, 1u128),
                    }
                });
                
        }


        #[test]
        fn update_config() {
            let (mut app, points_contract, _) = proper_instantiate();

            //Successful UpdateConfig
            let msg = ExecuteMsg::UpdateConfig { 
                owner: Some(String::from("new_owner")), 
                positions_contract: Some(String::from("new_pos_contract")),
                cdt_denom:  Some(String::from("new_cdt_denom")),
                oracle_contract: Some(String::from("new_oracle_contract")),
                stability_pool_contract: Some(String::from("new_sp_contract")),
                liq_queue_contract: Some(String::from("new_lq_contract")),
                governance_contract:    Some(String::from("new_gov_contract")),
                osmosis_proxy_contract: Some(String::from("new_router_contract")),
                mbrn_per_point: Some(Decimal::zero()),
                max_mbrn_distribution: Some(Uint128::zero()),
                points_per_dollar: Some(Decimal::zero()),
            };
            let cosmos_msg = points_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            
            //Query Config
            let config: Config = app
                .wrap()
                .query_wasm_smart(
                    points_contract.addr(),
                    &QueryMsg::Config {},
                )
                .unwrap();
            assert_eq!(
                config, 
                Config {
                    owner: Addr::unchecked(ADMIN), 
                    positions_contract:  Addr::unchecked("new_pos_contract"), 
                    cdt_denom: String::from("new_cdt_denom"),
                    oracle_contract: Addr::unchecked("new_oracle_contract"),
                    stability_pool_contract: Addr::unchecked("new_sp_contract"),
                    liq_queue_contract: Addr::unchecked("new_lq_contract"),
                    governance_contract: Addr::unchecked("new_gov_contract"),
                    osmosis_proxy_contract: Addr::unchecked("new_router_contract"),
                    mbrn_per_point: Decimal::zero(),
                    max_mbrn_distribution: Uint128::zero(),
                    points_per_dollar: Decimal::zero(),
                    total_mbrn_distribution: Uint128::zero(),                    
            });
        }
    }
}
