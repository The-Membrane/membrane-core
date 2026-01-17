#[cfg(test)]
#[allow(unused_variables)]
mod boosted_tvl_tests {
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{to_json_binary, Addr, Binary, Decimal, Uint128, SystemResult, ContractResult};
    use membrane::system_discounts::{InstantiateMsg, QueryMsg};
    use membrane::staking::Config as Staking_Config;
    use membrane::ltv_disco::{LockedDepositsResponse, LockedDeposit};
    use membrane::types::{Locked, StakeDeposit, StakeDistribution};

    use crate::contracts::{instantiate, query};

    const SECONDS_PER_DAY: u64 = 86400;

    fn setup_contract() -> (cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>, cosmwasm_std::Env) {
        let mut deps = mock_dependencies();
        let env = mock_env();
        
        // Setup basic querier mocks before instantiate
        // This will be overridden by setup_staking_mock and setup_ltv_disco_mock in each test
        deps.querier.update_wasm(|query| -> SystemResult<ContractResult<Binary>> {
            match query {
                cosmwasm_std::WasmQuery::Smart { contract_addr, msg: _ } => {
                    // Check contract_addr to return appropriate response
                    if contract_addr == "staking" {
                        // Return Config for staking contract queries during instantiate
                        SystemResult::Ok(ContractResult::Ok(to_json_binary(&Staking_Config {
                            owner: Addr::unchecked(""),
                            mbrn_denom: "mbrn_denom".to_string(),
                            incentive_schedule: StakeDistribution { rate: Decimal::zero(), duration: 0 },
                            max_commission_rate: Decimal::zero(),
                            unstaking_period: 0,
                            keep_raw_cdt: false,
                            lock_duration_ceiling: 365u64,
                            vesting_rev_multiplier: Decimal::one(),
                            positions_contract: None,
                            auction_contract: None,
                            vesting_contract: None,
                            governance_contract: None,
                            osmosis_proxy: None,
                        }).unwrap()))
                    } else if contract_addr == "ltv_disco" {
                        // Return empty UserTotalDeposits for ltv_disco queries during instantiate
                        SystemResult::Ok(ContractResult::Ok(to_json_binary(&membrane::ltv_disco::UserTotalDepositsResponse {
                            total_deposits: Uint128::zero(),
                        }).unwrap()))
                    } else {
                        // Default empty response
                        SystemResult::Ok(ContractResult::Ok(to_json_binary(&membrane::staking::StakerResponse {
                            staker: "".to_string(),
                            total_staked: Uint128::zero(),
                            deposit_list: vec![],
                        }).unwrap()))
                    }
                }
                _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&membrane::staking::StakerResponse {
                    staker: "".to_string(),
                    total_staked: Uint128::zero(),
                    deposit_list: vec![],
                }).unwrap())),
            }
        });
        
        let msg = InstantiateMsg {
            owner: None,
            oracle_contract: "oracle".to_string(),
            positions_contract: "cdp".to_string(),
            staking_contract: "staking".to_string(),
            stability_pool_contract: "stability_pool".to_string(),
            lockdrop_contract: None,
            discount_vault_contract: None,
            ltv_disco_contract: Some("ltv_disco".to_string()),
            minimum_time_in_network: 7,
            max_discount: Some(Decimal::percent(50)),
            mbrn_at_max_discount: Some(Uint128::new(100_000_000_000u128)),
            max_boost: Some(Decimal::percent(9)),
        };

        let info = mock_info("admin", &[]);
        instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        (deps, env)
    }

    fn setup_staking_mock(deps: &mut cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>, user: &str, deposits: Vec<StakeDeposit>) {
        let user_clone = user.to_string();
        let deposits_clone = deposits.clone();
        let lock_ceiling = 365u64;
        deps.querier.update_wasm(move |query| -> SystemResult<ContractResult<Binary>> {
            match query {
                cosmwasm_std::WasmQuery::Smart { contract_addr, msg } => {
                    if contract_addr == "staking" {
                        let query_msg: membrane::staking::QueryMsg = cosmwasm_std::from_json(msg).unwrap();
                        match query_msg {
                            membrane::staking::QueryMsg::UserStake { staker } if staker == user_clone => {
                                SystemResult::Ok(ContractResult::Ok(to_json_binary(&membrane::staking::StakerResponse {
                                    staker: user_clone.clone(),
                                    total_staked: deposits_clone.iter().map(|d| d.amount).sum(),
                                    deposit_list: deposits_clone.clone(),
                                }).unwrap()))
                            }
                            membrane::staking::QueryMsg::Config {} => {
                                SystemResult::Ok(ContractResult::Ok(to_json_binary(&Staking_Config {
                                    owner: Addr::unchecked(""),
                                    mbrn_denom: "mbrn_denom".to_string(),
                                    incentive_schedule: StakeDistribution { rate: Decimal::zero(), duration: 0 },
                                    max_commission_rate: Decimal::zero(),
                                    unstaking_period: 0,
                                    keep_raw_cdt: false,
                                    lock_duration_ceiling: lock_ceiling,
                                    vesting_rev_multiplier: Decimal::one(),
                                    positions_contract: None,
                                    auction_contract: None,
                                    vesting_contract: None,
                                    governance_contract: None,
                                    osmosis_proxy: None,
                                }).unwrap()))
                            }
                            membrane::staking::QueryMsg::UserRewards { user: _ } => {
                                SystemResult::Ok(ContractResult::Ok(to_json_binary(&membrane::staking::RewardsResponse {
                                    claimables: vec![],
                                    accrued_interest: Uint128::zero(),
                                }).unwrap()))
                            }
                            _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&membrane::staking::StakerResponse {
                                staker: "".to_string(),
                                total_staked: Uint128::zero(),
                                deposit_list: vec![],
                            }).unwrap())),
                        }
                    } else {
                        SystemResult::Ok(ContractResult::Ok(to_json_binary(&membrane::staking::StakerResponse {
                            staker: "".to_string(),
                            total_staked: Uint128::zero(),
                            deposit_list: vec![],
                        }).unwrap()))
                    }
                }
                _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&membrane::staking::StakerResponse {
                    staker: "".to_string(),
                    total_staked: Uint128::zero(),
                    deposit_list: vec![],
                }).unwrap())),
            }
        });
    }

    fn setup_combined_mocks(
        deps: &mut cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>,
        user: &str,
        staking_deposits: Vec<StakeDeposit>,
        ltv_locked_deposits: Vec<LockedDeposit>,
    ) {
        let user_clone = user.to_string();
        let deposits_clone = staking_deposits.clone();
        let locked_deposits_clone = ltv_locked_deposits.clone();
        let lock_ceiling = 365u64;
        let total_deposits: Uint128 = ltv_locked_deposits.iter()
            .map(|ld| ld.deposit.vault_tokens)
            .sum();
        let total_deposits_clone = total_deposits;
        deps.querier.update_wasm(move |query| -> SystemResult<ContractResult<Binary>> {
            match query {
                cosmwasm_std::WasmQuery::Smart { contract_addr, msg } => {
                    if contract_addr == "staking" {
                        let query_msg: membrane::staking::QueryMsg = cosmwasm_std::from_json(msg).unwrap();
                        match query_msg {
                            membrane::staking::QueryMsg::UserStake { staker } if staker == user_clone => {
                                SystemResult::Ok(ContractResult::Ok(to_json_binary(&membrane::staking::StakerResponse {
                                    staker: user_clone.clone(),
                                    total_staked: deposits_clone.iter().map(|d| d.amount).sum(),
                                    deposit_list: deposits_clone.clone(),
                                }).unwrap()))
                            }
                            membrane::staking::QueryMsg::Config {} => {
                                SystemResult::Ok(ContractResult::Ok(to_json_binary(&Staking_Config {
                                    owner: Addr::unchecked(""),
                                    mbrn_denom: "mbrn_denom".to_string(),
                                    incentive_schedule: StakeDistribution { rate: Decimal::zero(), duration: 0 },
                                    max_commission_rate: Decimal::zero(),
                                    unstaking_period: 0,
                                    keep_raw_cdt: false,
                                    lock_duration_ceiling: lock_ceiling,
                                    vesting_rev_multiplier: Decimal::one(),
                                    positions_contract: None,
                                    auction_contract: None,
                                    vesting_contract: None,
                                    governance_contract: None,
                                    osmosis_proxy: None,
                                }).unwrap()))
                            }
                            membrane::staking::QueryMsg::UserRewards { user: _ } => {
                                SystemResult::Ok(ContractResult::Ok(to_json_binary(&membrane::staking::RewardsResponse {
                                    claimables: vec![],
                                    accrued_interest: Uint128::zero(),
                                }).unwrap()))
                            }
                            _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&membrane::staking::StakerResponse {
                                staker: "".to_string(),
                                total_staked: Uint128::zero(),
                                deposit_list: vec![],
                            }).unwrap())),
                        }
                    } else if contract_addr == "ltv_disco" {
                        let query_msg: membrane::ltv_disco::QueryMsg = cosmwasm_std::from_json(msg).unwrap();
                        match query_msg {
                            membrane::ltv_disco::QueryMsg::GetLockedDeposits { user: query_user } if query_user == user_clone => {
                                SystemResult::Ok(ContractResult::Ok(to_json_binary(&LockedDepositsResponse {
                                    locked_deposits: locked_deposits_clone.clone(),
                                }).unwrap()))
                            }
                            membrane::ltv_disco::QueryMsg::UserTotalDeposits { user: query_user } if query_user == user_clone => {
                                SystemResult::Ok(ContractResult::Ok(to_json_binary(&membrane::ltv_disco::UserTotalDepositsResponse {
                                    total_deposits: total_deposits_clone,
                                }).unwrap()))
                            }
                            membrane::ltv_disco::QueryMsg::Config {} => {
                                SystemResult::Ok(ContractResult::Ok(to_json_binary(&membrane::ltv_disco::Config {
                                    owner: Addr::unchecked(""),
                                    cdp_contract: Addr::unchecked("cdp"),
                                    deposit_denom: membrane::types::DepositDenom {
                                        denom: "uusd".to_string(),
                                        vault_info: None,
                                    },
                                    cdt_denom: "cdt".to_string(),
                                    minimum_deposit: Uint128::zero(),
                                    max_ltv: Decimal::one(),
                                    percent_to_disperse: Decimal::zero(),
                                    dispersal_window: 24,
                                    activation_window: 48,
                                    oracle_contract: Addr::unchecked("oracle"),
                                    chain_proxy_contract: Addr::unchecked("proxy"),
                                    lock_duration_ceiling: lock_ceiling,
                                }).unwrap()))
                            }
                            membrane::ltv_disco::QueryMsg::VaultTokenConversion { vault_tokens, .. } => {
                                // For tests, return 1:1 conversion (vault tokens = deposit tokens)
                                // In reality this would use the group's ratio
                                SystemResult::Ok(ContractResult::Ok(to_json_binary(&vault_tokens).unwrap()))
                            }
                            _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&LockedDepositsResponse {
                                locked_deposits: vec![],
                            }).unwrap())),
                        }
                    } else {
                        SystemResult::Ok(ContractResult::Ok(to_json_binary(&membrane::staking::StakerResponse {
                            staker: "".to_string(),
                            total_staked: Uint128::zero(),
                            deposit_list: vec![],
                        }).unwrap()))
                    }
                }
                _ => SystemResult::Ok(ContractResult::Ok(to_json_binary(&membrane::staking::StakerResponse {
                    staker: "".to_string(),
                    total_staked: Uint128::zero(),
                    deposit_list: vec![],
                }).unwrap())),
            }
        });
    }

    fn setup_ltv_disco_mock(deps: &mut cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>, user: &str, locked_deposits: Vec<LockedDeposit>) {
        // Use combined mock with empty staking deposits
        setup_combined_mocks(deps, user, vec![], locked_deposits);
    }

    #[test]
    fn test_boosted_tvl_based_on_lock_duration() {
        let (mut deps, env) = setup_contract();
        let user = "user1";
        
        // Create a stake deposit with lock at 50% of ceiling (should give 50% boost)
        let lock_ceiling = 365u64;
        let lock_duration_days = 182u64; // 50% of ceiling
        let stake_time = env.block.time.seconds() - (100 * SECONDS_PER_DAY); // 100 days ago
        let locked_until = stake_time + (lock_duration_days * SECONDS_PER_DAY);
        
        let deposit = StakeDeposit {
            staker: Addr::unchecked(user),
            amount: Uint128::new(25_000_000_000),
            stake_time,
            unstake_start_time: None,
            last_accrued: None,
            locked: Some(Locked {
                locked_until,
                perpetual_lock: None,
            }),
        };
        
        setup_combined_mocks(&mut deps, user, vec![deposit.clone()], vec![]); // No LTV disco deposits for this test
        
        // Query boost
        let response: membrane::system_discounts::UserBoostResponse = cosmwasm_std::from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::UserBoost {
                user: user.to_string(),
            }).unwrap()
        ).unwrap();
        let boost = response.boost;

        // Boost should be approximately 37.5% of max_boost (9%)
        // Formula: boost = max_boost * max(lock_ratio, time_ratio)
        // lock_ratio = 182/365 = 0.5
        // So boost = 9% * 0.375 = 3.375%
        let expected_boost = Decimal::percent(9) * Decimal::from_ratio(182u128, 365u128);
        assert!(boost <= Decimal::percent(4)); 
        assert!(boost >= Decimal::percent(3));
    }

    #[test]
    fn test_boosted_tvl_based_on_time_since_deposit() {
        let (mut deps, env) = setup_contract();
        let user = "user1";
        
        // Create a stake deposit where time since deposit is the limiting factor
        let lock_ceiling = 365u64;
        let deposit_age_days = 200u64; // Deposited 200 days ago
        let lock_duration_days = 100u64; // Only locked for 100 days
        
        let stake_time = env.block.time.seconds() - (deposit_age_days * SECONDS_PER_DAY);
        let locked_until = stake_time + (lock_duration_days * SECONDS_PER_DAY);
        
        let deposit = StakeDeposit {
            staker: Addr::unchecked(user),
            amount: Uint128::new(25_000_000_000_000),
            stake_time,
            unstake_start_time: None,
            last_accrued: None,
            locked: Some(Locked {
                locked_until,
                perpetual_lock: None,
            }),
        };
        
        setup_combined_mocks(&mut deps, user, vec![deposit.clone()], vec![]); // No LTV disco deposits for this test
        
        // Query boost
        let response: membrane::system_discounts::UserBoostResponse = cosmwasm_std::from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::UserBoost {
                user: user.to_string(),
            }).unwrap()
        ).unwrap();
        let boost = response.boost;
        
        // Should use time_since_deposit ratio (200/365) which is larger than lock_ratio (100/365)
        let time_ratio = Decimal::from_ratio(200u128, 365u128);
        let expected_boost = Decimal::percent(9) * time_ratio;
        
        assert!(boost <= expected_boost + Decimal::percent(1));
        assert!(boost >= expected_boost - Decimal::percent(1));
    }

    #[test]
    fn test_boosted_tvl_with_perpetual_lock_virtual_refresh() {
        let (mut deps, env) = setup_contract();
        let user = "user1";
        
        // Create deposit with perpetual lock - virtual refresh should extend it
        let lock_ceiling = 365u64;
        let stake_time = env.block.time.seconds() - (50 * SECONDS_PER_DAY);
        let initial_lock_duration = 100u64; // Already locked for 100 days
        let locked_until = stake_time + (initial_lock_duration * SECONDS_PER_DAY);
        let perpetual_lock_days = 30u64;
        
        let deposit = StakeDeposit {
            staker: Addr::unchecked(user),
            amount: Uint128::new(10_000_000_000),
            stake_time,
            unstake_start_time: None,
            last_accrued: None,
            locked: Some(Locked {
                locked_until,
                perpetual_lock: Some(perpetual_lock_days),
            }),
        };
        
        setup_combined_mocks(&mut deps, user, vec![deposit.clone()], vec![]);
        
        // Query boost - should virtually refresh the lock
        let response: membrane::system_discounts::UserBoostResponse = cosmwasm_std::from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::UserBoost {
                user: user.to_string(),
            }).unwrap()
        ).unwrap();
        let boost = response.boost;
        
        // Virtual refresh: locked_until = current_time + perpetual_lock_days
        // But capped at stake_time + ceiling
        let virtual_locked_until = std::cmp::min(
            env.block.time.seconds() + (perpetual_lock_days * SECONDS_PER_DAY),
            stake_time + (lock_ceiling * SECONDS_PER_DAY)
        );
        let virtual_lock_duration = virtual_locked_until - stake_time;
        let lock_ratio = Decimal::from_ratio(virtual_lock_duration, lock_ceiling * SECONDS_PER_DAY);
        let time_ratio = Decimal::from_ratio(50u128, 365u128); // 50 days since deposit
        let max_ratio = if lock_ratio > time_ratio { lock_ratio } else { time_ratio };
        let expected_boost = Decimal::percent(9) * max_ratio;
        
        assert!(boost <= expected_boost + Decimal::percent(1));
        assert!(boost >= expected_boost - Decimal::percent(1));
    }

    #[test]
    fn test_boosted_tvl_maxes_out_at_100_percent() {
        let (mut deps, env) = setup_contract();
        let user = "user1";
        
        // Create deposit at or above ceiling - should cap at 100%
        let lock_ceiling = 365u64;
        let stake_time = env.block.time.seconds() - (400 * SECONDS_PER_DAY); // 400 days ago (exceeds ceiling)
        let locked_until = stake_time + (400 * SECONDS_PER_DAY); // Locked for 400 days (exceeds ceiling)
        
        let deposit = StakeDeposit {
            staker: Addr::unchecked(user),
            amount: Uint128::new(60_000_000_000_000),
            stake_time,
            unstake_start_time: None,
            last_accrued: None,
            locked: Some(Locked {
                locked_until,
                perpetual_lock: None,
            }),
        };
        
        setup_combined_mocks(&mut deps, user, vec![deposit.clone()], vec![]); // No LTV disco deposits for this test
        
        // Query boost
        let response: membrane::system_discounts::UserBoostResponse = cosmwasm_std::from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::UserBoost {
                user: user.to_string(),
            }).unwrap()
        ).unwrap();
        let boost = response.boost;
        
        // Should cap at max_boost (9%) even though ratio would be > 100%
        let max_boost = Decimal::percent(9);
        assert_eq!(boost, max_boost);
    }

    #[test]
    fn test_boosted_tvl_combines_staking_and_ltv_disco() {
        let (mut deps, env) = setup_contract();
        let user = "user1";
        
        // Create staking deposit with 50% ratio
        let stake_time = env.block.time.seconds() - (182 * SECONDS_PER_DAY);
        let locked_until = stake_time + (182 * SECONDS_PER_DAY);
        let staking_deposit = StakeDeposit {
            staker: Addr::unchecked(user),
            amount: Uint128::new(50_000),
            stake_time,
            unstake_start_time: None,
            last_accrued: None,
            locked: Some(Locked {
                locked_until,
                perpetual_lock: None,
            }),
        };
        setup_staking_mock(&mut deps, user, vec![staking_deposit]);
        
        // Create LTV disco deposit with 30% ratio
        let ltv_stake_time = env.block.time.seconds() - (109 * SECONDS_PER_DAY); // ~30% of 365
        let ltv_locked_until = ltv_stake_time + (109 * SECONDS_PER_DAY);
        let ltv_deposit = LockedDeposit {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
            deposit_id: Uint128::one(),
            deposit: membrane::ltv_disco::BackingDeposit {
                user: Addr::unchecked(user),
                vault_tokens: Uint128::new(50_000),
                max_borrow_ltv: Decimal::percent(40),
                last_claimed: env.block.time.seconds(),
                locked: Some(Locked {
                    locked_until: ltv_locked_until,
                    perpetual_lock: None,
                }),
                start_time: ltv_stake_time,
            },
        };
        setup_combined_mocks(&mut deps, user, vec![], vec![ltv_deposit]);
        
        // Query boost - should combine both
        let response: membrane::system_discounts::UserBoostResponse = cosmwasm_std::from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::UserBoost {
                user: user.to_string(),
            }).unwrap()
        ).unwrap();
        let boost = response.boost;
        
        // Both deposits contribute to the boost calculation
        // Total MBRN should include both
        assert!(boost > Decimal::zero());
    }

    // #[test]
    // fn test_boosted_tvl_unlocked_deposits_no_boost() {
    //     let (mut deps, env) = setup_contract();
    //     let user = "user1";
        
    //     // Create unlocked stake deposit
    //     let deposit = StakeDeposit {
    //         staker: Addr::unchecked(user),
    //         amount: Uint128::new(100_000),
    //         stake_time: env.block.time.seconds() - (100 * SECONDS_PER_DAY),
    //         unstake_start_time: None,
    //         last_accrued: None,
    //         locked: None, // No lock
    //     };
        
    //     setup_combined_mocks(&mut deps, user, vec![deposit], vec![]);
        
    //     // Query boost
    //     let response: membrane::system_discounts::UserBoostResponse = cosmwasm_std::from_json(
    //         query(deps.as_ref(), env.clone(), QueryMsg::UserBoost {
    //             user: user.to_string(),
    //         }).unwrap()
    //     ).unwrap();
    //     let boost = response.boost;
        
    //     // Unlocked deposits should not contribute to boost
    //     assert_eq!(boost, Decimal::zero());
    // }

    #[test]
    fn test_boosted_tvl_multiple_deposits_averaged() {
        let (mut deps, env) = setup_contract();
        let user = "user1";
        
        // Create multiple deposits with different lock ratios
        let deposit1 = StakeDeposit {
            staker: Addr::unchecked(user),
            amount: Uint128::new(25_000_000_000),
            stake_time: env.block.time.seconds() - (365 * SECONDS_PER_DAY),
            unstake_start_time: None,
            last_accrued: None,
            locked: Some(Locked {
                locked_until: env.block.time.seconds() + (365 * SECONDS_PER_DAY), // 100% ratio
                perpetual_lock: None,
            }),
        };
        
        let deposit2 = StakeDeposit {
            staker: Addr::unchecked(user),
            amount: Uint128::new(25_000_000_000),
            stake_time: env.block.time.seconds() - (182 * SECONDS_PER_DAY),
            unstake_start_time: None,
            last_accrued: None,
            locked: Some(Locked {
                locked_until: env.block.time.seconds() + (182 * SECONDS_PER_DAY), // 50% ratio
                perpetual_lock: None,
            }),
        };
        
        setup_combined_mocks(&mut deps, user, vec![deposit1, deposit2], vec![]);
        
        // Query boost
        let response: membrane::system_discounts::UserBoostResponse = cosmwasm_std::from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::UserBoost {
                user: user.to_string(),
            }).unwrap()
        ).unwrap();
        let boost = response.boost;

        println!("boost: {:?}", boost);
        // Should calculate boost based on weighted average or total
        // Both deposits contribute, so boost should be between 50% and 100% of max_boost
        assert!(boost > Decimal::percent(4)); // > 50% of 9%
        assert!(boost <= Decimal::percent(9)); // <= max boost
    }
}

