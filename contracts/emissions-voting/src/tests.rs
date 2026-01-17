#[cfg(test)]
mod tests {
    use cosmwasm_std::{
        testing::{mock_env, mock_info, MockApi, MockStorage},
        to_json_binary, Addr, Binary, ContractResult, Decimal, Empty, OwnedDeps, Querier,
        QuerierResult, QueryRequest, SystemError, SystemResult, Uint128, WasmQuery,
    };
    use std::str::FromStr;

    use membrane::emissions_voting::{
        AllVotesResponse, ConfigResponse, CurrentResultResponse, ExecuteMsg, Graph,
        GraphResponse, GraphType, GraphsResponse, InstantiateMsg, PeriodHistoryResponse, QueryMsg,
        UserVote, UserVoteResponse,
    };
    use membrane::ltv_disco::{BackingDeposit, LockedDeposit, LockedDepositsResponse};
    use membrane::staking::StakerResponse;
    use membrane::types::{Locked, StakeDeposit};

    use crate::contract::{execute, instantiate, query};
    use crate::error::ContractError;

    const OWNER: &str = "owner";
    const USER1: &str = "user1";
    const USER2: &str = "user2";
    const LTV_DISCO: &str = "ltv_disco_contract";
    const STAKING: &str = "staking_contract";
    const CALLBACK_CONTRACT: &str = "callback_contract";

    // Custom querier that mocks LTV Disco and Staking responses
    struct CustomQuerier {
        disco_deposits: Vec<(String, Vec<LockedDeposit>)>,
        staking_deposits: Vec<(String, StakerResponse)>,
    }

    impl CustomQuerier {
        fn new() -> Self {
            Self {
                disco_deposits: vec![],
                staking_deposits: vec![],
            }
        }

        fn with_disco_deposits(mut self, user: &str, deposits: Vec<LockedDeposit>) -> Self {
            self.disco_deposits.push((user.to_string(), deposits));
            self
        }

        fn with_staking_deposits(mut self, user: &str, response: StakerResponse) -> Self {
            self.staking_deposits.push((user.to_string(), response));
            self
        }
    }

    impl Querier for CustomQuerier {
        fn raw_query(&self, bin_request: &[u8]) -> QuerierResult {
            let request: QueryRequest<Empty> = cosmwasm_std::from_json(bin_request).unwrap();

            match request {
                QueryRequest::Wasm(WasmQuery::Smart { contract_addr, msg }) => {
                    if contract_addr == LTV_DISCO {
                        // Parse LTV Disco query
                        let query: membrane::ltv_disco::QueryMsg =
                            cosmwasm_std::from_json(&msg).unwrap();
                        match query {
                            membrane::ltv_disco::QueryMsg::GetLockedDeposits { user } => {
                                let deposits = self
                                    .disco_deposits
                                    .iter()
                                    .find(|(u, _)| u == &user)
                                    .map(|(_, d)| d.clone())
                                    .unwrap_or_default();
                                let response = LockedDepositsResponse {
                                    locked_deposits: deposits,
                                };
                                SystemResult::Ok(ContractResult::Ok(
                                    to_json_binary(&response).unwrap(),
                                ))
                            }
                            _ => SystemResult::Err(SystemError::InvalidRequest {
                                error: "Unsupported query".to_string(),
                                request: msg,
                            }),
                        }
                    } else if contract_addr == STAKING {
                        // Parse Staking query
                        let query: membrane::staking::QueryMsg =
                            cosmwasm_std::from_json(&msg).unwrap();
                        match query {
                            membrane::staking::QueryMsg::UserStake { staker } => {
                                let response = self
                                    .staking_deposits
                                    .iter()
                                    .find(|(u, _)| u == &staker)
                                    .map(|(_, r)| r.clone())
                                    .unwrap_or(StakerResponse {
                                        staker: staker.clone(),
                                        total_staked: Uint128::zero(),
                                        deposit_list: vec![],
                                    });
                                SystemResult::Ok(ContractResult::Ok(
                                    to_json_binary(&response).unwrap(),
                                ))
                            }
                            _ => SystemResult::Err(SystemError::InvalidRequest {
                                error: "Unsupported query".to_string(),
                                request: msg,
                            }),
                        }
                    } else {
                        SystemResult::Err(SystemError::InvalidRequest {
                            error: "Unknown contract".to_string(),
                            request: msg,
                        })
                    }
                }
                _ => SystemResult::Err(SystemError::InvalidRequest {
                    error: "Unsupported query type".to_string(),
                    request: Binary::default(),
                }),
            }
        }
    }

    fn mock_deps_with_querier(
        querier: CustomQuerier,
    ) -> OwnedDeps<MockStorage, MockApi, CustomQuerier, Empty> {
        OwnedDeps {
            storage: MockStorage::default(),
            api: MockApi::default(),
            querier,
            custom_query_type: std::marker::PhantomData,
        }
    }

    fn setup_contract(
        deps: &mut OwnedDeps<MockStorage, MockApi, CustomQuerier, Empty>,
    ) -> Result<(), ContractError> {
        let msg = InstantiateMsg {
            owner: Some(OWNER.to_string()),
            ltv_disco_contract: LTV_DISCO.to_string(),
            staking_contract: STAKING.to_string(),
        };
        let info = mock_info(OWNER, &[]);
        instantiate(deps.as_mut(), mock_env(), info, msg)?;
        Ok(())
    }

    fn create_locked_deposit(
        asset: &str,
        vault_tokens: u128,
        locked: Option<Locked>,
    ) -> LockedDeposit {
        LockedDeposit {
            asset: asset.to_string(),
            ltv: Decimal::percent(80),
            max_borrow_ltv: Decimal::percent(75),
            deposit_id: Uint128::one(),
            deposit: BackingDeposit {
                user: Addr::unchecked(USER1),
                vault_tokens: Uint128::from(vault_tokens),
                locked_vault_tokens: Uint128::from(vault_tokens),
                max_borrow_ltv: Decimal::percent(75),
                last_claimed: 0,
                locked,
                start_time: 0,
                compound_claims: false,
                manager: None,
                depositor: None,
                withdrawals_enabled: true,
            },
        }
    }

    fn create_stake_deposit(amount: u128, locked: Option<Locked>) -> StakeDeposit {
        StakeDeposit {
            staker: Addr::unchecked(USER1),
            amount: Uint128::from(amount),
            stake_time: 0,
            unstake_start_time: None,
            locked,
            last_accrued: None,
        }
    }

    // ==================== INSTANTIATION TESTS ====================

    #[test]
    fn test_instantiate() {
        let querier = CustomQuerier::new();
        let mut deps = mock_deps_with_querier(querier);

        let msg = InstantiateMsg {
            owner: Some(OWNER.to_string()),
            ltv_disco_contract: LTV_DISCO.to_string(),
            staking_contract: STAKING.to_string(),
        };
        let info = mock_info(OWNER, &[]);
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        assert_eq!(res.attributes.len(), 2);
        assert_eq!(res.attributes[0].key, "action");
        assert_eq!(res.attributes[0].value, "instantiate");

        // Query config
        let res: ConfigResponse =
            cosmwasm_std::from_json(query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap())
                .unwrap();
        assert_eq!(res.config.owner, Addr::unchecked(OWNER));
        assert_eq!(res.config.ltv_disco_contract, Addr::unchecked(LTV_DISCO));
        assert_eq!(res.config.staking_contract, Addr::unchecked(STAKING));
    }

    #[test]
    fn test_instantiate_default_owner() {
        let querier = CustomQuerier::new();
        let mut deps = mock_deps_with_querier(querier);

        let msg = InstantiateMsg {
            owner: None,
            ltv_disco_contract: LTV_DISCO.to_string(),
            staking_contract: STAKING.to_string(),
        };
        let info = mock_info(USER1, &[]);
        instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        let res: ConfigResponse =
            cosmwasm_std::from_json(query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap())
                .unwrap();
        assert_eq!(res.config.owner, Addr::unchecked(USER1));
    }

    // ==================== GRAPH CREATION TESTS ====================

    #[test]
    fn test_create_uint128_graph() {
        let querier = CustomQuerier::new();
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let msg = ExecuteMsg::CreateGraph {
            label: "emissions".to_string(),
            graph_type: GraphType::Uint128,
            range_min: "1000000".to_string(),
            range_max: "10000000".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        let info = mock_info(OWNER, &[]);
        let res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

        assert_eq!(res.attributes[0].value, "create_graph");
        assert_eq!(res.attributes[1].value, "emissions");

        // Query the graph
        let res: GraphResponse = cosmwasm_std::from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::Graph {
                    label: "emissions".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        match res.graph {
            Graph::Uint128(g) => {
                assert_eq!(g.label, "emissions");
                assert_eq!(g.range_min, Uint128::from(1000000u128));
                assert_eq!(g.range_max, Uint128::from(10000000u128));
                assert_eq!(g.period_days, 7);
            }
            _ => panic!("Expected Uint128 graph"),
        }
    }

    #[test]
    fn test_create_decimal_graph() {
        let querier = CustomQuerier::new();
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let msg = ExecuteMsg::CreateGraph {
            label: "multiplier".to_string(),
            graph_type: GraphType::Decimal,
            range_min: "0.5".to_string(),
            range_max: "2.0".to_string(),
            period_days: 14,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        let info = mock_info(OWNER, &[]);
        execute(deps.as_mut(), mock_env(), info, msg).unwrap();

        let res: GraphResponse = cosmwasm_std::from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::Graph {
                    label: "multiplier".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        match res.graph {
            Graph::Decimal(g) => {
                assert_eq!(g.label, "multiplier");
                assert_eq!(g.range_min, Decimal::from_str("0.5").unwrap());
                assert_eq!(g.range_max, Decimal::from_str("2.0").unwrap());
                assert_eq!(g.period_days, 14);
            }
            _ => panic!("Expected Decimal graph"),
        }
    }

    #[test]
    fn test_create_graph_unauthorized() {
        let querier = CustomQuerier::new();
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let msg = ExecuteMsg::CreateGraph {
            label: "test".to_string(),
            graph_type: GraphType::Uint128,
            range_min: "0".to_string(),
            range_max: "100".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        let info = mock_info(USER1, &[]); // Not owner
        let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();

        assert_eq!(err, ContractError::Unauthorized {});
    }

    #[test]
    fn test_create_graph_duplicate_label() {
        let querier = CustomQuerier::new();
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let msg = ExecuteMsg::CreateGraph {
            label: "test".to_string(),
            graph_type: GraphType::Uint128,
            range_min: "0".to_string(),
            range_max: "100".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        let info = mock_info(OWNER, &[]);
        execute(deps.as_mut(), mock_env(), info.clone(), msg.clone()).unwrap();

        let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();
        assert_eq!(
            err,
            ContractError::GraphAlreadyExists {
                label: "test".to_string()
            }
        );
    }

    #[test]
    fn test_create_graph_invalid_range() {
        let querier = CustomQuerier::new();
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let msg = ExecuteMsg::CreateGraph {
            label: "test".to_string(),
            graph_type: GraphType::Uint128,
            range_min: "100".to_string(),
            range_max: "50".to_string(), // min > max
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        let info = mock_info(OWNER, &[]);
        let err = execute(deps.as_mut(), mock_env(), info, msg).unwrap_err();

        assert_eq!(err, ContractError::InvalidRange {});
    }

    // ==================== VOTING TESTS ====================

    #[test]
    fn test_vote_uint128() {
        // Setup with voting power
        let querier = CustomQuerier::new()
            .with_disco_deposits(USER1, vec![create_locked_deposit("USDC", 1000000, None)])
            .with_staking_deposits(
                USER1,
                StakerResponse {
                    staker: USER1.to_string(),
                    total_staked: Uint128::from(500000u128),
                    deposit_list: vec![create_stake_deposit(500000, None)],
                },
            );
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        // Create graph
        let msg = ExecuteMsg::CreateGraph {
            label: "test".to_string(),
            graph_type: GraphType::Uint128,
            range_min: "0".to_string(),
            range_max: "1000".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(OWNER, &[]), msg).unwrap();

        // Vote
        let msg = ExecuteMsg::Vote {
            graph_label: "test".to_string(),
            vote_value: "500".to_string(),
        };
        let res = execute(deps.as_mut(), mock_env(), mock_info(USER1, &[]), msg).unwrap();

        assert_eq!(res.attributes[0].value, "vote");
        assert_eq!(res.attributes[2].value, "500");

        // Query user vote
        let res: UserVoteResponse = cosmwasm_std::from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::UserVote {
                    user: USER1.to_string(),
                    graph_label: "test".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert!(res.vote.is_some());
        match res.vote.unwrap() {
            UserVote::Uint128(v) => {
                assert_eq!(v.voted_value, Uint128::from(500u128));
                // 1000000 (disco) + 500000 (staking) = 1500000, no lock so multiplier is 1
                assert_eq!(v.voting_power, Uint128::from(1500000u128));
            }
            _ => panic!("Expected Uint128 vote"),
        }
    }

    #[test]
    fn test_vote_decimal() {
        let querier = CustomQuerier::new()
            .with_disco_deposits(USER1, vec![create_locked_deposit("USDC", 1000000, None)]);
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        // Create decimal graph
        let msg = ExecuteMsg::CreateGraph {
            label: "multiplier".to_string(),
            graph_type: GraphType::Decimal,
            range_min: "0.5".to_string(),
            range_max: "2.0".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(OWNER, &[]), msg).unwrap();

        // Vote
        let msg = ExecuteMsg::Vote {
            graph_label: "multiplier".to_string(),
            vote_value: "1.25".to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(USER1, &[]), msg).unwrap();

        let res: UserVoteResponse = cosmwasm_std::from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::UserVote {
                    user: USER1.to_string(),
                    graph_label: "multiplier".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        match res.vote.unwrap() {
            UserVote::Decimal(v) => {
                assert_eq!(v.voted_value, Decimal::from_str("1.25").unwrap());
            }
            _ => panic!("Expected Decimal vote"),
        }
    }

    #[test]
    fn test_vote_out_of_range() {
        let querier = CustomQuerier::new()
            .with_disco_deposits(USER1, vec![create_locked_deposit("USDC", 1000000, None)]);
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let msg = ExecuteMsg::CreateGraph {
            label: "test".to_string(),
            graph_type: GraphType::Uint128,
            range_min: "100".to_string(),
            range_max: "1000".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(OWNER, &[]), msg).unwrap();

        // Vote below range
        let msg = ExecuteMsg::Vote {
            graph_label: "test".to_string(),
            vote_value: "50".to_string(),
        };
        let err = execute(deps.as_mut(), mock_env(), mock_info(USER1, &[]), msg).unwrap_err();

        match err {
            ContractError::VoteOutOfRange { value, min, max } => {
                assert_eq!(value, "50");
                assert_eq!(min, "100");
                assert_eq!(max, "1000");
            }
            _ => panic!("Expected VoteOutOfRange error"),
        }
    }

    #[test]
    fn test_vote_no_voting_power() {
        let querier = CustomQuerier::new(); // No deposits
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let msg = ExecuteMsg::CreateGraph {
            label: "test".to_string(),
            graph_type: GraphType::Uint128,
            range_min: "0".to_string(),
            range_max: "100".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(OWNER, &[]), msg).unwrap();

        let msg = ExecuteMsg::Vote {
            graph_label: "test".to_string(),
            vote_value: "50".to_string(),
        };
        let err = execute(deps.as_mut(), mock_env(), mock_info(USER1, &[]), msg).unwrap_err();

        assert_eq!(err, ContractError::NoVotingPower {});
    }

    #[test]
    fn test_vote_update() {
        let querier = CustomQuerier::new()
            .with_disco_deposits(USER1, vec![create_locked_deposit("USDC", 1000000, None)]);
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let msg = ExecuteMsg::CreateGraph {
            label: "test".to_string(),
            graph_type: GraphType::Uint128,
            range_min: "0".to_string(),
            range_max: "1000".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(OWNER, &[]), msg).unwrap();

        // First vote
        let msg = ExecuteMsg::Vote {
            graph_label: "test".to_string(),
            vote_value: "200".to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(USER1, &[]), msg).unwrap();

        // Update vote
        let msg = ExecuteMsg::Vote {
            graph_label: "test".to_string(),
            vote_value: "800".to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(USER1, &[]), msg).unwrap();

        // Check vote is updated
        let res: UserVoteResponse = cosmwasm_std::from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::UserVote {
                    user: USER1.to_string(),
                    graph_label: "test".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        match res.vote.unwrap() {
            UserVote::Uint128(v) => {
                assert_eq!(v.voted_value, Uint128::from(800u128));
            }
            _ => panic!("Expected Uint128 vote"),
        }

        // Check current result reflects updated vote
        let res: CurrentResultResponse = cosmwasm_std::from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::CurrentResult {
                    label: "test".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(res.result_uint128, Some(Uint128::from(800u128)));
    }

    // ==================== LOCK MULTIPLIER TESTS ====================

    #[test]
    fn test_voting_power_with_perpetual_lock() {
        // 30-day perpetual lock = multiplier of 31
        let locked = Some(Locked {
            locked_until: 0,
            perpetual_lock: Some(30),
        });

        let querier = CustomQuerier::new()
            .with_disco_deposits(USER1, vec![create_locked_deposit("USDC", 1000, locked)]);
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let msg = ExecuteMsg::CreateGraph {
            label: "test".to_string(),
            graph_type: GraphType::Uint128,
            range_min: "0".to_string(),
            range_max: "100".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(OWNER, &[]), msg).unwrap();

        let msg = ExecuteMsg::Vote {
            graph_label: "test".to_string(),
            vote_value: "50".to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(USER1, &[]), msg).unwrap();

        let res: UserVoteResponse = cosmwasm_std::from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::UserVote {
                    user: USER1.to_string(),
                    graph_label: "test".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        match res.vote.unwrap() {
            UserVote::Uint128(v) => {
                // 1000 * 31 = 31000
                assert_eq!(v.voting_power, Uint128::from(31000u128));
            }
            _ => panic!("Expected Uint128 vote"),
        }
    }

    #[test]
    fn test_voting_power_with_time_lock() {
        let mut env = mock_env();
        let current_time = env.block.time.seconds();

        // Lock for 10 more days
        let lock_until = current_time + (10 * 86400);
        let locked = Some(Locked {
            locked_until: lock_until,
            perpetual_lock: None,
        });

        let querier = CustomQuerier::new()
            .with_disco_deposits(USER1, vec![create_locked_deposit("USDC", 1000, locked)]);
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let msg = ExecuteMsg::CreateGraph {
            label: "test".to_string(),
            graph_type: GraphType::Uint128,
            range_min: "0".to_string(),
            range_max: "100".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        execute(deps.as_mut(), env.clone(), mock_info(OWNER, &[]), msg).unwrap();

        let msg = ExecuteMsg::Vote {
            graph_label: "test".to_string(),
            vote_value: "50".to_string(),
        };
        execute(deps.as_mut(), env.clone(), mock_info(USER1, &[]), msg).unwrap();

        let res: UserVoteResponse = cosmwasm_std::from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::UserVote {
                    user: USER1.to_string(),
                    graph_label: "test".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        match res.vote.unwrap() {
            UserVote::Uint128(v) => {
                // 1000 * (10 + 1) = 11000
                assert_eq!(v.voting_power, Uint128::from(11000u128));
            }
            _ => panic!("Expected Uint128 vote"),
        }
    }

    #[test]
    fn test_voting_power_combined_sources() {
        let mut env = mock_env();
        let current_time = env.block.time.seconds();

        // Disco deposit with 20-day lock
        let disco_lock = Some(Locked {
            locked_until: current_time + (20 * 86400),
            perpetual_lock: None,
        });

        // Staking deposit with 10-day perpetual lock
        let stake_lock = Some(Locked {
            locked_until: 0,
            perpetual_lock: Some(10),
        });

        let querier = CustomQuerier::new()
            .with_disco_deposits(USER1, vec![create_locked_deposit("USDC", 1000, disco_lock)])
            .with_staking_deposits(
                USER1,
                StakerResponse {
                    staker: USER1.to_string(),
                    total_staked: Uint128::from(2000u128),
                    deposit_list: vec![create_stake_deposit(2000, stake_lock)],
                },
            );
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let msg = ExecuteMsg::CreateGraph {
            label: "test".to_string(),
            graph_type: GraphType::Uint128,
            range_min: "0".to_string(),
            range_max: "100".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        execute(deps.as_mut(), env.clone(), mock_info(OWNER, &[]), msg).unwrap();

        let msg = ExecuteMsg::Vote {
            graph_label: "test".to_string(),
            vote_value: "50".to_string(),
        };
        execute(deps.as_mut(), env.clone(), mock_info(USER1, &[]), msg).unwrap();

        let res: UserVoteResponse = cosmwasm_std::from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::UserVote {
                    user: USER1.to_string(),
                    graph_label: "test".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        match res.vote.unwrap() {
            UserVote::Uint128(v) => {
                // Disco: 1000 * (20 + 1) = 21000
                // Staking: 2000 * (10 + 1) = 22000
                // Total: 43000
                assert_eq!(v.voting_power, Uint128::from(43000u128));
            }
            _ => panic!("Expected Uint128 vote"),
        }
    }

    // ==================== WEIGHTED AVERAGE TESTS ====================

    #[test]
    fn test_weighted_average_single_voter() {
        let querier = CustomQuerier::new()
            .with_disco_deposits(USER1, vec![create_locked_deposit("USDC", 1000, None)]);
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let msg = ExecuteMsg::CreateGraph {
            label: "test".to_string(),
            graph_type: GraphType::Uint128,
            range_min: "0".to_string(),
            range_max: "1000".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(OWNER, &[]), msg).unwrap();

        let msg = ExecuteMsg::Vote {
            graph_label: "test".to_string(),
            vote_value: "750".to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(USER1, &[]), msg).unwrap();

        let res: CurrentResultResponse = cosmwasm_std::from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::CurrentResult {
                    label: "test".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(res.result_uint128, Some(Uint128::from(750u128)));
        assert_eq!(res.total_voting_power, Uint128::from(1000u128));
    }

    #[test]
    fn test_weighted_average_multiple_voters() {
        // USER1: 1000 power, votes 200
        // USER2: 2000 power, votes 500
        // Weighted avg = (1000*200 + 2000*500) / (1000 + 2000) = 1200000 / 3000 = 400
        let querier = CustomQuerier::new()
            .with_disco_deposits(USER1, vec![create_locked_deposit("USDC", 1000, None)])
            .with_disco_deposits(USER2, vec![create_locked_deposit("USDC", 2000, None)]);
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let msg = ExecuteMsg::CreateGraph {
            label: "test".to_string(),
            graph_type: GraphType::Uint128,
            range_min: "0".to_string(),
            range_max: "1000".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(OWNER, &[]), msg).unwrap();

        // USER1 votes 200
        let msg = ExecuteMsg::Vote {
            graph_label: "test".to_string(),
            vote_value: "200".to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(USER1, &[]), msg).unwrap();

        // USER2 votes 500
        let msg = ExecuteMsg::Vote {
            graph_label: "test".to_string(),
            vote_value: "500".to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(USER2, &[]), msg).unwrap();

        let res: CurrentResultResponse = cosmwasm_std::from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::CurrentResult {
                    label: "test".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(res.result_uint128, Some(Uint128::from(400u128)));
        assert_eq!(res.total_voting_power, Uint128::from(3000u128));
    }

    #[test]
    fn test_weighted_average_decimal() {
        let querier = CustomQuerier::new()
            .with_disco_deposits(USER1, vec![create_locked_deposit("USDC", 1000, None)])
            .with_disco_deposits(USER2, vec![create_locked_deposit("USDC", 3000, None)]);
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let msg = ExecuteMsg::CreateGraph {
            label: "mult".to_string(),
            graph_type: GraphType::Decimal,
            range_min: "0.5".to_string(),
            range_max: "2.0".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(OWNER, &[]), msg).unwrap();

        // USER1 votes 1.0
        let msg = ExecuteMsg::Vote {
            graph_label: "mult".to_string(),
            vote_value: "1.0".to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(USER1, &[]), msg).unwrap();

        // USER2 votes 1.5
        let msg = ExecuteMsg::Vote {
            graph_label: "mult".to_string(),
            vote_value: "1.5".to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(USER2, &[]), msg).unwrap();

        let res: CurrentResultResponse = cosmwasm_std::from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::CurrentResult {
                    label: "mult".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        // (1000 * 1.0 + 3000 * 1.5) / 4000 = 5500 / 4000 = 1.375
        assert_eq!(
            res.result_decimal,
            Some(Decimal::from_str("1.375").unwrap())
        );
    }

    // ==================== END VOTING TESTS ====================

    #[test]
    fn test_end_voting_period_not_ended() {
        let querier = CustomQuerier::new()
            .with_disco_deposits(USER1, vec![create_locked_deposit("USDC", 1000, None)]);
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let msg = ExecuteMsg::CreateGraph {
            label: "test".to_string(),
            graph_type: GraphType::Uint128,
            range_min: "0".to_string(),
            range_max: "100".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(OWNER, &[]), msg).unwrap();

        let msg = ExecuteMsg::Vote {
            graph_label: "test".to_string(),
            vote_value: "50".to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(USER1, &[]), msg).unwrap();

        // Try to end voting before period ends
        let msg = ExecuteMsg::EndVoting {
            graph_label: "test".to_string(),
        };
        let err = execute(deps.as_mut(), mock_env(), mock_info(USER1, &[]), msg).unwrap_err();

        match err {
            ContractError::PeriodNotEnded { .. } => {}
            _ => panic!("Expected PeriodNotEnded error"),
        }
    }

    #[test]
    fn test_end_voting_success() {
        let querier = CustomQuerier::new()
            .with_disco_deposits(USER1, vec![create_locked_deposit("USDC", 1000, None)]);
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let mut env = mock_env();

        let msg = ExecuteMsg::CreateGraph {
            label: "test".to_string(),
            graph_type: GraphType::Uint128,
            range_min: "0".to_string(),
            range_max: "100".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        execute(deps.as_mut(), env.clone(), mock_info(OWNER, &[]), msg).unwrap();

        let msg = ExecuteMsg::Vote {
            graph_label: "test".to_string(),
            vote_value: "75".to_string(),
        };
        execute(deps.as_mut(), env.clone(), mock_info(USER1, &[]), msg).unwrap();

        // Advance time past period end (8 days)
        env.block.time = env.block.time.plus_seconds(8 * 86400);

        let msg = ExecuteMsg::EndVoting {
            graph_label: "test".to_string(),
        };
        let res = execute(deps.as_mut(), env.clone(), mock_info(USER1, &[]), msg).unwrap();

        assert_eq!(res.attributes[0].value, "end_voting");
        assert_eq!(res.messages.len(), 1); // Callback message

        // Check history
        let res: PeriodHistoryResponse = cosmwasm_std::from_json(
            query(
                deps.as_ref(),
                env.clone(),
                QueryMsg::PeriodHistory {
                    label: "test".to_string(),
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(res.history.len(), 1);
        assert_eq!(res.history[0].result_uint128, Some(Uint128::from(75u128)));

        // Check votes are cleared
        let res: CurrentResultResponse = cosmwasm_std::from_json(
            query(
                deps.as_ref(),
                env,
                QueryMsg::CurrentResult {
                    label: "test".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(res.total_voting_power, Uint128::zero());
    }

    #[test]
    fn test_end_voting_no_votes() {
        let querier = CustomQuerier::new();
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let mut env = mock_env();

        let msg = ExecuteMsg::CreateGraph {
            label: "test".to_string(),
            graph_type: GraphType::Uint128,
            range_min: "0".to_string(),
            range_max: "100".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        execute(deps.as_mut(), env.clone(), mock_info(OWNER, &[]), msg).unwrap();

        // Advance time
        env.block.time = env.block.time.plus_seconds(8 * 86400);

        let msg = ExecuteMsg::EndVoting {
            graph_label: "test".to_string(),
        };
        let err = execute(deps.as_mut(), env, mock_info(USER1, &[]), msg).unwrap_err();

        assert_eq!(err, ContractError::NoVotes {});
    }

    // ==================== QUERY TESTS ====================

    #[test]
    fn test_query_graphs() {
        let querier = CustomQuerier::new();
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        // Create multiple graphs
        for i in 0..5 {
            let msg = ExecuteMsg::CreateGraph {
                label: format!("graph_{}", i),
                graph_type: GraphType::Uint128,
                range_min: "0".to_string(),
                range_max: "100".to_string(),
                period_days: 7,
                callback_contract: CALLBACK_CONTRACT.to_string(),
            };
            execute(deps.as_mut(), mock_env(), mock_info(OWNER, &[]), msg).unwrap();
        }

        let res: GraphsResponse = cosmwasm_std::from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::Graphs {
                    start_after: None,
                    limit: Some(3),
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(res.graphs.len(), 3);
    }

    #[test]
    fn test_query_all_votes() {
        let querier = CustomQuerier::new()
            .with_disco_deposits(USER1, vec![create_locked_deposit("USDC", 1000, None)])
            .with_disco_deposits(USER2, vec![create_locked_deposit("USDC", 2000, None)]);
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let msg = ExecuteMsg::CreateGraph {
            label: "test".to_string(),
            graph_type: GraphType::Uint128,
            range_min: "0".to_string(),
            range_max: "100".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(OWNER, &[]), msg).unwrap();

        // Both users vote
        execute(
            deps.as_mut(),
            mock_env(),
            mock_info(USER1, &[]),
            ExecuteMsg::Vote {
                graph_label: "test".to_string(),
                vote_value: "25".to_string(),
            },
        )
        .unwrap();

        execute(
            deps.as_mut(),
            mock_env(),
            mock_info(USER2, &[]),
            ExecuteMsg::Vote {
                graph_label: "test".to_string(),
                vote_value: "75".to_string(),
            },
        )
        .unwrap();

        let res: AllVotesResponse = cosmwasm_std::from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::AllVotes {
                    graph_label: "test".to_string(),
                    start_after: None,
                    limit: None,
                },
            )
            .unwrap(),
        )
        .unwrap();

        assert_eq!(res.votes.len(), 2);
        assert_eq!(res.total_power, Uint128::from(3000u128));
    }

    // ==================== UPDATE/REMOVE TESTS ====================

    #[test]
    fn test_update_graph() {
        let querier = CustomQuerier::new();
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let msg = ExecuteMsg::CreateGraph {
            label: "test".to_string(),
            graph_type: GraphType::Uint128,
            range_min: "0".to_string(),
            range_max: "100".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(OWNER, &[]), msg).unwrap();

        let msg = ExecuteMsg::UpdateGraph {
            label: "test".to_string(),
            period_days: Some(14),
            callback_contract: None,
        };
        execute(deps.as_mut(), mock_env(), mock_info(OWNER, &[]), msg).unwrap();

        let res: GraphResponse = cosmwasm_std::from_json(
            query(
                deps.as_ref(),
                mock_env(),
                QueryMsg::Graph {
                    label: "test".to_string(),
                },
            )
            .unwrap(),
        )
        .unwrap();

        match res.graph {
            Graph::Uint128(g) => {
                assert_eq!(g.period_days, 14);
            }
            _ => panic!("Expected Uint128 graph"),
        }
    }

    #[test]
    fn test_remove_graph() {
        let querier = CustomQuerier::new();
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let msg = ExecuteMsg::CreateGraph {
            label: "test".to_string(),
            graph_type: GraphType::Uint128,
            range_min: "0".to_string(),
            range_max: "100".to_string(),
            period_days: 7,
            callback_contract: CALLBACK_CONTRACT.to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(OWNER, &[]), msg).unwrap();

        let msg = ExecuteMsg::RemoveGraph {
            label: "test".to_string(),
        };
        execute(deps.as_mut(), mock_env(), mock_info(OWNER, &[]), msg).unwrap();

        // Query should fail
        let err = query(
            deps.as_ref(),
            mock_env(),
            QueryMsg::Graph {
                label: "test".to_string(),
            },
        )
        .unwrap_err();

        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn test_update_config() {
        let querier = CustomQuerier::new();
        let mut deps = mock_deps_with_querier(querier);
        setup_contract(&mut deps).unwrap();

        let msg = ExecuteMsg::UpdateConfig {
            owner: Some("new_owner".to_string()),
            ltv_disco_contract: None,
            staking_contract: None,
        };
        execute(deps.as_mut(), mock_env(), mock_info(OWNER, &[]), msg).unwrap();

        let res: ConfigResponse =
            cosmwasm_std::from_json(query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap())
                .unwrap();

        assert_eq!(res.config.owner, Addr::unchecked("new_owner"));
    }
}

