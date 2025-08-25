#[cfg(test)]
mod energy_payment_tests {
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{coins, coin, from_json, Coin, Uint128};

    use crate::contract::{instantiate, execute, query};
    use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};

    fn inst(deps: &mut cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>) {
        let env = mock_env();
        let info = mock_info("creator", &[]);
        let _ = instantiate(deps.as_mut(), env, info, InstantiateMsg {
            name: "Cars".to_string(),
            symbol: "CAR".to_string(),
            payment_options: Some(vec![Coin{ denom: "uosmo".to_string(), amount: Uint128::new(10)}])
        }).unwrap();
    }

    #[test]
    fn paid_mint_sets_energy_and_query() {
        let mut deps = mock_dependencies();
        inst(&mut deps);
        let mut env = mock_env();

        // Paid mint
        let exec = ExecuteMsg::CreateCar {
            owner: Some("alice".to_string()),
            token_uri: None,
            extension: Some(racing::types::CarMetadata { name: "Alice Car".to_string(), image_data: None, attributes: None, car_id: None })
        };
        let info_payer = mock_info("alice", &coins(10, "uosmo"));
        let _ = execute(deps.as_mut(), env.clone(), info_payer, exec).unwrap();

        // Query energy
        let bin = query(deps.as_ref(), env.clone(), QueryMsg::GetCarInfo { token_id: "1".to_string() }).unwrap();
        let info: membrane::car::CarInfoResponse = from_json(bin).unwrap();
        assert!(info.current_energy > 0);

        // Race engine consume setup
        let _ = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("creator", &[]),
            ExecuteMsg::UpdateConfig { payment_options: None, new_owner: None, race_engine_contract: Some("engine".to_string()) }
        ).unwrap();

        // Consume one training session
        let _ = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("engine", &[]),
            ExecuteMsg::ConsumeTrainingEnergy { token_id: "1".to_string(), sessions: 1 }
        ).unwrap();

        // Verify energy dropped
        let after = query(deps.as_ref(), env.clone(), QueryMsg::GetCarInfo { token_id: "1".to_string() }).unwrap();
        let after_info: membrane::car::CarInfoResponse = from_json(after).unwrap();
        assert!(after_info.current_energy < info.current_energy);

        // Advance time to allow some regen and query again
        env.block.time = env.block.time.plus_seconds(60 * 60); // +1 hour
        let regen_bin = query(deps.as_ref(), env.clone(), QueryMsg::GetCarInfo { token_id: "1".to_string() }).unwrap();
        let regen_info: membrane::car::CarInfoResponse = from_json(regen_bin).unwrap();
        assert!(regen_info.current_energy >= after_info.current_energy);
    }

    #[test]
    fn training_payment_refills_energy_and_anyone_can_pay() {
        let mut deps = mock_dependencies();
        inst(&mut deps);
        let mut env = mock_env();

        // Paid mint
        let exec = ExecuteMsg::CreateCar {
            owner: Some("alice".to_string()),
            token_uri: None,
            extension: Some(racing::types::CarMetadata { name: "Alice Car".to_string(), image_data: None, attributes: None, car_id: None })
        };
        let _ = execute(deps.as_mut(), env.clone(), mock_info("alice", &coins(10, "uosmo")), exec).unwrap();

        // Configure race engine and reduce energy via consume
        let _ = execute(
            deps.as_mut(), env.clone(), mock_info("creator", &[]),
            ExecuteMsg::UpdateConfig { payment_options: None, new_owner: None, race_engine_contract: Some("engine".to_string()) }
        ).unwrap();
        let before = query(deps.as_ref(), env.clone(), QueryMsg::GetCarInfo { token_id: "1".to_string() }).unwrap();
        let before_info: membrane::car::CarInfoResponse = from_json(before).unwrap();
        let _ = execute(
            deps.as_mut(), env.clone(), mock_info("engine", &[]),
            ExecuteMsg::ConsumeTrainingEnergy { token_id: "1".to_string(), sessions: 2 }
        ).unwrap();
        let low = query(deps.as_ref(), env.clone(), QueryMsg::GetCarInfo { token_id: "1".to_string() }).unwrap();
        let low_info: membrane::car::CarInfoResponse = from_json(low).unwrap();
        assert!(low_info.current_energy <= before_info.current_energy);

        // Initially, training payments are empty -> refilling is free
        let _ = execute(
            deps.as_mut(), env.clone(), mock_info("payer", &[]),
            ExecuteMsg::PayForTraining { token_id: "1".to_string() }
        ).unwrap();

        // Owner sets training payment options
        let _ = execute(
            deps.as_mut(), env.clone(), mock_info("creator", &[]),
            ExecuteMsg::UpdateTrainingPayments { training_payment_options: vec![Coin { denom: "uosmo".to_string(), amount: Uint128::new(5)}] }
        ).unwrap();

        // Anyone can pay to refill when options configured (requires payment)
        let _ = execute(
            deps.as_mut(), env.clone(), mock_info("anyone", &vec![coin(5, "uosmo")]),
            ExecuteMsg::PayForTraining { token_id: "1".to_string() }
        ).unwrap();

        let full = query(deps.as_ref(), env.clone(), QueryMsg::GetCarInfo { token_id: "1".to_string() }).unwrap();
        let full_info: membrane::car::CarInfoResponse = from_json(full).unwrap();
        assert!(full_info.current_energy >= before_info.current_energy);
    }

    #[test]
    fn free_pending_finalize_sets_energy() {
        let mut deps = mock_dependencies();
        let env = mock_env();
        let info = mock_info("creator", &[]);
        let _ = instantiate(deps.as_mut(), env.clone(), info, InstantiateMsg {
            name: "Cars".to_string(), symbol: "CAR".to_string(), payment_options: Some(vec![Coin{ denom: "free".to_string(), amount: Uint128::new(1)}])
        }).unwrap();

        // Create pending free car
        let create = ExecuteMsg::CreateCar { owner: Some("alice".to_string()), token_uri: None, extension: Some(racing::types::CarMetadata { name: "FreeCar".to_string(), image_data: None, attributes: None, car_id: None }) };
        let _ = execute(deps.as_mut(), env.clone(), mock_info("alice", &[]), create).unwrap();

        // Add paid option and finalize by third-party payer
        let _ = execute(deps.as_mut(), env.clone(), mock_info("creator", &[]), ExecuteMsg::UpdateConfig { payment_options: Some(vec![Coin{ denom: "uosmo".to_string(), amount: Uint128::new(10)}]), new_owner: None, race_engine_contract: None }).unwrap();
        let _ = execute(deps.as_mut(), env.clone(), mock_info("payer", &coins(10, "uosmo")), ExecuteMsg::PayToFinalize { token_id: "1".to_string() }).unwrap();

        // Car has energy set
        let bin = query(deps.as_ref(), env.clone(), QueryMsg::GetCarInfo { token_id: "1".to_string() }).unwrap();
        let info: membrane::car::CarInfoResponse = from_json(bin).unwrap();
        assert!(info.current_energy > 0);
    }
}


