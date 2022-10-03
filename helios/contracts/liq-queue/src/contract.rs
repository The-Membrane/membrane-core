use std::env;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, to_binary, Addr, Api, Binary, Deps, DepsMut, Env, MessageInfo, QueryRequest, Response,
    StdError, StdResult, Uint128, WasmQuery,
};
use cw2::set_contract_version;
use membrane::liq_queue::{ExecuteMsg, InstantiateMsg, QueryMsg};
use membrane::math::{Decimal256, Uint256};
use membrane::positions::{BasketResponse, QueryMsg as CDP_QueryMsg};
use membrane::types::{Asset, AssetInfo, PremiumSlot, Queue};

use crate::bid::{claim_liquidations, execute_liquidation, retract_bid, store_queue, submit_bid};
use crate::error::ContractError;
use crate::query::{
    query_bid, query_bids_by_user, query_config, query_liquidatible, query_premium_slot,
    query_premium_slots, query_queue, query_queues, query_user_claims,
};
use crate::state::{Config, CONFIG, QUEUES};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:liq-queue";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let config: Config;

    let positions_contract = deps.api.addr_validate(&msg.positions_contract)?;

    //Set Bid Asset
    let bid_asset: AssetInfo;
    if let Some(basket_id) = msg.basket_id {
        //Get bid_asset from Basket
        bid_asset = deps
            .querier
            .query::<BasketResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: positions_contract.to_string(),
                msg: to_binary(&CDP_QueryMsg::GetBasket { basket_id })?,
            }))?
            .credit_asset
            .info;
    } else if let Some(asset) = msg.bid_asset {
        bid_asset = asset;
    } else {
        return Err(ContractError::CustomError {
            val: String::from("Need a bid_asset"),
        });
    };

    if msg.owner.is_some() {
        config = Config {
            owner: deps.api.addr_validate(&msg.owner.unwrap())?,
            positions_contract,
            waiting_period: msg.waiting_period,
            added_assets: Some(vec![]),
            bid_asset,
        };
    } else {
        config = Config {
            owner: info.sender,
            positions_contract,
            waiting_period: msg.waiting_period,
            added_assets: Some(vec![]),
            bid_asset,
        };
    }

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;

    let res = Response::new();
    let mut attrs = vec![];

    attrs.push(("method", "instantiate"));

    let c = &config.owner.to_string();
    attrs.push(("owner", c));

    Ok(res.add_attributes(attrs))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        //Receive but don't act upon
        ExecuteMsg::Receive(Cw20ReceiveMsg) => Ok(Response::new().add_attribute(
            "asset_received",
            format!("{} {}", Cw20ReceiveMsg.amount, info.sender),
        )),
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
            bid_with,
            basket_id,
            position_id,
            position_owner,
        } => execute_liquidation(
            deps,
            env,
            info,
            collateral_amount,
            bid_for,
            collateral_price,
            credit_price,
            bid_with,
            basket_id,
            position_id,
            position_owner,
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
            waiting_period,
            basket_id,
        } => update_config(
            deps,
            info,
            owner,
            positions_contract,
            waiting_period,
            basket_id,
        ),
    }
} //Functions assume Cw20 asset amounts are taken from Messageinfo

fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    positions_contract: Option<String>,
    waiting_period: Option<u64>,
    basket_id: Option<Uint128>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    //Only owner can update
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    if owner.is_some() {
        config.owner = deps.api.addr_validate(&owner.unwrap())?;
    }
    if positions_contract.is_some() {
        config.positions_contract = deps.api.addr_validate(&positions_contract.unwrap())?;
    }
    if waiting_period.is_some() {
        config.waiting_period = waiting_period.unwrap();
    }
    if let Some(basket_id) = basket_id {
        //Get bid_asset from Basket
        let bid_asset = deps
            .querier
            .query::<BasketResponse>(&QueryRequest::Wasm(WasmQuery::Smart {
                contract_addr: config.clone().positions_contract.to_string(),
                msg: to_binary(&CDP_QueryMsg::GetBasket { basket_id })?,
            }))?
            .credit_asset
            .info;

        config.bid_asset = bid_asset;
    }

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attributes(vec![
        attr("method", "update_config"),
        attr("owner", config.owner.to_string()),
        attr("waiting_period", config.waiting_period.to_string()),
        attr("basket_id", config.bid_asset.to_string()),
    ]))
}

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

    if max_premium.is_some() {
        queue.max_premium = max_premium.unwrap();
    }
    if bid_threshold.is_some() {
        queue.bid_threshold = bid_threshold.unwrap();
    }

    store_queue(deps.storage, bid_for.to_string(), queue.clone())?;

    Ok(Response::new().add_attributes(vec![
        attr("method", "edit_queue"),
        attr("max_premium", queue.max_premium.to_string()),
        attr("bid_threshold", queue.bid_threshold.to_string()),
    ]))
}

fn add_queue(
    deps: DepsMut,
    info: MessageInfo,
    bid_for: AssetInfo,
    max_premium: Uint128, //A slot for each premium is created when queue is created
    bid_threshold: Uint256,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    let bid_asset = config.clone().bid_asset;

    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let mut slots: Vec<PremiumSlot> = vec![];

    let max_premium_plus_1 = (max_premium + Uint128::new(1u128)).u128();

    for premium in 0..max_premium_plus_1 as u64 {
        slots.push(PremiumSlot {
            bids: vec![],
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

//Refactored Terraswap function
pub fn assert_sent_native_token_balance(
    asset: &Asset,
    message_info: &MessageInfo,
) -> StdResult<()> {
    if let AssetInfo::NativeToken { denom } = &asset.info {
        match message_info.funds.iter().find(|x| x.denom == *denom) {
            Some(coin) => {
                if asset.amount == coin.amount {
                    Ok(())
                } else {
                    Err(StdError::generic_err(
                        "Sent coin.amount is different from asset.amount",
                    ))
                }
            }
            None => Err(StdError::generic_err(
                "Incorrect denomination, sent asset denom and asset.info.denom differ",
            )),
        }
    } else {
        Err(StdError::generic_err(
            "Asset type not native, check Msg schema and use AssetInfo::Token{ address: Addr }",
        ))
    }
}

//Validate Recipient
pub fn validate_position_owner(
    deps: &dyn Api,
    info: MessageInfo,
    recipient: Option<String>,
) -> StdResult<Addr> {
    let valid_recipient: Addr = if let Some(recipient) = recipient {
        deps.addr_validate(&recipient)?
    } else {
        info.sender
    };
    Ok(valid_recipient)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&query_config(deps)?),
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
        QueryMsg::Queue { bid_for } => to_binary(&query_queue(deps, bid_for)?),
        QueryMsg::Queues { start_after, limit } => {
            to_binary(&query_queues(deps, start_after, limit)?)
        }
    }
}
