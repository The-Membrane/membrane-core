use std::env;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, to_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response,
    StdResult, Uint128, QueryRequest, WasmQuery, 
};
use cw2::set_contract_version;
use membrane::cdp::QueryMsg as CDP_QueryMsg;
use membrane::liq_queue::{Config, ExecuteMsg, InstantiateMsg, QueryMsg};
use membrane::math::{Decimal256, Uint256};
use membrane::types::{Asset, AssetInfo, PremiumSlot, Queue, Basket};

use crate::bid::{claim_liquidations, execute_liquidation, retract_bid, submit_bid};
use crate::error::ContractError;
use crate::query::{
    query_bid, query_bids_by_user, query_liquidatible, query_premium_slot,
    query_premium_slots, query_queues, query_user_claims,
};
use crate::state::{CONFIG, QUEUES, OWNERSHIP_TRANSFER};

// Modifications from origin

// - Automatic activation after wait_period elapses. This increases computation time in return for less reliance on external contract calls.
// - Liquidations send the RepayMsg for the position in the Positions contract
// - Prices are taken from input by the Positions contract, the messages are guaranteed the same block so the price will be block_time + Position's config oracle_time_limit second's old.
// - The position is assumed insolvent since called by the Positions contract, ie there is no additional solvency check in this contract.
// - ExecuteMsg::Liquidate doesn't take any assets up front, instead receiving assets in the Reply fn of the Positions contract
// - Removed bid_with, instead saving the bid_asset from the Positions contract
// - Added minimum_bid amount & maximum_waiting_bids to config
// - Created a separate Vector for PremiumSlot waiting bids
// - Submitted bids on the (bid) threshold get split into 1 active bid & 1 waiting bid

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:liq-queue";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let config: Config;

    let positions_contract = deps.api.addr_validate(&msg.positions_contract)?;
    
    //Get bid_asset from Basket
    let bid_asset = deps
        .querier
        .query::<Basket>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: positions_contract.to_string(),
            msg: to_binary(&CDP_QueryMsg::GetBasket { })?,
        }))?
        .credit_asset
        .info;

    if msg.owner.is_some() {
        config = Config {
            owner: deps.api.addr_validate(&msg.owner.unwrap())?,
            positions_contract,
            osmosis_proxy_contract: deps.api.addr_validate(&msg.osmosis_proxy_contract)?,
            waiting_period: msg.waiting_period,
            added_assets: Some(vec![]),
            bid_asset,
            minimum_bid: msg.minimum_bid,
            maximum_waiting_bids: msg.maximum_waiting_bids,
        };
    } else {
        config = Config {
            owner: info.sender,
            positions_contract,
            osmosis_proxy_contract: deps.api.addr_validate(&msg.osmosis_proxy_contract)?,
            waiting_period: msg.waiting_period,
            added_assets: Some(vec![]),
            bid_asset,
            minimum_bid: msg.minimum_bid,
            maximum_waiting_bids: msg.maximum_waiting_bids,
        };
    }

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;
    

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::SubmitBid {
            bid_input,
            bid_owner,
        } => submit_bid(deps, info, env, bid_input, bid_owner),
        ExecuteMsg::RetractBid {
            bid_id,
            bid_for,
            amount,
        } => retract_bid(deps, info, env, bid_id, bid_for, amount),
        ExecuteMsg::Liquidate {
            credit_price,
            collateral_price,
            collateral_amount,
            bid_for,
        } => execute_liquidation(
            deps,
            env,
            info,
            collateral_amount,
            bid_for,
            collateral_price,
            credit_price,
        ),
        ExecuteMsg::ClaimLiquidations { bid_for, bid_ids } => {
            claim_liquidations(deps, env, info, bid_for, bid_ids)
        }
        ExecuteMsg::AddQueue {
            bid_for,
            max_premium,
            bid_threshold,
        } => add_queue(deps, info, bid_for, max_premium, bid_threshold),
        ExecuteMsg::UpdateQueue {
            bid_for,
            max_premium,
            bid_threshold,
        } => edit_queue(deps, info, bid_for, max_premium, bid_threshold),
        ExecuteMsg::UpdateConfig {
            owner,
            positions_contract,
            osmosis_proxy_contract,
            waiting_period,
            minimum_bid,
            maximum_waiting_bids,
        } => update_config(
            deps,
            info,
            owner,
            positions_contract,
            osmosis_proxy_contract,
            waiting_period,
            minimum_bid,
            maximum_waiting_bids,
        ),
    }
} //Functions assume Cw20 asset amounts are taken from Messageinfo

/// Update the contract config
fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    positions_contract: Option<String>,
    osmosis_proxy_contract: Option<String>,
    waiting_period: Option<u64>,
    minimum_bid: Option<Uint128>,
    maximum_waiting_bids: Option<u64>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    let mut attrs = vec![attr("method", "update_config")];

    //Assert Authority
    if info.sender != config.owner {
        //Check if ownership transfer is in progress & transfer if so
        if info.sender == OWNERSHIP_TRANSFER.load(deps.storage)? {
            config.owner = info.sender;
        } else {
            return Err(ContractError::Unauthorized {});
        }
    }

    if let Some(owner) = owner {
        let valid_addr = deps.api.addr_validate(&owner)?;

        //Set owner transfer state
        OWNERSHIP_TRANSFER.save(deps.storage, &valid_addr)?;
        attrs.push(attr("owner_transfer", valid_addr));     
    };
    if let Some(positions_contract) = positions_contract {
        let valid_addr = deps.api.addr_validate(&positions_contract)?;
        config.positions_contract = valid_addr;
    }
    if let Some(osmosis_proxy_contract) = osmosis_proxy_contract {
        let valid_addr = deps.api.addr_validate(&osmosis_proxy_contract)?;
        config.osmosis_proxy_contract = valid_addr;
    }
    if waiting_period.is_some() {
        config.waiting_period = waiting_period.unwrap();
    }
    if let Some(minimum_bid) = minimum_bid {
        config.minimum_bid = minimum_bid;
    }
    if let Some(maximum_waiting_bids) = maximum_waiting_bids {
        config.maximum_waiting_bids = maximum_waiting_bids;
    }

    CONFIG.save(deps.storage, &config)?;

    attrs.push(attr("updated_config", format!("{:?}", config)));  

    Ok(Response::new().add_attributes(attrs))
}

/// Edit queue
fn edit_queue(
    deps: DepsMut,
    info: MessageInfo,
    bid_for: AssetInfo,
    max_premium: Option<Uint128>,
    bid_threshold: Option<Uint256>,
) -> Result<Response, ContractError> {

    let config = CONFIG.load(deps.storage)?;

    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let mut queue = QUEUES.load(deps.storage, bid_for.to_string())?;

    if let Some(new_premium) = max_premium {
        //Enforce max_premium range of 1%-50%
        if new_premium > Uint128::from(50u128) || new_premium < Uint128::from(1u128) {
            return Err(ContractError::InvalidPremium {});
        }

        let new_premium_delta = match new_premium.checked_sub(queue.max_premium) {
            Ok(delta) => delta,
            Err(_err) => Uint128::zero(),
        };

        //Add new slots if there is a positive delta
        if !new_premium_delta.is_zero() {

            let premium_floor = (queue.max_premium.u128() + 1) as u64;
            let premium_ceiling = (new_premium.u128()) as u64;
    
            for premium in premium_floor..=premium_ceiling {
                queue.slots.push(PremiumSlot {
                    bids: vec![],
                    waiting_bids: vec![],
                    liq_premium: Decimal256::percent(premium), //This is a hard coded 1% per slot
                    sum_snapshot: Decimal256::zero(),
                    product_snapshot: Decimal256::one(),
                    total_bid_amount: Uint256::zero(),
                    last_total: 0u64,
                    current_epoch: Uint128::zero(),
                    current_scale: Uint128::zero(),
                    residue_collateral: Decimal256::zero(),
                    residue_bid: Decimal256::zero(),
                });
            }
        }
      
            
        //Set new max premium
        queue.max_premium = new_premium;

    }
    if let Some(bid_threshold) = bid_threshold {
        //Enforce bid_threshold range of 1M-10M
        if bid_threshold > Uint256::from(10_000_000u128) || bid_threshold < Uint256::from(1_000_000u128) {
            return Err(ContractError::InvalidBidThreshold {});
        } 
        queue.bid_threshold = bid_threshold;
    }

    QUEUES.save(deps.storage, bid_for.to_string(), &queue)?;

    Ok(Response::new().add_attributes(vec![
        attr("method", "edit_queue"),
        attr("max_premium", queue.max_premium.to_string()),
        attr("bid_threshold", queue.bid_threshold.to_string()),
    ]))
}

/// Add queue
fn add_queue(
    deps: DepsMut,
    info: MessageInfo,
    bid_for: AssetInfo,
    max_premium: Uint128, //A slot for each premium is created when queue is created
    bid_threshold: Uint256,
) -> Result<Response, ContractError> {

    let mut config = CONFIG.load(deps.storage)?;

    let bid_asset = config.clone().bid_asset;

    if info.sender != config.owner && info.sender != config.positions_contract{
        return Err(ContractError::Unauthorized {});
    }

    let mut slots: Vec<PremiumSlot> = vec![];

    let max_premium_plus_1 = (max_premium + Uint128::new(1u128)).u128();

    for premium in 0..max_premium_plus_1 as u64 {
        slots.push(PremiumSlot {
            bids: vec![],
            waiting_bids: vec![],
            liq_premium: Decimal256::percent(premium), //This is a hard coded 1% per slot
            sum_snapshot: Decimal256::zero(),
            product_snapshot: Decimal256::one(),
            total_bid_amount: Uint256::zero(),
            last_total: 0u64,
            current_epoch: Uint128::zero(),
            current_scale: Uint128::zero(),
            residue_collateral: Decimal256::zero(),
            residue_bid: Decimal256::zero(),
        });
    }

    let new_queue = Queue {
        bid_asset: Asset {
            info: bid_asset.clone(),
            amount: Uint128::zero(),
        },
        max_premium,
        slots,
        current_bid_id: Uint128::from(1u128),
        bid_threshold,
    };

    //Save new queue
    QUEUES.update(
        deps.storage,
        bid_for.to_string(),
        |queue| -> Result<Queue, ContractError> {
            match queue {
                Some(_queue) => Err(ContractError::DuplicateQueue {}),
                None => Ok(new_queue),
            }
        },
    )?;

    //Save Config
    let mut new_assets = config.added_assets.unwrap();
    new_assets.push(bid_for.clone());

    config.added_assets = Some(new_assets);

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attributes(vec![
        attr("method", "add_queue"),
        attr("bid_for", bid_for.to_string()),
        attr("bid_asset", bid_asset.to_string()),
        attr("max_premium", max_premium.to_string()),
        attr("bid_threshold", bid_threshold.to_string()),
    ]))
}


#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::CheckLiquidatible {
            bid_for,
            collateral_price,
            collateral_amount,
            credit_info,
            credit_price,
        } => to_binary(&query_liquidatible(
            deps,
            bid_for,
            collateral_price,
            collateral_amount,
            credit_info,
            credit_price,
        )?),
        QueryMsg::PremiumSlot { bid_for, premium } => {
            to_binary(&query_premium_slot(deps, bid_for, premium)?)
        }
        QueryMsg::PremiumSlots {
            bid_for,
            start_after,
            limit,
        } => to_binary(&query_premium_slots(deps, bid_for, start_after, limit)?),
        QueryMsg::UserClaims { user } => to_binary(&query_user_claims(deps, user)?),
        QueryMsg::Bid { bid_for, bid_id } => to_binary(&query_bid(deps, bid_for, bid_id)?),
        QueryMsg::BidsByUser {
            bid_for,
            user,
            limit,
            start_after,
        } => to_binary(&query_bids_by_user(
            deps,
            bid_for,
            user,
            limit,
            start_after,
        )?),
        QueryMsg::Queue { bid_for } => to_binary(&QUEUES.load(deps.storage, bid_for.to_string())?.into_queue_response()),
        QueryMsg::Queues { start_after, limit } => {
            to_binary(&query_queues(deps, start_after, limit)?)
        }
    }
}