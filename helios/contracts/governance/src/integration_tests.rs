#[cfg(test)]
#[allow(unused_variables)]

mod tests {

    use crate::helpers::GovContract;

    use ::membrane::builder_vesting::AllocationResponse;
    use membrane::governance::{
        ExecuteMsg, InstantiateMsg, QueryMsg, STAKE_INTERVAL, VOTING_PERIOD_INTERVAL,
    };
    use membrane::staking::{
        Config as StakingConfig, StakedResponse,
    };
    use membrane::types::{StakeDeposit, VestingPeriod};

    use cosmwasm_std::{
        coin, to_binary, Addr, Binary, Decimal, Empty, Response, StdResult, Uint128,
    };
    use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

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
        Config {},
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
                                amount: Uint128::new(100_000_002u128),
                                stake_time: 1u64,
                                unstake_start_time: None,
                            },
                            StakeDeposit {
                                staker: Addr::unchecked(ADMIN),
                                amount: Uint128::new(60_000_000u128),
                                stake_time: 1u64,
                                unstake_start_time: None,
                            },
                        ],
                    })?),
                    Staking_MockQueryMsg::Config {} => Ok(to_binary(&StakingConfig {
                        owner: Addr::unchecked(""),
                        positions_contract: Some(Addr::unchecked("")),
                        builders_contract: Some(Addr::unchecked("")),
                        governance_contract: Some(Addr::unchecked("")),
                        osmosis_proxy: Some(Addr::unchecked("")),
                        staking_rate: Decimal::zero(),
                        unstaking_period: 0,
                        fee_wait_period: 0,
                        mbrn_denom: String::from("mbrn_denom"),
                        dex_router: Some(Addr::unchecked("")),
                        max_spread: Some(Decimal::zero()),
                    })?),
                }
            },
        );
        Box::new(contract)
    }

    //Mock BV Contract
    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum BV_MockExecuteMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub struct BV_MockInstantiateMsg {}

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, JsonSchema)]
    #[serde(rename_all = "snake_case")]
    pub enum BV_MockQueryMsg {
        Allocation { receiver: String },
    }

    pub fn bv_contract() -> Box<dyn Contract<Empty>> {
        let contract = ContractWrapper::new(
            |deps, _, info, msg: BV_MockExecuteMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, _, _: Staking_MockInstantiateMsg| -> StdResult<Response> {
                Ok(Response::default())
            },
            |_, _, msg: BV_MockQueryMsg| -> StdResult<Binary> {
                match msg {
                    BV_MockQueryMsg::Allocation { receiver } => {
                        Ok(to_binary(&AllocationResponse {
                            amount: String::from("1000000000"),
                            amount_withdrawn: String::from("0"),
                            start_time_of_allocation: String::from("0"),
                            vesting_period: VestingPeriod {
                                cliff: 0u64,
                                linear: 0u64,
                            },
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
            builders_contract_addr: bv_contract_addr.to_string(),
            builders_voting_power_multiplier: Decimal::percent(33),
            proposal_voting_period: PROPOSAL_VOTING_PERIOD,
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
            Config, ProposalListResponse, ProposalMessage, ProposalResponse, ProposalStatus,
            ProposalVoteOption, ProposalVotesResponse, UpdateConfig,
        };

        #[test]
        fn stake_minimum() {
            let (mut app, gov_contract, bv_contract_addr) = proper_instantiate();

            //Submit Proposal Fail due to insufficient stake
            let msg = ExecuteMsg::SubmitProposal {
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: None,
                messages: None,
                receiver: None,
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app
                .execute(Addr::unchecked("not_staked"), cosmos_msg)
                .unwrap_err();
            assert_eq!(
                err.root_cause().to_string(),
                String::from("Insufficient stake!")
            );

            //Successful submission
            let msg = ExecuteMsg::SubmitProposal {
                title: "Test title!".to_string(),
                description: "Test description!".to_string(),
                link: None,
                messages: None,
                receiver: Some(String::from("receiver")),
                expedited: false,
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
                receiver: Some(String::from("receiver")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app.execute(Addr::unchecked(USER), cosmos_msg).unwrap_err();
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
                receiver: Some(String::from("receiver")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app
                .execute(Addr::unchecked(bv_contract_addr.clone()), cosmos_msg)
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
                receiver: Some(String::from("receiver")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app
                .execute(Addr::unchecked(bv_contract_addr.clone()), cosmos_msg)
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
                receiver: Some(String::from("receiver")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app
                .execute(Addr::unchecked(bv_contract_addr.clone()), cosmos_msg)
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
                receiver: Some(String::from("receiver")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app
                .execute(Addr::unchecked(bv_contract_addr.clone()), cosmos_msg)
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
                receiver: Some(String::from("receiver")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app
                .execute(Addr::unchecked(bv_contract_addr.clone()), cosmos_msg)
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
                receiver: Some(String::from("receiver")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app
                .execute(Addr::unchecked(bv_contract_addr.clone()), cosmos_msg)
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
                receiver: Some(String::from("receiver")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app
                .execute(Addr::unchecked(bv_contract_addr.clone()), cosmos_msg)
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
                receiver: Some(String::from("receiver")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(bv_contract_addr, cosmos_msg).unwrap();

            let proposal: ProposalResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
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
                            builders_contract_addr: None,
                            builders_voting_power_multiplier: None,
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
                        }))
                        .unwrap(),
                        funds: vec![],
                    }),
                }]),
                receiver: Some(String::from("receiver")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(bv_contract_addr.clone(), cosmos_msg).unwrap();

            ////Cast Votes
            // Fail bc they are proposal.submitter
            let msg = ExecuteMsg::CastVote {
                proposal_id: 1u64,
                vote: ProposalVoteOption::For,
                receiver: Some(String::from("receiver")),
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            let err = app
                .execute(bv_contract_addr, cosmos_msg)
                .unwrap_err();
            assert_eq!(err.root_cause().to_string(), String::from("Unauthorized"));

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
                    },
                )
                .unwrap();

            // Check proposal votes
            assert_eq!(proposal.for_power, Uint128::from(100_000_002u128));
            assert_eq!(proposal.against_power, Uint128::from(60_000_000u128));

            assert_eq!(proposal_votes.for_power, Uint128::from(100_000_002u128));
            assert_eq!(proposal_votes.against_power, Uint128::from(60_000_000u128));

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
                        builders: false, 
                    },
                )
                .unwrap();
            assert_eq!(voting_power_1, Uint128::new(100000002));

            //Query voting power
            let voting_power_2: Uint128 = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr(),
                    &QueryMsg::UserVotingPower { 
                        user: String::from("admin"), 
                        proposal_id: 1, 
                        builders: false, 
                    },
                )
                .unwrap();
            assert_eq!(voting_power_2, Uint128::new(60000000));

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
            assert_eq!(total_voting_power, voting_power_1 + voting_power_2);

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

            let proposal: ProposalResponse = app
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

            let proposal: ProposalResponse = app
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
                String::from("Proposal not completed!")
            );

            //Query Proposal
            let res: ProposalListResponse = app
                .wrap()
                .query_wasm_smart(
                    gov_contract.addr().to_string(),
                    &QueryMsg::Proposals {
                        start: None,
                        limit: None,
                    },
                )
                .unwrap();

            assert_eq!(res.proposal_list.len(), 1);
            assert_eq!(res.proposal_count, Uint64::from(1u32));

            // Remove expired proposal
            app.update_block(|bi| {
                bi.height += PROPOSAL_EXPIRATION_PERIOD + 1;
                bi.time = bi.time.plus_seconds(6 * (PROPOSAL_EXPIRATION_PERIOD + 1));
            });

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
                            builders_contract_addr: None,
                            builders_voting_power_multiplier: None,
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
                        }))
                        .unwrap(),
                        funds: vec![],
                    }),
                }]),
                receiver: Some(String::from("receiver")),
                expedited: false,
            };
            let cosmos_msg = gov_contract.call(msg, vec![]).unwrap();
            app.execute(bv_contract_addr, cosmos_msg).unwrap();

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
                            builders_contract_addr: None,
                            builders_voting_power_multiplier: None,
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
                        }))
                        .unwrap(),
                        funds: vec![],
                    }),
                }],
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
