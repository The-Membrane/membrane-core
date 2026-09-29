mod test_helpers;

use cosmwasm_std::testing::{mock_env, mock_info};
use cosmwasm_std::{coins, from_json, Decimal, Uint128};
use membrane::ltv_disco::{ExecuteMsg, InstantiateMsg, QueryMsg, AssetsResponse, Config};
use membrane::types::DepositDenom;
use ltv_disco::contract::{execute, instantiate, query};
use ltv_disco::state::CONFIG;
use test_helpers::*;

// =================== Instantiation ===================

#[test]
fn test_instantiate_default_config() {
    let mut deps = setup_deps();
    let config = setup_contract(&mut deps);
    assert_eq!(config.owner.as_str(), addr("owner"));
    assert_eq!(config.cdp_contract.as_str(), addr("cdp_contract"));
    assert_eq!(config.cdt_denom, CDT);
    assert_eq!(config.minimum_deposit, Uint128::new(1000));
    assert_eq!(config.unstaking_period, 172800);
    assert_eq!(config.affiliate_fee, Decimal::percent(1));
    assert_eq!(config.max_management_fee, Decimal::percent(5));
    assert!(config.auction_contract.is_some());
}

#[test]
fn test_instantiate_sender_as_owner() {
    let mut deps = setup_deps();
    let mut msg = standard_instantiate_msg();
    msg.owner = None;
    let info = mock_info(&addr("sender_owner"), &[]);
    instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
    let config = CONFIG.load(&deps.storage).unwrap();
    assert_eq!(config.owner.as_str(), addr("sender_owner"));
}

#[test]
fn test_instantiate_defaults_applied() {
    let mut deps = setup_deps();
    let msg = InstantiateMsg {
        owner: Some(addr("owner")),
        cdp_contract: addr("cdp_contract"),
        deposit_denom: DepositDenom { denom: COLLATERAL.to_string(), vault_info: None },
        cdt_denom: CDT.to_string(),
        minimum_deposit: Uint128::new(1000),
        unstaking_period: None, // Should default to 172800
        oracle_contract: addr("oracle"),
        chain_proxy_contract: addr("chain_proxy"),
        emissions_voting_contract: None,
        affiliate_fee: None, // Should default to 1%
        max_management_fee: None, // Should default to 5%
        points_system_contract: None,
        revenue_distributor: None,
        auction_contract: None,
        mbrn_denom: None,
        liquidation_engine_contract: None,
    };
    let info = mock_info(&addr("creator"), &[]);
    instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
    let config = CONFIG.load(&deps.storage).unwrap();
    assert_eq!(config.unstaking_period, 172800);
    assert_eq!(config.affiliate_fee, Decimal::percent(1));
    assert_eq!(config.max_management_fee, Decimal::percent(5));
}

#[test]
fn test_instantiate_rejects_high_management_fee() {
    let mut deps = setup_deps();
    let mut msg = standard_instantiate_msg();
    msg.max_management_fee = Some(Decimal::percent(101)); // > 100%
    let info = mock_info(&addr("creator"), &[]);
    let err = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap_err();
    assert!(err.to_string().contains("max_management_fee"));
}

// =================== Queue Creation ===================

#[test]
fn test_create_queue_success() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let res = query_queue(&deps, COLLATERAL);
    assert_eq!(res.queues.len(), 1);
    let (asset, queue) = &res.queues[0];
    assert_eq!(asset, COLLATERAL);
    // 50%-90% = 41 slots at 1% intervals
    assert_eq!(queue.slots.len(), 41);
    assert_eq!(queue.current_deposit_id, Uint128::zero());
    assert_eq!(queue.min_ltv, Decimal::percent(50));
    assert_eq!(queue.max_ltv, Decimal::percent(90));

    // Verify slots are sorted descending by max_ltv
    for (i, slot) in queue.slots.iter().enumerate() {
        assert_eq!(slot.max_ltv, Decimal::percent(90 - i as u64));
        assert!(slot.total_deposit_tokens.is_zero());
        assert!(slot.total_vault_tokens.is_zero());
        assert!(slot.bad_debt.is_zero());
    }
}

#[test]
fn test_create_queue_unauthorized() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    let info = mock_info(&addr("not_owner"), &[]);
    let err = execute(
        deps.as_mut(), mock_env(), info,
        ExecuteMsg::CreateQueue {
            asset: COLLATERAL.to_string(),
            min_ltv: Decimal::percent(50),
            max_ltv: Decimal::percent(90),
        },
    ).unwrap_err();
    assert_eq!(err.to_string(), "Unauthorized");
}

#[test]
fn test_create_queue_duplicate_rejected() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let info = mock_info(&addr("owner"), &[]);
    let err = execute(
        deps.as_mut(), mock_env(), info,
        ExecuteMsg::CreateQueue {
            asset: COLLATERAL.to_string(),
            min_ltv: Decimal::percent(50),
            max_ltv: Decimal::percent(90),
        },
    ).unwrap_err();
    assert!(err.to_string().contains("Queue already exists"));
}

#[test]
fn test_create_multiple_queues() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, "atom");
    create_queue(&mut deps, "osmo");
    create_queue(&mut deps, "usdc");

    let res: AssetsResponse = from_json(query(
        deps.as_ref(), mock_env(),
        QueryMsg::GetAssets {},
    ).unwrap()).unwrap();
    assert_eq!(res.assets.len(), 3);
}

#[test]
fn test_update_queue_unauthorized() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);
    create_queue(&mut deps, COLLATERAL);

    let info = mock_info(&addr("not_owner"), &[]);
    let err = execute(
        deps.as_mut(), mock_env(), info,
        ExecuteMsg::UpdateQueue {
            asset: COLLATERAL.to_string(),
            min_ltv: None,
            max_ltv: None,
        },
    ).unwrap_err();
    assert_eq!(err.to_string(), "Unauthorized");
}

// =================== Config Update ===================

#[test]
fn test_update_config_success() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);

    let info = mock_info(&addr("owner"), &[]);
    execute(
        deps.as_mut(), mock_env(), info,
        ExecuteMsg::UpdateConfig {
            owner: Some(addr("new_owner")),
            cdp_contract: None,
            deposit_denom: None,
            cdt_denom: None,
            minimum_deposit: Some(Uint128::new(5000)),
            unstaking_period: Some(86400),
            oracle_contract: None,
            chain_proxy_contract: None,
            emissions_voting_contract: None,
            affiliate_fee: None,
            max_management_fee: None,
            points_system_contract: None,
            revenue_distributor: None,
            auction_contract: None,
            mbrn_denom: None,
            liquidation_engine_contract: None,
        },
    ).unwrap();

    let config: Config = from_json(query(deps.as_ref(), mock_env(), QueryMsg::Config {}).unwrap()).unwrap();
    assert_eq!(config.owner.as_str(), addr("new_owner"));
    assert_eq!(config.minimum_deposit, Uint128::new(5000));
    assert_eq!(config.unstaking_period, 86400);
}

#[test]
fn test_update_config_unauthorized() {
    let mut deps = setup_deps();
    setup_contract(&mut deps);

    let info = mock_info(&addr("not_owner"), &[]);
    let err = execute(
        deps.as_mut(), mock_env(), info,
        ExecuteMsg::UpdateConfig {
            owner: Some(addr("hijack")),
            cdp_contract: None, deposit_denom: None, cdt_denom: None,
            minimum_deposit: None, unstaking_period: None,
            oracle_contract: None, chain_proxy_contract: None,
            emissions_voting_contract: None, affiliate_fee: None,
            max_management_fee: None, points_system_contract: None,
            revenue_distributor: None, auction_contract: None, mbrn_denom: None,
            liquidation_engine_contract: None,
        },
    ).unwrap_err();
    assert_eq!(err.to_string(), "Unauthorized");
}
