use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{coins, Uint128, Decimal};
use std::str::FromStr;
use membrane::osmosis_proxy::{ExecuteMsg, InstantiateMsg};
use membrane::types::{TransmutationPair, TransmutationPairEntry};

use crate::contract::{execute, instantiate};

#[test]
fn test_transmutation_basic_functionality() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("owner", &[]);

    // Instantiate contract
    let instantiate_msg = InstantiateMsg {};
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    // Set up transmutation pairs
    let transmutation_pairs = vec![TransmutationPairEntry {
        transmutation_pair: TransmutationPair {
            token_in: "factory/osmo1abc123/token1".to_string(),
            token_to_mint: "factory/osmo1abc123/token2".to_string(),
            mint_ratio: Decimal::from_str("1.5").unwrap(),
        },
        remove: false,
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
        restrict_mbrn_mints: None,
    };

    execute(deps.as_mut(), env.clone(), info.clone(), update_config_msg).unwrap();

    // Test transmutation with valid token
    let transmute_info = mock_info("user", &coins(1000, "factory/osmo1abc123/token1"));
    let transmute_msg = ExecuteMsg::TransmuteTokens {};
    let transmute_resp = execute(deps.as_mut(), env.clone(), transmute_info, transmute_msg).unwrap();

    // Verify transmutation response
    assert!(transmute_resp.messages.len() > 0);
    assert!(transmute_resp.attributes.iter().any(|attr| attr.key == "method" && attr.value == "transmute_tokens"));
    assert!(transmute_resp.attributes.iter().any(|attr| attr.key == "transmutation_pair"));
}

#[test]
fn test_transmutation_filtering_prevents_asset_theft() {
    let mut deps = mock_dependencies();
    let env = mock_env();
    let info = mock_info("owner", &[]);

    // Instantiate contract
    let instantiate_msg = InstantiateMsg {};
    instantiate(deps.as_mut(), env.clone(), info.clone(), instantiate_msg).unwrap();

    // Set up transmutation pairs
    let transmutation_pairs = vec![TransmutationPairEntry {
        transmutation_pair: TransmutationPair {
            token_in: "factory/osmo1abc123/token1".to_string(),
            token_to_mint: "factory/osmo1abc123/token2".to_string(),
            mint_ratio: Decimal::from_str("1.5").unwrap(),
        },
        remove: false,
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
        restrict_mbrn_mints: None,
    };

    execute(deps.as_mut(), env.clone(), info.clone(), update_config_msg).unwrap();

    // Test 1: Transmute tokens first
    let transmute_info = mock_info("user", &coins(1000, "factory/osmo1abc123/token1"));
    let transmute_msg = ExecuteMsg::TransmuteTokens {};
    let transmute_resp = execute(deps.as_mut(), env.clone(), transmute_info, transmute_msg).unwrap();
    
    // Verify transmutation was successful
    assert!(transmute_resp.messages.len() > 0);
    assert!(transmute_resp.attributes.iter().any(|attr| attr.key == "method" && attr.value == "transmute_tokens"));

    // Test 2: Attempt to swap the transmuted token (should be filtered out)
    let swap_info = mock_info("user", &coins(1000, "factory/osmo1abc123/token1"));
    let swap_msg = ExecuteMsg::ExecuteSwaps {
        max_slippage: Decimal::from_str("0.01").unwrap(),
        token_out: "uosmo".to_string(),
    };
    let swap_resp = execute(deps.as_mut(), env.clone(), swap_info, swap_msg).unwrap();

    // Verify that the transmuted token was filtered out
    let filtered_attr = swap_resp.attributes.iter()
        .find(|attr| attr.key == "filtered_transmutation_tokens")
        .unwrap();
    assert!(filtered_attr.value.contains("factory/osmo1abc123/token1"));
}



