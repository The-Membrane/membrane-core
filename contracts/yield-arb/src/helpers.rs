use cosmwasm_std::{Deps, DepsMut, Env, StdResult, Storage, Uint128};
use membrane::yield_arb::{MarketConditions, UserPosition, TVLSnapshot, DeploymentSnapshot};

use crate::state::{CONFIG, MARKET_CONDITIONS, MARKET_CONDITIONS_LIMIT, MARKET_CONDITIONS_WINDOW, TVL_TRACKER_WINDOW, TVL_TRACKER, TVL_TRACKER_LIMIT, USER_POSITIONS, USER_POSITIONS_LIMIT, DEPLOYMENT_SNAPSHOTS};
use membrane::mars_vault_token::{QueryMsg as MarsQueryMsg, Config as MarsConfig};

pub fn try_update_tvl(deps: &mut DepsMut, env: Env, new_tvl: Uint128) -> StdResult<()> {
    let mut tvls = TVL_TRACKER.may_load(deps.storage)?.unwrap_or_default();
    if let Some(last) = tvls.last() {
        if env.block.time.seconds() - last.timestamp < 86_400 { // one day
            return Ok(());
        }
    }
    //Only append data if its not the same TVL or if its been at least a day
    if !tvls.is_empty() && 
    tvls.last().map(|last| last.tvl == new_tvl) == Some(true)  ||
    tvls.last().map(|last| last.timestamp > env.block.time.seconds() - TVL_TRACKER_WINDOW) == Some(true) {
        return Ok(());
    }
    //Append data
    tvls.push(TVLSnapshot { tvl: new_tvl, timestamp: env.block.time.seconds() });
    //Enforce limit by removing oldest entries
    if tvls.len() > TVL_TRACKER_LIMIT { tvls.remove(0); }
    //Save data
    TVL_TRACKER.save(deps.storage, &tvls)?;
    Ok(())
}

pub fn get_current_tvl(deps: Deps) -> Uint128 {
    let cfg = match CONFIG.load(deps.storage) { Ok(c) => c, Err(_) => return Uint128::zero() };
    deps.querier
        .query_wasm_smart::<MarsConfig>(cfg.mars_vault_addr, &MarsQueryMsg::Config { })
        .map(|c: MarsConfig| c.total_deposit_tokens)
        .unwrap_or_else(|_| Uint128::zero())
}

pub fn append_user_position(deps: &mut DepsMut, snapshot: UserPosition) -> StdResult<()> {
    let user_key = snapshot.user.to_string();
    let mut positions = USER_POSITIONS.may_load(deps.storage, user_key.clone())?.unwrap_or_default();

    //Only append data if its not the same
    if !positions.is_empty() && 
    positions.last().map(|last| last.timestamp > snapshot.timestamp && last.collateral_amount == snapshot.collateral_amount && last.debt_amount == snapshot.debt_amount) == Some(true) {
        return Ok(());
    }
    //Append data
    positions.push(snapshot);
    // Enforce limit by removing oldest entries
    if positions.len() > USER_POSITIONS_LIMIT {
        positions.remove(0);
    }
    
    USER_POSITIONS.save(deps.storage, user_key, &positions)
}

pub fn append_market_conditions(deps: &mut DepsMut, mc: MarketConditions) -> StdResult<()> {
    let mut vec = MARKET_CONDITIONS.may_load(deps.storage)?.unwrap_or_default();

    //Only append data if its not the same and if its been at least a MARKET_CONDITIONS_WINDOW in seconds
    if !vec.is_empty() && 
    vec.last().map(|last| last.timestamp > mc.timestamp - MARKET_CONDITIONS_WINDOW) == Some(true) ||
    vec.last().map(|last| last.cdt_mint_cost == mc.cdt_mint_cost && last.vault_apr == mc.vault_apr) == Some(true) {
        return Ok(());
    }
    vec.push(mc);
    if vec.len() > MARKET_CONDITIONS_LIMIT {
        vec.remove(0);
    }
    MARKET_CONDITIONS.save(deps.storage, &vec)
}

/// Save or update deployment snapshot
/// On first loop: saves collateral_assets and block_time
/// On every loop: updates amount_looped (cumulative) and debt_taken (current)
pub fn update_deployment_snapshot(
    storage: &mut dyn Storage,
    user_key: String,
    collateral_assets: Option<Vec<membrane::types::cAsset>>,
    block_time: Option<u64>,
    amount_looped: Uint128,
    debt_taken: Uint128,
) -> StdResult<()> {
    let mut snapshot = DEPLOYMENT_SNAPSHOTS
        .may_load(storage, user_key.clone())?
        .unwrap_or_else(|| DeploymentSnapshot {
            collateral_assets: vec![],
            block_time: 0,
            amount_looped: Uint128::zero(),
            debt_taken: Uint128::zero(),
        });
    
    // Only update collateral_assets and block_time if this is the first loop
    if let Some(assets) = collateral_assets {
        snapshot.collateral_assets = assets;
    }
    if let Some(time) = block_time {
        snapshot.block_time = time;
    }
    
    // Always update amount_looped (cumulative) and debt_taken (current)
    snapshot.amount_looped = amount_looped;
    snapshot.debt_taken = debt_taken;
    
    DEPLOYMENT_SNAPSHOTS.save(storage, user_key, &snapshot)
}


