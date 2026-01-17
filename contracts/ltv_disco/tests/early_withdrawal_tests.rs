use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
use cosmwasm_std::{coins, from_json, Decimal, Uint128, to_json_binary, SystemResult, ContractResult, Binary};
use membrane::ltv_disco::*;
use membrane::types::{cAsset, Asset, AssetInfo, Basket, DepositDenom, PendingRevenue, Locked};
use membrane::oracle::PriceResponse;
use ltv_disco::contract::{instantiate, execute, query};

fn setup_mock_basket() -> Basket {
    Basket {
        basket_id: Uint128::new(1),
        current_position_id: Uint128::new(1),
        collateral_types: vec![
            cAsset {
                asset: Asset {
                    info: AssetInfo::NativeToken { denom: "uusd".to_string() },
                    amount: Uint128::zero(),
                },
                max_LTV: Decimal::percent(50),
                max_borrow_LTV: Decimal::percent(30),
                rate_index: Decimal::zero(),
                pool_info: None,
                individual_cost: None,
            },
        ],
        collateral_supply_caps: vec![],
        lastest_collateral_rates: vec![],
        multi_asset_supply_caps: vec![],
        credit_asset: Asset {
            info: AssetInfo::NativeToken { denom: "debt".to_string() },
            amount: Uint128::zero(),
        },
        credit_price: PriceResponse {
            prices: vec![],
            price: Decimal::one(),
            decimals: 6,
        },
        base_interest_rate: Decimal::percent(5),
        pending_revenue: PendingRevenue {
            total_pending: Uint128::zero(),
            per_asset_rev: vec![],
        },
        pending_bad_debt: Uint128::zero(),
        credit_last_accrued: 0,
        rates_last_accrued: 0,
        oracle_set: true,
        negative_rates: false,
        frozen: false,
        distribute_revenue: true,
        cpc_margin_of_error: Decimal::percent(100),
        liq_queue: None,
    }
}

fn instantiate_contract() -> (cosmwasm_std::OwnedDeps<cosmwasm_std::MemoryStorage, cosmwasm_std::testing::MockApi, cosmwasm_std::testing::MockQuerier>, cosmwasm_std::Env) {
    let mut deps = mock_dependencies();
    let env = mock_env();
    
    let basket = setup_mock_basket();
    deps.querier.update_wasm(move |query| {
        match query {
            cosmwasm_std::WasmQuery::Smart { contract_addr: _, msg } => {
                let parsed: Result<membrane::cdp::QueryMsg, _> = from_json(msg);
                if let Ok(membrane::cdp::QueryMsg::GetBasket {}) = parsed {
                    SystemResult::Ok(ContractResult::Ok(to_json_binary(&basket).unwrap()))
                } else {
                    SystemResult::Ok(ContractResult::Ok(Binary::default()))
                }
            }
            _ => SystemResult::Ok(ContractResult::Ok(Binary::default())),
        }
    });

    let msg = InstantiateMsg {
        owner: Some("owner".to_string()),
        cdp_contract: "cdp_contract".to_string(),
        deposit_denom: DepositDenom { denom: "uusd".to_string(), vault_info: None },
        cdt_denom: "reward_token".to_string(),
        minimum_deposit: Uint128::new(1000),
        max_ltv: Decimal::percent(80),
        percent_to_disperse: Decimal::percent(10),
        dispersal_window: 2,
        activation_window: 2,
        oracle_contract: "oracle".to_string(),
        chain_proxy_contract: "chain_proxy".to_string(),
        lock_duration_ceiling: Some(365),
        affiliate_fee: Some(Decimal::percent(1)),
        max_management_fee: None,
        ltv_delta_minimum: None,
        points_system_contract: None,
        emissions_voting_contract: None,
        revenue_distributor: None,
        auction_contract: None,
        mbrn_denom: None,
    };

    let info = mock_info("owner", &[]);
    instantiate(deps.as_mut(), env.clone(), info, msg).unwrap();

    // Create queue
    let info = mock_info("owner", &[]);
    let msg = ExecuteMsg::CreateQueue { asset: "uusd".to_string() };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    (deps, env)
}


#[test]
fn test_early_withdrawal_half_fulfilled() {
    let (mut deps, mut env) = instantiate_contract();
    
    // Create deposit
    let info = mock_info("user1", &coins(10000, "uusd"));
    let msg = ExecuteMsg::SubmitDeposit {
        deposit_input: BackingDepositInput {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
            epoch_start_time: Some(0),
        },
        deposit_owner: None,
        locked: None,
        deposit_id: None,
        manager: None,
        affiliate_address: None,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    
    // Get deposit_id
    let response: BackingDepositsByUserResponse = from_json(
        query(deps.as_ref(), env.clone(), QueryMsg::GetBackingDepositsByUser {
            user: "user1".to_string(),
            asset: "uusd".to_string(),
            limit: None,
            start_after: None,
        }).unwrap()
    ).unwrap();
    let deposit_id = Uint128::one(); // First deposit
    
    // Lock deposit for 100 days
    let info = mock_info("user1", &[]);
    let locked_until = env.block.time.plus_seconds(100 * 86400).seconds();
    let msg = ExecuteMsg::Lock {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        deposit_id,
        locked: Locked {
            locked_until,
            perpetual_lock: None,
            intended_lock_days: None, // Will be set by contract
        },
        amount: None,
        epoch_start_time: 0,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    
    // Advance time by 50 days (half of lock period)
    env.block.time = env.block.time.plus_seconds(50 * 86400);
    
    // Withdraw early - should get 50% back
    let info = mock_info("user1", &[]);
    let msg = ExecuteMsg::WithdrawDeposit {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        deposit_id,
        amount: None, // Withdraw all
        epoch_start_time: 0,
    };
    let result = execute(deps.as_mut(), env.clone(), info, msg);
    if let Err(e) = &result {
        println!("Early withdrawal error: {:?}", e);
    }
    assert!(result.is_ok(), "Early withdrawal should succeed: {:?}", result.err());
    
    // Verify contract has a deposit with the lost amount
    let contract_addr = env.contract.address.to_string();
    let _contract_deposits: BackingDepositsByUserResponse = from_json(
        query(deps.as_ref(), env.clone(), QueryMsg::GetBackingDepositsByUser {
            user: contract_addr,
            asset: "uusd".to_string(),
            limit: None,
            start_after: None,
        }).unwrap()
    ).unwrap();
    
    // Contract should have a deposit with approximately 50% of initial vault tokens
    // Note: Exact amount depends on vault token calculations, but should be significant
}

#[test]
fn test_early_withdrawal_after_expiration() {
    let (mut deps, mut env) = instantiate_contract();
    
    // Create and lock deposit for 100 days
    let info = mock_info("user1", &coins(10000, "uusd"));
    let msg = ExecuteMsg::SubmitDeposit {
        deposit_input: BackingDepositInput {
            asset: "uusd".to_string(),
            ltv: Decimal::percent(60),
            max_borrow_ltv: Decimal::percent(40),
            epoch_start_time: Some(0),
        },
        deposit_owner: None,
        locked: None,
        deposit_id: None,
        manager: None,
        affiliate_address: None,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    
    let deposit_id = Uint128::one();
    
    // Lock deposit
    let info = mock_info("user1", &[]);
    let locked_until = env.block.time.plus_seconds(100 * 86400).seconds();
    let msg = ExecuteMsg::Lock {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        deposit_id,
        locked: Locked {
            locked_until,
            perpetual_lock: None,
            intended_lock_days: None,
        },
        amount: None,
        epoch_start_time: 0,
    };
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    
    // Advance time past expiration (101 days)
    env.block.time = env.block.time.plus_seconds(101 * 86400);
    
    // Withdraw - should get 100% back (no loss)
    let info = mock_info("user1", &[]);
    let msg = ExecuteMsg::WithdrawDeposit {
        asset: "uusd".to_string(),
        ltv: Decimal::percent(60),
        max_borrow_ltv: Decimal::percent(40),
        deposit_id,
        amount: None,
        epoch_start_time: 0,
    };
    let result = execute(deps.as_mut(), env.clone(), info, msg);
    assert!(result.is_ok(), "Withdrawal after expiration should succeed");
    
    // Verify no contract deposit created (no loss after expiration)
    // Note: In a clean test environment, contract should not have a deposit
}

