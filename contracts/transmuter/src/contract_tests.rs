use cosmwasm_std::{coin, coins, Addr, Binary, Decimal, Deps, DepsMut, Empty, Env, MessageInfo, Response, StdError, StdResult, Uint128};
use cw_multi_test::{App, Contract, ContractWrapper, Executor};

use membrane::tokenfactory::{ExecuteMsg as TfExecuteMsg, InstantiateMsg as TfInstantiateMsg};
use membrane::cdp::QueryMsg as CdpQueryMsg;
use membrane::transmuter::{ExecuteMsg, InstantiateMsg, QueryMsg, AssetPair, TransmuteHistoryResponse, VolumeHistoryResponse, VaultInfoResponse, RateLimitStatusResponse, RateLimitManyResponse, GlobalRateLimitResponse};

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

                router
                .bank
                .init_balance(
                    storage,
                    &Addr::unchecked("other2"),
                    vec![coin(INITIAL_BALANCE, ASSET_A), coin(INITIAL_BALANCE, ASSET_B)],
                )
                .unwrap();
                router
                .bank
                .init_balance(
                    storage,
                    &Addr::unchecked("other3"),
                    vec![coin(INITIAL_BALANCE, ASSET_A), coin(INITIAL_BALANCE, ASSET_B)],
                )
                .unwrap();
                router
                .bank
                .init_balance(
                    storage,
                    &Addr::unchecked("other4"),
                    vec![coin(INITIAL_BALANCE, ASSET_A), coin(INITIAL_BALANCE, ASSET_B)],
                )
                .unwrap();
                router
                .bank
                .init_balance(
                    storage,
                    &Addr::unchecked("other5"),
                    vec![coin(INITIAL_BALANCE, ASSET_A), coin(INITIAL_BALANCE, ASSET_B)],
                )
                .unwrap();
    });
    app
}

fn mock_cdp_contract() -> Box<dyn Contract<Empty>> {
    // Query msg type is CdpQueryMsg so the framework decodes it for us
    Box::new(ContractWrapper::new(
        |_deps, _env, _info, _msg: Empty| -> Result<Response, StdError> { Ok(Response::new()) },
        |_deps, _env, _info, _msg: Empty| -> StdResult<Response> { Ok(Response::new()) },
        |_deps, _env, q: CdpQueryMsg| -> StdResult<Binary> {
            match q {
                CdpQueryMsg::GetActiveDeploymentVenues { venue, .. } => {
                    let list = match venue {
                        Some(v) if v == USER => vec![USER.to_string()],
                        _ => Vec::<String>::new(),
                    };
                    cosmwasm_std::to_json_binary(&list)
                },
                _ => Err(StdError::generic_err("unsupported mock cdp query")),
            }
        },
    ))
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
    let cdp_code = app.store_code(mock_cdp_contract());
    let cdp_addr = app
        .instantiate_contract(
            cdp_code,
            Addr::unchecked(ADMIN),
            &Empty {},
            &[],
            "mock-cdp",
            None,
        )
        .unwrap();
    let msg = InstantiateMsg {
        owner: Some(ADMIN.to_string()),
        tokenfactory_contract: Some(tokenfactory_addr),
        revenue_contract: ADMIN.to_string(),
        cdp_contract: cdp_addr.to_string(),
        vault_subdenom: VAULT_SUBDENOM.to_string(),
        deposit_pair: AssetPair {
            cdt: ASSET_A.to_string(),
            paired_asset: ASSET_B.to_string(),
        },
        composition_leeway: Decimal::percent(1),
        asset_a_to_b_rate: Decimal::one(),
        target_ratio: Decimal::percent(50),
        usage_fee: Some(Decimal::percent(0)),
        swap_history_cap: 5,
        volume_history_cap: 5,
        rate_limit_window_secs: Some(60 * 60 * 8),
        rate_limit_threshold: Some(Decimal::percent(5)),
        allowlist: Some(vec![]),
        allowlist_rate_limit_threshold: Some(Decimal::percent(10)),
        global_rate_limit_window_secs: Some(60 * 60 * 24), // 24 hours
        global_rate_limit_threshold: Some(Decimal::percent(20)), // 20%
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

fn query_rate_limit(app: &App, contract: &Addr, address: &str) -> RateLimitStatusResponse {
    let resp: RateLimitManyResponse = app
        .wrap()
        .query_wasm_smart(contract, &QueryMsg::RateLimitMany { addresses: Some(vec![address.to_string()]), start_after: None, limit: None })
        .unwrap();
    resp.records.into_iter().next().unwrap()
}

fn query_rate_limit_many(app: &App, contract: &Addr, addrs: Option<Vec<String>>, start_after: Option<u64>, limit: Option<u32>) -> RateLimitManyResponse {
    app.wrap()
        .query_wasm_smart(contract, &QueryMsg::RateLimitMany { addresses: addrs, start_after, limit })
        .unwrap()
}

fn query_global_rate_limit(app: &App, contract: &Addr) -> GlobalRateLimitResponse {
    app.wrap()
        .query_wasm_smart(contract, &QueryMsg::GlobalRateLimit {})
        .unwrap()
}

#[test]
fn rate_limit_blocks_when_threshold_exceeded_and_nets_flows() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Enter vault to set deposits baseline (so threshold calc > 0)
    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None },
        &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)],
    ).unwrap();

    // Perform alternating flows that net to zero within window
    // A->B (-10k A)
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_A)).unwrap(); 
    // B->A (+10k A)
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap(); 

    let status = query_rate_limit(&app, &contract, USER);
    assert_eq!(status.status.net_flow_base, 0);
    assert!(status.status.remaining_base > Uint128::zero());

    //+10k
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap(); // B->A (+10k A)

    // Now push only B->A until exceeding 5% of deposits (deposits ~ 200k A base; 5% = 10k)
    // We already did +10k; next +1 pushes over -> should error
    let res = app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(1, ASSET_B));
    assert!(res.is_err());

    //Flip to -set back net to 0
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_A)).unwrap(); 
    //Error @ -10k + 1
    let res = app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_001, ASSET_A)); 
    assert!(res.is_err());
    //Set to -10k
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_A)).unwrap(); 
    //Error @ -10k + 1
    let res = app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(1, ASSET_A)); 
    assert!(res.is_err());

}


#[test]
fn allowlist_uses_higher_threshold() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Update config to add USER to allowlist and set small base deposits
    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            asset_a_to_b_rate: None,
            target_ratio: None,
            tokenfactory_contract: None,
            cdp_contract: None,
            revenue_contract: None,
            usage_fee: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: Some(vec![membrane::types::StringEntry { entry: USER.to_string(), remove: false }]),
            allowlist_rate_limit_threshold: Some(Decimal::percent(20)),
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: None,
        },
        &[],
    ).unwrap();

    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None },
        &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)],
    ).unwrap();

    // Seed B
    // app.execute_contract(Addr::unchecked(ADMIN), contract.clone(), &ExecuteMsg::DepositFee {}, &coins(1_000_000, ASSET_B)).unwrap();

    // USER can move up to 20% before block
    // 20% of 200k = 40k. Try 39,999 -> ok, 1 more -> block
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(39_999, ASSET_B)).unwrap();
    let res = app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(2, ASSET_B));
    assert!(res.is_err());
}

#[test]
fn rate_limit_many_paginates() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Add two allowlist entries
    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            asset_a_to_b_rate: None,
            target_ratio: None,
            tokenfactory_contract: None,
            cdp_contract: None,
            revenue_contract: None,
            usage_fee: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: Some(vec![
                membrane::types::StringEntry { entry: USER.to_string(), remove: false },
                membrane::types::StringEntry { entry: OTHER.to_string(), remove: false },
            ]),
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: None,
        },
        &[],
    ).unwrap();

    let page1 = query_rate_limit_many(&app, &contract, None, None, Some(1));
    assert_eq!(page1.records.len(), 1);
    let page2 = query_rate_limit_many(&app, &contract, None, page1.next_start_after, Some(1));
    assert_eq!(page2.records.len(), 1);
}

#[test]
fn window_expiry_unblocks_usage() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Set shorter window (2 hours)
    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            asset_a_to_b_rate: None,
            target_ratio: None,
            tokenfactory_contract: None,
            cdp_contract: None,
            revenue_contract: None,
            usage_fee: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: Some(3 * 60 * 60),
            rate_limit_threshold: None,
            allowlist: None,
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: None,
        },
        &[],
    ).unwrap();

    // Deposits only (no extra liquidity that would inflate threshold)
    app.execute_contract(Addr::unchecked(ADMIN), contract.clone(), &ExecuteMsg::EnterVault { recipient: None }, &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)]).unwrap();

    // Add several entries spreading over time
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(4_000, ASSET_B)).unwrap();
    let s1 = query_rate_limit(&app, &contract, USER);
    assert_eq!(s1.status.entries_count, 1);

    app.update_block(|b| { b.time = b.time.plus_seconds(60 * 60); b.height += 1; });
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(3_000, ASSET_B)).unwrap();
    let s2 = query_rate_limit(&app, &contract, USER);
    assert_eq!(s2.status.entries_count, 2);

    app.update_block(|b| { b.time = b.time.plus_seconds(60 * 60 * 2); b.height += 1; });
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(3_000, ASSET_B)).unwrap();
    let s3 = query_rate_limit(&app, &contract, USER);
    assert_eq!(s3.status.entries_count, 3);

    // Move window forward just past the first entry; expect 2 entries remain
    app.update_block(|b| { b.time = b.time.plus_seconds(1); b.height += 1; });
    let s4 = query_rate_limit(&app, &contract, USER);
    assert_eq!(s4.status.entries_count, 2);

    // Move window forward again past the second
    app.update_block(|b| { b.time = b.time.plus_seconds(60 * 60 + 1); b.height += 1; });
    let s5 = query_rate_limit(&app, &contract, USER);
    assert_eq!(s5.status.entries_count, 1);

    // Finally past the third, entries should be 0
    app.update_block(|b| { b.time = b.time.plus_seconds(60 * 60 * 2 + 1); b.height += 1; });
    let s6 = query_rate_limit(&app, &contract, USER);
    assert_eq!(s6.status.entries_count, 0);
}

#[test]
fn usage_fee_applied_for_non_cdp_and_non_deployable() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Set usage fee to 10%
    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            asset_a_to_b_rate: None,
            target_ratio: None,
            tokenfactory_contract: None,
            cdp_contract: None,
            revenue_contract: None,
            usage_fee: Some(Decimal::percent(10)),
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: None,
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: None,
        },
        &[],
    ).unwrap();

    // Seed contract with B liquidity to pay out A->B swaps
    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None },
        &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)],
    ).unwrap();

    // USER (non-CDP, non-deployable) pays usage fee; send 10_000 A, after 10% fee => 9_000 A considered
    app.execute_contract(
        Addr::unchecked(USER),
        contract.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(10_000, ASSET_A),
    ).unwrap();

    let swaps = query_swap_history(&app, &contract);
    println!("swaps: {:?}", swaps);
    let last = swaps.records.last().unwrap();
    assert_eq!(last.offered_asset, ASSET_A);
    assert_eq!(last.offered_amount, Uint128::from(9_000u64));
    assert_eq!(last.received_asset, ASSET_B);
    assert_eq!(last.received_amount, Uint128::from(9_000u64));

    // CDP address should be exempt from usage fee
    let cfg: membrane::transmuter::Config = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::Config {})
        .unwrap();
    let cdp_addr = Addr::unchecked(cfg.cdp_contract);

    // Fund CDP with A to perform the swap
    app.send_tokens(Addr::unchecked(ADMIN), cdp_addr.clone(), &coins(20_000, ASSET_A)).unwrap();

    app.execute_contract(
        cdp_addr,
        contract.clone(),
        &ExecuteMsg::Transmute { recipient: None },
        &coins(10_000, ASSET_A),
    ).unwrap();

    let swaps2 = query_swap_history(&app, &contract);
    let last2 = swaps2.records.last().unwrap();
    assert_eq!(last2.offered_asset, ASSET_A);
    assert_eq!(last2.offered_amount, Uint128::from(10_000u64)); // no fee applied
    assert_eq!(last2.received_asset, ASSET_B);
    assert_eq!(last2.received_amount, Uint128::from(10_000u64));
}

#[test]
fn paired_asset_outstanding_tracks_allowlisted_flows() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // query initial outstanding
    let start: membrane::transmuter::DeployedPairedAssetResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::DeployedPairedAsset {})
        .unwrap();
    assert_eq!(start.amount, Uint128::zero());

    // Add USER to allowlist in config (simulating allowlisted venue)
    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            asset_a_to_b_rate: None,
            target_ratio: None,
            tokenfactory_contract: None,
            cdp_contract: None,
            revenue_contract: None,
            usage_fee: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: Some(vec![membrane::types::StringEntry { entry: USER.to_string(), remove: false }]),
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: None,
        },
        &[],
    ).unwrap();

    // Seed enough cdt so CDT->USDC can be paid out
    app.execute_contract(Addr::unchecked(ADMIN), contract.clone(), &ExecuteMsg::EnterVault { recipient: None }, &[coin(50_000, ASSET_A), coin(50_000, ASSET_B)]).unwrap();

    // Allowlisted CDT->USDC should increment outstanding by received paired_asset amount (rate=1)
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_A)).unwrap();
    let after_cdt_to_usdc: membrane::transmuter::DeployedPairedAssetResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::DeployedPairedAsset {})
        .unwrap();
    assert_eq!(after_cdt_to_usdc.amount, Uint128::from(10_000u64));

    // Allowlisted USDC->CDT should decrement outstanding by offered paired_asset
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(4_000, ASSET_B)).unwrap();
    let after_usdc_to_cdt: membrane::transmuter::DeployedPairedAssetResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::DeployedPairedAsset {})
        .unwrap();
    assert_eq!(after_usdc_to_cdt.amount, Uint128::from(6_000u64));

    // Non-allowlisted swaps should NOT change outstanding
    let before = after_usdc_to_cdt.amount;
    // OTHER performs USDC->CDT (paired->cdt); contract pays out CDT which it has from vault
    app.execute_contract(Addr::unchecked(OTHER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(1_000, ASSET_B)).unwrap();
    let check1: membrane::transmuter::DeployedPairedAssetResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::DeployedPairedAsset {})
        .unwrap();
    assert_eq!(check1.amount, before);

    // Provide some paired_asset liquidity so A->B payouts can succeed
    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None },
        &[coin(0, ASSET_A), coin(2_000, ASSET_B)],
    ).unwrap();

    // OTHER performs CDT->USDC; contract pays out paired_asset; outstanding should remain unchanged
    app.execute_contract(Addr::unchecked(OTHER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(1_000, ASSET_A)).unwrap();
    let check2: membrane::transmuter::DeployedPairedAssetResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::DeployedPairedAsset {})
        .unwrap();
    assert_eq!(check2.amount, before);
}

#[test]
fn effective_target_reflects_deployed_value_and_bounds() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // With zero deposits, target should be config.target_ratio (50%)
    let eff0: membrane::transmuter::EffectiveTargetResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::EffectiveTarget {})
        .unwrap();
    assert_eq!(eff0.target, Decimal::percent(50));

    // Add deposits 100k cdt + 100k paired, no deployed yet -> target stays 50%
    app.execute_contract(Addr::unchecked(ADMIN), contract.clone(), &ExecuteMsg::EnterVault { recipient: None }, &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)]).unwrap();
    let eff1: membrane::transmuter::EffectiveTargetResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::EffectiveTarget {})
        .unwrap();
    assert_eq!(eff1.target, Decimal::percent(50));

    // Mark USER allowlisted and as deployment venue via mock, then do CDT->USDC (10k) to increase deployed tally
    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            asset_a_to_b_rate: None,
            target_ratio: None,
            tokenfactory_contract: None,
            cdp_contract: None,
            revenue_contract: None,
            usage_fee: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: Some(vec![membrane::types::StringEntry { entry: USER.to_string(), remove: false }]),
            allowlist_rate_limit_threshold: Some(Decimal::one()),
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: None,
        },
        &[],
    ).unwrap();

    // Ensure contract has paired_asset liquidity for payouts
    app.execute_contract(Addr::unchecked(ADMIN), contract.clone(), &ExecuteMsg::EnterVault { recipient: None }, &[coin(0, ASSET_A), coin(20_000, ASSET_B)]).unwrap();

    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_A)).unwrap();

    // Now effective min target should be deployed_value / total_value = 10k / 220k ~= 4.545% < base 50%, so still 50%
    let eff2: membrane::transmuter::EffectiveTargetResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::EffectiveTarget {})
        .unwrap();
    assert_eq!(eff2.target, Decimal::percent(50));

    // Push deployed higher than base target: deploy 150k paired -> need CDT deposits to enable; simulate by multiple CDT->USDC
    // Add more paired liquidity to allow payout
    app.execute_contract(Addr::unchecked(ADMIN), contract.clone(), &ExecuteMsg::EnterVault { recipient: None }, &[coin(0, ASSET_A), coin(200_000, ASSET_B)]).unwrap();
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(120_000, ASSET_A)).unwrap();

    // Total deposits base = (100k + 0) + (100k + 220k converted to base 1:1) = 420k; deployed ~130k (prev 10k + 120k)
    // effective target = max(50%, 130/420 ~= 30.95%) = 50%
    let eff3: membrane::transmuter::EffectiveTargetResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::EffectiveTarget {})
        .unwrap();
    assert_eq!(eff3.target, Decimal::percent(50));

    // Lower base target to 10% to allow deployed to dominate
    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            asset_a_to_b_rate: None,
            target_ratio: Some(Decimal::percent(10)),
            tokenfactory_contract: None,
            cdp_contract: None,
            revenue_contract: None,
            usage_fee: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: None,
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: None,
        },
        &[],
    ).unwrap();

    // Now effective should be ~31% (> 10%)
    let eff4: membrane::transmuter::EffectiveTargetResponse = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::EffectiveTarget {})
        .unwrap();
    println!("eff4: {:?}", eff4);
    assert!(eff4.target.to_string() == String::from("0.309523809523809523"));
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
    assert_eq!(config.deposit_pair.cdt, ASSET_A);
    assert_eq!(config.deposit_pair.paired_asset, ASSET_B);
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
    assert_eq!(info.cdt_balance, Uint128::from(USER_DEPOSIT));
    assert_eq!(info.paired_asset_balance, Uint128::from(USER_DEPOSIT));
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
    assert_eq!(info.cdt_balance, Uint128::from(50_000u64));
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
            cdp_contract: None,
            revenue_contract: None,
            usage_fee: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: None,
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: None,
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

// ===== GLOBAL RATE LIMIT TESTS =====

#[test]
fn global_rate_limit_blocks_when_threshold_exceeded() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Enter vault to set deposits baseline (so threshold calc > 0)
    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None },
        &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)],
    ).unwrap();

    // Check initial global rate limit status
    let global_status = query_global_rate_limit(&app, &contract);
    assert_eq!(global_status.net_flow_base, 0);
    assert_eq!(global_status.entries_count, 0);
    assert!(global_status.remaining_base > Uint128::zero());

    // Multiple non-whitelisted users contribute to global limit
    // Total deposits ~200k A base; 20% = 40k
    // Per-address limit: 5% = 10k, so we need to stay under that
    // USER does 5k B->A (+5k A)
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();
    
    // OTHER does 5k B->A (+5k A) 
    app.execute_contract(Addr::unchecked(OTHER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();

    // Check global status after 10k total
    let global_status = query_global_rate_limit(&app, &contract);
    assert_eq!(global_status.net_flow_base, 10_000);
    assert_eq!(global_status.entries_count, 2);
    assert!(global_status.remaining_base > Uint128::zero());

    // Try to push over 20% threshold (40k) - should fail
    // Need to do this with multiple users to avoid per-address limit
    // Add more users to fill up global limit
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();
    app.execute_contract(Addr::unchecked(OTHER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();
    app.execute_contract(Addr::unchecked("other2"), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();
    app.execute_contract(Addr::unchecked("other3"), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();
    app.execute_contract(Addr::unchecked("other4"), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();
    app.execute_contract(Addr::unchecked("other5"), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();
    
    // Now we should be at 40k total, try one more - should fail
    let res = app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(1, ASSET_B));
    assert!(res.is_err());
    // println!("res: {:?}", res.().unwrap_err().to_string());
    // assert!(res.unwrap_err().to_string().contains("Global rate limit exceeded"));
}

#[test]
fn global_rate_limit_nets_flows_correctly() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Enter vault to set deposits baseline
    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None },
        &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)],
    ).unwrap();

    // USER does B->A (+5k A) - stay under per-address limit
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();
    
    // OTHER does A->B (-2k A) - stay under per-address limit
    app.execute_contract(Addr::unchecked(OTHER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(2_000, ASSET_A)).unwrap();

    // Net should be +3k A
    let global_status = query_global_rate_limit(&app, &contract);
    assert_eq!(global_status.net_flow_base, 3_000);
    assert_eq!(global_status.entries_count, 2);

    // USER does A->B (-3k A) to net to zero
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(3_000, ASSET_A)).unwrap();

    // Net should be 0
    let global_status = query_global_rate_limit(&app, &contract);
    assert_eq!(global_status.net_flow_base, 0);
    assert_eq!(global_status.entries_count, 3);
}

#[test]
fn global_rate_limit_whitelisted_addresses_bypass() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Add USER to allowlist
    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            asset_a_to_b_rate: None,
            target_ratio: None,
            tokenfactory_contract: None,
            cdp_contract: None,
            revenue_contract: None,
            usage_fee: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: Some(vec![membrane::types::StringEntry { entry: USER.to_string(), remove: false }]),
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: None,
        },
        &[],
    ).unwrap();

    // Enter vault to set deposits baseline
    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None },
        &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)],
    ).unwrap();

    // OTHER (non-whitelisted) does large swap to fill global limit
    // Stay under per-address limit (5% = 10k) but fill global limit (20% = 40k)
    app.execute_contract(Addr::unchecked(OTHER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();
    app.execute_contract(Addr::unchecked("other2"), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();
    app.execute_contract(Addr::unchecked("other3"), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();
    app.execute_contract(Addr::unchecked("other4"), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();

    // Check global status - should show OTHER's contribution
    let global_status = query_global_rate_limit(&app, &contract);
    assert_eq!(global_status.net_flow_base, 40_000);
    assert_eq!(global_status.entries_count, 4);

    // USER (whitelisted) should be able to do large swap without affecting global limit
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(5_000, ASSET_B)).unwrap();

    // Global status should be unchanged
    let global_status = query_global_rate_limit(&app, &contract);
    assert_eq!(global_status.net_flow_base, 40_000); // Still only OTHER's contribution
    assert_eq!(global_status.entries_count, 4);
}

#[test]
fn global_rate_limit_separate_window_from_per_address() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Update config to have different windows: 1 hour for per-address, 2 hours for global
    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            asset_a_to_b_rate: None,
            target_ratio: None,
            tokenfactory_contract: None,
            cdp_contract: None,
            revenue_contract: None,
            usage_fee: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: Some(60 * 60), // 1 hour
            rate_limit_threshold: None,
            allowlist: None,
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: Some(60 * 60 * 2), // 2 hours
            global_rate_limit_threshold: None,
        },
        &[],
    ).unwrap();

    // Enter vault to set deposits baseline
    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None },
        &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)],
    ).unwrap();

    // USER does swap
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();

    // Check both limits
    let per_address_status = query_rate_limit(&app, &contract, USER);
    let global_status = query_global_rate_limit(&app, &contract);
    
    assert_eq!(per_address_status.status.net_flow_base, 10_000);
    assert_eq!(global_status.net_flow_base, 10_000);

    // Fast forward 1.5 hours (per-address window expires, global window still active)
    app.update_block(|block| {
        block.time = block.time.plus_seconds(60 * 60 * 1 + 60 * 30); // 1.5 hours
    });

    // Check limits after time passage
    let per_address_status = query_rate_limit(&app, &contract, USER);
    let global_status = query_global_rate_limit(&app, &contract);
    
    // Per-address should be reset (window expired)
    assert_eq!(per_address_status.status.net_flow_base, 0);
    assert_eq!(per_address_status.status.entries_count, 0);
    
    // Global should still show the entry (window hasn't expired)
    assert_eq!(global_status.net_flow_base, 10_000);
    assert_eq!(global_status.entries_count, 1);
}

#[test]
fn global_rate_limit_configuration_updates() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Check initial config
    let config: membrane::transmuter::Config = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::Config {})
        .unwrap();
    
    assert_eq!(config.global_rate_limit_window_secs, 60 * 60 * 24); // 24 hours
    assert_eq!(config.global_rate_limit_threshold, Decimal::percent(20)); // 20%

    // Update global rate limit configuration
    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            asset_a_to_b_rate: None,
            target_ratio: None,
            tokenfactory_contract: None,
            cdp_contract: None,
            revenue_contract: None,
            usage_fee: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: None,
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: Some(60 * 60 * 12), // 12 hours
            global_rate_limit_threshold: Some(Decimal::percent(15)), // 15%
        },
        &[],
    ).unwrap();

    // Check updated config
    let config: membrane::transmuter::Config = app
        .wrap()
        .query_wasm_smart(&contract, &QueryMsg::Config {})
        .unwrap();
    
    assert_eq!(config.global_rate_limit_window_secs, 60 * 60 * 12); // 12 hours
    assert_eq!(config.global_rate_limit_threshold, Decimal::percent(15)); // 15%
}

#[test]
fn global_rate_limit_validation_errors() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Test invalid global window (zero)
    let res = app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            asset_a_to_b_rate: None,
            target_ratio: None,
            tokenfactory_contract: None,
            cdp_contract: None,
            revenue_contract: None,
            usage_fee: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: None,
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: Some(0), // Invalid
            global_rate_limit_threshold: None,
        },
        &[],
    );
    assert!(res.is_err());
    // let error_msg = res.unwrap_err().to_string();
    // println!("Actual error: {}", error_msg);
    // assert!(error_msg.contains("Validation error"));

    // Test invalid global threshold (zero)
    let res = app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            asset_a_to_b_rate: None,
            target_ratio: None,
            tokenfactory_contract: None,
            cdp_contract: None,
            revenue_contract: None,
            usage_fee: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: None,
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: Some(Decimal::zero()), // Invalid
        },
        &[],
    );
    assert!(res.is_err());
    // assert!(res.unwrap_err().to_string().contains("Validation error"));

    // Test invalid global threshold (> 1)
    let res = app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::UpdateConfig {
            owner: None,
            deposit_pair: None,
            composition_leeway: None,
            asset_a_to_b_rate: None,
            target_ratio: None,
            tokenfactory_contract: None,
            cdp_contract: None,
            revenue_contract: None,
            usage_fee: None,
            swap_history_cap: None,
            volume_history_cap: None,
            rate_limit_window_secs: None,
            rate_limit_threshold: None,
            allowlist: None,
            allowlist_rate_limit_threshold: None,
            global_rate_limit_window_secs: None,
            global_rate_limit_threshold: Some(Decimal::percent(101)), // Invalid (> 1)
        },
        &[],
    );
    assert!(res.is_err());
    // assert!(res.unwrap_err().to_string().contains("Validation error"));
}

#[test]
fn global_rate_limit_dual_enforcement() {
    let mut app = setup_app();
    let contract = instantiate_transmuter(&mut app);

    // Enter vault to set deposits baseline
    app.execute_contract(
        Addr::unchecked(ADMIN),
        contract.clone(),
        &ExecuteMsg::EnterVault { recipient: None },
        &[coin(100_000, ASSET_A), coin(100_000, ASSET_B)],
    ).unwrap();

    // USER does swap that would exceed per-address limit but not global limit
    // Per-address: 5% of 200k = 10k threshold
    // Global: 20% of 200k = 40k threshold
    
    // First, fill up per-address limit
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();
    
    // Try to exceed per-address limit (should fail even though global limit not reached)
    let res = app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(1, ASSET_B));
    assert!(res.is_err());
    // assert!(res.unwrap_err().to_string().contains("Rate limit exceeded"));

    // Reset USER's per-address limit by doing opposite swap
    app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_A)).unwrap();

    // Now fill up global limit with OTHER user (multiple swaps to stay under per-address limit)
    app.execute_contract(Addr::unchecked(OTHER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();
    app.execute_contract(Addr::unchecked("other2"), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();
    app.execute_contract(Addr::unchecked("other3"), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();
    app.execute_contract(Addr::unchecked("other4"), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(10_000, ASSET_B)).unwrap();

    // USER should now be blocked by global limit even though per-address limit is fine
    let res = app.execute_contract(Addr::unchecked(USER), contract.clone(), &ExecuteMsg::Transmute { recipient: None }, &coins(1, ASSET_B));
    assert!(res.is_err());
    // assert!(res.unwrap_err().to_string().contains("Global rate limit exceeded"));
}
