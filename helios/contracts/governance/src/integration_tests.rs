#[cfg(test)]
mod tests {
    
    use crate::helpers::{ GovContract };  
    
    use cw20::BalanceResponse;
    use membrane::governance::{ InstantiateMsg, QueryMsg, ExecuteMsg, VOTING_PERIOD_INTERVAL, STAKE_INTERVAL };
    use::membrane::builder_vesting::{ QueryMsg as BVQueryMsg, AllocationResponse };
    use membrane::staking::{ RewardsResponse, StakedResponse, ConfigResponse as StakingConfigResponse };
    use membrane::osmosis_proxy::{ GetDenomResponse };
    use membrane::types::{AssetInfo, Asset, VestingPeriod, StakeDeposit };
    use membrane::math::Uint256;

    
    use osmo_bindings::{ SpotPriceResponse, PoolStateResponse, ArithmeticTwapToNowResponse };
    use cosmwasm_std::{Addr, Coin, Empty, Uint128, Decimal, Response, StdResult, Binary, to_binary, coin, attr, StdError };
    use cw_multi_test::{App, AppBuilder, Contract, ContractWrapper, Executor, BankKeeper};
    use schemars::JsonSchema;
    use serde::{ Deserialize, Serialize };


    const USER: &str = "user";
    const ADMIN: &str = "admin";

    const PROPOSAL_VOTING_PERIOD: u64 = *VOTING_PERIOD_INTERVAL.start();
    const PROPOSAL_EFFECTIVE_DELAY: u64 = 14399;
    const PROPOSAL_EXPIRATION_PERIOD: u64 = 100799;
    const PROPOSAL_REQUIRED_STAKE: u128 = *STAKE_INTERVAL.start();
    const PROPOSAL_REQUIRED_QUORUM: &str = "0.50";
    const PROPOSAL_REQUIRED_THRESHOLD: &str = "0.60";

    //Gov Contract
    pub fn gov_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new_with_empty(
            crate::contracts::execute,
            crate::contracts::instantiate,
            crate::contracts::query,
        );
        Box::new(contract)
    }
      
    //Mock Osmo Proxy Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Osmo_MockExecuteMsg {
        MintTokens {
            denom: String,
            amount: Uint128,
            mint_to_address: String,
        },
        BurnTokens {
            denom: String,
            amount: Uint128,
            burn_from_address: String,
        },
        CreateDenom {
            subdenom: String,
        },
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Osmo_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Osmo_MockQueryMsg {
        SpotPrice {
            asset: String,
        },
        PoolState {
            id: u64,
        },
        GetDenom {
            creator_address: String,
            subdenom: String,
        },
        ArithmeticTwapToNow {
            id: u64,
            quote_asset_denom: String,
            base_asset_denom: String,
            start_time: i64,
        },
    }

    pub fn osmosis_proxy_contract()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Osmo_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Osmo_MockExecuteMsg::MintTokens { 
                            denom, 
                            amount, 
                            mint_to_address
                     } => {
                        
                        Ok(Response::new())
                    },
                    Osmo_MockExecuteMsg::BurnTokens {
                        denom,
                        amount,
                        burn_from_address,
                    } => {
                        Ok(Response::new())
                    },
                    Osmo_MockExecuteMsg::CreateDenom { 
                        subdenom
                    } => {

                        Ok(Response::new().add_attributes(vec![
                            attr("basket_id", "1"),
                            attr("subdenom", "credit_fulldenom")]
                        ))
                    }
                }
            },
            |_, _, _, _: Osmo_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Osmo_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Osmo_MockQueryMsg::SpotPrice { 
                        asset,
                    } => 
                        Ok(
                            to_binary(&SpotPriceResponse {
                                price: Decimal::one(),
                            })?
                        ),
                    Osmo_MockQueryMsg::PoolState { id } => 
                    if id == 99u64 {
                        Ok(
                            to_binary(&PoolStateResponse {
                                assets: vec![ coin( 100_000_000 , "base" ), coin( 100_000_000 , "quote" ) ],
                                shares: coin( 100_000_000, "lp_denom" ),
                            }

                            )?
                        )
                    } else {
                        Ok(
                            to_binary(&PoolStateResponse {
                                assets: vec![ coin( 49_999 , "credit_fulldenom" ) ],
                                shares: coin( 0, "shares" ),
                            }

                            )?
                        )
                    },
                    Osmo_MockQueryMsg::GetDenom { 
                        creator_address, 
                        subdenom 
                    } => {
                        Ok(
                            to_binary(&GetDenomResponse {
                                denom: String::from( "credit_fulldenom" ),
                            })?
                        )
                    },
                    Osmo_MockQueryMsg::ArithmeticTwapToNow { 
                        id, 
                        quote_asset_denom, 
                        base_asset_denom, 
                        start_time 
                    } => {
                        if base_asset_denom == String::from("base") {

                            Ok(
                                to_binary(&ArithmeticTwapToNowResponse {
                                    twap: Decimal::percent(100),
                                })?
                            )

                        } else {

                            Ok(
                                to_binary(&ArithmeticTwapToNowResponse {
                                    twap: Decimal::percent(100),
                                })?
                            )

                        }
                    }
                }},
        );
        Box::new(contract)
    }

    
    //Mock Staking Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Staking_MockExecuteMsg {
        DepositFee {
            fee_assets: Vec<Asset>,
        },
        ClaimRewards {
            claim_as_cw20: Option<String>,
            claim_as_native: Option<String>,
            send_to: Option<String>,
            restake: bool,
        },
        Stake{
            user: Option<String>,
        },
    }
    
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Staking_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Staking_MockQueryMsg {
        StakerRewards {
            staker: String,
        },
        Staked {
            limit: Option<u64>,
            start_after: Option<u64>,
            end_before: Option<u64>,
            unstaking: bool,
        },
        Config {},
    }

    pub fn staking_contract()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Staking_MockExecuteMsg| -> StdResult<Response> {
                match msg {
                    Staking_MockExecuteMsg::DepositFee {
                        fee_assets
                     }  => {                        
                        Ok( Response::default() )
                    },
                    Staking_MockExecuteMsg::ClaimRewards {
                        claim_as_cw20,
                        claim_as_native,
                        send_to,
                        restake,
                    } => {
                        Ok( Response::default() )
                    },
                    Staking_MockExecuteMsg::Stake {
                        user
                    } => {
                        Ok( Response::default() )
                    },
                }
            },
            |_, _, _, _: Staking_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: Staking_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Staking_MockQueryMsg::StakerRewards {
                        staker
                    } => {
                        Ok(
                            to_binary( &RewardsResponse {
                                claimables: vec![
                                    Asset {
                                        info: AssetInfo::NativeToken { denom: String::from("debit") },
                                        amount: Uint128::new(1_000_000u128),
                                    },
                                    Asset {
                                        info: AssetInfo::NativeToken { denom: String::from("2nddebit") },
                                        amount: Uint128::new(1_000_000u128),
                                    },
                                ],
                                accrued_interest: Uint128::zero(),
                            })?
                        )
                    },
                    Staking_MockQueryMsg::Staked { 
                        limit, 
                        start_after, 
                        end_before, 
                        unstaking,
                    } => {
                        
                        Ok(
                            to_binary(&StakedResponse {
                                stakers: vec![ 
                                StakeDeposit { 
                                    staker: Addr::unchecked(USER), 
                                    amount: Uint128::new(100_000_002u128), 
                                    stake_time: 1u64, 
                                    unstake_start_time: None, 
                                },
                                StakeDeposit { 
                                    staker: Addr::unchecked(ADMIN), 
                                    amount: Uint128::new(60_000_000u128), 
                                    stake_time: 1u64, 
                                    unstake_start_time: None, 
                                }],
                            })?
                        )
                    },
                    Staking_MockQueryMsg::Config {  } => {
                        Ok(
                            to_binary(&StakingConfigResponse {
                                owner: String::from(""),
                                positions_contract: String::from(""),
                                builders_contract: String::from(""),
                                osmosis_proxy: String::from(""),
                                staking_rate: String::from(""),
                                unstaking_period: String::from(""),
                                fee_wait_period: String::from(""),
                                mbrn_denom: String::from("mbrn_denom"),
                                dex_router: String::from(""),
                                max_spread: String::from(""),
                            })?
                        )
                    }
                }
             },
        );
        Box::new(contract)
    }

    //Mock BV Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum BV_MockExecuteMsg { }
    
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct BV_MockInstantiateMsg { }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum BV_MockQueryMsg {
        Allocation {
            receiver: String,
        },
    }

    pub fn bv_contract()-> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: BV_MockExecuteMsg| -> StdResult<Response> {
                Ok( Response::default() )
            },
            |_, _, _, _: Staking_MockInstantiateMsg| -> StdResult<Response> { Ok(Response::default()) },
            |_, _, msg: BV_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    BV_MockQueryMsg::Allocation {
                        receiver
                    } => {
                        Ok(
                            to_binary( &AllocationResponse {
                                amount: String::from("1000000000"),
                                amount_withdrawn: String::from("0"),
                                start_time_of_allocation: String::from("0"),
                                vesting_period: VestingPeriod { cliff: 0u64, linear: 0u64 },
                            })?
                        )
                    },
                }
             },
        );
        Box::new(contract)
    }

    
    fn mock_app() -> App {
            AppBuilder::new().build(|router, _, storage| {
                                    
                let bank = BankKeeper::new();
                
                bank.init_balance(storage, &Addr::unchecked("contract3"), vec![coin(30_000_000_000_000, "mbrn_denom")])
                .unwrap(); //contract3 = Builders contract                
                bank.init_balance(storage, &Addr::unchecked("coin_God"), vec![coin(100_000_000, "debit"), coin(100_000_000, "2nddebit")])
                .unwrap();
              

                router
                    .bank = bank;
                    
            })
        }

    fn proper_instantiate(  ) -> (App, GovContract, Addr ) {
        let mut app = mock_app();

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

        //Instaniate Builder's Contract
        let bv_id = app.store_code(bv_contract());

        let bv_contract_addr = app
            .instantiate_contract(
                bv_id,
                Addr::unchecked(ADMIN),
                &Staking_MockInstantiateMsg {},
                &[],
                "test",
                None,
            )
            .unwrap();

        //Instantiate Gov contract
        let gov_id = app.store_code(gov_contract());

        let msg = InstantiateMsg {
            mbrn_staking_contract_addr: staking_contract_addr.to_string(), 
            builders_contract_addr: bv_contract_addr.to_string(),
            builders_voting_power_multiplier: Decimal::percent(33),
            proposal_voting_period: PROPOSAL_VOTING_PERIOD,
            proposal_effective_delay: PROPOSAL_EFFECTIVE_DELAY,
            proposal_expiration_period: PROPOSAL_EXPIRATION_PERIOD,
            proposal_required_stake: Uint128::from(PROPOSAL_REQUIRED_STAKE),
            proposal_required_quorum: String::from(PROPOSAL_REQUIRED_QUORUM),
            proposal_required_threshold: String::from(PROPOSAL_REQUIRED_THRESHOLD),
            whitelisted_links: vec!["https://some.link/".to_string()],
        };        

        let gov_contract_addr = app
            .instantiate_contract(
                gov_id,
                Addr::unchecked(ADMIN),
                &msg,
                &[],
                "test",
                None,
            )
            .unwrap();

        let gov_contract = GovContract(gov_contract_addr);


        (app, gov_contract, bv_contract_addr )
    }
   


    mod gov {
        
        use super::*;
        use cosmwasm_std::{BlockInfo, Uint64, WasmMsg};
        use cw20::Cw20ReceiveMsg;
        use membrane::governance::{ProposalMessage, ProposalResponse, ProposalStatus, ProposalVoteOption, ProposalVotesResponse, UpdateConfig, Config, ProposalListResponse};
        

        #[test]
        fn stake_minimum() {
            let (mut app, gov_contract, bv_contract_addr) = proper_instantiate( );

            //Submit Proposal Fail due to insufficient stake
            let msg = ExecuteMsg::SubmitProposal { 
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: None,
                messages: None,
                receiver: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked("not_staked"), cosmos_msg).unwrap_err();
            assert_eq!( err.root_cause().to_string(), String::from("Insufficient stake!"));

            //Successful submission
            let msg = ExecuteMsg::SubmitProposal { 
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: None,
                messages: None,
                receiver: Some( String::from("receiver") ),
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(bv_contract_addr, cosmos_msg).unwrap();

            //Successful submission
            let msg = ExecuteMsg::SubmitProposal { 
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: None,
                messages: None,
                receiver: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            
        }

        #[test]
        fn submit_proposal() {
            let (mut app, gov_contract, bv_contract_addr) = proper_instantiate( );
            
            //Title too short
            let msg = ExecuteMsg::SubmitProposal { 
                title: "X".to_string(),
                description: "Test description!".to_string(),
                link: None,
                messages: None,
                receiver: Some( String::from("receiver") ),
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!( err.root_cause().to_string(), String::from("Generic error: Title too short!"));

            //Title too long
            let msg = ExecuteMsg::SubmitProposal { 
                title: String::from_utf8(vec![b'X'; 65]).unwrap(),
                description: "Test description!".to_string(),
                link: None,
                messages: None,
                receiver: Some( String::from("receiver") ),
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(bv_contract_addr.clone()), cosmos_msg).unwrap_err();
            assert_eq!( err.root_cause().to_string(), String::from("Generic error: Title too long!"));

            //Description too short
            let msg = ExecuteMsg::SubmitProposal { 
                title: "Test Title".to_string(),
                description: "X".to_string(),
                link: None,
                messages: None,
                receiver: Some( String::from("receiver") ),
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(bv_contract_addr.clone()), cosmos_msg).unwrap_err();
            assert_eq!( err.root_cause().to_string(), String::from("Generic error: Description too short!"));

            //Description too long
            let msg = ExecuteMsg::SubmitProposal { 
                title: "Test Title".to_string(),
                description: String::from_utf8(vec![b'X'; 1025]).unwrap(),
                link: None,
                messages: None,
                receiver: Some( String::from("receiver") ),
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(bv_contract_addr.clone()), cosmos_msg).unwrap_err();
            assert_eq!( err.root_cause().to_string(), String::from("Generic error: Description too long!"));

            //Link too short
            let msg = ExecuteMsg::SubmitProposal { 
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link:  Some( String::from("X") ),
                messages: None,
                receiver: Some( String::from("receiver") ),
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(bv_contract_addr.clone()), cosmos_msg).unwrap_err();
            assert_eq!( err.root_cause().to_string(), String::from("Generic error: Link too short!"));

            //Link too long
            let msg = ExecuteMsg::SubmitProposal { 
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: Some( String::from_utf8(vec![b'X'; 129]).unwrap() ),
                messages: None,
                receiver: Some( String::from("receiver") ),
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(bv_contract_addr.clone()), cosmos_msg).unwrap_err();
            assert_eq!( err.root_cause().to_string(), String::from("Generic error: Link too long!"));

            //Link not whitelisted
            let msg = ExecuteMsg::SubmitProposal { 
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: Some( String::from("https://some1.link") ),
                messages: None,
                receiver: Some( String::from("receiver") ),
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(bv_contract_addr.clone()), cosmos_msg).unwrap_err();
            assert_eq!( err.root_cause().to_string(), String::from("Generic error: Link is not whitelisted!"));

            //Link unsafe characters
            let msg = ExecuteMsg::SubmitProposal { 
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: Some( String::from(
                    "https://some.link/<script>alert('test');</script>",
                ) ),
                messages: None,
                receiver: Some( String::from("receiver") ),
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(bv_contract_addr.clone()), cosmos_msg).unwrap_err();
            assert_eq!( err.root_cause().to_string(), String::from("Generic error: Link is not properly formatted or contains unsafe characters!"));

            //Submit Proposal
            let msg = ExecuteMsg::SubmitProposal { 
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: Some( String::from("https://some.link/linker") ),
                messages: Some( vec![
                    ProposalMessage { 
                        order: Uint64::new(1u64), 
                        msg: cosmwasm_std::CosmosMsg::Wasm(WasmMsg::Execute { 
                            contract_addr: String::from("addr"), 
                            msg: to_binary(&"msg").unwrap(), 
                            funds: vec![], 
                        }),
                    }] ),
                receiver: Some( String::from("receiver") ),
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(bv_contract_addr.clone(), cosmos_msg).unwrap();

            let proposal: ProposalResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.clone().addr(),
                    &QueryMsg::Proposal { proposal_id: 1 },
                )
                .unwrap();

            assert_eq!(proposal.proposal_id, Uint64::from(1u64));
            assert_eq!(proposal.submitter, Addr::unchecked("receiver"));
            assert_eq!(proposal.status, ProposalStatus::Active);
            assert_eq!(proposal.for_power, Uint128::zero());
            assert_eq!(proposal.against_power, Uint128::zero());
            assert_eq!(proposal.start_block, 12_345);
            assert_eq!(proposal.end_block, 12_345 + PROPOSAL_VOTING_PERIOD);
            assert_eq!(proposal.title, String::from("Test title!"));
            assert_eq!(proposal.description, String::from("Test description!"));
            assert_eq!(proposal.link, Some(String::from("https://some.link/linker")));
            assert_eq!(
                proposal.messages, Some(vec![
                    ProposalMessage { 
                        order: Uint64::new(1u64), 
                        msg: cosmwasm_std::CosmosMsg::Wasm(WasmMsg::Execute { 
                            contract_addr: String::from("addr"), 
                            msg: to_binary(&"msg").unwrap(), 
                            funds: vec![], 
                        }),
                    }]));
        }

        #[test]
        fn successful_proposal() {
            let (mut app, gov_contract, bv_contract_addr) = proper_instantiate( );
            
            
            //Submit Proposal
            let msg = ExecuteMsg::SubmitProposal { 
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: Some( String::from("https://some.link/linker") ),
                messages: Some( vec![
                    ProposalMessage { 
                        order: Uint64::new(1u64), 
                        msg: cosmwasm_std::CosmosMsg::Wasm(WasmMsg::Execute { 
                            contract_addr: gov_contract.clone().addr().to_string(), 
                            msg: to_binary(&ExecuteMsg::UpdateConfig(UpdateConfig {
                                mbrn_denom: None,
                                staking_contract: None,
                                builders_contract_addr: None,
                                builders_voting_power_multiplier: None,
                                proposal_voting_period: Some(PROPOSAL_VOTING_PERIOD + 1000),
                                proposal_effective_delay: None,
                                proposal_expiration_period: None,
                                proposal_required_stake: None,
                                proposal_required_quorum: None,
                                proposal_required_threshold: None,
                                whitelist_add: Some(vec![
                                    "https://some1.link/".to_string(),
                                    "https://some2.link/".to_string(),
                                ]),
                                whitelist_remove: Some(vec!["https://some.link/".to_string()]),
                            }))
                            .unwrap(), 
                            funds: vec![], 
                        }),
                    }] ),
                receiver: Some( String::from("receiver") ),
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(bv_contract_addr.clone(), cosmos_msg).unwrap();

            ////Cast Votes
            // Fail bc they are proposal.submitter
            let msg = ExecuteMsg::CastVote { 
                proposal_id: 1u64, 
                vote: ProposalVoteOption::For, 
                receiver: Some( String::from("receiver") ), 
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(bv_contract_addr.clone(), cosmos_msg).unwrap_err();
            assert_eq!( err.root_cause().to_string(), String::from("Unauthorized"));

            //For
            let msg = ExecuteMsg::CastVote { 
                proposal_id: 1u64, 
                vote: ProposalVoteOption::For, 
                receiver: None, 
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            //Against
            let msg = ExecuteMsg::CastVote { 
                proposal_id: 1u64, 
                vote: ProposalVoteOption::Against, 
                receiver: None, 
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Assertations
            let proposal: ProposalResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.clone().addr(),
                    &QueryMsg::Proposal { proposal_id: 1 },
                )
                .unwrap();

            let proposal_votes: ProposalVotesResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.clone().addr(),
                    &QueryMsg::ProposalVotes { proposal_id: 1 },
                )
                .unwrap();

            let proposal_for_voters: Vec<Addr> = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.clone().addr(),
                    &QueryMsg::ProposalVoters {
                        proposal_id: 1,
                        vote_option: ProposalVoteOption::For,
                        start: None,
                        limit: None,
                    },
                )
                .unwrap();

            let proposal_against_voters: Vec<Addr> = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.clone().addr(),
                    &QueryMsg::ProposalVoters {
                        proposal_id: 1,
                        vote_option: ProposalVoteOption::Against,
                        start: None,
                        limit: None,
                    },
                )
                .unwrap();

            // Check proposal votes
            assert_eq!(proposal.for_power, Uint128::from(100_000_002u128));
            assert_eq!(proposal.against_power, Uint128::from(60_000_000u128));

            assert_eq!(proposal_votes.for_power, Uint128::from(100_000_002u128));
            assert_eq!(proposal_votes.against_power, Uint128::from(60_000_000u128));

            assert_eq!(
                proposal_for_voters,
                vec![
                    Addr::unchecked("user")
                ]
            );
            assert_eq!(
                proposal_against_voters,
                vec![
                    Addr::unchecked("admin")
                ]
            );

            // Skip voting period
            app.update_block(|bi| {
                bi.height += PROPOSAL_VOTING_PERIOD + 1;
                bi.time = bi.time.plus_seconds(6 * (PROPOSAL_VOTING_PERIOD + 1));
            });

            //Error bc voting period has passed
            let msg = ExecuteMsg::CastVote { 
                proposal_id: 1u64, 
                vote: ProposalVoteOption::For, 
                receiver: None, 
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!( err.root_cause().to_string(), String::from("Voting period ended!"));

            //Try to execute the proposal before ending it
            let msg = ExecuteMsg::ExecuteProposal { proposal_id: 1u64 };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!( err.root_cause().to_string(), String::from("Proposal not passed!"));

            //Successful End
            let msg = ExecuteMsg::EndProposal { proposal_id: 1u64 };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            let proposal: ProposalResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.clone().addr(),
                    &QueryMsg::Proposal { proposal_id: 1 },
                )
                .unwrap();

            assert_eq!(proposal.status, ProposalStatus::Passed);

            //Try to Execute before the delay
            let msg = ExecuteMsg::ExecuteProposal { proposal_id: 1u64 };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Proposal delay not ended!"));

            // Skip blocks
            app.update_block(|bi| {
                bi.height += PROPOSAL_EFFECTIVE_DELAY + 1;
                bi.time = bi.time.plus_seconds(6 * (PROPOSAL_EFFECTIVE_DELAY + 1));
            });

            //Try to Execute after the delay
            let msg = ExecuteMsg::ExecuteProposal { proposal_id: 1u64 };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            // Check execution result
            let config: Config = app
                .wrap()
                .query_wasm_smart(gov_contract.clone().addr().to_string(), &QueryMsg::Config {})
                .unwrap();
     
            let proposal: ProposalResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.clone().addr().to_string(),
                    &QueryMsg::Proposal { proposal_id: 1 },
                )
                .unwrap();
            assert_eq!(config.proposal_voting_period, PROPOSAL_VOTING_PERIOD + 1000);
            assert_eq!(
                config.whitelisted_links,
                vec![
                    "https://some1.link/".to_string(),
                    "https://some2.link/".to_string(),
                ]
            );
            assert_eq!(proposal.status, ProposalStatus::Executed);

            //Try to remove proposal before expiration period
            let msg = ExecuteMsg::RemoveCompletedProposal { proposal_id: 1u64 };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!( err.root_cause().to_string(), String::from("Proposal not completed!"));

            // Remove expired proposal
            app.update_block(|bi| {
                bi.height += PROPOSAL_EXPIRATION_PERIOD + 1;
                bi.time = bi.time.plus_seconds(6 * (PROPOSAL_EXPIRATION_PERIOD + 1));
            });

            app.execute_contract(
                Addr::unchecked("user0"),
                gov_contract.clone().addr(),
                &ExecuteMsg::RemoveCompletedProposal { proposal_id: 1 },
                &[],
            )
            .unwrap();

            let res: ProposalListResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.clone().addr().to_string(),
                    &QueryMsg::Proposals {
                        start: None,
                        limit: None,
                    },
                )
                .unwrap();

            assert_eq!(res.proposal_list, vec![]);
            // proposal_count should not be changed after removing a proposal
            assert_eq!(res.proposal_count, Uint64::from(1u32));

        }

        #[test]
        fn unsuccessful_proposal() {
            let (mut app, gov_contract, bv_contract_addr) = proper_instantiate( );
            
            //Submit Proposal
            let msg = ExecuteMsg::SubmitProposal { 
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: Some( String::from("https://some.link/linker") ),
                messages: Some( vec![
                    ProposalMessage { 
                        order: Uint64::new(1u64), 
                        msg: cosmwasm_std::CosmosMsg::Wasm(WasmMsg::Execute { 
                            contract_addr: gov_contract.clone().addr().to_string(), 
                            msg: to_binary(&ExecuteMsg::UpdateConfig(UpdateConfig {
                                mbrn_denom: None,
                                staking_contract: None,
                                builders_contract_addr: None,
                                builders_voting_power_multiplier: None,
                                proposal_voting_period: Some(PROPOSAL_VOTING_PERIOD + 1000),
                                proposal_effective_delay: None,
                                proposal_expiration_period: None,
                                proposal_required_stake: None,
                                proposal_required_quorum: None,
                                proposal_required_threshold: None,
                                whitelist_add: Some(vec![
                                    "https://some1.link/".to_string(),
                                    "https://some2.link/".to_string(),
                                ]),
                                whitelist_remove: Some(vec!["https://some.link/".to_string()]),
                            }))
                            .unwrap(), 
                            funds: vec![], 
                        }),
                    }] ),
                receiver: Some( String::from("receiver") ),
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(bv_contract_addr.clone(), cosmos_msg).unwrap();

            //For
            let msg = ExecuteMsg::CastVote { 
                proposal_id: 1u64, 
                vote: ProposalVoteOption::Against, 
                receiver: None, 
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            //Against
            let msg = ExecuteMsg::CastVote { 
                proposal_id: 1u64, 
                vote: ProposalVoteOption::For, 
                receiver: None, 
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
           
            // Skip voting period
            app.update_block(|bi| {
                bi.height += PROPOSAL_VOTING_PERIOD + 1;
                bi.time = bi.time.plus_seconds(6 * (PROPOSAL_VOTING_PERIOD + 1));
            });

            //Successful End
            let msg = ExecuteMsg::EndProposal { proposal_id: 1u64 };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            let proposal: ProposalResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.clone().addr(),
                    &QueryMsg::Proposal { proposal_id: 1 },
                )
                .unwrap();

            assert_eq!(proposal.status, ProposalStatus::Rejected);

            // Remove expired proposal
            app.update_block(|bi| {
                bi.height += PROPOSAL_EXPIRATION_PERIOD + PROPOSAL_EFFECTIVE_DELAY + 1;
                bi.time = bi
                    .time
                    .plus_seconds(6 * (PROPOSAL_EXPIRATION_PERIOD + PROPOSAL_EFFECTIVE_DELAY + 1));
            });

            app.execute_contract(
                Addr::unchecked(USER),
                gov_contract.clone().addr(),
                &ExecuteMsg::RemoveCompletedProposal { proposal_id: 1 },
                &[],
            )
            .unwrap();

            let res: ProposalListResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.clone().addr().to_string(),
                    &QueryMsg::Proposals {
                        start: None,
                        limit: None,
                    },
                )
                .unwrap();

            assert_eq!(res.proposal_list, vec![]);
            // proposal_count should not be changed after removing
            assert_eq!(res.proposal_count, Uint64::from(1u32));

        }

        #[test]
        fn check_messages() {
            let (mut app, gov_contract, bv_contract_addr) = proper_instantiate( );            

            let config_before: Config = app
                .wrap()
                .query_wasm_smart(gov_contract.clone().addr().to_string(), &QueryMsg::Config {})
                .unwrap();

            let msg = ExecuteMsg::CheckMessages { messages: vec![
                ProposalMessage { 
                    order: Uint64::new(1u64), 
                    msg: cosmwasm_std::CosmosMsg::Wasm(WasmMsg::Execute { 
                        contract_addr: gov_contract.clone().addr().to_string(), 
                        msg: to_binary(&ExecuteMsg::UpdateConfig(UpdateConfig {
                            mbrn_denom: None,
                            staking_contract: None,
                            builders_contract_addr: None,
                            builders_voting_power_multiplier: None,
                            proposal_voting_period: Some(PROPOSAL_VOTING_PERIOD + 1000),
                            proposal_effective_delay: None,
                            proposal_expiration_period: None,
                            proposal_required_stake: None,
                            proposal_required_quorum: None,
                            proposal_required_threshold: None,
                            whitelist_add: Some(vec![
                                "https://some1.link/".to_string(),
                                "https://some2.link/".to_string(),
                            ]),
                            whitelist_remove: Some(vec!["https://some.link/".to_string()]),
                        }))
                        .unwrap(), 
                        funds: vec![], 
                    }),
                }] };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(bv_contract_addr.clone(), cosmos_msg).unwrap_err();
            assert_eq!(
                &err.root_cause().to_string(),
                "Messages check passed. Nothing was committed to the blockchain"
            );
            
            let config_after: Config = app
                .wrap()
                .query_wasm_smart(gov_contract.clone().addr().to_string(), &QueryMsg::Config {})
                .unwrap();
            assert_eq!( config_before, config_after );
        }

    }
}