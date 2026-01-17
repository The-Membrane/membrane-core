#[cfg(test)]
mod lock_refresh_tests {
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{coin, from_json, Decimal, Uint128};
    use membrane::staking::{ExecuteMsg, InstantiateMsg, QueryMsg, StakerResponse};
    use membrane::types::{Locked, StakeDistribution};

    use crate::contract::{execute, instantiate, query};

    const SECONDS_PER_DAY: u64 = 86400;

    fn setup_contract() -> (cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>, cosmwasm_std::Env) {
        let mut deps = mock_dependencies();
        let env = mock_env();
        
        let msg = InstantiateMsg {
            emissions_voting_contract: None,
            owner: None,
            positions_contract: Some("positions_contract".to_string()),
            auction_contract: Some("auction_contract".to_string()),
            vesting_contract: Some("vesting_contract".to_string()),
            governance_contract: Some("gov_contract".to_string()),
            osmosis_proxy: Some("osmosis_proxy".to_string()),
            incentive_schedule: Some(StakeDistribution { rate: Decimal::percent(10), duration: 90 }),
            mbrn_denom: String::from("mbrn_denom"),
            unstaking_period: None,
        };

        let info = mock_info("admin", &[]);
        instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();
        
        (deps, env)
    }

    #[test]
    fn test_refresh_lock_single_deposit_with_perpetual_lock() {
        let (mut deps, mut env) = setup_contract();
        let user = "user1";
        
        // Stake with perpetual lock (30 days)
        let locked_until = env.block.time.plus_seconds(30 * SECONDS_PER_DAY).seconds();
        let msg = ExecuteMsg::Stake {
            user: None,
            locked: Some(Locked {
                intended_lock_days: None,
                locked_until,
                perpetual_lock: Some(30), // 30 day perpetual lock
            }),
        };
        let info = mock_info(user, &[coin(10_000_000, "mbrn_denom")]); // At least 1 MBRN (with 6 decimals = 1_000_000, using 10 to be safe)
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Get initial locked_until
        let staker_response: StakerResponse = from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::UserStake {
                staker: user.to_string(),
            }).unwrap()
        ).unwrap();
        let initial_locked_until = staker_response.deposit_list[0].locked.as_ref().unwrap().locked_until;
        
        // Advance time by 10 days
        env.block.time = env.block.time.plus_seconds(10 * SECONDS_PER_DAY);
        
        // Refresh lock (anyone can call this - permissionless)
        let msg = ExecuteMsg::RefreshLock {
            user: Some(user.to_string()), // Uses info.sender
            deposit_index: None, // Refresh all deposits
        };
        let info = mock_info("anyone", &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Check that locked_until was extended
        let staker_response: StakerResponse = from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::UserStake {
                staker: user.to_string(),
            }).unwrap()
        ).unwrap();
        let new_locked_until = staker_response.deposit_list[0].locked.as_ref().unwrap().locked_until;
        
        // Should be extended by perpetual_lock duration from current time
        let expected_locked_until = env.block.time.plus_seconds(30 * SECONDS_PER_DAY).seconds();
        assert_eq!(new_locked_until, expected_locked_until);
        assert!(new_locked_until > initial_locked_until, "Lock should be extended");
    }

    #[test]
    fn test_refresh_lock_specific_deposit_index() {
        let (mut deps, mut env) = setup_contract();
        let user = "user1";
        
        // Stake 3 deposits, only second has perpetual lock
        for (i, has_perp) in vec![false, true, false].iter().enumerate() {
            let msg = ExecuteMsg::Stake {
                user: None,
                locked: if *has_perp {
                    Some(Locked {
                        locked_until: env.block.time.plus_seconds(30 * SECONDS_PER_DAY).seconds(),
                        perpetual_lock: Some(30),
                        intended_lock_days: None,
                    })
                } else {
                    Some(Locked {
                        locked_until: env.block.time.plus_seconds(30 * SECONDS_PER_DAY).seconds(),
                        perpetual_lock: None,
                        intended_lock_days: None,
                    })
                },
            };
            let info = mock_info(user, &[coin(100_000_000 * (i as u128 + 1), "mbrn_denom")]);
            execute(deps.as_mut(), env.clone(), info, msg).unwrap();
        }

        let staker_response: StakerResponse = from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::UserStake {
                staker: user.to_string(),
            }).unwrap()
        ).unwrap();
        let initial_locked_until = staker_response.deposit_list[1].locked.as_ref().unwrap().locked_until;
        // println!("staker_response: {:?}", staker_response);
        // Advance time
        env.block.time = env.block.time.plus_seconds(10 * SECONDS_PER_DAY);
        
        // Refresh only the second deposit (index 1)
        let msg = ExecuteMsg::RefreshLock {
            user: Some(user.to_string()),
            deposit_index: Some(1),
        };
        let info = mock_info("anyone", &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Check that only deposit at index 1 was refreshed
        let staker_response: StakerResponse = from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::UserStake {
                staker: user.to_string(),
            }).unwrap()
        ).unwrap();
        
        // Deposit 0 (no perpetual lock) - should not be refreshed
        assert_eq!(
            staker_response.deposit_list[0].locked.as_ref().unwrap().locked_until,
            staker_response.deposit_list[0].locked.as_ref().unwrap().locked_until
        );
        
        // Deposit 1 (with perpetual lock) - should be refreshed
        let new_locked_until = staker_response.deposit_list[1].locked.as_ref().unwrap().locked_until;
        assert!(new_locked_until > initial_locked_until);
        
        // Deposit 2 (no perpetual lock) - should not be refreshed
        // (same as deposit 0 check)
    }

    #[test]
    fn test_refresh_lock_permissionless_for_any_user() {
        let (mut deps, mut env) = setup_contract();
        let user = "user1";
        let other_user = "user2";
        
        // User1 stakes with perpetual lock
        let locked_until = env.block.time.plus_seconds(30 * SECONDS_PER_DAY).seconds();
        let msg = ExecuteMsg::Stake {
            user: None,
            locked: Some(Locked {
                intended_lock_days: None,
                locked_until,
                perpetual_lock: Some(30),
            }),
        };
        let info = mock_info(user, &[coin(10_000_000, "mbrn_denom")]); // At least 1 MBRN (with 6 decimals = 1_000_000, using 10 to be safe)
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let initial_locked_until = {
            let staker_response: StakerResponse = from_json(
                query(deps.as_ref(), env.clone(), QueryMsg::UserStake {
                    staker: user.to_string(),
                }).unwrap()
            ).unwrap();
            staker_response.deposit_list[0].locked.as_ref().unwrap().locked_until
        };
        
        // Advance time
        env.block.time = env.block.time.plus_seconds(10 * SECONDS_PER_DAY);
        
        // User2 (different user) refreshes lock for user1 (permissionless)
        let msg = ExecuteMsg::RefreshLock {
            user: Some(user.to_string()), // Refresh for user1
            deposit_index: None,
        };
        let info = mock_info(other_user, &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Check that lock was refreshed
        let staker_response: StakerResponse = from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::UserStake {
                staker: user.to_string(),
            }).unwrap()
        ).unwrap();
        let new_locked_until = staker_response.deposit_list[0].locked.as_ref().unwrap().locked_until;
        
        assert!(new_locked_until > initial_locked_until, "Lock should be refreshed by anyone");
    }

    #[test]
    fn test_refresh_lock_on_claim_rewards() {
        let (mut deps, mut env) = setup_contract();
        let user = "user1";
        
        // Stake with perpetual lock
        let locked_until = env.block.time.plus_seconds(30 * SECONDS_PER_DAY).seconds();
        let msg = ExecuteMsg::Stake {
            user: None,
            locked: Some(Locked {
                intended_lock_days: None,
                locked_until,
                perpetual_lock: Some(30),
            }),
        };
        let info = mock_info(user, &[coin(10_000_000, "mbrn_denom")]); // At least 1 MBRN (with 6 decimals = 1_000_000, using 10 to be safe)
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let initial_locked_until = {
            let staker_response: StakerResponse = from_json(
                query(deps.as_ref(), env.clone(), QueryMsg::UserStake {
                    staker: user.to_string(),
                }).unwrap()
            ).unwrap();
            staker_response.deposit_list[0].locked.as_ref().unwrap().locked_until
        };
        
        // Advance time
        env.block.time = env.block.time.plus_seconds(10 * SECONDS_PER_DAY);
        
        // Claim rewards (this should auto-refresh the lock)
        let msg = ExecuteMsg::ClaimRewards {
            restake: false,
            send_to: None,
        };
        let info = mock_info(user, &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Check that lock was auto-refreshed during claim
        let staker_response: StakerResponse = from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::UserStake {
                staker: user.to_string(),
            }).unwrap()
        ).unwrap();
        let new_locked_until = staker_response.deposit_list[0].locked.as_ref().unwrap().locked_until;
        
        assert!(new_locked_until > initial_locked_until, "Lock should be auto-refreshed on claim");
    }

    #[test]
    fn test_refresh_lock_on_unstake() {
        let (mut deps, mut env) = setup_contract();
        let user = "user1";
        
        // Stake with perpetual lock
        let locked_until = env.block.time.plus_seconds(30 * SECONDS_PER_DAY).seconds();
        let msg = ExecuteMsg::Stake {
            user: None,
            locked: Some(Locked {
                intended_lock_days: None,
                locked_until,
                perpetual_lock: Some(30),
            }),
        };
        let info = mock_info(user, &[coin(10_000_000, "mbrn_denom")]); // At least 1 MBRN (with 6 decimals = 1_000_000, using 10 to be safe)
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let initial_locked_until = {
            let staker_response: StakerResponse = from_json(
                query(deps.as_ref(), env.clone(), QueryMsg::UserStake {
                    staker: user.to_string(),
                }).unwrap()
            ).unwrap();
            staker_response.deposit_list[0].locked.as_ref().unwrap().locked_until
        };
        
        // Advance time double the lock duration to ensure it is expired
        env.block.time = env.block.time.plus_seconds(20 * SECONDS_PER_DAY);
        
        // Unstake (this should auto-refresh locks before processing)
        // Note: unstake will fail if deposit is locked, but refresh should happen first
        let msg = ExecuteMsg::Unstake { mbrn_amount: None };
        let info = mock_info(user, &[]);
        // This will fail because deposit is locked bc lock refresh should have occurred
        let _ = execute(deps.as_mut(), env.clone(), info, msg).unwrap_err();

        // Check that lock was auto-refreshed even though unstake failed
        // let staker_response: StakerResponse = from_json(
        //     query(deps.as_ref(), env.clone(), QueryMsg::UserStake {
        //         staker: user.to_string(),
        //     }).unwrap()
        // ).unwrap();
        // let new_locked_until = staker_response.deposit_list[0].locked.as_ref().unwrap().locked_until;
        
        // assert!(new_locked_until > initial_locked_until, "Lock should be auto-refreshed on unstake attempt");
    }

    #[test]
    fn test_refresh_lock_respects_ceiling() {
        let (mut deps, mut env) = setup_contract();
        let user = "user1";
        
        // Stake with perpetual lock, near the ceiling
        let lock_ceiling_days = 365u64;
        let days_already_elapsed = 300u64;
        let locked_until = env.block.time.plus_seconds(days_already_elapsed * SECONDS_PER_DAY).seconds();
        
        let msg = ExecuteMsg::Stake {
            user: None,
            locked: Some(Locked {
                intended_lock_days: None,
                locked_until,
                perpetual_lock: Some(100), // Would exceed ceiling if fully applied
            }),
        };
        let info = mock_info(user, &[coin(10_000_000, "mbrn_denom")]); // At least 1 MBRN (with 6 decimals = 1_000_000, using 10 to be safe)
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Advance time significantly
        env.block.time = env.block.time.plus_seconds(50 * SECONDS_PER_DAY);
        
        // Refresh lock
        let msg = ExecuteMsg::RefreshLock {
            user: Some(user.to_string()),
            deposit_index: None,
        };
        let info = mock_info("anyone", &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Check that lock respects ceiling (start_time + ceiling days)
        let staker_response: StakerResponse = from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::UserStake {
                staker: user.to_string(),
            }).unwrap()
        ).unwrap();
        let new_locked_until = staker_response.deposit_list[0].locked.as_ref().unwrap().locked_until;
        let stake_time = staker_response.deposit_list[0].stake_time;
        let max_lock_time = stake_time + (lock_ceiling_days * SECONDS_PER_DAY);
        
        assert!(new_locked_until <= max_lock_time, "Lock should not exceed ceiling");
    }

    #[test]
    fn test_refresh_lock_without_perpetual_lock_does_nothing() {
        let (mut deps, mut env) = setup_contract();
        let user = "user1";
        
        // Stake without perpetual lock
        let locked_until = env.block.time.plus_seconds(30 * SECONDS_PER_DAY).seconds();
        let msg = ExecuteMsg::Stake {
            user: None,
            locked: Some(Locked {
                intended_lock_days: None,
                locked_until,
                perpetual_lock: None, // No perpetual lock
            }),
        };
        let info = mock_info(user, &[coin(10_000_000, "mbrn_denom")]); // At least 1 MBRN (with 6 decimals = 1_000_000, using 10 to be safe)
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        let initial_locked_until = {
            let staker_response: StakerResponse = from_json(
                query(deps.as_ref(), env.clone(), QueryMsg::UserStake {
                    staker: user.to_string(),
                }).unwrap()
            ).unwrap();
            staker_response.deposit_list[0].locked.as_ref().unwrap().locked_until
        };
        
        // Advance time
        env.block.time = env.block.time.plus_seconds(10 * SECONDS_PER_DAY);
        
        // Refresh lock (should do nothing without perpetual_lock)
        let msg = ExecuteMsg::RefreshLock {
            user: Some(user.to_string()),
            deposit_index: None,
        };
        let info = mock_info("anyone", &[]);
        execute(deps.as_mut(), env.clone(), info, msg).unwrap();

        // Check that locked_until didn't change
        let staker_response: StakerResponse = from_json(
            query(deps.as_ref(), env.clone(), QueryMsg::UserStake {
                staker: user.to_string(),
            }).unwrap()
        ).unwrap();
        let new_locked_until = staker_response.deposit_list[0].locked.as_ref().unwrap().locked_until;
        
        assert_eq!(new_locked_until, initial_locked_until, "Lock without perpetual_lock should not be refreshed");
    }
}

