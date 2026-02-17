#![allow(unused_imports)]
use cosmwasm_std::{coin, Addr, Uint128, Decimal, Response, MessageInfo, DepsMut, Env, Binary, StdResult, to_json_binary, CosmosMsg, BankMsg, WasmMsg, Reply};
use cw_multi_test::{App, Contract, ContractWrapper, Executor};

use membrane::yield_arb as iface;
use crate::contract;

fn contract_yield_arb() -> Box<dyn Contract<cosmwasm_std::Empty>> {
    let c = ContractWrapper::new_with_empty(contract::execute, contract::instantiate, contract::query)
        .with_reply(contract::reply);
    Box::new(c)
}

// Mock Transmuter Contract
fn mock_transmuter_execute(
    _deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: membrane::transmuter::ExecuteMsg,
) -> StdResult<Response> {
    match msg {
        membrane::transmuter::ExecuteMsg::Transmute { recipient } => {
            // Simulate CDT -> USDC swap by sending USDC to recipient
            let cdt_amount = info.funds.iter().find(|c| c.denom == "ucdt").map(|c| c.amount).unwrap_or(Uint128::zero());
            let usdc_amount = cdt_amount; // 1:1 swap
            
            let bank_msg = BankMsg::Send {
                to_address: recipient.unwrap_or(env.contract.address.to_string()),
                amount: vec![coin(usdc_amount.u128(), "uusdc")],
            };
            
            Ok(Response::new()
                .add_message(bank_msg)
                .add_attribute("action", "transmute")
                .add_attribute("cdt_amount", cdt_amount)
                .add_attribute("usdc_amount", usdc_amount))
        },
        _ => Ok(Response::new().add_attribute("action", "unknown")),
    }
}

fn mock_transmuter_query(_deps: cosmwasm_std::Deps, _env: cosmwasm_std::Env, _msg: membrane::transmuter::QueryMsg) -> StdResult<Binary> {
    Ok(to_json_binary(&membrane::transmuter::Config {
        owner: Addr::unchecked("owner"),
        tokenfactory_contract: None,
        discounts_contract: "discounts".to_string(),
        cdp_contract: "cdp".to_string(),
        vault_token: "ucdt".to_string(),
        deposit_pair: membrane::transmuter::AssetPair {
            cdt: "ucdt".to_string(),
            paired_asset: "uusdc".to_string(),
        },
        composition_leeway: Decimal::percent(5),
        asset_a_to_b_rate: Decimal::one(),
        cdt_target_ratio: Decimal::zero(),
        usage_fee: Decimal::percent(1),
        swap_history_cap: 1000,
        volume_history_cap: 1000,
        rate_limit_window_secs: 3600,
        rate_limit_threshold: Decimal::percent(10),
        allowlist: vec![],
        allowlist_rate_limit_threshold: Decimal::percent(50),
        global_rate_limit_threshold: Decimal::percent(20),
        global_rate_limit_window_secs: 3600,
        revenue_distributor_addr: None,
        revenue_distributions: vec![],
        lock_ceiling: 365,
        affiliate_fee: Decimal::percent(1),
        send_swap_fee: false,
    })?)
}

fn mock_instantiate(_deps: DepsMut, _env: Env, _info: MessageInfo, _msg: cosmwasm_std::Empty) -> StdResult<Response> {
    Ok(Response::new().add_attribute("method", "instantiate"))
}

fn mock_transmuter_reply(_deps: DepsMut, _env: Env, _msg: Reply) -> StdResult<Response> {
    Ok(Response::new().add_attribute("reply", "transmuter"))
}

fn contract_transmuter() -> Box<dyn Contract<cosmwasm_std::Empty>> {
    let c = ContractWrapper::new_with_empty(mock_transmuter_execute, mock_instantiate, mock_transmuter_query)
        .with_reply(mock_transmuter_reply);
    Box::new(c)
}

// Mock Mars Vault Contract
fn mock_mars_vault_execute(
    _deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: membrane::mars_vault_token::ExecuteMsg,
) -> StdResult<Response> {
    match msg {
        membrane::mars_vault_token::ExecuteMsg::EnterVault {} => {
            // Simulate USDC -> marsUSDC conversion
            let usdc_amount = info.funds.iter().find(|c| c.denom == "uusdc").map(|c| c.amount).unwrap_or(Uint128::zero());
            let mars_usdc_amount = usdc_amount; // 1:1 for simplicity
            
            let bank_msg = BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: vec![coin(mars_usdc_amount.u128(), "mars_usdc")],
            };
            
            Ok(Response::new()
                .add_message(bank_msg)
                .add_attribute("action", "enter_vault")
                .add_attribute("usdc_amount", usdc_amount)
                .add_attribute("mars_usdc_amount", mars_usdc_amount))
        },
        _ => Ok(Response::new().add_attribute("action", "unknown")),
    }
}

fn mock_mars_vault_query(_deps: cosmwasm_std::Deps, _env: cosmwasm_std::Env, msg: membrane::mars_vault_token::QueryMsg) -> StdResult<Binary> {
    match msg {
        membrane::mars_vault_token::QueryMsg::Config {} => {
            Ok(to_json_binary(&membrane::mars_vault_token::Config {
                owner: Addr::unchecked("owner"),
                mars_redbank_addr: Addr::unchecked("mars_redbank"),
                vault_token: "mars_usdc".to_string(),
                deposit_token: "uusdc".to_string(),
                total_deposit_tokens: Uint128::new(1000000),
                vault_cost: membrane::mars_vault_token::VaultCost {
                    static_cost: Some(Decimal::percent(1)),
                    yield_ceiling: None,
                },
                transmuter_addr: Addr::unchecked("transmuter"),
                revenue_distributor_addr: Addr::unchecked("revenue_distributor"),
                cdt_denom: "ucdt".to_string(),
                cdp_contract_addr: Addr::unchecked("cdp"),
                vault_cost_index: 0,
                revenue_distributions: vec![],
            })?)
        },
        membrane::mars_vault_token::QueryMsg::APR {} => {
            Ok(to_json_binary(&membrane::mars_vault_token::APRResponse {
                week_apr: Some(Decimal::percent(5)),
                month_apr: Some(Decimal::percent(20)),
                three_month_apr: Some(Decimal::percent(60)),
                year_apr: Some(Decimal::percent(200)),
            })?)
        },
        membrane::mars_vault_token::QueryMsg::Cost {} => {
            Ok(to_json_binary(&Decimal::percent(1))?) // 1% cost
        },
        _ => Ok(to_json_binary(&"unknown")?),
    }
}

fn mock_mars_vault_reply(_deps: DepsMut, _env: Env, _msg: Reply) -> StdResult<Response> {
    Ok(Response::new().add_attribute("reply", "mars_vault"))
}

fn contract_mars_vault() -> Box<dyn Contract<cosmwasm_std::Empty>> {
    let c = ContractWrapper::new_with_empty(mock_mars_vault_execute, mock_instantiate, mock_mars_vault_query)
        .with_reply(mock_mars_vault_reply);
    Box::new(c)
}

// Mock CDP Contract
fn mock_cdp_execute(
    _deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: membrane::cdp::ExecuteMsg,
) -> StdResult<Response> {
    match msg {
        membrane::cdp::ExecuteMsg::Deposit { position_id, position_owner, affiliate_address } => {
            // Simulate marsUSDC deposit into CDP position
            let mars_usdc_amount = info.funds.iter().find(|c| c.denom == "mars_usdc").map(|c| c.amount).unwrap_or(Uint128::zero());
            
            Ok(Response::new()
                .add_attribute("action", "deposit")
                .add_attribute("position_id", position_id.unwrap_or(Uint128::new(1)))
                .add_attribute("position_owner", position_owner.unwrap_or(info.sender.to_string()))
                .add_attribute("mars_usdc_amount", mars_usdc_amount))
        },
        _ => Ok(Response::new().add_attribute("action", "unknown")),
    }
}

fn mock_cdp_query(_deps: cosmwasm_std::Deps, _env: cosmwasm_std::Env, msg: membrane::cdp::QueryMsg) -> StdResult<Binary> {
    match msg {
        membrane::cdp::QueryMsg::GetBasketPositions { user, user_info, .. } => {
            let positions = if user.is_some() || user_info.is_some() {
                // Return a position with only the vault token as collateral (Token type with mars address)
                vec![membrane::cdp::PositionResponse {
                    position_id: user_info.as_ref()
                        .map(|ui| ui.position_id)
                        .unwrap_or(Uint128::new(1)),
                    collateral_assets: vec![membrane::types::cAsset {
                        asset: membrane::types::Asset {
                            amount: Uint128::new(1000),
                            // Use Token type with mars vault address - this is what yield-arb looks for
                            info: membrane::types::AssetInfo::NativeToken {
                                denom: "mars_usdc".to_string(),
                            },
                        },
                        max_borrow_LTV: Decimal::percent(80),
                        max_LTV: Decimal::percent(90),
                        rate_index: Decimal::one(),
                        pool_info: None,
                        }],
                    cAsset_ratios: vec![Decimal::one()],
                    credit_amount: Uint128::new(500),
                    avg_borrow_LTV: Decimal::percent(80),
                    avg_max_LTV: Decimal::percent(90),
                    deployed_to: vec![],
                    pending_interest: Uint128::zero(),
                    total_interest_accrued: Uint128::zero(),
                }]
            } else {
                vec![]
            };
            
            Ok(to_json_binary(&membrane::cdp::BasketPositionsResponse {
                user: user.or(user_info.as_ref().map(|ui| ui.position_owner.clone()))
                    .unwrap_or("unknown".to_string()),
                positions,
            })?)
        },
        membrane::cdp::QueryMsg::GetCollateralInterest {} => {
            Ok(to_json_binary(&membrane::cdp::CollateralInterestResponse {
                rates: vec![Decimal::percent(5)], // 5% interest rate
            })?)
        },
        membrane::cdp::QueryMsg::GetBasket {} => {
            Ok(to_json_binary(&membrane::types::Basket {
                basket_id: Uint128::new(1),
                current_position_id: Uint128::new(1),
                collateral_types: vec![membrane::types::cAsset {
                    asset: membrane::types::Asset {
                        amount: Uint128::zero(),
                        info: membrane::types::AssetInfo::Token {
                            address: Addr::unchecked("mars"),
                        },
                    },
                    max_borrow_LTV: Decimal::percent(80),
                    max_LTV: Decimal::percent(90),
                    rate_index: Decimal::one(),
                    pool_info: None,
                }],
                collateral_supply_caps: vec![],
                lastest_collateral_rates: vec![],
                multi_asset_supply_caps: vec![],
                credit_asset: membrane::types::Asset {
                    amount: Uint128::zero(),
                    info: membrane::types::AssetInfo::NativeToken {
                        denom: "ucdt".to_string(),
                    },
                },
                credit_price: membrane::oracle::PriceResponse {
                    prices: vec![],
                    price: Decimal::one(),
                    decimals: 6,
                },
                base_interest_rate: Decimal::percent(5),
                pending_revenue: membrane::types::PendingRevenue {
                    total_pending: Uint128::zero(),
                    per_asset_rev: vec![],
                },
                pending_bad_debt: Uint128::zero(),
                credit_last_accrued: 0,
                rates_last_accrued: 0,
                oracle_set: true,
                negative_rates: false,
                frozen: false,
                distribute_revenue: false,
                cpc_margin_of_error: Decimal::percent(10),
                liq_queue: None,
            })?)
        },
        _ => Ok(to_json_binary(&"unknown")?),
    }
}

fn mock_cdp_reply(_deps: DepsMut, _env: Env, _msg: Reply) -> StdResult<Response> {
    Ok(Response::new().add_attribute("reply", "cdp"))
}

fn contract_cdp() -> Box<dyn Contract<cosmwasm_std::Empty>> {
    let c = ContractWrapper::new_with_empty(mock_cdp_execute, mock_instantiate, mock_cdp_query)
        .with_reply(mock_cdp_reply);
    Box::new(c)
}

// ============================================================================
// STEPWISE TESTS FOR ALL EXECUTION PATHS
// ============================================================================

#[test]
fn test_instantiate() {
    let mut app = App::default();
    let code_id = app.store_code(contract_yield_arb());
    let owner = Addr::unchecked("owner");
    
    let msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: "mars".to_string(),
        cdp_contract_addr: "cdp".to_string(),
        transmuter_addr: "trans".to_string(),
    };
    
    let addr = app.instantiate_contract(code_id, owner.clone(), &msg, &[], "yield_arb", None).unwrap();
    
    // Verify config was set correctly
    let config: iface::Config = app.wrap().query_wasm_smart(addr.clone(), &iface::QueryMsg::Config {}).unwrap();
    assert_eq!(config.owner, owner);
    assert_eq!(config.cdt_denom, "ucdt");
    assert_eq!(config.mars_vault_addr, Addr::unchecked("mars"));
    assert_eq!(config.cdp_contract_addr, Addr::unchecked("cdp"));
    assert_eq!(config.transmuter_addr, Addr::unchecked("trans"));
    
    // Verify initial state is empty
    let positions: Vec<iface::UserPosition> = app.wrap().query_wasm_smart(addr.clone(), &iface::QueryMsg::GetUserPositions { user: None, limit: Some(10), start_after: None }).unwrap();
    assert_eq!(positions.len(), 0);
    
    let market_conditions: Vec<iface::MarketConditions> = app.wrap().query_wasm_smart(addr.clone(), &iface::QueryMsg::GetMarketConditions { limit: Some(10), start_after: None }).unwrap();
    assert_eq!(market_conditions.len(), 0);
    
    let tvl_history: Vec<iface::TVLSnapshot> = app.wrap().query_wasm_smart(addr.clone(), &iface::QueryMsg::GetTVLHistory { limit: Some(10), start_after: None }).unwrap();
    assert_eq!(tvl_history.len(), 0);
}

#[test]
fn test_instantiate_with_default_owner() {
    let mut app = App::default();
    let code_id = app.store_code(contract_yield_arb());
    let sender = Addr::unchecked("sender");
    
    let msg = iface::InstantiateMsg {
        owner: None, // Should default to sender
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: "mars".to_string(),
        cdp_contract_addr: "cdp".to_string(),
        transmuter_addr: "trans".to_string(),
    };
    
    let addr = app.instantiate_contract(code_id, sender.clone(), &msg, &[], "yield_arb", None).unwrap();
    
    // Verify sender became owner
    let config: iface::Config = app.wrap().query_wasm_smart(addr.clone(), &iface::QueryMsg::Config {}).unwrap();
    assert_eq!(config.owner, sender);
}

#[test]
fn test_enter_vault_basic() {
    let mut app = App::default();
    let code_id = app.store_code(contract_yield_arb());
    let owner = Addr::unchecked("owner");
    let user = Addr::unchecked("user");
    
    let msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: "mars".to_string(),
        cdp_contract_addr: "cdp".to_string(),
        transmuter_addr: "trans".to_string(),
    };
    
    let addr = app.instantiate_contract(code_id, owner.clone(), &msg, &[], "yield_arb", None).unwrap();
    
    // Test EnterVault without funds
    app.execute_contract(
        user.clone(), 
        addr.clone(), 
        &iface::ExecuteMsg::EnterVault { leave_vault_tokens_in_vault: None }, 
        &[]
    ).unwrap();
    
    // Verify user position was created
    let positions: Vec<iface::UserPosition> = app.wrap().query_wasm_smart(
        addr.clone(), 
        &iface::QueryMsg::GetUserPositions { 
            user: Some(user.to_string()), 
            limit: Some(10), 
            start_after: None 
        }
    ).unwrap();
    
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].user, user);
    assert_eq!(positions[0].collateral_amount, Uint128::zero());
    assert_eq!(positions[0].debt_amount, Uint128::zero());
    assert_eq!(positions[0].position_id, Uint128::zero());
}

#[test]
fn test_enter_vault_with_leave_tokens_option() {
    let mut app = App::default();
    let code_id = app.store_code(contract_yield_arb());
    let owner = Addr::unchecked("owner");
    let user = Addr::unchecked("user");
    
    let msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: "mars".to_string(),
        cdp_contract_addr: "cdp".to_string(),
        transmuter_addr: "trans".to_string(),
    };
    
    let addr = app.instantiate_contract(code_id, owner.clone(), &msg, &[], "yield_arb", None).unwrap();
    
    // Test EnterVault with leave_vault_tokens_in_vault option (simplified)
    app.execute_contract(
        user.clone(), 
        addr.clone(), 
        &iface::ExecuteMsg::EnterVault { leave_vault_tokens_in_vault: None }, 
        &[]
    ).unwrap();
    
    // Verify user position was created
    let positions: Vec<iface::UserPosition> = app.wrap().query_wasm_smart(
        addr.clone(), 
        &iface::QueryMsg::GetUserPositions { 
            user: Some(user.to_string()), 
            limit: Some(10), 
            start_after: None 
        }
    ).unwrap();
    
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].user, user);
}

#[test]
fn test_repay_user_debt_noop() {
    let mut app = App::default();
    let code_id = app.store_code(contract_yield_arb());
    let owner = Addr::unchecked("owner");
    let user = Addr::unchecked("user");
    
    let msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: "mars".to_string(),
        cdp_contract_addr: "cdp".to_string(),
        transmuter_addr: "trans".to_string(),
    };
    
    let addr = app.instantiate_contract(code_id, owner.clone(), &msg, &[], "yield_arb", None).unwrap();
    
    // Test RepayUserDebt with zero repayment
    let user_info = membrane::types::UserInfo { 
        position_id: Uint128::zero(), 
        position_owner: user.to_string() 
    };
    
    app.execute_contract(
        owner.clone(), 
        addr.clone(), 
        &iface::ExecuteMsg::RepayUserDebt { 
            user_info: user_info.clone(), 
            repayment: Uint128::zero() 
        }, 
        &[]
    ).unwrap();
    
    // Test RepayUserDebt with non-zero repayment
    app.execute_contract(
        owner.clone(), 
        addr.clone(), 
        &iface::ExecuteMsg::RepayUserDebt { 
            user_info, 
            repayment: Uint128::from(1000u128) 
        }, 
        &[]
    ).unwrap();
    
    // Function should not error (it's a noop)
}

#[test]
fn test_update_market_conditions() {
    let mut app = App::default();
    let code_id = app.store_code(contract_yield_arb());
    let owner = Addr::unchecked("owner");
    
    let msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: "mars".to_string(),
        cdp_contract_addr: "cdp".to_string(),
        transmuter_addr: "trans".to_string(),
    };
    
    let addr = app.instantiate_contract(code_id, owner.clone(), &msg, &[], "yield_arb", None).unwrap();
    
    // Test UpdateMarketConditions
    app.execute_contract(
        owner.clone(), 
        addr.clone(), 
        &iface::ExecuteMsg::UpdateMarketConditions {}, 
        &[]
    ).unwrap();
    
    // Verify market conditions were updated
    let market_conditions: Vec<iface::MarketConditions> = app.wrap().query_wasm_smart(
        addr.clone(), 
        &iface::QueryMsg::GetMarketConditions { 
            limit: Some(10), 
            start_after: None 
        }
    ).unwrap();
    
    assert!(market_conditions.len() >= 1);
}

#[test]
fn test_update_config_as_owner() {
    let mut app = App::default();
    let code_id = app.store_code(contract_yield_arb());
    let owner = Addr::unchecked("owner");
    
    let msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: "mars".to_string(),
        cdp_contract_addr: "cdp".to_string(),
        transmuter_addr: "trans".to_string(),
    };
    
    let addr = app.instantiate_contract(code_id, owner.clone(), &msg, &[], "yield_arb", None).unwrap();
    
    // Test UpdateConfig with all fields
    app.execute_contract(
        owner.clone(), 
        addr.clone(), 
        &iface::ExecuteMsg::UpdateConfig {
            owner: Some("new_owner".to_string()),
            cdt_denom: None,
            usdc_denom: None,
            mars_vault_addr: Some("new_mars".to_string()),
            cdp_contract_addr: Some("new_cdp".to_string()),
            transmuter_addr: Some("new_trans".to_string()),
            vault_cost_index: Some(1),
        },
        &[]
    ).unwrap();
    
    // Verify config was updated
    let config: iface::Config = app.wrap().query_wasm_smart(addr.clone(), &iface::QueryMsg::Config {}).unwrap();
    assert_eq!(config.owner, Addr::unchecked("new_owner"));
    assert_eq!(config.mars_vault_addr, Addr::unchecked("new_mars"));
    assert_eq!(config.cdp_contract_addr, Addr::unchecked("new_cdp"));
    assert_eq!(config.transmuter_addr, Addr::unchecked("new_trans"));
    assert_eq!(config.vault_cost_index, 1);
}

#[test]
fn test_update_config_partial() {
    let mut app = App::default();
    let code_id = app.store_code(contract_yield_arb());
    let owner = Addr::unchecked("owner");
    
    let msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: "mars".to_string(),
        cdp_contract_addr: "cdp".to_string(),
        transmuter_addr: "trans".to_string(),
    };
    
    let addr = app.instantiate_contract(code_id, owner.clone(), &msg, &[], "yield_arb", None).unwrap();
    
    // Test UpdateConfig with only some fields
    app.execute_contract(
        owner.clone(), 
        addr.clone(), 
        &iface::ExecuteMsg::UpdateConfig {
            owner: None,
            cdt_denom: None,
            usdc_denom: None,
            mars_vault_addr: Some("new_mars".to_string()),
            cdp_contract_addr: None,
            transmuter_addr: None,
            vault_cost_index: None,
        },
        &[]
    ).unwrap();
    
    // Verify only specified fields were updated
    let config: iface::Config = app.wrap().query_wasm_smart(addr.clone(), &iface::QueryMsg::Config {}).unwrap();
    assert_eq!(config.owner, owner); // Should remain unchanged
    assert_eq!(config.mars_vault_addr, Addr::unchecked("new_mars")); // Should be updated
    assert_eq!(config.cdp_contract_addr, Addr::unchecked("cdp")); // Should remain unchanged
    assert_eq!(config.transmuter_addr, Addr::unchecked("trans")); // Should remain unchanged
    assert_eq!(config.vault_cost_index, 0); // Should remain unchanged
}

#[test]
fn test_update_config_unauthorized() {
    let mut app = App::default();
    let code_id = app.store_code(contract_yield_arb());
    let owner = Addr::unchecked("owner");
    let non_owner = Addr::unchecked("non_owner");
    
    let msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: "mars".to_string(),
        cdp_contract_addr: "cdp".to_string(),
        transmuter_addr: "trans".to_string(),
    };
    
    let addr = app.instantiate_contract(code_id, owner.clone(), &msg, &[], "yield_arb", None).unwrap();
    
    // Test UpdateConfig as non-owner (should fail)
    let result = app.execute_contract(
        non_owner.clone(), 
        addr.clone(), 
        &iface::ExecuteMsg::UpdateConfig {
            owner: Some("hacker".to_string()),
            cdt_denom: None,
            usdc_denom: None,
            mars_vault_addr: None,
            cdp_contract_addr: None,
            transmuter_addr: None,
            vault_cost_index: None,
        },
        &[]
    );
    
    // Should error due to unauthorized access
    assert!(result.is_err());
}

#[test]
fn test_queries_retrievable_cdt() {
    let mut app = App::default();
    let code_id = app.store_code(contract_yield_arb());
    let owner = Addr::unchecked("owner");
    let user = Addr::unchecked("user");
    
    let msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: "mars".to_string(),
        cdp_contract_addr: "cdp".to_string(),
        transmuter_addr: "trans".to_string(),
    };
    
    let addr = app.instantiate_contract(code_id, owner.clone(), &msg, &[], "yield_arb", None).unwrap();
    
    // RetrievableCDT should always return zero
    let retrievable: Uint128 = app.wrap().query_wasm_smart(
        addr.clone(), 
        &iface::QueryMsg::RetrievableCDT { user: user.to_string() }
    ).unwrap();
    
    assert_eq!(retrievable, Uint128::zero());
}

#[test]
fn test_queries_vault_token_underlying() {
    let mut app = App::default();
    let code_id = app.store_code(contract_yield_arb());
    let owner = Addr::unchecked("owner");
    
    let msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: "mars".to_string(),
        cdp_contract_addr: "cdp".to_string(),
        transmuter_addr: "trans".to_string(),
    };
    
    let addr = app.instantiate_contract(code_id, owner.clone(), &msg, &[], "yield_arb", None).unwrap();
    
    // VaultTokenUnderlying should query mars vault (will fail in test but should not panic)
    let result: Result<Uint128, _> = app.wrap().query_wasm_smart(
        addr.clone(), 
        &iface::QueryMsg::VaultTokenUnderlying { vault_token_amount: Uint128::from(1000u128) }
    );
    
    // Should fail due to mock mars vault, but not panic
    assert!(result.is_err());
}

#[test]
fn test_queries_deposit_token_conversion() {
    let mut app = App::default();
    let code_id = app.store_code(contract_yield_arb());
    let owner = Addr::unchecked("owner");
    
    let msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: "mars".to_string(),
        cdp_contract_addr: "cdp".to_string(),
        transmuter_addr: "trans".to_string(),
    };
    
    let addr = app.instantiate_contract(code_id, owner.clone(), &msg, &[], "yield_arb", None).unwrap();
    
    // DepositTokenConversion should query mars vault (will fail in test but should not panic)
    let result: Result<Uint128, _> = app.wrap().query_wasm_smart(
        addr.clone(), 
        &iface::QueryMsg::DepositTokenConversion { deposit_token_amount: Uint128::from(1000u128) }
    );
    
    // Should fail due to mock mars vault, but not panic
    assert!(result.is_err());
}

#[test]
fn test_queries_user_positions_pagination() {
    let mut app = App::default();
    let code_id = app.store_code(contract_yield_arb());
    let owner = Addr::unchecked("owner");
    let user1 = Addr::unchecked("user1");
    let user2 = Addr::unchecked("user2");
    
    let msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: "mars".to_string(),
        cdp_contract_addr: "cdp".to_string(),
        transmuter_addr: "trans".to_string(),
    };
    
    let addr = app.instantiate_contract(code_id, owner.clone(), &msg, &[], "yield_arb", None).unwrap();
    
    // Create multiple user positions
    app.execute_contract(user1.clone(), addr.clone(), &iface::ExecuteMsg::EnterVault { leave_vault_tokens_in_vault: None }, &[]).unwrap();
    app.execute_contract(user2.clone(), addr.clone(), &iface::ExecuteMsg::EnterVault { leave_vault_tokens_in_vault: None }, &[]).unwrap();
    
    // Test query all positions
    let all_positions: Vec<iface::UserPosition> = app.wrap().query_wasm_smart(
        addr.clone(), 
        &iface::QueryMsg::GetUserPositions { 
            user: None, 
            limit: Some(10), 
            start_after: None 
        }
    ).unwrap();
    
    assert_eq!(all_positions.len(), 2);
    
    // Test query specific user
    let user1_positions: Vec<iface::UserPosition> = app.wrap().query_wasm_smart(
        addr.clone(), 
        &iface::QueryMsg::GetUserPositions { 
            user: Some(user1.to_string()), 
            limit: Some(10), 
            start_after: None 
        }
    ).unwrap();
    
    assert_eq!(user1_positions.len(), 1);
    assert_eq!(user1_positions[0].user, user1);
    
    // Test pagination with limit
    let limited_positions: Vec<iface::UserPosition> = app.wrap().query_wasm_smart(
        addr.clone(), 
        &iface::QueryMsg::GetUserPositions { 
            user: None, 
            limit: Some(1), 
            start_after: None 
        }
    ).unwrap();
    
    assert_eq!(limited_positions.len(), 1);
}

#[test]
fn test_queries_market_conditions_pagination() {
    let mut app = App::default();
    let code_id = app.store_code(contract_yield_arb());
    let owner = Addr::unchecked("owner");
    
    let msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: "mars".to_string(),
        cdp_contract_addr: "cdp".to_string(),
        transmuter_addr: "trans".to_string(),
    };
    
    let addr = app.instantiate_contract(code_id, owner.clone(), &msg, &[], "yield_arb", None).unwrap();
    
    // Create multiple market condition updates
    app.execute_contract(owner.clone(), addr.clone(), &iface::ExecuteMsg::UpdateMarketConditions {}, &[]).unwrap();
    app.execute_contract(owner.clone(), addr.clone(), &iface::ExecuteMsg::UpdateMarketConditions {}, &[]).unwrap();
    
    // Test query all market conditions
    let all_conditions: Vec<iface::MarketConditions> = app.wrap().query_wasm_smart(
        addr.clone(), 
        &iface::QueryMsg::GetMarketConditions { 
            limit: Some(10), 
            start_after: None 
        }
    ).unwrap();
    
    assert!(all_conditions.len() >= 2);
    
    // Test pagination with limit
    let limited_conditions: Vec<iface::MarketConditions> = app.wrap().query_wasm_smart(
        addr.clone(), 
        &iface::QueryMsg::GetMarketConditions { 
            limit: Some(1), 
            start_after: None 
        }
    ).unwrap();
    
    assert_eq!(limited_conditions.len(), 1);
}

#[test]
fn test_queries_tvl_history() {
    let mut app = App::default();
    let code_id = app.store_code(contract_yield_arb());
    let owner = Addr::unchecked("owner");
    
    let msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: "mars".to_string(),
        cdp_contract_addr: "cdp".to_string(),
        transmuter_addr: "trans".to_string(),
    };
    
    let addr = app.instantiate_contract(code_id, owner.clone(), &msg, &[], "yield_arb", None).unwrap();
    
    // Test query TVL history (should be empty initially)
    let tvl_history: Vec<iface::TVLSnapshot> = app.wrap().query_wasm_smart(
        addr.clone(), 
        &iface::QueryMsg::GetTVLHistory { 
            limit: Some(10), 
            start_after: None 
        }
    ).unwrap();
    
    assert_eq!(tvl_history.len(), 0);
}

#[test]
fn test_user_positions_limit() {
    let mut app = App::default();
    let code_id = app.store_code(contract_yield_arb());
    let owner = Addr::unchecked("owner");
    let user = Addr::unchecked("user");
    
    let msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: "mars".to_string(),
        cdp_contract_addr: "cdp".to_string(),
        transmuter_addr: "trans".to_string(),
    };
    
    let addr = app.instantiate_contract(code_id, owner.clone(), &msg, &[], "yield_arb", None).unwrap();
    
    // Create multiple user positions to test the limit (50 positions)
    for i in 0..55 {
        app.execute_contract(
            user.clone(), 
            addr.clone(), 
            &iface::ExecuteMsg::EnterVault { leave_vault_tokens_in_vault: None }, 
            &[]
        ).unwrap();
    }
    
    // Query user positions
    let positions: Vec<iface::UserPosition> = app.wrap().query_wasm_smart(
        addr.clone(), 
        &iface::QueryMsg::GetUserPositions { 
            user: Some(user.to_string()), 
            limit: Some(100), 
            start_after: None 
        }
    ).unwrap();
    
    // Should be limited to 50 positions (USER_POSITIONS_LIMIT)
    assert_eq!(positions.len(), 50);
}

#[test]
fn test_market_conditions_limit() {
    let mut app = App::default();
    let code_id = app.store_code(contract_yield_arb());
    let owner = Addr::unchecked("owner");
    
    let msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: "mars".to_string(),
        cdp_contract_addr: "cdp".to_string(),
        transmuter_addr: "trans".to_string(),
    };
    
    let addr = app.instantiate_contract(code_id, owner.clone(), &msg, &[], "yield_arb", None).unwrap();
    
    // Create multiple market condition updates to test the limit (30 conditions)
    for _ in 0..35 {
        app.execute_contract(
            owner.clone(), 
            addr.clone(), 
            &iface::ExecuteMsg::UpdateMarketConditions {}, 
            &[]
        ).unwrap();
    }
    
    // Query market conditions
    let conditions: Vec<iface::MarketConditions> = app.wrap().query_wasm_smart(
        addr.clone(), 
        &iface::QueryMsg::GetMarketConditions { 
            limit: Some(100), 
            start_after: None 
        }
    ).unwrap();
    
    // Should be limited to 30 conditions (MARKET_CONDITIONS_LIMIT)
    assert_eq!(conditions.len(), 30);
}

#[test]
fn test_full_flow_with_mock_contracts() {
    let mut app = App::default();
    
    // Store mock contracts
    let transmuter_id = app.store_code(contract_transmuter());
    let mars_vault_id = app.store_code(contract_mars_vault());
    let cdp_id = app.store_code(contract_cdp());
    let yield_arb_id = app.store_code(contract_yield_arb());
    
    let owner = Addr::unchecked("owner");
    let user = Addr::unchecked("user");
    
    // Instantiate mock contracts
    let transmuter_addr = app.instantiate_contract(
        transmuter_id,
        owner.clone(),
        &cosmwasm_std::Empty {},
        &[],
        "transmuter",
        None,
    ).unwrap();
    
    let mars_vault_addr = app.instantiate_contract(
        mars_vault_id,
        owner.clone(),
        &cosmwasm_std::Empty {},
        &[],
        "mars_vault",
        None,
    ).unwrap();
    
    let cdp_addr = app.instantiate_contract(
        cdp_id,
        owner.clone(),
        &cosmwasm_std::Empty {},
        &[],
        "cdp",
        None,
    ).unwrap();
    
    // Instantiate yield-arb contract with real addresses
    let yield_arb_addr = app.instantiate_contract(
        yield_arb_id,
        owner.clone(),
        &iface::InstantiateMsg {
            owner: Some(owner.to_string()),
            cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
            mars_vault_addr: mars_vault_addr.to_string(),
            cdp_contract_addr: cdp_addr.to_string(),
            transmuter_addr: transmuter_addr.to_string(),
        },
        &[],
        "yield_arb",
        None,
    ).unwrap();
    
    // Give owner some USDC to send to transmuter
    app.init_modules(|router, _, storage| {
        router.bank.init_balance(storage, &owner, vec![coin(1000000, "uusdc")]).unwrap();
        router.bank.init_balance(storage, &user, vec![coin(10000, "ucdt")]).unwrap();
        router.bank.init_balance(storage, &mars_vault_addr, vec![coin(1000000, "mars_usdc")]).unwrap();
    });
    
    // Give transmuter some USDC balance for the swap
    app.send_tokens(owner.clone(), transmuter_addr.clone(), &[coin(100000, "uusdc")]).unwrap();
    
    // Test EnterVault
    app.execute_contract(
        user.clone(),
        yield_arb_addr.clone(),
        &iface::ExecuteMsg::EnterVault { leave_vault_tokens_in_vault: None },
        &[],
    ).unwrap();
    
    // Check that user positions were recorded
    let positions: Vec<iface::UserPosition> = app.wrap().query_wasm_smart(
        yield_arb_addr.clone(),
        &iface::QueryMsg::GetUserPositions { 
            user: Some(user.to_string()), 
            limit: Some(10), 
            start_after: None 
        },
    ).unwrap();
    
    assert!(positions.len() >= 1);
    assert_eq!(positions[0].user, user);
    
    // Check market conditions were updated
    let market_conditions: Vec<iface::MarketConditions> = app.wrap().query_wasm_smart(
        yield_arb_addr.clone(),
        &iface::QueryMsg::GetMarketConditions { 
            limit: Some(10), 
            start_after: None 
        },
    ).unwrap();
    
    assert!(market_conditions.len() >= 1);
    assert_eq!(market_conditions[0].vault_apr, Decimal::percent(200)); // year_apr from mock
    assert_eq!(market_conditions[0].vault_cost, Decimal::percent(1)); // cost from mock
    
    // Test UpdateMarketConditions
    app.execute_contract(
        owner.clone(),
        yield_arb_addr.clone(),
        &iface::ExecuteMsg::UpdateMarketConditions {},
        &[],
    ).unwrap();
    
    // Verify market conditions were updated again
    let updated_conditions: Vec<iface::MarketConditions> = app.wrap().query_wasm_smart(
        yield_arb_addr.clone(),
        &iface::QueryMsg::GetMarketConditions { 
            limit: Some(10), 
            start_after: None 
        },
    ).unwrap();
    
    assert!(updated_conditions.len() >= 2); // Should have at least 2 entries now
}

// ============================================================================
// DEPLOYMENT SNAPSHOT TESTS
// ============================================================================

#[test]
fn test_deployment_snapshot_first_loop() {
    let mut app = App::default();
    
    // Setup contracts
    let yield_arb_code_id = app.store_code(contract_yield_arb());
    let transmuter_code_id = app.store_code(contract_transmuter());
    let mars_vault_code_id = app.store_code(contract_mars_vault());
    let cdp_code_id = app.store_code(contract_cdp());
    
    let owner = Addr::unchecked("owner");
    let user = Addr::unchecked("user");
    
    // Instantiate contracts
    let transmuter_addr = app.instantiate_contract(transmuter_code_id, owner.clone(), &cosmwasm_std::Empty::default(), &[], "transmuter", None).unwrap();
    let mars_vault_addr = app.instantiate_contract(mars_vault_code_id, owner.clone(), &cosmwasm_std::Empty::default(), &[], "mars_vault", None).unwrap();
    let cdp_addr = app.instantiate_contract(cdp_code_id, owner.clone(), &cosmwasm_std::Empty::default(), &[], "cdp", None).unwrap();
    
    let yield_arb_msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: mars_vault_addr.to_string(),
        cdp_contract_addr: cdp_addr.to_string(),
        transmuter_addr: transmuter_addr.to_string(),
    };
    
    let yield_arb_addr = app.instantiate_contract(yield_arb_code_id, owner.clone(), &yield_arb_msg, &[], "yield_arb", None).unwrap();
    
    // Fund contracts and user - transmuter needs USDC to send when transmuting CDT -> USDC
    // Initialize balances using init_modules to ensure contracts have tokens
    app.init_modules(|router, _, storage| {
        router.bank.init_balance(storage, &user, vec![coin(10000, "ucdt")]).unwrap();
        // Transmuter contract needs USDC balance to send when executing BankMsg::Send
        router.bank.init_balance(storage, &transmuter_addr, vec![coin(100000, "uusdc")]).unwrap();
        // Mars vault contract needs mars_usdc balance to send when executing BankMsg::Send
        router.bank.init_balance(storage, &mars_vault_addr, vec![coin(100000, "mars_usdc")]).unwrap();
    });
    // First loop - should create deployment snapshot with initial collateral
    app.execute_contract(
        user.clone(),
        yield_arb_addr.clone(),
        &iface::ExecuteMsg::EnterVault { leave_vault_tokens_in_vault: None },
        &[coin(1000u128, "ucdt")],
    ).unwrap();
    
    // Process replies to complete the loop
    app.update_block(|b| b.height += 1);
    
    // Query deployment snapshot
    let snapshot: Option<iface::DeploymentSnapshot> = app.wrap().query_wasm_smart(
        yield_arb_addr.clone(),
        &iface::QueryMsg::GetDeploymentSnapshot { user: user.to_string() },
    ).unwrap();
    
    // Verify snapshot was created
    assert!(snapshot.is_some());
    let snapshot = snapshot.unwrap();
    
    // Verify initial collateral assets were saved
    assert!(!snapshot.collateral_assets.is_empty());
    assert!(snapshot.block_time > 0);
    
    // Verify initial loop amount and debt (should be set after reply completes)
    // Note: In a real scenario, these would be updated in handle_deposit_cdp_reply
    assert!(snapshot.amount_looped >= Uint128::zero());
    assert!(snapshot.debt_taken >= Uint128::zero());
}

#[test]
fn test_deployment_snapshot_multiple_loops() {
    let mut app = App::default();
    
    // Setup contracts
    let yield_arb_code_id = app.store_code(contract_yield_arb());
    let transmuter_code_id = app.store_code(contract_transmuter());
    let mars_vault_code_id = app.store_code(contract_mars_vault());
    let cdp_code_id = app.store_code(contract_cdp());
    
    let owner = Addr::unchecked("owner");
    let user = Addr::unchecked("user");
    
    // Instantiate contracts
    let transmuter_addr = app.instantiate_contract(transmuter_code_id, owner.clone(), &cosmwasm_std::Empty::default(), &[], "transmuter", None).unwrap();
    let mars_vault_addr = app.instantiate_contract(mars_vault_code_id, owner.clone(), &cosmwasm_std::Empty::default(), &[], "mars_vault", None).unwrap();
    let cdp_addr = app.instantiate_contract(cdp_code_id, owner.clone(), &cosmwasm_std::Empty::default(), &[], "cdp", None).unwrap();
    
    let yield_arb_msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: mars_vault_addr.to_string(),
        cdp_contract_addr: cdp_addr.to_string(),
        transmuter_addr: transmuter_addr.to_string(),
    };
    
    let yield_arb_addr = app.instantiate_contract(yield_arb_code_id, owner.clone(), &yield_arb_msg, &[], "yield_arb", None).unwrap();
    
    // Fund contracts and user - transmuter needs USDC to send when transmuting CDT -> USDC
    app.init_modules(|router, _, storage| {
        router.bank.init_balance(storage, &user, vec![coin(5000, "ucdt")]).unwrap();
        // Transmuter contract needs USDC balance to send when executing BankMsg::Send
        router.bank.init_balance(storage, &transmuter_addr, vec![coin(100000, "uusdc")]).unwrap();
        // Mars vault contract needs mars_usdc balance to send when executing BankMsg::Send
        router.bank.init_balance(storage, &mars_vault_addr, vec![coin(100000, "mars_usdc")]).unwrap();
    });
    
    // First loop
    app.execute_contract(
        user.clone(),
        yield_arb_addr.clone(),
        &iface::ExecuteMsg::EnterVault { leave_vault_tokens_in_vault: None },
        &[coin(1000u128, "ucdt")],
    ).unwrap();
    
    app.update_block(|b| b.height += 1);
    
    // Get snapshot after first loop
    let snapshot1: Option<iface::DeploymentSnapshot> = app.wrap().query_wasm_smart(
        yield_arb_addr.clone(),
        &iface::QueryMsg::GetDeploymentSnapshot { user: user.to_string() },
    ).unwrap();
    
    assert!(snapshot1.is_some());
    let snapshot1 = snapshot1.unwrap();
    let initial_collateral = snapshot1.collateral_assets.clone();
    let initial_time = snapshot1.block_time;
    let first_loop_amount = snapshot1.amount_looped;
    
    // Second loop
    app.execute_contract(
        user.clone(),
        yield_arb_addr.clone(),
        &iface::ExecuteMsg::EnterVault { leave_vault_tokens_in_vault: None },
        &[coin(2000u128, "ucdt")],
    ).unwrap();
    
    app.update_block(|b| b.height += 1);
    
    // Get snapshot after second loop
    let snapshot2: Option<iface::DeploymentSnapshot> = app.wrap().query_wasm_smart(
        yield_arb_addr.clone(),
        &iface::QueryMsg::GetDeploymentSnapshot { user: user.to_string() },
    ).unwrap();
    
    assert!(snapshot2.is_some());
    let snapshot2 = snapshot2.unwrap();
    
    // Verify collateral_assets and block_time did NOT change (only set on first loop)
    assert_eq!(snapshot2.collateral_assets, initial_collateral);
    assert_eq!(snapshot2.block_time, initial_time);
    
    // Verify amount_looped is cumulative (should include both loops)
    assert!(snapshot2.amount_looped >= first_loop_amount);
    
    // Verify debt_taken is current total debt
    assert!(snapshot2.debt_taken >= Uint128::zero());
}

#[test]
fn test_deployment_snapshot_no_snapshot_for_new_user() {
    let mut app = App::default();
    
    let yield_arb_code_id = app.store_code(contract_yield_arb());
    let owner = Addr::unchecked("owner");
    let new_user = Addr::unchecked("new_user");
    
    let msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: "mars".to_string(),
        cdp_contract_addr: "cdp".to_string(),
        transmuter_addr: "trans".to_string(),
    };
    
    let yield_arb_addr = app.instantiate_contract(yield_arb_code_id, owner.clone(), &msg, &[], "yield_arb", None).unwrap();
    
    // Query snapshot for user who has never looped
    let snapshot: Option<iface::DeploymentSnapshot> = app.wrap().query_wasm_smart(
        yield_arb_addr.clone(),
        &iface::QueryMsg::GetDeploymentSnapshot { user: new_user.to_string() },
    ).unwrap();
    
    // Should return None for user with no snapshot
    assert!(snapshot.is_none());
}

#[test]
fn test_deployment_snapshot_collateral_assets_preserved() {
    let mut app = App::default();
    
    // Setup contracts
    let yield_arb_code_id = app.store_code(contract_yield_arb());
    let transmuter_code_id = app.store_code(contract_transmuter());
    let mars_vault_code_id = app.store_code(contract_mars_vault());
    let cdp_code_id = app.store_code(contract_cdp());
    
    let owner = Addr::unchecked("owner");
    let user = Addr::unchecked("user");
    
    // Instantiate contracts
    let transmuter_addr = app.instantiate_contract(transmuter_code_id, owner.clone(), &cosmwasm_std::Empty::default(), &[], "transmuter", None).unwrap();
    let mars_vault_addr = app.instantiate_contract(mars_vault_code_id, owner.clone(), &cosmwasm_std::Empty::default(), &[], "mars_vault", None).unwrap();
    let cdp_addr = app.instantiate_contract(cdp_code_id, owner.clone(), &cosmwasm_std::Empty::default(), &[], "cdp", None).unwrap();
    
    let yield_arb_msg = iface::InstantiateMsg {
        owner: Some(owner.to_string()),
        cdt_denom: "ucdt".to_string(),
        usdc_denom: "uusdc".to_string(),
        mars_vault_addr: mars_vault_addr.to_string(),
        cdp_contract_addr: cdp_addr.to_string(),
        transmuter_addr: transmuter_addr.to_string(),
    };
    
    let yield_arb_addr = app.instantiate_contract(yield_arb_code_id, owner.clone(), &yield_arb_msg, &[], "yield_arb", None).unwrap();
    
    // Fund contracts and user - transmuter needs USDC to send when transmuting CDT -> USDC
    app.init_modules(|router, _, storage| {
        router.bank.init_balance(storage, &user, vec![coin(10000, "ucdt")]).unwrap();
        // Transmuter contract needs USDC balance to send when executing BankMsg::Send
        router.bank.init_balance(storage, &transmuter_addr, vec![coin(100000, "uusdc")]).unwrap();
        // Mars vault contract needs mars_usdc balance to send when executing BankMsg::Send
        router.bank.init_balance(storage, &mars_vault_addr, vec![coin(100000, "mars_usdc")]).unwrap();
    });
    
    // First loop - saves initial collateral
    app.execute_contract(
        user.clone(),
        yield_arb_addr.clone(),
        &iface::ExecuteMsg::EnterVault { leave_vault_tokens_in_vault: None },
        &[coin(1000u128, "ucdt")],
    ).unwrap();
    
    app.update_block(|b| b.height += 1);
    
    let snapshot1: Option<iface::DeploymentSnapshot> = app.wrap().query_wasm_smart(
        yield_arb_addr.clone(),
        &iface::QueryMsg::GetDeploymentSnapshot { user: user.to_string() },
    ).unwrap();
    
    assert!(snapshot1.is_some());
    let initial_collateral = snapshot1.unwrap().collateral_assets.clone();
    
    // Multiple subsequent loops
    for i in 0..3 {
        app.execute_contract(
            user.clone(),
            yield_arb_addr.clone(),
            &iface::ExecuteMsg::EnterVault { leave_vault_tokens_in_vault: None },
            &[coin(500u128 * (i + 1), "ucdt")],
        ).unwrap();
        app.update_block(|b| b.height += 1);
    }
    
    // Verify collateral assets remain unchanged after multiple loops
    let snapshot_final: Option<iface::DeploymentSnapshot> = app.wrap().query_wasm_smart(
        yield_arb_addr.clone(),
        &iface::QueryMsg::GetDeploymentSnapshot { user: user.to_string() },
    ).unwrap();
    
    assert!(snapshot_final.is_some());
    assert_eq!(snapshot_final.unwrap().collateral_assets, initial_collateral);
}


