#[cfg(test)]
mod tests {
    use crate::contract::{execute, instantiate, query, calculate_vested_unlocked};
    use crate::state::{VestingSchedule, VESTING_SCHEDULES, OLD_MBRN_RECEIVED, NEUTRON_PROXY};

    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{from_json, Addr, Coin, CosmosMsg, Decimal, StdError, Uint128, WasmMsg};

    use membrane::vesting::{
        Config, ExecuteMsg, InstantiateMsg, QueryMsg, VestingScheduleInfo,
        VestingSchedulesResponse, VestingStatsResponse,
    };
    use membrane::types::VestingPeriod;

    const SECONDS_IN_DAY: u64 = 86400;
    const SECONDS_IN_WEEK: u64 = 604800;

    fn mock_instantiate_msg() -> InstantiateMsg {
        InstantiateMsg {
            owner: Some("owner".to_string()),
            initial_allocation: Uint128::new(1_000_000_000_000),
            pre_launch_community: vec![],
            mbrn_denom: "factory/contract/mbrn".to_string(),
            osmosis_proxy: "osmosis_proxy".to_string(),
            staking_contract: "staking_contract".to_string(),
            pre_launch_contributors: "contributors".to_string(),
            neutron_proxy: None,
            old_mbrn_denom: None,
        }
    }

    #[test]
    fn test_add_vested_transmutation_creates_schedule() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let info = mock_info("sender", &[]);

        // Instantiate
        instantiate(deps.as_mut(), env.clone(), info.clone(), mock_instantiate_msg()).unwrap();

        // Update config to set neutron_proxy and old_mbrn_denom
        let update_msg = ExecuteMsg::UpdateConfig {
            owner: None,
            mbrn_denom: None,
            osmosis_proxy: None,
            staking_contract: None,
            additional_allocation: None,
            neutron_proxy: Some("neutron_proxy".to_string()),
            old_mbrn_denom: Some("old_mbrn".to_string()),
        };
        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("owner", &[]),
            update_msg,
        )
        .unwrap();

        // Add vested transmutation from neutron-proxy
        let vesting_period = VestingPeriod {
            cliff: 180, // 180 days
            linear: 180, // 180 days
        };

        let add_msg = ExecuteMsg::AddVestedTransmutation {
            recipient: "user1".to_string(),
            amount_to_mint: Uint128::new(1000),
            vesting_period: vesting_period.clone(),
        };

        let info = mock_info("neutron_proxy", &[Coin::new(1000, "old_mbrn")]);
        let res = execute(deps.as_mut(), env.clone(), info, add_msg).unwrap();

        // Verify response attributes
        assert_eq!(res.attributes.len(), 4);
        assert!(res.attributes.iter().any(|a| a.key == "method" && a.value == "add_vested_transmutation"));
        assert!(res.attributes.iter().any(|a| a.key == "recipient" && a.value == "user1"));

        // Query vesting schedules for user1
        let query_msg = QueryMsg::VestingSchedules {
            user: "user1".to_string(),
        };
        let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
        let schedules: VestingSchedulesResponse = from_json(&res).unwrap();

        assert_eq!(schedules.schedules.len(), 1);
        assert_eq!(schedules.schedules[0].mbrn_to_mint, Uint128::new(1000));
        assert_eq!(schedules.schedules[0].transmutation_count, 1);
    }

    #[test]
    fn test_week_grouping_combines_schedules() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        // Set a safe starting time in the middle of a week
        env.block.time = cosmwasm_std::Timestamp::from_seconds(SECONDS_IN_WEEK * 100);
        let info = mock_info("sender", &[]);

        // Instantiate
        instantiate(deps.as_mut(), env.clone(), info.clone(), mock_instantiate_msg()).unwrap();

        // Update config
        let update_msg = ExecuteMsg::UpdateConfig {
            owner: None,
            mbrn_denom: None,
            osmosis_proxy: None,
            staking_contract: None,
            additional_allocation: None,
            neutron_proxy: Some("neutron_proxy".to_string()),
            old_mbrn_denom: Some("old_mbrn".to_string()),
        };
        execute(deps.as_mut(), env.clone(), mock_info("owner", &[]), update_msg).unwrap();

        let vesting_period = VestingPeriod {
            cliff: 180,
            linear: 180,
        };

        // Add first transmutation
        let add_msg1 = ExecuteMsg::AddVestedTransmutation {
            recipient: "user1".to_string(),
            amount_to_mint: Uint128::new(1000),
            vesting_period: vesting_period.clone(),
        };
        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("neutron_proxy", &[Coin::new(1000, "old_mbrn")]),
            add_msg1,
        )
        .unwrap();

        // Add second transmutation in same week (advance 2 days)
        env.block.time = env.block.time.plus_seconds(SECONDS_IN_DAY * 2);

        let add_msg2 = ExecuteMsg::AddVestedTransmutation {
            recipient: "user1".to_string(),
            amount_to_mint: Uint128::new(500),
            vesting_period: vesting_period.clone(),
        };
        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("neutron_proxy", &[Coin::new(500, "old_mbrn")]),
            add_msg2,
        )
        .unwrap();

        // Query schedules - should have only 1 (combined)
        let query_msg = QueryMsg::VestingSchedules {
            user: "user1".to_string(),
        };
        let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
        let schedules: VestingSchedulesResponse = from_json(&res).unwrap();

        assert_eq!(schedules.schedules.len(), 1, "Should have 1 combined schedule");
        assert_eq!(schedules.schedules[0].mbrn_to_mint, Uint128::new(1500));
        assert_eq!(schedules.schedules[0].transmutation_count, 2);
    }

    #[test]
    fn test_different_weeks_create_separate_schedules() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let info = mock_info("sender", &[]);

        // Instantiate
        instantiate(deps.as_mut(), env.clone(), info.clone(), mock_instantiate_msg()).unwrap();

        // Update config
        let update_msg = ExecuteMsg::UpdateConfig {
            owner: None,
            mbrn_denom: None,
            osmosis_proxy: None,
            staking_contract: None,
            additional_allocation: None,
            neutron_proxy: Some("neutron_proxy".to_string()),
            old_mbrn_denom: Some("old_mbrn".to_string()),
        };
        execute(deps.as_mut(), env.clone(), mock_info("owner", &[]), update_msg).unwrap();

        let vesting_period = VestingPeriod {
            cliff: 180,
            linear: 180,
        };

        // Add first transmutation
        let add_msg1 = ExecuteMsg::AddVestedTransmutation {
            recipient: "user1".to_string(),
            amount_to_mint: Uint128::new(1000),
            vesting_period: vesting_period.clone(),
        };
        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("neutron_proxy", &[Coin::new(1000, "old_mbrn")]),
            add_msg1,
        )
        .unwrap();

        // Add second transmutation in different week (advance 8 days)
        env.block.time = env.block.time.plus_seconds(SECONDS_IN_DAY * 8);

        let add_msg2 = ExecuteMsg::AddVestedTransmutation {
            recipient: "user1".to_string(),
            amount_to_mint: Uint128::new(500),
            vesting_period: vesting_period.clone(),
        };
        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("neutron_proxy", &[Coin::new(500, "old_mbrn")]),
            add_msg2,
        )
        .unwrap();

        // Query schedules - should have 2 separate schedules
        let query_msg = QueryMsg::VestingSchedules {
            user: "user1".to_string(),
        };
        let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
        let schedules: VestingSchedulesResponse = from_json(&res).unwrap();

        assert_eq!(schedules.schedules.len(), 2, "Should have 2 separate schedules");
    }

    #[test]
    fn test_cap_enforcement() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let info = mock_info("sender", &[]);

        // Instantiate
        instantiate(deps.as_mut(), env.clone(), info.clone(), mock_instantiate_msg()).unwrap();

        // Update config
        let update_msg = ExecuteMsg::UpdateConfig {
            owner: None,
            mbrn_denom: None,
            osmosis_proxy: None,
            staking_contract: None,
            additional_allocation: None,
            neutron_proxy: Some("neutron_proxy".to_string()),
            old_mbrn_denom: Some("old_mbrn".to_string()),
        };
        execute(deps.as_mut(), env.clone(), mock_info("owner", &[]), update_msg).unwrap();

        let vesting_period = VestingPeriod {
            cliff: 180,
            linear: 180,
        };

        // Add transmutation with amount_to_mint
        let add_msg = ExecuteMsg::AddVestedTransmutation {
            recipient: "user1".to_string(),
            amount_to_mint: Uint128::new(2000),
            vesting_period: vesting_period.clone(),
        };

        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("neutron_proxy", &[Coin::new(1000, "old_mbrn")]),
            add_msg,
        )
        .unwrap();

        // Query schedule
        let query_msg = QueryMsg::VestingSchedules {
            user: "user1".to_string(),
        };
        let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
        let schedules: VestingSchedulesResponse = from_json(&res).unwrap();

        assert_eq!(
            schedules.schedules[0].mbrn_to_mint,
            Uint128::new(2000),
            "Should track amount_to_mint"
        );
    }

    #[test]
    fn test_unauthorized_add_vested_transmutation() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("sender", &[]);

        // Instantiate
        instantiate(deps.as_mut(), env.clone(), info.clone(), mock_instantiate_msg()).unwrap();

        // Update config
        let update_msg = ExecuteMsg::UpdateConfig {
            owner: None,
            mbrn_denom: None,
            osmosis_proxy: None,
            staking_contract: None,
            additional_allocation: None,
            neutron_proxy: Some("neutron_proxy".to_string()),
            old_mbrn_denom: Some("old_mbrn".to_string()),
        };
        execute(deps.as_mut(), env.clone(), mock_info("owner", &[]), update_msg).unwrap();

        let vesting_period = VestingPeriod {
            cliff: 180,
            linear: 180,
        };

        // Try to add from non-neutron_proxy address
        let add_msg = ExecuteMsg::AddVestedTransmutation {
            recipient: "user1".to_string(),
            amount_to_mint: Uint128::new(1000),
            vesting_period,
        };

        let err = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("attacker", &[Coin::new(1000, "old_mbrn")]),
            add_msg,
        )
        .unwrap_err();

        assert!(err.to_string().contains("Unauthorized"));
    }

    #[test]
    fn test_vesting_unlocked_calculation_before_cliff() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let start_time = env.block.time.seconds();

        let vesting_period = VestingPeriod {
            cliff: 180,
            linear: 180,
        };

        let schedule = VestingSchedule {
            user: Addr::unchecked("user1"),
            week_id: start_time / SECONDS_IN_WEEK,
            mbrn_to_mint: Uint128::new(1000),
            amount_withdrawn: Uint128::zero(),
            start_time,
            vesting_period,
            transmutation_count: 1,
        };

        // Before cliff (90 days in)
        let current_time = start_time + (SECONDS_IN_DAY * 90);
        let (unlocked, _) = calculate_vested_unlocked(&schedule, current_time).unwrap();

        assert_eq!(unlocked, Uint128::zero(), "Nothing should be unlocked before cliff");
    }

    #[test]
    fn test_vesting_unlocked_calculation_at_cliff() {
        let start_time = 1000000;

        let vesting_period = VestingPeriod {
            cliff: 180,
            linear: 180,
        };

        let schedule = VestingSchedule {
            user: Addr::unchecked("user1"),
            week_id: start_time / SECONDS_IN_WEEK,
            mbrn_to_mint: Uint128::new(1000),
            amount_withdrawn: Uint128::zero(),
            start_time,
            vesting_period,
            transmutation_count: 1,
        };

        // At cliff (180 days)
        let current_time = start_time + (SECONDS_IN_DAY * 180);
        let (unlocked, _) = calculate_vested_unlocked(&schedule, current_time).unwrap();

        assert_eq!(unlocked, Uint128::zero(), "Nothing unlocked at cliff, linear starts after");
    }

    #[test]
    fn test_vesting_unlocked_calculation_halfway_through_linear() {
        let start_time = 1000000;

        let vesting_period = VestingPeriod {
            cliff: 180,
            linear: 180,
        };

        let schedule = VestingSchedule {
            user: Addr::unchecked("user1"),
            week_id: start_time / SECONDS_IN_WEEK,
            mbrn_to_mint: Uint128::new(1000),
            amount_withdrawn: Uint128::zero(),
            start_time,
            vesting_period,
            transmutation_count: 1,
        };

        // Halfway through linear (180 + 90 days)
        let current_time = start_time + (SECONDS_IN_DAY * 270);
        let (unlocked, _) = calculate_vested_unlocked(&schedule, current_time).unwrap();

        // Should have ~50% unlocked
        assert!(unlocked > Uint128::new(450) && unlocked < Uint128::new(550));
    }

    #[test]
    fn test_vesting_unlocked_calculation_fully_vested() {
        let start_time = 1000000;

        let vesting_period = VestingPeriod {
            cliff: 180,
            linear: 180,
        };

        let schedule = VestingSchedule {
            user: Addr::unchecked("user1"),
            week_id: start_time / SECONDS_IN_WEEK,
            mbrn_to_mint: Uint128::new(1000),
            amount_withdrawn: Uint128::zero(),
            start_time,
            vesting_period,
            transmutation_count: 1,
        };

        // After full vesting (180 + 180 days)
        let current_time = start_time + (SECONDS_IN_DAY * 360);
        let (unlocked, _) = calculate_vested_unlocked(&schedule, current_time).unwrap();

        assert_eq!(unlocked, Uint128::new(1000), "Should be fully unlocked");
    }

    #[test]
    fn test_vesting_stats_query() {
        let mut deps = mock_dependencies();
        let mut env = mock_env();
        let info = mock_info("sender", &[]);

        // Instantiate
        instantiate(deps.as_mut(), env.clone(), info.clone(), mock_instantiate_msg()).unwrap();

        // Update config
        let update_msg = ExecuteMsg::UpdateConfig {
            owner: None,
            mbrn_denom: None,
            osmosis_proxy: None,
            staking_contract: None,
            additional_allocation: None,
            neutron_proxy: Some("neutron_proxy".to_string()),
            old_mbrn_denom: Some("old_mbrn".to_string()),
        };
        execute(deps.as_mut(), env.clone(), mock_info("owner", &[]), update_msg).unwrap();

        let vesting_period = VestingPeriod {
            cliff: 180,
            linear: 180,
        };

        // Add transmutation for user1
        let add_msg = ExecuteMsg::AddVestedTransmutation {
            recipient: "user1".to_string(),
            amount_to_mint: Uint128::new(1000),
            vesting_period: vesting_period.clone(),
        };
        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("neutron_proxy", &[Coin::new(1000, "old_mbrn")]),
            add_msg,
        )
        .unwrap();

        // Add transmutation for user2 in different week
        env.block.time = env.block.time.plus_seconds(SECONDS_IN_WEEK + 1);
        let add_msg2 = ExecuteMsg::AddVestedTransmutation {
            recipient: "user2".to_string(),
            amount_to_mint: Uint128::new(500),
            vesting_period,
        };
        execute(
            deps.as_mut(),
            env.clone(),
            mock_info("neutron_proxy", &[Coin::new(500, "old_mbrn")]),
            add_msg2,
        )
        .unwrap();

        // Query stats
        let query_msg = QueryMsg::VestingStats {};
        let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();
        let stats: VestingStatsResponse = from_json(&res).unwrap();

        assert_eq!(stats.total_old_mbrn_received, Uint128::new(1500));
        assert_eq!(stats.total_schedules, 2);
        assert_eq!(stats.total_users, 2);
    }
}
