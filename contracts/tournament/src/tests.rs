use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{coins, from_json};

use crate::contract::{execute, instantiate, query};
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};

#[test]
fn test_instantiate() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    let msg = InstantiateMsg {
        admin: "creator".to_string(),
        race_engine: "race_engine".to_string(),
    };

    let res = instantiate(deps.as_mut(), env, info, msg).unwrap();
    assert_eq!(0, res.messages.len());
}

#[test]
fn test_start_tournament() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
        race_engine: "race_engine".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Start tournament
    let msg = ExecuteMsg::StartTournament {
        criteria: racing::types::TournamentCriteria::Random,
        track_id: "track_1".to_string(),
        max_participants: Some(8),
    };

    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    assert_eq!(0, res.messages.len());
    
    // Verify tournament was started
    let query_msg = QueryMsg::GetTournamentState {};
    let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
    let state_response: crate::msg::GetTournamentStateResponse = from_json(&res).unwrap();
    
    assert_eq!(state_response.status, racing::types::TournamentStatus::InProgress);
    assert_eq!(state_response.current_round, 1);
    assert_eq!(state_response.participants.len(), 8);
}

#[test]
fn test_start_tournament_with_different_criteria() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
        race_engine: "race_engine".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Test different criteria
    let criteria_list = vec![
        racing::types::TournamentCriteria::Random,
        racing::types::TournamentCriteria::TopTrained { min_training_updates: 10 },
        racing::types::TournamentCriteria::AllCars,
    ];

    for (i, criteria) in criteria_list.iter().enumerate() {
        let msg = ExecuteMsg::StartTournament {
            criteria: criteria.clone(),
            track_id: format!("track_{}", i + 1),
            max_participants: Some(4),
        };

        let res = execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();
        assert_eq!(0, res.messages.len());
    }
}

#[test]
fn test_run_next_round() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
        race_engine: "race_engine".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Start tournament
    let msg = ExecuteMsg::StartTournament {
        criteria: racing::types::TournamentCriteria::Random,
        track_id: "track_1".to_string(),
        max_participants: Some(4),
    };
    execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Run next round
    let msg = ExecuteMsg::RunNextRound {};
    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    assert_eq!(0, res.messages.len());
    
    // Verify round was advanced
    let query_msg = QueryMsg::GetTournamentState {};
    let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
    let state_response: crate::msg::GetTournamentStateResponse = from_json(&res).unwrap();
    
    assert_eq!(state_response.current_round, 2);
}

#[test]
fn test_end_tournament() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
        race_engine: "race_engine".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Start tournament
    let msg = ExecuteMsg::StartTournament {
        criteria: racing::types::TournamentCriteria::Random,
        track_id: "track_1".to_string(),
        max_participants: Some(2),
    };
    execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Run next round (final round for 2 participants)
    let msg = ExecuteMsg::RunNextRound {};
    execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // End tournament
    let msg = ExecuteMsg::EndTournament {};
    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    assert_eq!(0, res.messages.len());
    
    // Verify tournament was ended
    let query_msg = QueryMsg::GetTournamentState {};
    let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
    let state_response: crate::msg::GetTournamentStateResponse = from_json(&res).unwrap();
    
    assert_eq!(state_response.status, racing::types::TournamentStatus::Completed);
}

#[test]
fn test_query_current_bracket() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
        race_engine: "race_engine".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Start tournament
    let msg = ExecuteMsg::StartTournament {
        criteria: racing::types::TournamentCriteria::Random,
        track_id: "track_1".to_string(),
        max_participants: Some(4),
    };
    execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Query current bracket
    let query_msg = QueryMsg::GetCurrentBracket {};
    let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
    let bracket_response: crate::msg::GetCurrentBracketResponse = from_json(&res).unwrap();
    
    assert_eq!(bracket_response.round, 1);
    assert_eq!(bracket_response.participants.len(), 4);
    assert_eq!(bracket_response.matches.len(), 2); // 4 participants = 2 matches
}

#[test]
fn test_query_tournament_results() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
        race_engine: "race_engine".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Start tournament
    let msg = ExecuteMsg::StartTournament {
        criteria: racing::types::TournamentCriteria::Random,
        track_id: "track_1".to_string(),
        max_participants: Some(2),
    };
    execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Run next round (final round)
    let msg = ExecuteMsg::RunNextRound {};
    execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // End tournament
    let msg = ExecuteMsg::EndTournament {};
    execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Query tournament results
    let query_msg = QueryMsg::GetTournamentResults {};
    let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
    let results_response: crate::msg::GetTournamentResultsResponse = from_json(&res).unwrap();
    
    assert!(results_response.winner.is_some());
    assert_eq!(results_response.total_participants, 1); // Only winner in final rankings
}

#[test]
fn test_query_is_participant() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
        race_engine: "race_engine".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Start tournament
    let msg = ExecuteMsg::StartTournament {
        criteria: racing::types::TournamentCriteria::Random,
        track_id: "track_1".to_string(),
        max_participants: Some(4),
    };
    execute(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Query if car is participant
    let query_msg = QueryMsg::IsParticipant { car_id: "car_1".to_string() };
    let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
    let participant_response: crate::msg::IsParticipantResponse = from_json(&res).unwrap();
    
    assert!(participant_response.is_participant); // Should be true since car_1 is in mock participants
}

#[test]
fn test_tournament_with_large_participant_count() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("creator", &coins(1000, "earth"));

    // Instantiate
    let msg = InstantiateMsg {
        admin: "creator".to_string(),
        race_engine: "race_engine".to_string(),
    };
    instantiate(deps.as_mut(), env.clone(), info.clone(), msg).unwrap();

    // Start tournament with large participant count
    let msg = ExecuteMsg::StartTournament {
        criteria: racing::types::TournamentCriteria::Random,
        track_id: "track_1".to_string(),
        max_participants: Some(16),
    };

    let res = execute(deps.as_mut(), env, info, msg).unwrap();
    assert_eq!(0, res.messages.len());
    
    // Verify tournament was started with correct round calculation
    let query_msg = QueryMsg::GetTournamentState {};
    let res = query(deps.as_ref(), mock_env(), query_msg).unwrap();
    let state_response: crate::msg::GetTournamentStateResponse = from_json(&res).unwrap();
    
    assert_eq!(state_response.participants.len(), 16);
    assert_eq!(state_response.total_rounds, 4); // log2(16) = 4 rounds
}

// Integration tests using cw-multi-test
#[cfg(test)]
mod integration_tests {
    use super::*;
    use cosmwasm_std::Addr;
    use cw_multi_test::{App, AppBuilder, Contract, ContractWrapper, Executor};

    fn tournament_contract() -> Box<dyn Contract<cosmwasm_std::Empty>> {
        let contract = ContractWrapper::new(
            crate::contract::execute,
            crate::contract::instantiate,
            crate::contract::query,
        );
        Box::new(contract)
    }

    #[test]
    fn test_integration_tournament_lifecycle() {
        let mut app = AppBuilder::new().build(|router, _, storage| {
            router
                .bank
                .init_balance(storage, &Addr::unchecked("admin"), coins(1000, "earth"))
                .unwrap();
        });

        // Upload and instantiate tournament contract
        let tournament_contract_id = app.store_code(tournament_contract());
        let tournament_addr = app
            .instantiate_contract(
                tournament_contract_id,
                Addr::unchecked("admin"),
                &InstantiateMsg {
                    admin: "admin".to_string(),
                    race_engine: "race_engine".to_string(),
                },
                &[],
                "Tournament",
                None,
            )
            .unwrap();

        // Start tournament
        let start_msg = ExecuteMsg::StartTournament {
            criteria: racing::types::TournamentCriteria::Random,
            track_id: "track_1".to_string(),
            max_participants: Some(4),
        };

        let result = app
            .execute_contract(
                Addr::unchecked("admin"),
                tournament_addr.clone(),
                &start_msg,
                &[],
            )
            .unwrap();

        // Verify tournament start was successful
        assert!(result.events.iter().any(|event| {
            event.ty == "wasm" && event.attributes.iter().any(|attr| {
                attr.key == "method" && attr.value == "start_tournament"
            })
        }));

        // Query tournament state
        let state: crate::msg::GetTournamentStateResponse = app
            .wrap()
            .query_wasm_smart(&tournament_addr, &QueryMsg::GetTournamentState {})
            .unwrap();

        assert_eq!(state.status, racing::types::TournamentStatus::InProgress);
        assert_eq!(state.current_round, 1);
        assert_eq!(state.participants.len(), 4);

        // Run next round
        let run_round_msg = ExecuteMsg::RunNextRound {};

        let result = app
            .execute_contract(
                Addr::unchecked("admin"),
                tournament_addr.clone(),
                &run_round_msg,
                &[],
            )
            .unwrap();

        // Verify round was run successfully
        assert!(result.events.iter().any(|event| {
            event.ty == "wasm" && event.attributes.iter().any(|attr| {
                attr.key == "method" && attr.value == "run_next_round"
            })
        }));

        // Query updated tournament state
        let state: crate::msg::GetTournamentStateResponse = app
            .wrap()
            .query_wasm_smart(&tournament_addr, &QueryMsg::GetTournamentState {})
            .unwrap();

        assert_eq!(state.current_round, 2);
    }

    #[test]
    fn test_integration_tournament_with_different_criteria() {
        let mut app = AppBuilder::new().build(|router, _, storage| {
            router
                .bank
                .init_balance(storage, &Addr::unchecked("admin"), coins(1000, "earth"))
                .unwrap();
        });

        // Upload and instantiate tournament contract
        let tournament_contract_id = app.store_code(tournament_contract());
        let tournament_addr = app
            .instantiate_contract(
                tournament_contract_id,
                Addr::unchecked("admin"),
                &InstantiateMsg {
                    admin: "admin".to_string(),
                    race_engine: "race_engine".to_string(),
                },
                &[],
                "Tournament",
                None,
            )
            .unwrap();

        // Test different tournament criteria
        let criteria_list = vec![
            racing::types::TournamentCriteria::Random,
            racing::types::TournamentCriteria::TopTrained { min_training_updates: 10 },
            racing::types::TournamentCriteria::AllCars,
        ];

        for (i, criteria) in criteria_list.iter().enumerate() {
            let start_msg = ExecuteMsg::StartTournament {
                criteria: criteria.clone(),
                track_id: format!("track_{}", i + 1),
                max_participants: Some(4),
            };

            app.execute_contract(
                Addr::unchecked("admin"),
                tournament_addr.clone(),
                &start_msg,
                &[],
            )
            .unwrap();

            // Query tournament state to verify
            let state: crate::msg::GetTournamentStateResponse = app
                .wrap()
                .query_wasm_smart(&tournament_addr, &QueryMsg::GetTournamentState {})
                .unwrap();

            assert_eq!(state.status, racing::types::TournamentStatus::InProgress);
            assert_eq!(state.participants.len(), 4);
        }
    }

    #[test]
    fn test_integration_tournament_bracket_queries() {
        let mut app = AppBuilder::new().build(|router, _, storage| {
            router
                .bank
                .init_balance(storage, &Addr::unchecked("admin"), coins(1000, "earth"))
                .unwrap();
        });

        // Upload and instantiate tournament contract
        let tournament_contract_id = app.store_code(tournament_contract());
        let tournament_addr = app
            .instantiate_contract(
                tournament_contract_id,
                Addr::unchecked("admin"),
                &InstantiateMsg {
                    admin: "admin".to_string(),
                    race_engine: "race_engine".to_string(),
                },
                &[],
                "Tournament",
                None,
            )
            .unwrap();

        // Start tournament
        let start_msg = ExecuteMsg::StartTournament {
            criteria: racing::types::TournamentCriteria::Random,
            track_id: "track_1".to_string(),
            max_participants: Some(8),
        };

        app.execute_contract(
            Addr::unchecked("admin"),
            tournament_addr.clone(),
            &start_msg,
            &[],
        )
        .unwrap();

        // Query current bracket
        let bracket: crate::msg::GetCurrentBracketResponse = app
            .wrap()
            .query_wasm_smart(&tournament_addr, &QueryMsg::GetCurrentBracket {})
            .unwrap();

        assert_eq!(bracket.round, 1);
        assert_eq!(bracket.participants.len(), 8);
        assert_eq!(bracket.matches.len(), 4); // 8 participants = 4 matches

        // Query if specific car is participant
        let participant: crate::msg::IsParticipantResponse = app
            .wrap()
            .query_wasm_smart(
                &tournament_addr,
                &QueryMsg::IsParticipant { car_id: "car_1".to_string() }
            )
            .unwrap();

        assert!(participant.is_participant);
    }

    #[test]
    fn test_integration_tournament_completion() {
        let mut app = AppBuilder::new().build(|router, _, storage| {
            router
                .bank
                .init_balance(storage, &Addr::unchecked("admin"), coins(1000, "earth"))
                .unwrap();
        });

        // Upload and instantiate tournament contract
        let tournament_contract_id = app.store_code(tournament_contract());
        let tournament_addr = app
            .instantiate_contract(
                tournament_contract_id,
                Addr::unchecked("admin"),
                &InstantiateMsg {
                    admin: "admin".to_string(),
                    race_engine: "race_engine".to_string(),
                },
                &[],
                "Tournament",
                None,
            )
            .unwrap();

        // Start tournament with 2 participants (1 round)
        let start_msg = ExecuteMsg::StartTournament {
            criteria: racing::types::TournamentCriteria::Random,
            track_id: "track_1".to_string(),
            max_participants: Some(2),
        };

        app.execute_contract(
            Addr::unchecked("admin"),
            tournament_addr.clone(),
            &start_msg,
            &[],
        )
        .unwrap();

        // Run next round (final round)
        let run_round_msg = ExecuteMsg::RunNextRound {};

        app.execute_contract(
            Addr::unchecked("admin"),
            tournament_addr.clone(),
            &run_round_msg,
            &[],
        )
        .unwrap();

        // End tournament
        let end_msg = ExecuteMsg::EndTournament {};

        let result = app
            .execute_contract(
                Addr::unchecked("admin"),
                tournament_addr.clone(),
                &end_msg,
                &[],
            )
            .unwrap();

        // Verify tournament end was successful
        assert!(result.events.iter().any(|event| {
            event.ty == "wasm" && event.attributes.iter().any(|attr| {
                attr.key == "method" && attr.value == "end_tournament"
            })
        }));

        // Query tournament results
        let results: crate::msg::GetTournamentResultsResponse = app
            .wrap()
            .query_wasm_smart(&tournament_addr, &QueryMsg::GetTournamentResults {})
            .unwrap();

        assert!(results.winner.is_some());
        assert_eq!(results.total_participants, 1);

        // Query final tournament state
        let state: crate::msg::GetTournamentStateResponse = app
            .wrap()
            .query_wasm_smart(&tournament_addr, &QueryMsg::GetTournamentState {})
            .unwrap();

        assert_eq!(state.status, racing::types::TournamentStatus::Completed);
    }

    #[test]
    fn test_integration_error_handling() {
        let mut app = AppBuilder::new().build(|router, _, storage| {
            router
                .bank
                .init_balance(storage, &Addr::unchecked("admin"), coins(1000, "earth"))
                .unwrap();
        });

        // Upload and instantiate tournament contract
        let tournament_contract_id = app.store_code(tournament_contract());
        let tournament_addr = app
            .instantiate_contract(
                tournament_contract_id,
                Addr::unchecked("admin"),
                &InstantiateMsg {
                    admin: "admin".to_string(),
                    race_engine: "race_engine".to_string(),
                },
                &[],
                "Tournament",
                None,
            )
            .unwrap();

        // Try to run next round without starting tournament
        let run_round_msg = ExecuteMsg::RunNextRound {};

        let result = app.execute_contract(
            Addr::unchecked("admin"),
            tournament_addr.clone(),
            &run_round_msg,
            &[],
        );

        assert!(result.is_err()); // Should fail because no tournament is in progress

        // Try to end tournament without completing it
        let end_msg = ExecuteMsg::EndTournament {};

        let result = app.execute_contract(
            Addr::unchecked("admin"),
            tournament_addr.clone(),
            &end_msg,
            &[],
        );

        assert!(result.is_err()); // Should fail because tournament is not completed
    }
} 