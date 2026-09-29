/// Shared test helpers for ltv_disco contract tests
use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info, MockApi, MockQuerier};
use cosmwasm_std::{
    coins, from_json, to_json_binary, Coin, Decimal, MemoryStorage, OwnedDeps, Uint128, Env,
    SystemResult, ContractResult, Binary, Api,
};
use membrane::ltv_disco::{
    BackingDepositInput, Config, ExecuteMsg, InstantiateMsg, QueryMsg,
    AssetQueueResponse, BackingDepositResponse, PendingClaimsResponse,
    AllUserDepositsResponse, UnstakeRequestsResponse, SlotWeightsResponse,
};
use membrane::types::DepositDenom;
use ltv_disco::contract::{execute, instantiate, query};
use ltv_disco::state::CONFIG;

// Denoms (non-address strings)
pub const COLLATERAL: &str = "collateral";
pub const CDT: &str = "cdt";

/// Vault tokens per base token on first deposit (precision multiplier)
pub const VT_SCALE: u128 = 1_000_000;

/// Helper to convert base amount to vault token amount (first deposit 1:1 ratio)
pub fn vt(base_amount: u128) -> Uint128 {
    Uint128::new(base_amount * VT_SCALE)
}

/// Make a valid bech32 address from a human-readable label
pub fn addr(label: &str) -> String {
    MockApi::default().addr_make(label).to_string()
}

/// Create test deps with default mocked querier (returns empty for all wasm queries)
pub fn setup_deps() -> OwnedDeps<MemoryStorage, MockApi, MockQuerier> {
    let mut deps = mock_dependencies();
    // Default: return empty binary for any wasm query (prevents panics)
    deps.querier.update_wasm(|_query| {
        SystemResult::Ok(ContractResult::Ok(Binary::default()))
    });
    deps
}

/// Create a standard InstantiateMsg
pub fn standard_instantiate_msg() -> InstantiateMsg {
    InstantiateMsg {
        owner: Some(addr("owner")),
        cdp_contract: addr("cdp_contract"),
        deposit_denom: DepositDenom {
            denom: COLLATERAL.to_string(),
            vault_info: None,
        },
        cdt_denom: CDT.to_string(),
        minimum_deposit: Uint128::new(1000),
        unstaking_period: Some(172800), // 2 days
        oracle_contract: addr("oracle"),
        chain_proxy_contract: addr("chain_proxy"),
        emissions_voting_contract: None,
        affiliate_fee: Some(Decimal::percent(1)),
        max_management_fee: Some(Decimal::percent(5)),
        points_system_contract: None,
        revenue_distributor: None,
        auction_contract: Some(addr("auction_contract")),
        mbrn_denom: None,
        liquidation_engine_contract: None,
    }
}

/// Instantiate the contract with standard config
pub fn setup_contract(deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>) -> Config {
    let msg = standard_instantiate_msg();
    let info = mock_info(&addr("creator"), &[]);
    let env = mock_env();
    instantiate(deps.as_mut(), env, info, msg).unwrap();
    CONFIG.load(&deps.storage).unwrap()
}

/// Instantiate with emissions voting contract
pub fn setup_contract_with_emissions(deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>) -> Config {
    let mut msg = standard_instantiate_msg();
    msg.emissions_voting_contract = Some(addr("emissions_contract"));
    let info = mock_info(&addr("creator"), &[]);
    let env = mock_env();
    instantiate(deps.as_mut(), env, info, msg).unwrap();
    CONFIG.load(&deps.storage).unwrap()
}

/// Create queue for an asset with default LTV range (50%-90%)
pub fn create_queue(deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>, asset: &str) {
    create_queue_with_ltv(deps, asset, Decimal::percent(50), Decimal::percent(90));
}

/// Create queue for an asset with specific LTV range
pub fn create_queue_with_ltv(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>,
    asset: &str,
    min_ltv: Decimal,
    max_ltv: Decimal,
) {
    let env = mock_env();
    let info = mock_info(&addr("owner"), &[]);
    execute(
        deps.as_mut(),
        env,
        info,
        ExecuteMsg::CreateQueue {
            asset: asset.to_string(),
            min_ltv,
            max_ltv,
        },
    ).unwrap();
}

/// Submit a deposit and return the response
pub fn submit_deposit(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>,
    env: &Env,
    sender: &str,
    asset: &str,
    slot: u8,
    amount: u128,
) -> cosmwasm_std::Response {
    let info = mock_info(sender, &coins(amount, asset));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: asset.to_string(),
                slot,
            },
            deposit_owner: None,
            deposit_id: None,
            manager: None,
            affiliate_address: None,
            revenue_destination: None,
        },
    ).unwrap()
}

/// Submit deposit on behalf of another user
pub fn submit_deposit_for_user(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>,
    env: &Env,
    sender: &str,
    owner: &str,
    asset: &str,
    slot: u8,
    amount: u128,
    manager: Option<String>,
) -> cosmwasm_std::Response {
    let info = mock_info(sender, &coins(amount, asset));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::SubmitDeposit {
            deposit_input: BackingDepositInput {
                asset: asset.to_string(),
                slot,
            },
            deposit_owner: Some(owner.to_string()),
            deposit_id: None,
            manager,
            affiliate_address: None,
            revenue_destination: None,
        },
    ).unwrap()
}

/// Query asset queue
pub fn query_queue(deps: &OwnedDeps<MemoryStorage, MockApi, MockQuerier>, asset: &str) -> AssetQueueResponse {
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::GetAssetQueue {
            assets: vec![asset.to_string()],
            limit: None,
            start_after: None,
        },
    ).unwrap();
    from_json(res).unwrap()
}

/// Query a specific backing deposit
pub fn query_deposit(
    deps: &OwnedDeps<MemoryStorage, MockApi, MockQuerier>,
    user: &str,
    asset: &str,
    slot: u8,
    deposit_id: u128,
) -> BackingDepositResponse {
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::GetBackingDeposit {
            user: user.to_string(),
            asset: asset.to_string(),
            slot,
            deposit_id: Uint128::new(deposit_id),
        },
    ).unwrap();
    from_json(res).unwrap()
}

/// Query all user deposits
pub fn query_all_deposits(deps: &OwnedDeps<MemoryStorage, MockApi, MockQuerier>, user: &str) -> AllUserDepositsResponse {
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::GetAllUserDeposits { user: user.to_string() },
    ).unwrap();
    from_json(res).unwrap()
}

/// Query pending claims
pub fn query_claims(deps: &OwnedDeps<MemoryStorage, MockApi, MockQuerier>, user: &str, asset: &str) -> PendingClaimsResponse {
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::PendingClaims { user: user.to_string(), asset: asset.to_string() },
    ).unwrap();
    from_json(res).unwrap()
}

/// Query unstake requests
pub fn query_unstakes(deps: &OwnedDeps<MemoryStorage, MockApi, MockQuerier>, user: &str, asset: &str) -> UnstakeRequestsResponse {
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::GetUnstakeRequests { user: user.to_string(), asset: asset.to_string() },
    ).unwrap();
    from_json(res).unwrap()
}

/// Query slot weights
pub fn query_weights(deps: &OwnedDeps<MemoryStorage, MockApi, MockQuerier>, asset: &str) -> SlotWeightsResponse {
    let res = query(
        deps.as_ref(),
        mock_env(),
        QueryMsg::GetSlotWeights { asset: asset.to_string() },
    ).unwrap();
    from_json(res).unwrap()
}

/// Advance env time by seconds
pub fn advance_time(env: &mut Env, seconds: u64) {
    env.block.time = env.block.time.plus_seconds(seconds);
    env.block.height += seconds / 5; // ~5 seconds per block
}

/// Add revenue to asset queue
pub fn add_revenue(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>,
    env: &Env,
    asset: &str,
    cdt_amount: u128,
) -> cosmwasm_std::Response {
    let info = mock_info(&addr("anyone"), &coins(cdt_amount, CDT));
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::AddRevenue { asset: asset.to_string() },
    ).unwrap()
}

/// Request unstake
pub fn request_unstake(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>,
    env: &Env,
    sender: &str,
    asset: &str,
    slot: u8,
    deposit_id: u128,
    amount: Option<u128>,
) -> cosmwasm_std::Response {
    let info = mock_info(sender, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::RequestUnstake {
            asset: asset.to_string(),
            slot,
            deposit_id: Uint128::new(deposit_id),
            amount: amount.map(Uint128::new),
        },
    ).unwrap()
}

/// Complete unstake
pub fn complete_unstake(
    deps: &mut OwnedDeps<MemoryStorage, MockApi, MockQuerier>,
    env: &Env,
    sender: &str,
    asset: &str,
    slot: u8,
    deposit_id: u128,
) -> cosmwasm_std::Response {
    let info = mock_info(sender, &[]);
    execute(
        deps.as_mut(),
        env.clone(),
        info,
        ExecuteMsg::CompleteUnstake {
            asset: asset.to_string(),
            slot,
            deposit_id: Uint128::new(deposit_id),
        },
    ).unwrap()
}
