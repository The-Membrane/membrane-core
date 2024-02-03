#[cfg(test)]
#[allow(unused_variables)]

mod tests {

    use crate::helpers::GovContract;

    use ::membrane::vesting::AllocationResponse;
    use membrane::governance::{
        ExecuteMsg, InstantiateMsg, QueryMsg, STAKE_INTERVAL, VOTING_PERIOD_INTERVAL,
    };
    use membrane::staking::{
        Config as StakingConfig, StakedResponse, TotalStakedResponse, DelegationResponse
    };
    use membrane::types::{StakeDeposit, VestingPeriod, StakeDistribution, DelegationInfo, Delegation, Allocation};

    use cosmwasm_std::{
        coin, to_binary, Addr, Binary, Decimal, Empty, Response, StdResult, Uint128,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use membrane::vesting::{RecipientsResponse, RecipientResponse};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    const USER: &str = "user";
    const ADMIN: &str = "admin";

    const PROPOSAL_VOTING_PERIOD: u64 = *VOTING_PERIOD_INTERVAL.start();
    const PROPOSAL_EFFECTIVE_DELAY: u64 = 2399;
    const PROPOSAL_EXPIRATION_PERIOD: u64 = 2399*14;
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
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Osmo_MockExecuteMsg { }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Osmo_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Osmo_MockQueryMsg { }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct MockResponse { }

    pub fn osmosis_proxy_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Osmo_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: Osmo_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Osmo_MockQueryMsg| -> StdResult<Binary> {
                Ok(to_binary(&MockResponse { })?)
            },
        );
        Box::new(contract)
    }

    //Mock Staking Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Staking_MockExecuteMsg { }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Staking_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Staking_MockQueryMsg {
        Staked {
            limit: Option<u64>,
            start_after: Option<u64>,
            end_before: Option<u64>,
            unstaking: bool,
        },
        Delegations {
            limit: Option<u32>,
            start_after: Option<String>,
            user: Option<String>,
        },
        Config {},
        TotalStaked {},
    }

    pub fn staking_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Staking_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: Staking_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Staking_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Staking_MockQueryMsg::Staked {
                        limit,
                        start_after,
                        end_before,
                        unstaking,
                    } => Ok(to_binary(&StakedResponse {
                        stakers: vec![
                            StakeDeposit {
                                staker: Addr::unchecked(USER),
                                amount: Uint128::new(1_000_000_002u128),
                                stake_time: 1u64,
                                unstake_start_time: None,
                            },
                            StakeDeposit {
                                staker: Addr::unchecked(ADMIN),
                                amount: Uint128::new(60_000_000u128),
                                stake_time: 1u64,
                                unstake_start_time: None,
                            },
                            StakeDeposit {
                                staker: Addr::unchecked("alignment"),
                                amount: Uint128::new(980_000_000u128),
                                stake_time: 1u64,
                                unstake_start_time: None,
                            },                            
                            StakeDeposit {
                                staker: Addr::unchecked("alignment2.0"),
                                amount: Uint128::new(980_000_000_000u128),
                                stake_time: 1u64,
                                unstake_start_time: None,
                            },
                        ],
                    })?),
                    Staking_MockQueryMsg::Delegations {
                        limit,
                        start_after,
                        user,
                    } => Ok(to_binary(&vec![DelegationResponse {
                        user: Addr::unchecked(USER),
                        delegation_info: DelegationInfo {
                            delegated: vec![
                                Delegation { 
                                    delegate: Addr::unchecked(USER), 
                                    amount: Uint128::new(110000000),
                                    fluidity: false,
                                    voting_power_delegation: true,
                                    time_of_delegation: 0,
                                },
                                Delegation { 
                                    delegate: Addr::unchecked(USER), 
                                    amount: Uint128::new(110000000),
                                    fluidity: false,
                                    voting_power_delegation: false,
                                    time_of_delegation: 0,
                                },                         
                                Delegation { 
                                    delegate: Addr::unchecked(USER), 
                                    amount: Uint128::new(100000000464),
                                    fluidity: false, 
                                    voting_power_delegation: true,
                                    time_of_delegation: 999999999999,
                                }
                            ],
                            delegated_to: vec![
                                Delegation { 
                                    delegate: Addr::unchecked(USER), 
                                    amount: Uint128::new(100000000),
                                    fluidity: false, 
                                    voting_power_delegation: true,
                                    time_of_delegation: 0,
                                },                                
                                Delegation { 
                                    delegate: Addr::unchecked(USER), 
                                    amount: Uint128::new(100000000000),
                                    fluidity: false, 
                                    voting_power_delegation: true,
                                    time_of_delegation: 999999999999,
                                }
                            ],
                            commission: Decimal::zero(),
                        },
                    },
                    DelegationResponse {
                        user: Addr::unchecked("who"),
                        delegation_info: DelegationInfo {
                            delegated: vec![
                                Delegation { 
                                    delegate: Addr::unchecked(ADMIN), 
                                    amount: Uint128::new(60_000_000),
                                    fluidity: false,
                                    voting_power_delegation: true,
                                    time_of_delegation: 0,
                                },
                                Delegation { 
                                    delegate: Addr::unchecked("alignment"), 
                                    amount: Uint128::new(980_000_000u128),
                                    fluidity: false,
                                    voting_power_delegation: true,
                                    time_of_delegation: 0,
                                },
                                Delegation { 
                                    delegate: Addr::unchecked("alignment2.0"), 
                                    amount: Uint128::new(980_000_000_000u128),
                                    fluidity: false,
                                    voting_power_delegation: true,
                                    time_of_delegation: 0,
                                },
                            ],
                            delegated_to: vec![],
                            commission: Decimal::zero(),
                        },
                    }])?),
                    Staking_MockQueryMsg::Config {} => Ok(to_binary(&StakingConfig {
                        owner: Addr::unchecked(""),
                        positions_contract: Some(Addr::unchecked("")),
                        vesting_contract: Some(Addr::unchecked("")),
                        governance_contract: Some(Addr::unchecked("")),
                        osmosis_proxy: Some(Addr::unchecked("")),                        
                        auction_contract: Some(Addr::unchecked("")),
                        incentive_schedule: StakeDistribution {
                            rate: Decimal::zero(),
                            duration: 0,
                        },
                        unstaking_period: 0,
                        mbrn_denom: String::from("mbrn_denom"),
                        max_commission_rate: Decimal::zero(),
                        keep_raw_cdt: false,
                        vesting_rev_multiplier: Decimal::zero(),
                    })?),
                    Staking_MockQueryMsg::TotalStaked {  } => Ok(to_binary(&TotalStakedResponse {
                        total_not_including_vested: Uint128::new(1000000_000000u128),
                        vested_total: Uint128::zero(),   
                    })?),
                }
            },
        );
        Box::new(contract)
    }

    //Mock Vesting Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Vesting_MockExecuteMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct Vesting_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum Vesting_MockQueryMsg {
        Allocation { recipient: String },
        Recipients { },
    }

    pub fn bv_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: Vesting_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: Staking_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: Vesting_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    Vesting_MockQueryMsg::Allocation { recipient } => {
                        Ok(to_binary(&AllocationResponse {
                            amount: Uint128::new(3333_000000),
                            amount_withdrawn: Uint128::zero(),
                            start_time_of_allocation: 0,
                            vesting_period: VestingPeriod {
                                cliff: 0u64,
                                linear: 0u64,
                            },
                        })?)
                    },
                    Vesting_MockQueryMsg::Recipients {  } => {
                        Ok(to_binary(&RecipientsResponse {
                            recipients: vec![
                                RecipientResponse {
                                    allocation: Some(Allocation {
                                        amount: Uint128::new(3333_000000),
                                        amount_withdrawn: Uint128::zero(),
                                        start_time_of_allocation: 0,
                                        vesting_period: VestingPeriod {
                                            cliff: 0u64,
                                            linear: 0u64,
                                        },
                                    }),
                                    claimables: vec![],
                                    recipient: String::from("recipient"),
                                }
                            ],
                        })?)
                    }
                }
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

    fn proper_instantiate() -> (App, GovContract, Addr) {
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
            vesting_contract_addr: bv_contract_addr.to_string(),
            vesting_voting_power_multiplier: Decimal::percent(33),
            proposal_voting_period: PROPOSAL_VOTING_PERIOD+1,
            expedited_proposal_voting_period: PROPOSAL_VOTING_PERIOD,
            proposal_effective_delay: PROPOSAL_EFFECTIVE_DELAY,
            proposal_expiration_period: PROPOSAL_EXPIRATION_PERIOD,
            proposal_required_stake: Uint128::from(PROPOSAL_REQUIRED_STAKE),
            proposal_required_quorum: String::from(PROPOSAL_REQUIRED_QUORUM),
            proposal_required_threshold: String::from(PROPOSAL_REQUIRED_THRESHOLD),
            whitelisted_links: vec!["https://some.link/".to_string()],
        };

        let gov_contract_addr = app
            .instantiate_contract(gov_id, Addr::unchecked(ADMIN), &msg, &[], "test", None)
            .unwrap();

        let gov_contract = GovContract(gov_contract_addr);

        (app, gov_contract, bv_contract_addr)
    }

    mod gov {

        use super::*;
        use cosmwasm_std::{Uint64, WasmMsg};
        use membrane::governance::{
            Config, ProposalListResponse, ProposalMessage, ProposalStatus,
            ProposalVoteOption, ProposalVotesResponse, UpdateConfig, Proposal
        };

        #[test]
        fn stake_minimum() {
            let (mut app, gov_contract, bv_contract_addr) = proper_instantiate();

            //Submit Proposal: Under required stake 
            let msg = ExecuteMsg::SubmitProposal {
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: None,
                messages: None,
                recipient: None,
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app
                .execute(Addr::unchecked(ADMIN), cosmos_msg)
                .unwrap();

            //Can't Vote bc proposal hasn't reached the required stake
            let msg = ExecuteMsg::CastVote {
                proposal_id: 1u64,
                vote: ProposalVoteOption::For,
                recipient: Some(String::from("recipient")),
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app
                .execute(Addr::unchecked("recipient"), cosmos_msg)
                .unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Proposal not active!"));
            //Align
            let msg = ExecuteMsg::CastVote {
                proposal_id: 1u64,
                vote: ProposalVoteOption::Align,
                recipient: Some(String::from("recipient")),
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app
                .execute(Addr::unchecked("recipient"), cosmos_msg)
                .unwrap();
            // Fail bc they aligned with the proposal            
            // let msg = ExecuteMsg::CastVote {
            //     proposal_id: 1u64,
            //     vote: ProposalVoteOption::For,
            //     recipient: Some(String::from("recipient")),
            // };
            // let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            // let err = app
            //     .execute(Addr::unchecked("recipient"), cosmos_msg)
            //     .unwrap_err();
            // assert_eq!(err.root_cause().to_string(), String::from("Unauthorized"));

            //Successful submission
            let msg = ExecuteMsg::SubmitProposal {
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: None,
                messages: None,
                recipient: Some(String::from("recipient")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("recipient"), cosmos_msg).unwrap();

            //Successful submission
            let msg = ExecuteMsg::SubmitProposal {
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: None,
                messages: None,
                recipient: None,
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
        }

        #[test]
        fn submit_proposal() {
            let (mut app, gov_contract, bv_contract_addr) = proper_instantiate();

            //Title too short
            let msg = ExecuteMsg::SubmitProposal {
                title: "X".to_string(),
                description: "Test description!".to_string(),
                link: None,
                messages: None,
                recipient: Some(String::from("recipient")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked("recipient"), cosmos_msg).unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Generic error: Title too short!")
            );

            //Title too long
            let msg = ExecuteMsg::SubmitProposal {
                title: String::from_utf8(vec![b'X'; 65]).unwrap(),
                description: "Test description!".to_string(),
                link: None,
                messages: None,
                recipient: Some(String::from("recipient")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app
                .execute(Addr::unchecked(Addr::unchecked("recipient")), cosmos_msg)
                .unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Generic error: Title too long!")
            );

            //Description too short
            let msg = ExecuteMsg::SubmitProposal {
                title: "Test Title".to_string(),
                description: "X".to_string(),
                link: None,
                messages: None,
                recipient: Some(String::from("recipient")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app
                .execute(Addr::unchecked(Addr::unchecked("recipient")), cosmos_msg)
                .unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Generic error: Description too short!")
            );

            //Description too long
            let msg = ExecuteMsg::SubmitProposal {
                title: "Test Title".to_string(),
                description: String::from_utf8(vec![b'X'; 1025]).unwrap(),
                link: None,
                messages: None,
                recipient: Some(String::from("recipient")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app
                .execute(Addr::unchecked(Addr::unchecked("recipient")), cosmos_msg)
                .unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Generic error: Description too long!")
            );

            //Link too short
            let msg = ExecuteMsg::SubmitProposal {
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: Some(String::from("X")),
                messages: None,
                recipient: Some(String::from("recipient")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app
                .execute(Addr::unchecked(Addr::unchecked("recipient")), cosmos_msg)
                .unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Generic error: Link too short!")
            );

            //Link too long
            let msg = ExecuteMsg::SubmitProposal {
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: Some(String::from_utf8(vec![b'X'; 129]).unwrap()),
                messages: None,
                recipient: Some(String::from("recipient")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app
                .execute(Addr::unchecked(Addr::unchecked("recipient")), cosmos_msg)
                .unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Generic error: Link too long!")
            );

            //Link not whitelisted
            let msg = ExecuteMsg::SubmitProposal {
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: Some(String::from("https://some1.link")),
                messages: None,
                recipient: Some(String::from("recipient")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app
                .execute(Addr::unchecked(Addr::unchecked("recipient")), cosmos_msg)
                .unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Generic error: Link is not whitelisted!")
            );

            //Link unsafe characters
            let msg = ExecuteMsg::SubmitProposal {
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: Some(String::from(
                    "https://some.link/<script>alert('test');</script>",
                )),
                messages: None,
                recipient: Some(String::from("recipient")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app
                .execute(Addr::unchecked(Addr::unchecked("recipient")), cosmos_msg)
                .unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from(
                    "Generic error: Link is not properly formatted or contains unsafe characters!"
                )
            );

            //Submit Proposal
            let msg = ExecuteMsg::SubmitProposal {
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: Some(String::from("https://some.link/linker")),
                messages: Some(vec![ProposalMessage {
                    order: Uint64::new(1u64),
                    msg: cosmwasm_std::CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: String::from("addr"),
                        msg: to_binary(&"msg").unwrap(),
                        funds: vec![],
                    }),
                }]),
                recipient: Some(String::from("recipient")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("recipient"), cosmos_msg).unwrap();

            let proposal: Proposal = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::Proposal { proposal_id: 1 },
                )
                .unwrap();

            assert_eq!(proposal.proposal_id, Uint64::from(1u64));
            assert_eq!(proposal.submitter, Addr::unchecked("recipient"));
            assert_eq!(proposal.status, ProposalStatus::Active);
            assert_eq!(proposal.for_power, Uint128::zero());
            assert_eq!(proposal.against_power, Uint128::zero());
            assert_eq!(proposal.start_block, 12_345);
            assert_eq!(proposal.end_block, proposal.start_time + (6 * (7 * 14400))); //Executables have a minimum 7 day voting period
            assert_eq!(proposal.title, String::from("Test title!"));
            assert_eq!(proposal.description, String::from("Test description!"));
            assert_eq!(
                proposal.link,
                Some(String::from("https://some.link/linker"))
            );
            assert_eq!(
                proposal.messages,
                Some(vec![ProposalMessage {
                    order: Uint64::new(1u64),
                    msg: cosmwasm_std::CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: String::from("addr"),
                        msg: to_binary(&"msg").unwrap(),
                        funds: vec![],
                    }),
                }])
            );
        }

        #[test]
        fn successful_proposal() {
            let (mut app, gov_contract, bv_contract_addr) = proper_instantiate();

            //Submit Proposal
            let msg = ExecuteMsg::SubmitProposal {
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: Some(String::from("https://some.link/linker")),
                messages: Some(vec![ProposalMessage {
                    order: Uint64::new(1u64),
                    msg: cosmwasm_std::CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: gov_contract.addr().to_string(),
                        msg: to_binary(&ExecuteMsg::UpdateConfig(UpdateConfig {
                            mbrn_denom: None,
                            staking_contract: None,
                            vesting_contract_addr: None,
                            vesting_voting_power_multiplier: None,
                            minimum_total_stake: None,
                            proposal_voting_period: Some(PROPOSAL_VOTING_PERIOD + 1000),
                            expedited_proposal_voting_period: Some(PROPOSAL_VOTING_PERIOD + 1000),
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
                            quadratic_voting: None,
                        }))
                        .unwrap(),
                        funds: vec![],
                    }),
                }]),
                recipient: Some(String::from("recipient")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("recipient"), cosmos_msg).unwrap();

            ////Cast Votes
            // Fail bc they are proposal.submitter
            let msg = ExecuteMsg::CastVote {
                proposal_id: 1u64,
                vote: ProposalVoteOption::For,
                recipient: Some(String::from("recipient")),
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app
                .execute(Addr::unchecked("recipient"), cosmos_msg)
                .unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Unauthorized"));

            //For
            let msg = ExecuteMsg::CastVote {
                proposal_id: 1u64,
                vote: ProposalVoteOption::For,
                recipient: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            //Against
            let msg = ExecuteMsg::CastVote {
                proposal_id: 1u64,
                vote: ProposalVoteOption::Against,
                recipient: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            //Against
            //Testing that delegates without stake can vote
            let msg = ExecuteMsg::CastVote {
                proposal_id: 1u64,
                vote: ProposalVoteOption::Align,
                recipient: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("who"), cosmos_msg).unwrap();
            //Align to pass Quorum
            let msg = ExecuteMsg::CastVote {
                proposal_id: 1u64,
                vote: ProposalVoteOption::Align,
                recipient: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("alignment2.0"), cosmos_msg).unwrap();

            //Assertations
            let proposal: Proposal = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::Proposal { proposal_id: 1 },
                )
                .unwrap();

            let proposal_votes: ProposalVotesResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::ProposalVotes { proposal_id: 1 },
                )
                .unwrap();

            let proposal_for_voters: Vec<Addr> = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::ProposalVoters {
                        proposal_id: 1,
                        vote_option: ProposalVoteOption::For,
                        start: None,
                        limit: None,
                        specific_user: None,
                    },
                )
                .unwrap();

            let proposal_against_voters: Vec<Addr> = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::ProposalVoters {
                        proposal_id: 1,
                        vote_option: ProposalVoteOption::Against,
                        start: None,
                        limit: None,
                        specific_user: None,
                    },
                )
                .unwrap();

            // Check proposal votes & assert quadratic weighing
            assert_eq!(proposal.for_power, Uint128::from(32112u128)); 
            assert_eq!(proposal.against_power, Uint128::from(7746u128));

            assert_eq!(proposal_votes.for_power, Uint128::from(32112u128));
            assert_eq!(proposal_votes.against_power, Uint128::from(7746u128));

            assert_eq!(proposal_for_voters, vec![Addr::unchecked("user")]);
            assert_eq!(proposal_against_voters, vec![Addr::unchecked("admin")]);

            //Query voting power
            let voting_power_1: Uint128 = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::UserVotingPower { 
                        user: String::from("user"), 
                        proposal_id: 1, 
                        vesting: false, 
                    },
                )
                .unwrap();
            assert_eq!(voting_power_1, Uint128::new(32112));

            //Query voting power
            let voting_power_2: Uint128 = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::UserVotingPower { 
                        user: String::from("admin"), 
                        proposal_id: 1, 
                        vesting: false, 
                    },
                )
                .unwrap();
            assert_eq!(voting_power_2, Uint128::new(7746));

            //Query voting power
            let voting_power_3: Uint128 = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::UserVotingPower { 
                        user: String::from("recipient"), 
                        proposal_id: 1, 
                        vesting: true, 
                    },
                )
                .unwrap();
            assert_eq!(voting_power_3, Uint128::new(33165));

            //Query voting power
            let voting_power_4: Uint128 = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::UserVotingPower { 
                        user: String::from("alignment"), 
                        proposal_id: 1, 
                        vesting: false, 
                    },
                )
                .unwrap();
            assert_eq!(voting_power_4, Uint128::new(31305));

            //Query voting power
            let voting_power_5: Uint128 = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::UserVotingPower { 
                        user: String::from("alignment2.0"), 
                        proposal_id: 1, 
                        vesting: false, 
                    },
                )
                .unwrap();
            assert_eq!(voting_power_5, Uint128::new(989950));
            
            //Query voting power
            let voting_power_6: Uint128 = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::UserVotingPower { 
                        user: String::from("who"), 
                        proposal_id: 1, 
                        vesting: false, 
                    },
                )
                .unwrap();
            assert_eq!(voting_power_5, Uint128::new(989950));

            //Query total voting power
            let total_voting_power: Uint128 = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::TotalVotingPower { 
                        proposal_id: 1, 
                    },
                )
                .unwrap();
            assert_eq!(total_voting_power + Uint128::new(1), voting_power_1 + voting_power_2 + voting_power_3 + voting_power_4 + voting_power_5 + voting_power_6); //1 is a descrepancy from rounding differences

            //Assert that delegated voting power is equal to the sum of the delegations
            let delegate_vp: Uint128 = app
                .wrap()
                .query_wasm_smart(
                gov_contract.addr(),
                &QueryMsg::UserVotingPower { 
                    user: String::from("who"), 
                    proposal_id: 1,
                    vesting: false, 
                },
            )
            .unwrap();
            assert_eq!(voting_power_2 + voting_power_4 + voting_power_5, delegate_vp);

            //Change the initial "Against" to "For"
            let msg = ExecuteMsg::CastVote {
                proposal_id: 1u64,
                vote: ProposalVoteOption::For,
                recipient: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Assertations
            let proposal: Proposal = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::Proposal { proposal_id: 1 },
                )
                .unwrap();

            let proposal_votes: ProposalVotesResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::ProposalVotes { proposal_id: 1 },
                )
                .unwrap();

            let proposal_for_voters: Vec<Addr> = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::ProposalVoters {
                        proposal_id: 1,
                        vote_option: ProposalVoteOption::For,
                        start: None,
                        limit: None,
                        specific_user: None,
                    },
                )
                .unwrap();

            let proposal_against_voters: Vec<Addr> = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::ProposalVoters {
                        proposal_id: 1,
                        vote_option: ProposalVoteOption::Against,
                        start: None,
                        limit: None,
                        specific_user: None,
                    },
                )
                .unwrap();

            // Check proposal votes & assert quadratic weighing
            assert_eq!(proposal.for_power, Uint128::new(39858)); 
            assert_eq!(proposal.against_power, Uint128::zero());

            assert_eq!(proposal_votes.for_power, Uint128::new(39858));
            assert_eq!(proposal_votes.against_power, Uint128::zero());

            assert_eq!(proposal_for_voters, vec![Addr::unchecked("user"), Addr::unchecked("admin")]);
            assert_eq!(proposal_against_voters.len(), 0 as usize);

            // Skip voting period
            app.update_block(|bi| {
                bi.height += 7 * PROPOSAL_VOTING_PERIOD + 1;
                bi.time = bi.time.plus_seconds(6 * (7* PROPOSAL_VOTING_PERIOD + 1));
            });

            //Error bc voting period has passed
            let msg = ExecuteMsg::CastVote {
                proposal_id: 1u64,
                vote: ProposalVoteOption::For,
                recipient: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Voting period ended!")
            );

            //Try to execute the proposal before ending it
            let msg = ExecuteMsg::ExecuteProposal { proposal_id: 1u64 };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Proposal not passed!")
            );

            //Successful End
            let msg = ExecuteMsg::EndProposal { proposal_id: 1u64 };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            let proposal: Proposal = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::Proposal { proposal_id: 1 },
                )
                .unwrap();

            assert_eq!(proposal.status, ProposalStatus::Passed);

            //Try to Execute before the delay
            let msg = ExecuteMsg::ExecuteProposal { proposal_id: 1u64 };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Proposal delay not ended!")
            );

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
                .query_wasm_smart(
                    gov_contract.addr().to_string(),
                    &QueryMsg::Config {},
                )
                .unwrap();

            let proposal: Proposal = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr().to_string(),
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
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Proposal can't be removed!")
            );

            //Query Proposal
            let res: ProposalListResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr().to_string(),
                    &QueryMsg::ActiveProposals {
                        start: None,
                        limit: None,
                    },
                )
                .unwrap();

            assert_eq!(res.proposal_list.len(), 1);
            assert_eq!(res.proposal_count, Uint64::from(1u32));

            app.update_block(|bi| {
                bi.height += PROPOSAL_EXPIRATION_PERIOD + 1;
                bi.time = bi.time.plus_seconds(6 * (PROPOSAL_EXPIRATION_PERIOD + 2));
            });
            // Remove expired proposal
            app.execute_contract(
                Addr::unchecked("user0"),
                gov_contract.addr(),
                &ExecuteMsg::RemoveCompletedProposal { proposal_id: 1 },
                &[],
            )
            .unwrap();

            let res: ProposalListResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr().to_string(),
                    &QueryMsg::ActiveProposals {
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
        fn expedited_proposal(){
            let (mut app, gov_contract, bv_contract_addr) = proper_instantiate();

            //Submit Proposal
            let msg = ExecuteMsg::SubmitProposal {
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: Some(String::from("https://some.link/linker")),
                messages: Some(vec![ProposalMessage {
                    order: Uint64::new(1u64),
                    msg: cosmwasm_std::CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: gov_contract.addr().to_string(),
                        msg: to_binary(&ExecuteMsg::UpdateConfig(UpdateConfig {
                            mbrn_denom: None,
                            staking_contract: None,
                            vesting_contract_addr: None,
                            vesting_voting_power_multiplier: None,
                            minimum_total_stake: None,
                            proposal_voting_period: Some(PROPOSAL_VOTING_PERIOD + 1000),
                            expedited_proposal_voting_period: Some(PROPOSAL_VOTING_PERIOD + 1000),
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
                            quadratic_voting: None,
                        }))
                        .unwrap(),
                        funds: vec![],
                    }),
                }]),
                recipient: Some(String::from("recipient")),
                expedited: true,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("recipient"), cosmos_msg).unwrap();

            //Query proposal to assert voting period
            let proposal: Proposal = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::Proposal { proposal_id: 1 },
                )
                .unwrap();
            assert_eq!(proposal.start_time + (14400*6), proposal.end_block);

            //End Proposal without passing quorum
            app.update_block(|bi| {
                bi.height += 14400 + 1;
                bi.time = bi.time.plus_seconds(6 * (14400 + 1));
            });
            let msg = ExecuteMsg::EndProposal { proposal_id: 1u64 };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Query proposal to assert:
            //- extended voting period
            //- Active status
            let proposal: Proposal = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::Proposal { proposal_id: 1 },
                )
                .unwrap();
            assert_eq!(proposal.start_time + ((14400+1) * 6), proposal.end_block);
            assert_eq!(proposal.status, ProposalStatus::Active);

        }

        #[test]
        fn successful_amend_and_remove_proposal() {
            let (mut app, gov_contract, bv_contract_addr) = proper_instantiate();

            //Submit Proposal
            let msg = ExecuteMsg::SubmitProposal {
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: Some(String::from("https://some.link/linker")),
                messages: Some(vec![ProposalMessage {
                    order: Uint64::new(1u64),
                    msg: cosmwasm_std::CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: gov_contract.addr().to_string(),
                        msg: to_binary(&ExecuteMsg::UpdateConfig(UpdateConfig {
                            mbrn_denom: None,
                            staking_contract: None,
                            vesting_contract_addr: None,
                            vesting_voting_power_multiplier: None,
                            minimum_total_stake: None,
                            proposal_voting_period: Some(PROPOSAL_VOTING_PERIOD + 1000),
                            expedited_proposal_voting_period: Some(PROPOSAL_VOTING_PERIOD + 1000),
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
                            quadratic_voting: None,
                        }))
                        .unwrap(),
                        funds: vec![],
                    }),
                }]),
                recipient: Some(String::from("recipient")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("recipient"), cosmos_msg).unwrap();

            ////Cast Votes
            //For
            let msg = ExecuteMsg::CastVote {
                proposal_id: 1u64,
                vote: ProposalVoteOption::For,
                recipient: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            //Amend
            let msg = ExecuteMsg::CastVote {
                proposal_id: 1u64,
                vote: ProposalVoteOption::Amend,
                recipient: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            //Align to pass Quorum
            let msg = ExecuteMsg::CastVote {
                proposal_id: 1u64,
                vote: ProposalVoteOption::Align,
                recipient: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("alignment2.0"), cosmos_msg).unwrap();
            //Align to pass Quorum
            let msg = ExecuteMsg::CastVote {
                proposal_id: 1u64,
                vote: ProposalVoteOption::Align,
                recipient: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("who"), cosmos_msg).unwrap();

            //Assertations
            let proposal: Proposal = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::Proposal { proposal_id: 1 },
                )
                .unwrap();

            let proposal_votes: ProposalVotesResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::ProposalVotes { proposal_id: 1 },
                )
                .unwrap();

            let proposal_for_voters: Vec<Addr> = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::ProposalVoters {
                        proposal_id: 1,
                        vote_option: ProposalVoteOption::For,
                        start: None,
                        limit: None,
                        specific_user: None,
                    },
                )
                .unwrap();

            let proposal_amend_voters: Vec<Addr> = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::ProposalVoters {
                        proposal_id: 1,
                        vote_option: ProposalVoteOption::Amend,
                        start: None,
                        limit: None,
                        specific_user: None,
                    },
                )
                .unwrap();

            // Check proposal votes & assert quadratic weighing
            assert_eq!(proposal.amendment_power, Uint128::from(32112u128)); 
            assert_eq!(proposal.for_power, Uint128::from(7746u128));

            assert_eq!(proposal_votes.amendment_power, Uint128::from(32112u128));
            assert_eq!(proposal_votes.for_power, Uint128::from(7746u128));

            assert_eq!(proposal_for_voters, vec![Addr::unchecked("admin")]);
            assert_eq!(proposal_amend_voters, vec![Addr::unchecked("user")]);

            
            // Skip voting period
            app.update_block(|bi| {
                bi.height += 7 * PROPOSAL_VOTING_PERIOD + 1;
                bi.time = bi.time.plus_seconds(6 * (7* PROPOSAL_VOTING_PERIOD + 1));
            });

            //Successful End: AmendmentDesired
            let msg = ExecuteMsg::EndProposal { proposal_id: 1u64 };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            let proposal: Proposal = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::Proposal { proposal_id: 1 },
                )
                .unwrap();

            assert_eq!(proposal.status, ProposalStatus::AmendmentDesired);

            /////Proposal 2 - Removed for Spam/////
            //Submit Proposal
            let msg = ExecuteMsg::SubmitProposal {
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: Some(String::from("https://some.link/linker")),
                messages: Some(vec![ProposalMessage {
                    order: Uint64::new(1u64),
                    msg: cosmwasm_std::CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: gov_contract.addr().to_string(),
                        msg: to_binary(&ExecuteMsg::UpdateConfig(UpdateConfig {
                            mbrn_denom: None,
                            staking_contract: None,
                            vesting_contract_addr: None,
                            vesting_voting_power_multiplier: None,
                            minimum_total_stake: None,
                            proposal_voting_period: Some(PROPOSAL_VOTING_PERIOD + 1000),
                            expedited_proposal_voting_period: Some(PROPOSAL_VOTING_PERIOD + 1000),
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
                            quadratic_voting: None,
                        }))
                        .unwrap(),
                        funds: vec![],
                    }),
                }]),
                recipient: Some(String::from("recipient")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("recipient"), cosmos_msg).unwrap();

            ////Cast Votes
            //Remove
            let msg = ExecuteMsg::CastVote {
                proposal_id: 2u64,
                vote: ProposalVoteOption::Remove,
                recipient: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            //Remove
            let msg = ExecuteMsg::CastVote {
                proposal_id: 2u64,
                vote: ProposalVoteOption::Remove,
                recipient: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            //Align to pass Quorum
            let msg = ExecuteMsg::CastVote {
                proposal_id: 2u64,
                vote: ProposalVoteOption::Align,
                recipient: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("alignment2.0"), cosmos_msg).unwrap();
            //Align to pass Quorum
            let msg = ExecuteMsg::CastVote {
                proposal_id: 2u64,
                vote: ProposalVoteOption::Align,
                recipient: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("who"), cosmos_msg).unwrap();

            //Assertations
            let proposal: Proposal = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::Proposal { proposal_id: 2 },
                )
                .unwrap();

            let proposal_votes: ProposalVotesResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::ProposalVotes { proposal_id: 2 },
                )
                .unwrap();

            let proposal_removal_voters: Vec<Addr> = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::ProposalVoters {
                        proposal_id: 2,
                        vote_option: ProposalVoteOption::Remove,
                        start: None,
                        limit: None,
                        specific_user: None,
                    },
                )
                .unwrap();

            // Check proposal votes & assert quadratic weighing
            assert_eq!(proposal.removal_power, Uint128::from(32112u128 + 7746u128)); 
            assert_eq!(proposal_votes.removal_power, Uint128::from(32112u128 + 7746u128));

            assert_eq!(proposal_removal_voters, vec![Addr::unchecked("admin"), Addr::unchecked("user")]);
            
            // Skip voting period
            app.update_block(|bi| {
                bi.height += 7 * PROPOSAL_VOTING_PERIOD + 1;
                bi.time = bi.time.plus_seconds(6 * (7* PROPOSAL_VOTING_PERIOD + 1));
            });

            //Successful End: Removed
            let msg = ExecuteMsg::EndProposal { proposal_id: 2u64 };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            //Query Proposal
            let res: ProposalListResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr().to_string(),
                    &QueryMsg::ActiveProposals {
                        start: None,
                        limit: None,
                    },
                )
                .unwrap();

            assert_eq!(res.proposal_list.len(), 1); //Length should stay 1 since the 2nd was removed
            assert_eq!(res.proposal_count, Uint64::from(2u32));
            
        }

        #[test]
        fn pending_proposals() {
            let (mut app, gov_contract, bv_contract_addr) = proper_instantiate();

            //Submit Proposal
            let msg = ExecuteMsg::SubmitProposal {
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: Some(String::from("https://some.link/linker")),
                messages: Some(vec![ProposalMessage {
                    order: Uint64::new(1u64),
                    msg: cosmwasm_std::CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: gov_contract.addr().to_string(),
                        msg: to_binary(&ExecuteMsg::UpdateConfig(UpdateConfig {
                            mbrn_denom: None,
                            staking_contract: None,
                            vesting_contract_addr: None,
                            vesting_voting_power_multiplier: None,
                            minimum_total_stake: None,
                            proposal_voting_period: Some(PROPOSAL_VOTING_PERIOD + 1000),
                            expedited_proposal_voting_period: Some(PROPOSAL_VOTING_PERIOD + 1000),
                            proposal_effective_delay: None,
                            proposal_expiration_period: None,
                            proposal_required_stake: Some(70_000_000),
                            proposal_required_quorum: None,
                            proposal_required_threshold: None,
                            whitelist_add: Some(vec![
                                "https://some1.link/".to_string(),
                                "https://some2.link/".to_string(),
                            ]),
                            whitelist_remove: Some(vec!["https://some.link/".to_string()]),
                            quadratic_voting: None,
                        }))
                        .unwrap(),
                        funds: vec![],
                    }),
                }]),
                recipient: None,
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Query that its pending
            let proposals: ProposalListResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::PendingProposals { start: None, limit: None },
                )
                .unwrap();
            assert_eq!(proposals.proposal_list.len(), 1);

            ////Cast Votes to align
            let msg = ExecuteMsg::CastVote {
                proposal_id: 1u64,
                vote: ProposalVoteOption::Align,
                recipient: None,
            };
            let cosmos_msg = gov_contract.call(msg.clone(), vec![]).unwrap();
            app
                .execute(Addr::unchecked("alignment"), cosmos_msg)
                .unwrap();

            //Assert that its now Active
            let proposal: Proposal = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::Proposal { proposal_id: 1 },
                )
                .unwrap();
            //Assert voting power used non-quadratic before the threshold
            assert_eq!(proposal.aligned_power, Uint128::new(1000006324));

            //Submit Proposal
            let msg = ExecuteMsg::SubmitProposal {
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: Some(String::from("https://some.link/linker")),
                messages: Some(vec![ProposalMessage {
                    order: Uint64::new(1u64),
                    msg: cosmwasm_std::CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: gov_contract.addr().to_string(),
                        msg: to_binary(&ExecuteMsg::UpdateConfig(UpdateConfig {
                            mbrn_denom: None,
                            staking_contract: None,
                            vesting_contract_addr: None,
                            vesting_voting_power_multiplier: None,
                            minimum_total_stake: None,
                            proposal_voting_period: Some(PROPOSAL_VOTING_PERIOD + 1000),
                            expedited_proposal_voting_period: Some(PROPOSAL_VOTING_PERIOD + 1000),
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
                            quadratic_voting: None,
                        }))
                        .unwrap(),
                        funds: vec![],
                    }),
                }]),
                recipient: None,
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();

            //Remove completed pending proposal that expired without getting alignment       
            // Remove expired proposal
            app.update_block(|bi| {
                bi.height += 14401;
                bi.time = bi
                    .time
                    .plus_seconds(14401*6);
            });

            app.execute_contract(
                Addr::unchecked(USER),
                gov_contract.addr(),
                &ExecuteMsg::RemoveCompletedProposal { proposal_id: 2 },
                &[],
            )
            .unwrap();

            //Assert removal
            let proposals: ProposalListResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::PendingProposals { start: None, limit: None },
                )
                .unwrap();
            assert_eq!(proposals.proposal_list.len(), 0);

        }

        #[test]
        fn unsuccessful_proposal() {
            let (mut app, gov_contract, bv_contract_addr) = proper_instantiate();

            //Submit Proposal
            let msg = ExecuteMsg::SubmitProposal {
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: Some(String::from("https://some.link/linker")),
                messages: Some(vec![ProposalMessage {
                    order: Uint64::new(1u64),
                    msg: cosmwasm_std::CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: gov_contract.addr().to_string(),
                        msg: to_binary(&ExecuteMsg::UpdateConfig(UpdateConfig {
                            mbrn_denom: None,
                            staking_contract: None,
                            vesting_contract_addr: None,
                            vesting_voting_power_multiplier: None,
                            minimum_total_stake: None,
                            proposal_voting_period: Some(PROPOSAL_VOTING_PERIOD + 1000),
                            expedited_proposal_voting_period: Some(PROPOSAL_VOTING_PERIOD + 1000),
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
                            quadratic_voting: None,
                        }))
                        .unwrap(),
                        funds: vec![],
                    }),
                }]),
                recipient: Some(String::from("recipient")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("recipient"), cosmos_msg).unwrap();

            //For
            let msg = ExecuteMsg::CastVote {
                proposal_id: 1u64,
                vote: ProposalVoteOption::Against,
                recipient: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();
            //Against
            let msg = ExecuteMsg::CastVote {
                proposal_id: 1u64,
                vote: ProposalVoteOption::For,
                recipient: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(ADMIN), cosmos_msg).unwrap();
            //Align to pass Quorum
            let msg = ExecuteMsg::CastVote {
                proposal_id: 1u64,
                vote: ProposalVoteOption::Align,
                recipient: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked("alignment2.0"), cosmos_msg).unwrap();

            // Skip voting period
            app.update_block(|bi| {
                bi.height += 7 * PROPOSAL_VOTING_PERIOD + 1;
                bi.time = bi.time.plus_seconds(6 * (7 * PROPOSAL_VOTING_PERIOD + 1));
            });

            //Successful End
            let msg = ExecuteMsg::EndProposal { proposal_id: 1u64 };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(Addr::unchecked(USER), cosmos_msg).unwrap();

            let proposal: Proposal = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
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
                gov_contract.addr(),
                &ExecuteMsg::RemoveCompletedProposal { proposal_id: 1 },
                &[],
            )
            .unwrap();

            let res: ProposalListResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr().to_string(),
                    &QueryMsg::ActiveProposals {
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
            let (mut app, gov_contract, bv_contract_addr) = proper_instantiate();

            let config_before: Config = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr().to_string(),
                    &QueryMsg::Config {},
                )
                .unwrap();

            let msg = ExecuteMsg::CheckMessages {
                messages: vec![ProposalMessage {
                    order: Uint64::new(1u64),
                    msg: cosmwasm_std::CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: gov_contract.addr().to_string(),
                        msg: to_binary(&ExecuteMsg::UpdateConfig(UpdateConfig {
                            mbrn_denom: None,
                            staking_contract: None,
                            vesting_contract_addr: None,
                            vesting_voting_power_multiplier: None,
                            minimum_total_stake: None,
                            proposal_voting_period: Some(PROPOSAL_VOTING_PERIOD + 1000),
                            expedited_proposal_voting_period: Some(PROPOSAL_VOTING_PERIOD + 1000),
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
                            quadratic_voting: None,
                        }))
                        .unwrap(),
                        funds: vec![],
                    }),
                }],
                msg_switch: None,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app
                .execute(bv_contract_addr, cosmos_msg)
                .unwrap_err();
            assert_eq!(
                &err.root_cause().to_string(),
                "Messages check passed. Nothing was committed to the blockchain"
            );

            let config_after: Config = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr().to_string(),
                    &QueryMsg::Config {},
                )
                .unwrap();
            assert_eq!(config_before, config_after);
        }
    }
}
