use std::env;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, from_binary, to_binary, Addr, Binary, CosmosMsg, Decimal, Deps, DepsMut, Env,
    MessageInfo, Reply, Response, StdError, StdResult, Uint128, WasmMsg, QuerierWrapper, QueryRequest, WasmQuery,
};
use cw2::set_contract_version;
use cw20::{Cw20ReceiveMsg, Cw20QueryMsg, BalanceResponse};

use membrane::debt_auction::ExecuteMsg as AuctionExecuteMsg;
use membrane::liq_queue::ExecuteMsg as LQ_ExecuteMsg;
use membrane::positions::{Config, CallbackMsg, Cw20HookMsg, ExecuteMsg, InstantiateMsg, QueryMsg};
use membrane::types::{
    cAsset, Asset, AssetInfo, Basket, Position, UserInfo,
};

use crate::error::ContractError;
use crate::positions::{
    assert_basket_assets, clone_basket, create_basket, deposit,
    edit_basket, increase_debt,
    liq_repay, mint_revenue, repay,
    withdraw, BAD_DEBT_REPLY_ID, CREATE_DENOM_REPLY_ID, WITHDRAW_REPLY_ID,
};
use crate::query::{
    query_bad_debt, query_basket, query_basket_credit_interest, query_basket_debt_caps,
    query_basket_insolvency, query_basket_positions, query_baskets, query_collateral_rates,
    query_position, query_position_insolvency, query_prop,
    query_user_positions,
};
use crate::liquidations::{liquidate, LIQ_QUEUE_REPLY_ID,
    SELL_WALL_REPLY_ID, USER_SP_REPAY_REPLY_ID, STABILITY_POOL_REPLY_ID,};
use crate::reply::{handle_liq_queue_reply, handle_stability_pool_reply, handle_sell_wall_reply, handle_create_denom_reply, handle_withdraw_reply, handle_sp_repay_reply};
use crate::state::{
    BASKETS, CONFIG, POSITIONS,
};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:cdp";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let mut config = Config {
        liq_fee: msg.liq_fee,
        owner: info.sender.clone(),
        current_basket_id: Uint128::from(1u128),
        stability_pool: None,
        dex_router: None,
        interest_revenue_collector: None,
        staking_contract: None,
        oracle_contract: None,
        osmosis_proxy: None,
        debt_auction: None,
        liquidity_contract: None,
        oracle_time_limit: msg.oracle_time_limit,
        cpc_margin_of_error: Decimal::percent(1),
        rate_slope_multiplier: Decimal::one(),
        debt_minimum: msg.debt_minimum,
        base_debt_cap_multiplier: Uint128::new(21u128),
        collateral_twap_timeframe: msg.collateral_twap_timeframe,
        credit_twap_timeframe: msg.credit_twap_timeframe,
    };

    //Set optional config parameters
    match msg.owner {
        Some(address) => match deps.api.addr_validate(&address) {
            Ok(addr) => config.owner = addr,
            Err(_) => {}
        },
        None => {}
    };
    match msg.stability_pool {
        Some(address) => match deps.api.addr_validate(&address) {
            Ok(addr) => config.stability_pool = Some(addr),
            Err(_) => {}
        },
        None => {}
    };
    match msg.dex_router {
        Some(address) => match deps.api.addr_validate(&address) {
            Ok(addr) => config.dex_router = Some(addr),
            Err(_) => {}
        },
        None => {}
    };
    match msg.staking_contract {
        Some(address) => match deps.api.addr_validate(&address) {
            Ok(addr) => config.staking_contract = Some(addr),
            Err(_) => {}
        },
        None => {}
    };
    match msg.oracle_contract {
        Some(address) => match deps.api.addr_validate(&address) {
            Ok(addr) => config.oracle_contract = Some(addr),
            Err(_) => {}
        },
        None => {}
    };
    match msg.interest_revenue_collector {
        Some(address) => match deps.api.addr_validate(&address) {
            Ok(addr) => config.interest_revenue_collector = Some(addr),
            Err(_) => {}
        },
        None => {}
    };
    match msg.osmosis_proxy {
        Some(address) => match deps.api.addr_validate(&address) {
            Ok(addr) => config.osmosis_proxy = Some(addr),
            Err(_) => {}
        },
        None => {}
    };
    match msg.debt_auction {
        Some(address) => match deps.api.addr_validate(&address) {
            Ok(addr) => config.debt_auction = Some(addr),
            Err(_) => {}
        },
        None => {}
    };
    if let Some(contract) = msg.liquidity_contract {
        match deps.api.addr_validate(&contract) {
            Ok(addr) => config.liquidity_contract = Some(addr),
            Err(_) => {}
        }
    }

    CONFIG.save(deps.storage, &config)?;

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    let mut attrs = vec![];
    attrs.push(attr("method", "instantiate"));
    attrs.push(attr("owner", info.sender.to_string()));

    Ok(Response::new().add_attributes(attrs))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::UpdateConfig {
            owner,
            stability_pool,
            dex_router,
            osmosis_proxy,
            debt_auction,
            staking_contract,
            oracle_contract,
            liquidity_contract,
            interest_revenue_collector,
            liq_fee,
            debt_minimum,
            base_debt_cap_multiplier,
            oracle_time_limit,
            collateral_twap_timeframe,
            credit_twap_timeframe,
            cpc_margin_of_error,
            rate_slope_multiplier,
        } => update_config(
            deps,
            info,
            owner,
            stability_pool,
            dex_router,
            osmosis_proxy,
            debt_auction,
            staking_contract,
            oracle_contract,
            liquidity_contract,
            interest_revenue_collector,
            liq_fee,
            debt_minimum,
            base_debt_cap_multiplier,
            oracle_time_limit,
            collateral_twap_timeframe,
            credit_twap_timeframe,
            cpc_margin_of_error,
            rate_slope_multiplier,
        ),
        ExecuteMsg::Receive(msg) => receive_cw20(deps, env, info, msg),
        ExecuteMsg::Deposit {
            position_owner,
            position_id,
            basket_id,
        } => {
            //Set asset from funds sent
            let valid_assets = info
                .clone()
                .funds
                .into_iter()
                .map(|coin| Asset {
                    info: AssetInfo::NativeToken { denom: coin.denom },
                    amount: coin.amount,
                })
                .collect::<Vec<Asset>>();

            let cAssets: Vec<cAsset> = assert_basket_assets(
                deps.storage,
                deps.querier,
                env.clone(),
                basket_id,
                valid_assets,
                true,
            )?;

            deposit(
                deps,
                env,
                info,
                position_owner,
                position_id,
                basket_id,
                cAssets,
            )
        }
        ExecuteMsg::Withdraw {
            position_id,
            basket_id,
            assets,
        } => {
            let cAssets: Vec<cAsset> = assert_basket_assets(
                deps.storage,
                deps.querier,
                env.clone(),
                basket_id,
                assets,
                false,
            )?;
            //If there is nothing being withdrawn, error
            if cAssets == vec![] { return Err(ContractError::CustomError { val: String::from("No withdrawal assets passed") }) }
            withdraw(deps, env, info, position_id, basket_id, cAssets)
        }

        ExecuteMsg::IncreaseDebt {
            basket_id,
            position_id,
            amount,
        } => increase_debt(deps, env, info, basket_id, position_id, amount),
        ExecuteMsg::Repay {
            basket_id,
            position_id,
            position_owner,
        } => {
            let basket: Basket = match BASKETS.load(deps.storage, basket_id.to_string()) {
                Err(_) => return Err(ContractError::NonExistentBasket {}),
                Ok(basket) => basket,
            };

            let credit_asset = assert_sent_native_token_balance(basket.credit_asset.info, &info)?;
            repay(
                deps.storage,
                deps.querier,
                deps.api,
                env,
                info,
                basket_id,
                position_id,
                position_owner,
                credit_asset,
            )
        }
        ExecuteMsg::LiqRepay {} => {
            if info.clone().funds.len() != 0 as usize {
                let credit_asset = Asset {
                    info: AssetInfo::NativeToken {
                        denom: info.clone().funds[0].clone().denom,
                    },
                    amount: info.clone().funds[0].amount,
                };
                liq_repay(deps, env, info, credit_asset)
            } else {
                return Err(ContractError::InvalidCredit {});
            }
        }
        ExecuteMsg::EditAdmin { owner } => edit_contract_owner(deps, info, owner),
        ExecuteMsg::EditcAsset {
            basket_id,
            asset,
            max_borrow_LTV,
            max_LTV,
        } => edit_cAsset(deps, info, basket_id, asset, max_borrow_LTV, max_LTV),
        ExecuteMsg::EditBasket {
            basket_id,
            added_cAsset,
            owner,
            liq_queue,
            credit_pool_ids,
            liquidity_multiplier,
            collateral_supply_caps,
            base_interest_rate,
            desired_debt_cap_util,
            credit_asset_twap_price_source,
            negative_rates,
        } => edit_basket(
            deps,
            info,
            basket_id,
            added_cAsset,
            owner,
            liq_queue,
            credit_pool_ids,
            liquidity_multiplier,
            collateral_supply_caps,
            base_interest_rate,
            desired_debt_cap_util,
            credit_asset_twap_price_source,
            negative_rates,
        ),
        ExecuteMsg::CreateBasket {
            owner,
            collateral_types,
            credit_asset,
            credit_price,
            base_interest_rate,
            desired_debt_cap_util,
            credit_pool_ids,
            liquidity_multiplier_for_debt_caps,
            liq_queue,
        } => create_basket(
            deps,
            info,
            env,
            owner,
            collateral_types,
            credit_asset,
            credit_price,
            base_interest_rate,
            desired_debt_cap_util,
            credit_pool_ids,
            liquidity_multiplier_for_debt_caps,
            liq_queue,
        ),
        ExecuteMsg::CloneBasket { basket_id } => clone_basket(deps, basket_id),
        ExecuteMsg::Liquidate {
            basket_id,
            position_id,
            position_owner,
        } => liquidate(
            deps.storage,
            deps.api,
            deps.querier,
            env,
            info,
            basket_id,
            position_id,
            position_owner,
        ),
        ExecuteMsg::MintRevenue {
            basket_id,
            send_to,
            repay_for,
            amount,
        } => mint_revenue(deps, info, env, basket_id, send_to, repay_for, amount),
        ExecuteMsg::Callback(msg) => {
            if info.sender == env.contract.address {
                callback_handler(deps, env, msg)
            } else {
                return Err(ContractError::Unauthorized {});
            }
        }
    }
}

fn edit_cAsset(
    deps: DepsMut,
    info: MessageInfo,
    basket_id: Uint128,
    asset: AssetInfo,
    max_borrow_LTV: Option<Decimal>,
    max_LTV: Option<Decimal>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    //Assert Authority
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let mut basket: Basket = match BASKETS.load(deps.storage, basket_id.to_string()) {
        Err(_) => return Err(ContractError::NonExistentBasket {}),
        Ok(basket) => basket,
    };

    let mut attrs = vec![
        attr("method", "edit_cAsset"),
        attr("basket", basket_id.clone().to_string()),
    ];

    let new_asset: cAsset;
    let mut msgs: Vec<CosmosMsg> = vec![];

    match basket
        .clone()
        .collateral_types
        .into_iter()
        .find(|cAsset| cAsset.asset.info.equal(&asset))
    {
        Some(mut asset) => {
            attrs.push(attr("asset", asset.clone().asset.info.to_string()));

            if let Some(LTV) = max_LTV {
                asset.max_LTV = LTV.clone();

                //Edit the asset's liq_queue max_premium
                //Create Liquidation Queue for its assets
                if basket.clone().liq_queue.is_some() {
                    //Gets Liquidation Queue max premium.
                    //The premium has to be at most 5% less than the difference between max_LTV and 100%
                    //The ideal variable for the 5% is the avg caller_liq_fee during high traffic periods
                    let max_premium = Uint128::new(95u128) - LTV.atomics();

                    msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: basket.clone().liq_queue.unwrap().into_string(),
                        msg: to_binary(&LQ_ExecuteMsg::UpdateQueue {
                            bid_for: asset.clone().asset.info,
                            max_premium: Some(max_premium),
                            bid_threshold: None,
                        })?,
                        funds: vec![],
                    }));
                }

                attrs.push(attr("max_LTV", LTV.to_string()));
            }

            if let Some(LTV) = max_borrow_LTV {
                if LTV < Decimal::percent(100) && LTV < asset.max_LTV {
                    asset.max_borrow_LTV = LTV.clone();
                    attrs.push(attr("max_borrow_LTV", LTV.to_string()));
                }
            }
            new_asset = asset;
        }
        None => {
            return Err(ContractError::CustomError {
                val: format!(
                    "Collateral type doesn't exist in basket {}",
                    basket_id.clone().to_string()
                ),
            })
        }
    };
    //Set and Save new basket
    basket.collateral_types = basket
        .clone()
        .collateral_types
        .into_iter()
        .filter(|asset| !asset.asset.info.equal(&new_asset.asset.info))
        .collect::<Vec<cAsset>>();

    basket.collateral_types.push(new_asset);

    BASKETS.save(deps.storage, basket_id.to_string(), &basket)?;

    Ok(Response::new().add_attributes(attrs).add_messages(msgs))
}

fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    stability_pool: Option<String>,
    dex_router: Option<String>,
    osmosis_proxy: Option<String>,
    debt_auction: Option<String>,
    staking_contract: Option<String>,
    oracle_contract: Option<String>,
    liquidity_contract: Option<String>,
    interest_revenue_collector: Option<String>,
    liq_fee: Option<Decimal>,
    debt_minimum: Option<Uint128>,
    base_debt_cap_multiplier: Option<Uint128>,
    oracle_time_limit: Option<u64>,
    collateral_twap_timeframe: Option<u64>,
    credit_twap_timeframe: Option<u64>,
    cpc_margin_of_error: Option<Decimal>,
    rate_slope_multiplier: Option<Decimal>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    //Assert Authority
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let mut attrs = vec![attr("method", "update_config")];

    //Match Optionals
    match owner {
        Some(owner) => {
            let valid_addr = deps.api.addr_validate(&owner)?;
            config.owner = valid_addr.clone();
            attrs.push(attr("new_owner", valid_addr.to_string()));
        }
        None => {}
    }
    match stability_pool {
        Some(stability_pool) => {
            let valid_addr = deps.api.addr_validate(&stability_pool)?;
            config.stability_pool = Some(valid_addr.clone());
            attrs.push(attr("new_stability_pool", valid_addr.to_string()));
        }
        None => {}
    }
    match dex_router {
        Some(dex_router) => {
            let valid_addr = deps.api.addr_validate(&dex_router)?;
            config.dex_router = Some(valid_addr.clone());
            attrs.push(attr("new_dex_router", valid_addr.to_string()));
        }
        None => {}
    }
    match osmosis_proxy {
        Some(osmosis_proxy) => {
            let valid_addr = deps.api.addr_validate(&osmosis_proxy)?;
            config.osmosis_proxy = Some(valid_addr.clone());
            attrs.push(attr("new_osmosis_proxy", valid_addr.to_string()));
        }
        None => {}
    }
    match debt_auction {
        Some(debt_auction) => {
            let valid_addr = deps.api.addr_validate(&debt_auction)?;
            config.debt_auction = Some(valid_addr.clone());
            attrs.push(attr("new_debt_auction", valid_addr.to_string()));
        }
        None => {}
    }
    match staking_contract {
        Some(staking_contract) => {
            let valid_addr = deps.api.addr_validate(&staking_contract)?;
            config.staking_contract = Some(valid_addr.clone());
            attrs.push(attr("new_staking_contract", valid_addr.to_string()));
        }
        None => {}
    }
    match oracle_contract {
        Some(oracle_contract) => {
            let valid_addr = deps.api.addr_validate(&oracle_contract)?;
            config.oracle_contract = Some(valid_addr.clone());
            attrs.push(attr("new_oracle_contract", valid_addr.to_string()));
        }
        None => {}
    }
    match liquidity_contract {
        Some(liquidity_contract) => {
            let valid_addr = deps.api.addr_validate(&liquidity_contract)?;
            config.liquidity_contract = Some(valid_addr.clone());
            attrs.push(attr("new_liquidity_contract", valid_addr.to_string()));
        }
        None => {}
    }
    match interest_revenue_collector {
        Some(interest_revenue_collector) => {
            let valid_addr = deps.api.addr_validate(&interest_revenue_collector)?;
            config.interest_revenue_collector = Some(valid_addr.clone());
            attrs.push(attr(
                "new_interest_revenue_collector",
                valid_addr.to_string(),
            ));
        }
        None => {}
    }
    match liq_fee {
        Some(liq_fee) => {
            config.liq_fee = liq_fee.clone();
            attrs.push(attr("new_liq_fee", liq_fee.to_string()));
        }
        None => {}
    }
    match debt_minimum {
        Some(debt_minimum) => {
            config.debt_minimum = debt_minimum.clone();
            attrs.push(attr("new_debt_minimum", debt_minimum.to_string()));
        }
        None => {}
    }
    match base_debt_cap_multiplier {
        Some(base_debt_cap_multiplier) => {
            config.base_debt_cap_multiplier = base_debt_cap_multiplier.clone();
            attrs.push(attr(
                "new_base_debt_cap_multiplier",
                base_debt_cap_multiplier.to_string(),
            ));
        }
        None => {}
    }
    match oracle_time_limit {
        Some(oracle_time_limit) => {
            config.oracle_time_limit = oracle_time_limit.clone();
            attrs.push(attr("new_oracle_time_limit", oracle_time_limit.to_string()));
        }
        None => {}
    }
    match collateral_twap_timeframe {
        Some(collateral_twap_timeframe) => {
            config.collateral_twap_timeframe = collateral_twap_timeframe.clone();
            attrs.push(attr(
                "new_collateral_twap_timeframe",
                collateral_twap_timeframe.to_string(),
            ));
        }
        None => {}
    }
    match credit_twap_timeframe {
        Some(credit_twap_timeframe) => {
            config.credit_twap_timeframe = credit_twap_timeframe.clone();
            attrs.push(attr(
                "new_credit_twap_timeframe",
                credit_twap_timeframe.to_string(),
            ));
        }
        None => {}
    }
    match cpc_margin_of_error {
        Some(cpc_margin_of_error) => {
            config.cpc_margin_of_error = cpc_margin_of_error.clone();
            attrs.push(attr(
                "new_cpc_margin_of_error",
                cpc_margin_of_error.to_string(),
            ));
        }
        None => {}
    }
    match rate_slope_multiplier {
        Some(rate_slope_multiplier) => {
            config.rate_slope_multiplier = rate_slope_multiplier.clone();
            attrs.push(attr(
                "new_rate_slope_multiplier",
                rate_slope_multiplier.to_string(),
            ));
        }
        None => {}
    }

    //Save new Config
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attributes(attrs))
}

pub fn callback_handler(
    deps: DepsMut,
    env: Env,
    msg: CallbackMsg,
) -> Result<Response, ContractError> {
    match msg {
        CallbackMsg::BadDebtCheck {
            basket_id,
            position_owner,
            position_id,
        } => check_for_bad_debt(deps, env, basket_id, position_id, position_owner),
    }
}

fn check_for_bad_debt(
    deps: DepsMut,
    env: Env,
    basket_id: Uint128,
    position_id: Uint128,
    position_owner: Addr,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    let basket: Basket = match BASKETS.load(deps.storage, basket_id.to_string()) {
        Err(_) => return Err(ContractError::NonExistentBasket {}),
        Ok(basket) => basket,
    };
    let positions: Vec<Position> = match POSITIONS.load(
        deps.storage,
        (basket_id.to_string(), position_owner.clone()),
    ) {
        Err(_) => return Err(ContractError::NoUserPositions {}),
        Ok(positions) => positions,
    };

    //Filter position by id
    let target_position = match positions.into_iter().find(|x| x.position_id == position_id) {
        Some(position) => position,
        None => return Err(ContractError::NonExistentPosition {}),
    };

    //We do a lazy check for bad debt by checking if there is debt without any assets left in the position
    //This is allowed bc any calls here will be after a liquidation where the sell wall would've sold all it could to cover debts
    let total_assets: Uint128 = target_position
        .collateral_assets
        .iter()
        .map(|asset| asset.asset.amount)
        .collect::<Vec<Uint128>>()
        .iter()
        .sum();

    if total_assets > Uint128::zero() || target_position.credit_amount.is_zero() {
        return Err(ContractError::PositionSolvent {});
    } else {
        let mut messages: Vec<CosmosMsg> = vec![];
        let mut bad_debt_amount = target_position.credit_amount;

        //If the basket has revenue, mint and repay the bad debt
        if !basket.pending_revenue.is_zero() {
            if bad_debt_amount >= basket.pending_revenue {
                //If bad_debt is greater or equal, mint all revenue to repay
                //and send the rest to the auction
                let mint_msg = ExecuteMsg::MintRevenue {
                    basket_id,
                    send_to: None,
                    repay_for: Some(UserInfo {
                        basket_id,
                        position_id,
                        position_owner: position_owner.to_string(),
                    }),
                    amount: None,
                };

                messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: env.contract.address.to_string(),
                    msg: to_binary(&mint_msg)?,
                    funds: vec![],
                }));

                bad_debt_amount -= basket.pending_revenue;
            } else {
                //If less than revenue, repay the debt and no auction
                let mint_msg = ExecuteMsg::MintRevenue {
                    basket_id,
                    send_to: None,
                    repay_for: Some(UserInfo {
                        basket_id,
                        position_id,
                        position_owner: position_owner.to_string(),
                    }),
                    amount: Some(bad_debt_amount),
                };

                messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: env.contract.address.to_string(),
                    msg: to_binary(&mint_msg)?,
                    funds: vec![],
                }));

                bad_debt_amount = Uint128::zero();
            }
        }

        //Send bad debt amount to the auction contract if greater than 0
        if config.debt_auction.is_some() && !bad_debt_amount.is_zero() {
            let auction_msg = AuctionExecuteMsg::StartAuction {
                repayment_position_info: UserInfo {
                    basket_id,
                    position_id,
                    position_owner: position_owner.to_string(),
                },
                debt_asset: Asset {
                    amount: bad_debt_amount,
                    info: basket.clone().credit_asset.info,
                },
            };

            messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.debt_auction.unwrap().to_string(),
                msg: to_binary(&auction_msg)?,
                funds: vec![],
            }));
        } else {
            return Err(ContractError::CustomError {
                val: "Debt Auction contract not added to config".to_string(),
            });
        }

        return Ok(Response::new().add_messages(messages).add_attributes(vec![
            attr("method", "check_for_bad_debt"),
            attr("bad_debt_amount", bad_debt_amount),
        ]));
    }
}

//From a receive cw20 hook. Comes from the contract address so easy to validate sent funds.
//Check if sent funds are equal to amount in msg so we don't have to recheck in the function
pub fn receive_cw20(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    cw20_msg: Cw20ReceiveMsg,
) -> Result<Response, ContractError> {
    let passed_asset: Asset = Asset {
        info: AssetInfo::Token {
            address: info.sender.clone(),
        },
        amount: cw20_msg.amount,
    };

    match from_binary(&cw20_msg.msg) {
        //This only allows 1 cw20 token at a time when opening a position, whereas you can add multiple native assets
        Ok(Cw20HookMsg::Deposit {
            position_owner,
            basket_id,
            position_id,
        }) => {
            let valid_owner_addr: Addr = if let Some(position_owner) = position_owner {
                deps.api.addr_validate(&position_owner)?
            } else {
                deps.api.addr_validate(&cw20_msg.sender.clone())?
            };

            let cAssets: Vec<cAsset> = assert_basket_assets(
                deps.storage,
                deps.querier,
                env.clone(),
                basket_id,
                vec![passed_asset],
                true,
            )?;

            deposit(
                deps,
                env,
                info,
                Some(valid_owner_addr.to_string()),
                position_id,
                basket_id,
                cAssets,
            )
        }
        Err(_) => Err(ContractError::Cw20MsgError {}),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.id {
        LIQ_QUEUE_REPLY_ID => handle_liq_queue_reply(deps, msg, env),
        STABILITY_POOL_REPLY_ID => handle_stability_pool_reply(deps, env, msg),
        SELL_WALL_REPLY_ID => handle_sell_wall_reply(deps, msg, env),
        CREATE_DENOM_REPLY_ID => handle_create_denom_reply(deps, msg),
        WITHDRAW_REPLY_ID => handle_withdraw_reply(deps, env, msg),
        USER_SP_REPAY_REPLY_ID => handle_sp_repay_reply(deps, env, msg),
        BAD_DEBT_REPLY_ID => Ok(Response::new()),
        id => Err(StdError::generic_err(format!("invalid reply id: {}", id))),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::GetPosition {
            position_id,
            basket_id,
            position_owner,
        } => {
            let valid_addr: Addr = deps.api.addr_validate(&position_owner)?;
            to_binary(&query_position(
                deps,
                env,
                position_id,
                basket_id,
                valid_addr,
            )?)
        }
        QueryMsg::GetUserPositions {
            basket_id,
            user,
            limit,
        } => {
            let valid_addr: Addr = deps.api.addr_validate(&user)?;
            to_binary(&query_user_positions(
                deps, env, basket_id, valid_addr, limit,
            )?)
        }
        QueryMsg::GetBasketPositions {
            basket_id,
            start_after,
            limit,
        } => to_binary(&query_basket_positions(
            deps,
            basket_id,
            start_after,
            limit,
        )?),
        QueryMsg::GetBasket { basket_id } => to_binary(&query_basket(deps, basket_id)?),
        QueryMsg::GetAllBaskets { start_after, limit } => {
            to_binary(&query_baskets(deps, start_after, limit)?)
        }
        QueryMsg::Propagation {} => to_binary(&query_prop(deps)?),
        QueryMsg::GetBasketDebtCaps { basket_id } => {
            to_binary(&query_basket_debt_caps(deps, env, basket_id)?)
        }
        QueryMsg::GetBasketBadDebt { basket_id } => to_binary(&query_bad_debt(deps, basket_id)?),
        QueryMsg::GetBasketInsolvency {
            basket_id,
            start_after,
            limit,
        } => to_binary(&query_basket_insolvency(
            deps,
            env,
            basket_id,
            start_after,
            limit,
        )?),
        QueryMsg::GetPositionInsolvency {
            basket_id,
            position_id,
            position_owner,
        } => to_binary(&query_position_insolvency(
            deps,
            env,
            basket_id,
            position_id,
            position_owner,
        )?),
        QueryMsg::GetBasketInterest { basket_id } => {
            to_binary(&query_basket_credit_interest(deps, env, basket_id)?)
        }
        QueryMsg::GetCollateralInterest { basket_id } => {
            to_binary(&query_collateral_rates(deps, env, basket_id)?)
        }
    }
}


pub fn get_contract_balances(
    querier: QuerierWrapper,
    env: Env,
    assets: Vec<AssetInfo>,
) -> Result<Vec<Uint128>, ContractError> {
    let mut balances = vec![];

    for asset in assets {
        match asset {
            AssetInfo::NativeToken { denom } => {
                balances.push(
                    querier
                        .query_balance(env.clone().contract.address, denom)?
                        .amount,
                );
            }
            AssetInfo::Token { address } => {
                let res: BalanceResponse =
                    querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
                        contract_addr: address.to_string(),
                        msg: to_binary(&Cw20QueryMsg::Balance {
                            address: env.contract.address.to_string(),
                        })?,
                    }))?;

                balances.push(res.balance);
            }
        }
    }

    Ok(balances)
}

pub fn edit_contract_owner(
    deps: DepsMut,
    info: MessageInfo,
    owner: String,
) -> Result<Response, ContractError> {
    let mut config: Config = CONFIG.load(deps.storage)?;

    if info.sender == config.owner {
        let valid_owner: Addr = deps.api.addr_validate(&owner)?;

        config.owner = valid_owner;

        CONFIG.save(deps.storage, &config)?;
    } else {
        return Err(ContractError::NotContractOwner {});
    }

    let response = Response::new()
        .add_attribute("method", "edit_contract_owner")
        .add_attribute("new_owner", owner);

    Ok(response)
}

//Refactored Terraswap function
pub fn assert_sent_native_token_balance(
    asset_info: AssetInfo,
    message_info: &MessageInfo,
) -> StdResult<Asset> {
    let asset: Asset;

    if let AssetInfo::NativeToken { denom } = &asset_info {
        match message_info.funds.iter().find(|x| x.denom == *denom) {
            Some(coin) => {
                if coin.amount > Uint128::zero() {
                    asset = Asset {
                        info: asset_info,
                        amount: coin.amount,
                    };
                } else {
                    return Err(StdError::generic_err("You gave me nothing to deposit"));
                }
            }
            None => {
                return Err(StdError::generic_err(
                    "Incorrect denomination, sent asset denom and asset.info.denom differ",
                ))
            }
        }
    } else {
        return Err(StdError::generic_err("Asset type not native, check Msg schema and use AssetInfo::NativeToken{ denom: String }"));
    }

    Ok(asset)
}
