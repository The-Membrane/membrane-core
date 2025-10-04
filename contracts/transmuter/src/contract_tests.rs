use cosmwasm_std::{coin, coins, Addr, Binary, Decimal, Deps, DepsMut, Empty, Env, MessageInfo, Response, StdError, StdResult, Uint128};
use cw_multi_test::{App, Contract, ContractWrapper, Executor};

use membrane::tokenfactory::{ExecuteMsg as TfExecuteMsg, InstantiateMsg as TfInstantiateMsg};
use membrane::transmuter::{ExecuteMsg, InstantiateMsg, QueryMsg, AssetPair, TransmuteHistoryResponse, VolumeHistoryResponse, VaultInfoResponse};

use crate::contract::{execute, instantiate, query};

const ADMIN: &str = "admin";
const USER: &str = "user";
const OTHER: &str = "other";
const ASSET_A: &str = "asset-a";
const ASSET_B: &str = "asset-b";
const VAULT_SUBDENOM: &str = "vault-token";
const INITIAL_BALANCE: u128 = 1_000_000_000;
const USER_DEPOSIT: u128 = 100_000;

fn transmuter_contract() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(execute, instantiate, query))
}

fn mock_tokenfactory_contract() -> Box<dyn Contract<Empty>> {
    Box::new(ContractWrapper::new(
        mock_tokenfactory_execute,
        mock_tokenfactory_instantiate,
        mock_tokenfactory_query,
    ))
}

fn setup_app() -> App {
    let mut app = App::default();
    app.init_modules(|router, _, storage| {
        router
            .bank
            .init_balance(
                storage,
                &Addr::unchecked(ADMIN),
                vec![coin(INITIAL_BALANCE, ASSET_A), coin(INITIAL_BALANCE, ASSET_B), coin(200000000000, "factory/contract1/vault-token")],
            )
            .unwrap();
        router
            .bank
            .init_balance(
                storage,
                &Addr::unchecked(USER),
                vec![coin(INITIAL_BALANCE, ASSET_A), coin(INITIAL_BALANCE, ASSET_B)],
            )
            .unwrap();
        router
            .bank
            .init_balance(
                storage,
                &Addr::unchecked(OTHER),
                vec![coin(INITIAL_BALANCE, ASSET_A), coin(INITIAL_BALANCE, ASSET_B)],
            )
            .unwrap();
    });
    app
}

fn instantiate_transmuter(app: &mut App) -> Addr {
    let tf_code = app.store_code(mock_tokenfactory_contract());
    let tokenfactory_addr = app
        .instantiate_contract(
            tf_code,
            Addr::unchecked(ADMIN),
            &TfInstantiateMsg { owner: Some(ADMIN.to_string()) },
            &[],
            "mock-tokenfactory",
            None,
        )
        .unwrap();

    let code_id = app.store_code(transmuter_contract());
    let msg = InstantiateMsg {
        owner: Some(ADMIN.to_string()),
        tokenfactory_contract: Some(tokenfactory_addr),
        vault_subdenom: VAULT_SUBDENOM.to_string(),
        deposit_pair: AssetPair {
            asset_a: ASSET_A.to_string(),
            asset_b: ASSET_B.to_string(),
        },
        composition_leeway: Decimal::percent(1),
        asset_a_to_b_rate: Decimal::one(),
        target_ratio: Decimal::percent(50),
        swap_history_cap: 5,
        volume_history_cap: 5,
    };

    app.instantiate_contract(
        code_id,
        Addr::unchecked(ADMIN),
        &msg,
        &[],
        "transmuter",
        None,
    )
    .unwrap()
}

fn mock_tokenfactory_instantiate(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    _msg: TfInstantiateMsg,
) -> StdResult<Response> {
    Ok(Response::new().add_attribute("method", "mock_tokenfactory_instantiate"))
}

fn mock_tokenfactory_execute(
    _deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: TfExecuteMsg,
) -> Result<Response, StdError> {
    let mut res = Response::new().add_attribute("contract", "mock_tokenfactory");
    match msg {
        TfExecuteMsg::CreateDenom { subdenom } => {
            res = res.add_attribute("create_denom", subdenom);
        }
        TfExecuteMsg::MintTokens { amount, mint_to_address } => {
            res = res
                .add_attribute("mint_amount", amount.map(|c| c.amount).unwrap_or_default())
                .add_attribute("mint_to", mint_to_address);
        }
        TfExecuteMsg::UpdateConfig { owner } => {
            res = res.add_attribute("update_owner", owner.unwrap_or_default());
        }
        TfExecuteMsg::BurnTokens {} => {
            res = res.add_attribute("burn", "true");
        }
    }
    Ok(res)
}

fn mock_tokenfactory_query(_deps: Deps, _env: Env, _msg: Binary) -> StdResult<Binary> {
    Err(StdError::generic_err("mock tokenfactory has no queries"))
}

fn query_vault_info(app: &App, contract: &Addr) -> VaultInfoResponse {
    app.wrap()
        .query_wasm_smart(contract, &QueryMsg::VaultInfo {})
        .unwrap()
}

fn query_swap_history(app: &App, contract: &Addr) -> TransmuteHistoryResponse {
    app.wrap()
        .query_wasm_smart(contract, &QueryMsg::TransmuteHistory { start_after: None, limit: None })
        .unwrap()
}

fn query_volume_history(app: &App, contract: &Addr) -> VolumeHistoryResponse {
    app.wrap()
        .query_wasm_smart(contract, &QueryMsg::VolumeHistory { start_after: None, limit: None })
        .unwrap()
}

#[test]
fn instantiate_sets_config() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    let config: membrane::transmuter::Config = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::Config {})
        .unwrap();

    assert_eq!(config.owner, Addr::unchecked(ADMIN));
    assert_eq!(config.deposit_pair.asset_a, ASSET_A);
    assert_eq!(config.deposit_pair.asset_b, ASSET_B);
    assert_eq!(config.target_ratio, Decimal::percent(50));
    assert!(config.tokenfactory_contract.is_some());
}

#[test]
fn enter_vault_mints_tokens_and_updates_state() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    app.execute_contract(
        Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None },
        &[coin(USER_DEPOSIT, ASSET_A), coin(USER_DEPOSIT, ASSET_B)],
    )
    .unwrap();

    let info = query_vault_info(&app, &contract);
    assert!(info.vault_token_supply > Uint128::zero());
    assert_eq!(info.asset_a_balance, Uint128::from(USER_DEPOSIT));
    assert_eq!(info.asset_b_balance, Uint128::from(USER_DEPOSIT));
}

#[test]
fn deposit_fee_accepts_single_asset_without_vault_tokens() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::DepositFee {},
        &coins(50_000, ASSET_A),
    )
    .unwrap();

    let info = query_vault_info(&app, &contract);
    assert_eq!(info.asset_a_balance, Uint128::from(50_000u64));
}

#[test]
fn exit_vault_withdraws_proportional_assets() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    app.execute_contract(
        Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None },
        &[coin(USER_DEPOSIT, ASSET_A), coin(USER_DEPOSIT, ASSET_B)],
    )
    .unwrap();

    //Assert user balance is correct minus what it deposited
    let user_balance = app.wrap().query_balance(&Addr::unchecked(USER), ASSET_A).unwrap();
    assert_eq!(user_balance.amount, Uint128::from(INITIAL_BALANCE - USER_DEPOSIT));
    let user_balance = app.wrap().query_balance(&Addr::unchecked(USER), ASSET_B).unwrap();
    assert_eq!(user_balance.amount, Uint128::from(INITIAL_BALANCE - USER_DEPOSIT));

    let info = query_vault_info(&app, &contract);
    let vault_tokens = info.vault_token_supply;

    println!("denom: {:?}, vts: {:?}", format!("factory/{}/{VAULT_SUBDENOM}", contract), vault_tokens.u128());

    //send the vault tokens to the user
    app.send_tokens(Addr::unchecked(ADMIN), Addr::unchecked(USER), &vec![coin(vault_tokens.u128(), &format!("factory/{}/{VAULT_SUBDENOM}", contract))]).unwrap();

    app.execute_contract(
        Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::ExitVault { recipient: None, withdraw_as: None },
        &coins(vault_tokens.u128(), &format!("factory/{}/{VAULT_SUBDENOM}", contract)),
    )
    .unwrap();

    //Assert user balance is back to its initial balance
    let user_balance = app.wrap().query_balance(&Addr::unchecked(USER), ASSET_A).unwrap();
    assert_eq!(user_balance.amount, Uint128::from(INITIAL_BALANCE));
    let user_balance = app.wrap().query_balance(&Addr::unchecked(USER), ASSET_B).unwrap();
    assert_eq!(user_balance.amount, Uint128::from(INITIAL_BALANCE));

    let post_info = query_vault_info(&app, &contract);
    assert_eq!(post_info.vault_token_supply, Uint128::zero());
}

#[test]
fn transmute_swaps_asset_a_for_b() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Seed contract with asset B so it can pay out swaps
    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::DepositFee {},
        &coins(500_000, ASSET_B),
    )
    .unwrap();

    app.execute_contract(
        Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(25_000, ASSET_A),
    )
    .unwrap();

    let swap_history = query_swap_history(&app, &contract);
    assert_eq!(swap_history.records.len(), 1);
    assert_eq!(swap_history.records[0].offered_asset, ASSET_A);
    println!("swap_history: {:?}", swap_history);

    let volume_history = query_volume_history(&app, &contract);
    assert_eq!(volume_history.records.len(), 1);
    println!("volume_history: {:?}", volume_history);
}

#[test]
fn update_config_changes_owner_and_ratio() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: Some(OTHER.to_string()),
            deposit_pair: None,
            tokenfactory_contract: None,
            composition_leeway: Some(Decimal::percent(5)),
            asset_a_to_b_rate: Some(Decimal::percent(120)),
            target_ratio: Some(Decimal::percent(60)),
            swap_history_cap: Some(20),
            volume_history_cap: Some(20),
        },
        &[],
    )
    .unwrap();

    let config: membrane::transmuter::Config = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::Config {})
        .unwrap();

    assert_eq!(config.owner, Addr::unchecked(OTHER));
    assert_eq!(config.target_ratio, Decimal::percent(60));
    assert_eq!(config.swap_history_cap, 20);
    assert!(config.tokenfactory_contract.is_some());
    assert_eq!(config.volume_history_cap, 20);
    assert_eq!(config.asset_a_to_b_rate, Decimal::percent(120));
    assert_eq!(config.composition_leeway, Decimal::percent(5));

}

#[test]
fn volume_window_updates_and_resets() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::DepositFee {},
        &coins(100_000, ASSET_B),
    )
    .unwrap();

    app.execute_contract(
        Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(10_000, ASSET_A),
    )
    .unwrap();

    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::UpdateVolumeWindow {},
        &[],
    )
    .unwrap();

    let history = query_volume_history(&app, &contract);
    println!("history: {:?}", history);
    assert_eq!(history.records.len(), 2);
}

#[test]
fn config_query_matches_instantiate() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    let config: membrane::transmuter::Config = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::Config {})
        .unwrap();

    assert_eq!(config.owner, Addr::unchecked(ADMIN));
    assert!(config.tokenfactory_contract.is_some());
}
