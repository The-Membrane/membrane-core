use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{coins, Addr, Uint128, Decimal};
use std::str::FromStr;
use membrane::neutron_proxy::{ExecuteMsg, InstantiateMsg, TransmutationPair};
use membrane::types::Owner;

use crate::contract::{execute, instantiate};

fn mock_instantiate_msg() -> InstantiateMsg {
    InstantiateMsg {
        owners: vec![Owner {
            owner: Addr::unchecked("owner"),
            total_minted: Uint128::zero(),
            stability_pool_ratio: Some(Decimal::from_str("0.5").unwrap()),
            non_token_contract_auth: false,
            is_position_contract: false,
        }],
        liquidity_multiplier: Decimal::from_str("1.0").unwrap(),
        debt_auction: Some(Addr::unchecked("debt_auction")),
        positions_contract: Some(Addr::unchecked("positions")),
        liquidity_contract: Some(Addr::unchecked("liquidity")),
        oracle_contract: Some(Addr::unchecked("oracle")),
        transmutation_pairs: vec![],
    }
}

#[test]
fn test_transmutation_filtering_prevents_asset_theft() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("owner", &[]);

    // Instantiate contract
    let instantiate_msg = mock_instantiate_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    // Set up transmutation pairs
    let transmutation_pairs = vec![TransmutationPair {
        token_in: "factory/neutron1abc123/token1".to_string(),
        token_to_mint: "factory/neutron1abc123/token2".to_string(),
        mint_ratio: Decimal::from_str("1.5").unwrap(),
    }];

    let update_config_msg = ExecuteMsg::UpdateConfig {
        owners: None,
        liquidity_multiplier: None,
        debt_auction: None,
        positions_contract: None,
        liquidity_contract: None,
        oracle_contract: None,
        add_owner: None,
        edit_routes: None,
        transmutation_pairs: Some(transmutation_pairs),
    };

    execute(deps.as_mut(), env.clone(), info.clone(), update_config_msg).unwrap();

    // Test 1: Transmute tokens first
    let transmute_info = mock_info("user", &coins(1000, "factory/neutron1abc123/token1"));
    let transmute_msg = ExecuteMsg::TransmuteTokens {};
    let transmute_resp = execute(deps.as_mut(), env.clone(), transmute_info, transmute_msg).unwrap();
    
    // Verify transmutation was successful
    assert!(transmute_resp.messages.len() > 0);
    assert!(transmute_resp.attributes.iter().any(|attr| attr.key == "method" && attr.value == "transmute_tokens"));

    // Test 2: Attempt to swap the transmuted token (should be filtered out)
    let swap_info = mock_info("user", &coins(1000, "factory/neutron1abc123/token1"));
    let swap_msg = ExecuteMsg::ExecuteSwaps {
        max_slippage: Decimal::from_str("0.01").unwrap(),
        token_out: "untrn".to_string(),
    };
    let swap_resp = execute(deps.as_mut(), env.clone(), swap_info, swap_msg).unwrap();

    // Verify that the transmuted token was filtered out
    let filtered_attr = swap_resp.attributes.iter()
        .find(|attr| attr.key == "filtered_transmutation_tokens")
        .unwrap();
    assert!(filtered_attr.value.contains("factory/neutron1abc123/token1"));

    // Test 3: Attempt to swap with mixed tokens (transmuted + normal)
    let mixed_info = mock_info("user", &[
        coins(1000, "factory/neutron1abc123/token1"), // transmuted token
        coins(500, "untrn"), // normal token
    ].concat());
    let mixed_swap_msg = ExecuteMsg::ExecuteSwaps {
        max_slippage: Decimal::from_str("0.01").unwrap(),
        token_out: "untrn".to_string(),
    };
    let mixed_swap_resp = execute(deps.as_mut(), env.clone(), mixed_info, mixed_swap_msg).unwrap();

    // Verify only normal tokens were processed
    let filtered_attr = mixed_swap_resp.attributes.iter()
        .find(|attr| attr.key == "filtered_transmutation_tokens")
        .unwrap();
    assert!(filtered_attr.value.contains("factory/neutron1abc123/token1"));
    
    // Verify normal tokens were still processed
    assert!(mixed_swap_resp.attributes.iter().any(|attr| attr.key == "method" && attr.value == "execute_swaps"));
}

#[test]
fn test_transmutation_filtering_with_multiple_pairs() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("owner", &[]);

    // Instantiate contract
    let instantiate_msg = mock_instantiate_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    // Set up multiple transmutation pairs
    let transmutation_pairs = vec![
        TransmutationPair {
            token_in: "factory/neutron1abc123/token1".to_string(),
            token_to_mint: "factory/neutron1abc123/token2".to_string(),
            mint_ratio: Decimal::from_str("1.5").unwrap(),
        },
        TransmutationPair {
            token_in: "factory/neutron1abc123/token3".to_string(),
            token_to_mint: "factory/neutron1abc123/token4".to_string(),
            mint_ratio: Decimal::from_str("2.0").unwrap(),
        },
    ];

    let update_config_msg = ExecuteMsg::UpdateConfig {
        owners: None,
        liquidity_multiplier: None,
        debt_auction: None,
        positions_contract: None,
        liquidity_contract: None,
        oracle_contract: None,
        add_owner: None,
        edit_routes: None,
        transmutation_pairs: Some(transmutation_pairs),
    };

    execute(deps.as_mut(), env.clone(), info.clone(), update_config_msg).unwrap();

    // Test filtering with multiple transmuted tokens
    let multi_info = mock_info("user", &[
        coins(1000, "factory/neutron1abc123/token1"), // transmuted token 1
        coins(500, "factory/neutron1abc123/token3"),  // transmuted token 2
        coins(200, "untrn"), // normal token
    ].concat());
    let multi_swap_msg = ExecuteMsg::ExecuteSwaps {
        max_slippage: Decimal::from_str("0.01").unwrap(),
        token_out: "untrn".to_string(),
    };
    let multi_swap_resp = execute(deps.as_mut(), env.clone(), multi_info, multi_swap_msg).unwrap();

    // Verify both transmuted tokens were filtered out
    let filtered_attr = multi_swap_resp.attributes.iter()
        .find(|attr| attr.key == "filtered_transmutation_tokens")
        .unwrap();
    assert!(filtered_attr.value.contains("factory/neutron1abc123/token1"));
    assert!(filtered_attr.value.contains("factory/neutron1abc123/token3"));
    
    // Verify normal tokens were still processed
    assert!(multi_swap_resp.attributes.iter().any(|attr| attr.key == "method" && attr.value == "execute_swaps"));
}

#[test]
fn test_transmutation_filtering_edge_cases() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("owner", &[]);

    // Instantiate contract
    let instantiate_msg = mock_instantiate_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    // Test 1: All tokens are transmuted (should return error)
    let transmutation_pairs = vec![TransmutationPair {
        token_in: "factory/neutron1abc123/token1".to_string(),
        token_to_mint: "factory/neutron1abc123/token2".to_string(),
        mint_ratio: Decimal::from_str("1.5").unwrap(),
    }];

    let update_config_msg = ExecuteMsg::UpdateConfig {
        owners: None,
        liquidity_multiplier: None,
        debt_auction: None,
        positions_contract: None,
        liquidity_contract: None,
        oracle_contract: None,
        add_owner: None,
        edit_routes: None,
        transmutation_pairs: Some(transmutation_pairs),
    };

    execute(deps.as_mut(), env.clone(), info.clone(), update_config_msg).unwrap();

    let all_transmuted_info = mock_info("user", &coins(1000, "factory/neutron1abc123/token1"));
    let all_transmuted_msg = ExecuteMsg::ExecuteSwaps {
        max_slippage: Decimal::from_str("0.01").unwrap(),
        token_out: "untrn".to_string(),
    };
    let all_transmuted_resp = execute(deps.as_mut(), env.clone(), all_transmuted_info, all_transmuted_msg).unwrap();

    // Should return error when all tokens are filtered out
    assert!(all_transmuted_resp.attributes.iter().any(|attr| attr.key == "error" && attr.value.contains("ZeroAmount")));

    // Test 2: No transmuted tokens (should work normally)
    let normal_info = mock_info("user", &coins(1000, "untrn"));
    let normal_msg = ExecuteMsg::ExecuteSwaps {
        max_slippage: Decimal::from_str("0.01").unwrap(),
        token_out: "untrn".to_string(),
    };
    let normal_resp = execute(deps.as_mut(), env.clone(), normal_info, normal_msg).unwrap();

    // Should work normally
    assert!(normal_resp.attributes.iter().any(|attr| attr.key == "method" && attr.value == "execute_swaps"));
}

#[test]
fn test_transmutation_pair_removal() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("owner", &[]);

    // Instantiate contract
    let instantiate_msg = mock_instantiate_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    // Add transmutation pair
    let transmutation_pairs = vec![TransmutationPair {
        token_in: "factory/neutron1abc123/token1".to_string(),
        token_to_mint: "factory/neutron1abc123/token2".to_string(),
        mint_ratio: Decimal::from_str("1.5").unwrap(),
    }];

    let update_config_msg = ExecuteMsg::UpdateConfig {
        owners: None,
        liquidity_multiplier: None,
        debt_auction: None,
        positions_contract: None,
        liquidity_contract: None,
        oracle_contract: None,
        add_owner: None,
        edit_routes: None,
        transmutation_pairs: Some(transmutation_pairs),
    };

    execute(deps.as_mut(), env.clone(), info.clone(), update_config_msg).unwrap();

    // Verify token is filtered
    let filtered_info = mock_info("user", &coins(1000, "factory/neutron1abc123/token1"));
    let filtered_msg = ExecuteMsg::ExecuteSwaps {
        max_slippage: Decimal::from_str("0.01").unwrap(),
        token_out: "untrn".to_string(),
    };
    let filtered_resp = execute(deps.as_mut(), env.clone(), filtered_info, filtered_msg).unwrap();
    assert!(filtered_resp.attributes.iter().any(|attr| attr.key == "error" && attr.value.contains("ZeroAmount")));

    // Remove transmutation pair
    let remove_pairs: Vec<TransmutationPair> = vec![];

    let remove_config_msg = ExecuteMsg::UpdateConfig {
        owners: None,
        liquidity_multiplier: None,
        debt_auction: None,
        positions_contract: None,
        liquidity_contract: None,
        oracle_contract: None,
        add_owner: None,
        edit_routes: None,
        transmutation_pairs: Some(remove_pairs),
    };

    execute(deps.as_mut(), env.clone(), info.clone(), remove_config_msg).unwrap();

    // Verify token is no longer filtered
    let unfiltered_info = mock_info("user", &coins(1000, "factory/neutron1abc123/token1"));
    let unfiltered_msg = ExecuteMsg::ExecuteSwaps {
        max_slippage: Decimal::from_str("0.01").unwrap(),
        token_out: "untrn".to_string(),
    };
    let unfiltered_resp = execute(deps.as_mut(), env.clone(), unfiltered_info, unfiltered_msg).unwrap();
    assert!(unfiltered_resp.attributes.iter().any(|attr| attr.key == "method" && attr.value == "execute_swaps"));
}

#[test]
fn test_transmute_tokens_basic_functionality() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("owner", &[]);

    // Instantiate contract
    let instantiate_msg = mock_instantiate_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    // Set up transmutation pairs
    let transmutation_pairs = vec![TransmutationPair {
        token_in: "factory/neutron1abc123/token1".to_string(),
        token_to_mint: "factory/neutron1abc123/token2".to_string(),
        mint_ratio: Decimal::from_str("1.5").unwrap(),
    }];

    let update_config_msg = ExecuteMsg::UpdateConfig {
        owners: None,
        liquidity_multiplier: None,
        debt_auction: None,
        positions_contract: None,
        liquidity_contract: None,
        oracle_contract: None,
        add_owner: None,
        edit_routes: None,
        transmutation_pairs: Some(transmutation_pairs),
    };

    execute(deps.as_mut(), env.clone(), info.clone(), update_config_msg).unwrap();

    // Test transmutation with valid token
    let transmute_info = mock_info("user", &coins(1000, "factory/neutron1abc123/token1"));
    let transmute_msg = ExecuteMsg::TransmuteTokens {};
    let transmute_resp = execute(deps.as_mut(), env.clone(), transmute_info, transmute_msg).unwrap();

    // Verify transmutation response
    assert!(transmute_resp.messages.len() > 0);
    assert!(transmute_resp.attributes.iter().any(|attr| attr.key == "method" && attr.value == "transmute_tokens"));
    assert!(transmute_resp.attributes.iter().any(|attr| attr.key == "transmutation_pair"));
}

#[test]
fn test_transmute_tokens_invalid_token() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("owner", &[]);

    // Instantiate contract
    let instantiate_msg = mock_instantiate_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    // Set up transmutation pairs
    let transmutation_pairs = vec![TransmutationPair {
        token_in: "factory/neutron1abc123/token1".to_string(),
        token_to_mint: "factory/neutron1abc123/token2".to_string(),
        mint_ratio: Decimal::from_str("1.5").unwrap(),
    }];

    let update_config_msg = ExecuteMsg::UpdateConfig {
        owners: None,
        liquidity_multiplier: None,
        debt_auction: None,
        positions_contract: None,
        liquidity_contract: None,
        oracle_contract: None,
        add_owner: None,
        edit_routes: None,
        transmutation_pairs: Some(transmutation_pairs),
    };

    execute(deps.as_mut(), env.clone(), info.clone(), update_config_msg).unwrap();

    // Test transmutation with invalid token
    let transmute_info = mock_info("user", &coins(1000, "factory/neutron1abc123/invalid"));
    let transmute_msg = ExecuteMsg::TransmuteTokens {};
    let transmute_resp = execute(deps.as_mut(), env.clone(), transmute_info, transmute_msg).unwrap();

    // Verify error response
    assert!(transmute_resp.attributes.iter().any(|attr| attr.key == "error" && attr.value.contains("Invalid transmutation token")));
}

#[test]
fn test_transmute_tokens_multiple_assets() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("owner", &[]);

    // Instantiate contract
    let instantiate_msg = mock_instantiate_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    // Set up transmutation pairs
    let transmutation_pairs = vec![TransmutationPair {
        token_in: "factory/neutron1abc123/token1".to_string(),
        token_to_mint: "factory/neutron1abc123/token2".to_string(),
        mint_ratio: Decimal::from_str("1.5").unwrap(),
    }];

    let update_config_msg = ExecuteMsg::UpdateConfig {
        owners: None,
        liquidity_multiplier: None,
        debt_auction: None,
        positions_contract: None,
        liquidity_contract: None,
        oracle_contract: None,
        add_owner: None,
        edit_routes: None,
        transmutation_pairs: Some(transmutation_pairs),
    };

    execute(deps.as_mut(), env.clone(), info.clone(), update_config_msg).unwrap();

    // Test transmutation with multiple assets (should fail)
    let transmute_info = mock_info("user", &[
        coins(1000, "factory/neutron1abc123/token1"),
        coins(500, "untrn"),
    ].concat());
    let transmute_msg = ExecuteMsg::TransmuteTokens {};
    let transmute_resp = execute(deps.as_mut(), env.clone(), transmute_info, transmute_msg).unwrap();

    // Verify error response
    assert!(transmute_resp.attributes.iter().any(|attr| attr.key == "error" && attr.value.contains("Invalid number of assets")));
}

#[test]
fn test_transmutation_pair_update() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("owner", &[]);

    // Instantiate contract
    let instantiate_msg = mock_instantiate_msg();
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    // Add initial transmutation pair
    let transmutation_pairs = vec![TransmutationPair {
        token_in: "factory/neutron1abc123/token1".to_string(),
        token_to_mint: "factory/neutron1abc123/token2".to_string(),
        mint_ratio: Decimal::from_str("1.5").unwrap(),
    }];

    let update_config_msg = ExecuteMsg::UpdateConfig {
        owners: None,
        liquidity_multiplier: None,
        debt_auction: None,
        positions_contract: None,
        liquidity_contract: None,
        oracle_contract: None,
        add_owner: None,
        edit_routes: None,
        transmutation_pairs: Some(transmutation_pairs),
    };

    execute(deps.as_mut(), env.clone(), info.clone(), update_config_msg).unwrap();

    // Update transmutation pair with new ratio
    let updated_pairs = vec![TransmutationPair {
        token_in: "factory/neutron1abc123/token1".to_string(),
        token_to_mint: "factory/neutron1abc123/token2".to_string(),
        mint_ratio: Decimal::from_str("2.0").unwrap(),
    }];

    let update_config_msg = ExecuteMsg::UpdateConfig {
        owners: None,
        liquidity_multiplier: None,
        debt_auction: None,
        positions_contract: None,
        liquidity_contract: None,
        oracle_contract: None,
        add_owner: None,
        edit_routes: None,
        transmutation_pairs: Some(updated_pairs),
    };

    execute(deps.as_mut(), env.clone(), info.clone(), update_config_msg).unwrap();

    // Test transmutation with updated ratio
    let transmute_info = mock_info("user", &coins(1000, "factory/neutron1abc123/token1"));
    let transmute_msg = ExecuteMsg::TransmuteTokens {};
    let transmute_resp = execute(deps.as_mut(), env.clone(), transmute_info, transmute_msg).unwrap();

    // Verify transmutation response
    assert!(transmute_resp.messages.len() > 0);
    assert!(transmute_resp.attributes.iter().any(|attr| attr.key == "method" && attr.value == "transmute_tokens"));
}
