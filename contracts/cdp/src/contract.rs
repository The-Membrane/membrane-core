use std::env;
use std::str::FromStr;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, to_json_binary, Addr, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, QueryRequest, QuerierWrapper, Reply, Response, StdError, StdResult, Storage, Uint128, WasmMsg, WasmQuery
};

use membrane::auction::ExecuteMsg as AuctionExecuteMsg;
use membrane::helpers::{assert_sent_native_token_balance, get_contract_balances, asset_to_coin};
use membrane::liq_queue::ExecuteMsg as LQ_ExecuteMsg;
use membrane::cdp::{Config, CallbackMsg, ExecuteMsg, InstantiateMsg, QueryMsg, UpdateConfig, MigrateMsg};
use membrane::math::decimal_multiplication;
use membrane::stability_pool_vault::calculate_base_tokens;
use membrane::ltv_disco::{QueryMsg as LTVDisco_QueryMsg, ExecuteMsg as LTVDisco_ExecuteMsg};
use membrane::deployable_venue::QueryMsg as DeployableVenue_QueryMsg;
use membrane::types::{
    cAsset, AffiliateData, Asset, AssetInfo, Basket, DeploymentVenue, StringEntry, UserInfo
};

use crate::error::ContractError;
use crate::rates::{external_accrue_call, get_total_debt_from_segments, get_total_position_debt};
use crate::risk_engine::assert_basket_assets;
use crate::positions::{
    close_position, create_basket, deposit, edit_basket, fulfill_intents, increase_debt, repay, set_intents, withdraw,
    BAD_DEBT_REPLY_ID, CLOSE_POSITION_REPLY_ID, LIQ_QUEUE_REPLY_ID, REVENUE_REPLY_ID, WITHDRAW_REPLY_ID, SELL_COLLATERAL_REPLY_ID, DEPLOYABLE_VENUE_REPLY_ID
};
use crate::query::{
    query_active_deployment_venues, query_basket_credit_interest, query_basket_positions, query_collateral_rates, query_liquidation_stats, query_user_intent_state, simulate_LTV_mint, query_historical_oracle_prices, query_historical_interest_rates, query_volatility_window, query_simulate_liquidation, query_historical_ltv, query_ltv_shift_info
};
use crate::liquidations::liquidate;
use crate::reply::{handle_close_position_reply, handle_liq_queue_reply, handle_revenue_reply, handle_sell_collateral_reply, handle_withdraw_reply, handle_deployable_venue_reply};
use crate::state::{ get_target_position, update_position, update_position_claims, ContractVersion, ACTIVE_DEPLOYMENT_VENUES, AFFILIATES, BASKET, CLOSE_POSITION, COLLATERAL_RATE_ASSURANCE, CONFIG, CONTRACT, LIQUIDATION, OWNERSHIP_TRANSFER, POSITIONS, RATES, ClosePositionPropagation};
use crate::ltv_updater::update_basket_ltvs;

// use membrane::range_bound_lp_vault::{QueryMsg as RBLP_QueryMsg, UserIntentResponse};
use membrane::osmosis_proxy::ExecuteMsg as OsmoExecuteMsg;

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:cdp";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

const AFFILIATE_LIMIT: usize = 10;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    
    let mut config = Config {
        liq_fee: msg.liq_fee,
        owner: info.clone().sender,
        staking_contract: None,
        oracle_contract: None,
        chain_proxy: None,
        debt_auction: None,
        liquidity_contract: None,
        discounts_contract: None,
        ltv_disco: deps.api.addr_validate(&msg.ltv_disco)?,
        revenue_distributor: None,
        oracle_time_limit: msg.oracle_time_limit,
        cpc_multiplier: Decimal::one(),
        rate_slope_multiplier: msg.rate_slope_multiplier,
        debt_minimum: msg.debt_minimum,
        base_debt_cap_multiplier: msg.base_debt_cap_multiplier,
        collateral_twap_timeframe: msg.collateral_twap_timeframe,
        credit_twap_timeframe: msg.credit_twap_timeframe,
        // redemption_fee: Decimal::from_str("0.005").unwrap(), //0.5%
        affiliate_fee_max: Decimal::percent(5), //5%
        skip_credit_price_accrual: true,
        liquidation_stat_limit: 500,
        ltv_upward_kp: Decimal::percent(5), // 5% of error per day
        ltv_downward_period: 1_209_600, // 2 weeks in seconds (14 days * 86400)
        ltv_max_downward_shift: Decimal::percent(5), // Max 5% shift per period
        transmuter_addr: None,
        irm_config: membrane::types::IRMConfig {
            adjustment_speed: Decimal::from_str("50").unwrap(),     // 50/year
            min_adaptive_rate: Decimal::from_str("0.001").unwrap(),// 0.1% floor
            max_adaptive_rate: Decimal::from_str("0.09").unwrap(), // 9% ceiling
        },
        points_contract: None,
    };

    //Set optional config parameters
    if let Some(address) = msg.owner {
        config.owner = deps.api.addr_validate(&address)?;
    };
    // stability_pool removed
    // dex_router removed
    if let Some(address) = msg.staking_contract {
        config.staking_contract = Some(deps.api.addr_validate(&address)?)
    };
    if let Some(address) = msg.oracle_contract {
        config.oracle_contract = Some(deps.api.addr_validate(&address)?)
    };
    if let Some(address) = msg.chain_proxy {
        config.chain_proxy = Some(deps.api.addr_validate(&address)?)
    };
    if let Some(address) = msg.debt_auction {
        config.debt_auction = Some(deps.api.addr_validate(&address)?)
    };
    if let Some(address) = msg.liquidity_contract {
        config.liquidity_contract = Some(deps.api.addr_validate(&address)?);
    };
    
    CONFIG.save(deps.storage, &config)?;
    ACTIVE_DEPLOYMENT_VENUES.save(deps.storage, &vec![])?;

    //Set contract version
    CONTRACT.save(deps.storage, &ContractVersion {
        contract: String::from(CONTRACT_NAME),
        version: String::from(CONTRACT_VERSION),
    })?;

    //Create basket
    create_basket(
        deps, 
        info, 
        env.clone(), 
        msg.create_basket.basket_id, 
        msg.create_basket.collateral_types, 
        msg.create_basket.credit_asset, 
        msg.create_basket.credit_price, 
        msg.create_basket.base_interest_rate, 
        msg.create_basket.credit_pool_infos, 
        msg.create_basket.liq_queue
    )?; 

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address)
    )
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::UpdateConfig (update) => update_config(deps, info, update),
        ExecuteMsg::Deposit { position_owner, position_id, affiliate_address} => {
            //Set valid_assets from funds sent
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
                valid_assets,
            )?;

            //If there is nothing being deposited, error
            if cAssets == vec![] { return Err(ContractError::CustomError { val: String::from("No deposit assets passed") }) }

            deposit(deps, env, info, position_owner, position_id, cAssets, affiliate_address)
        }
        ExecuteMsg::Withdraw {
            position_id,
            assets,
            send_to,
        } => {
            duplicate_asset_check(assets.clone())?;
            let cAssets: Vec<cAsset> = assert_basket_assets(
                deps.storage,
                deps.querier,
                env.clone(),
                assets,
            )?;
            //If there is nothing being withdrawn, error
            if cAssets == vec![] { return Err(ContractError::CustomError { val: String::from("No withdrawal assets passed") }) }
            withdraw(deps, env, info, position_id, cAssets, send_to)
        }

        ExecuteMsg::IncreaseDebt {
            position_id,
            amount,
            mint_to_addr,
            LTV,
            deployment_intent,
            debt_split,
            rollover_updates,
            peg_debt,
        } => increase_debt(deps, env, info, position_id, amount, LTV, mint_to_addr, deployment_intent, debt_split, rollover_updates, peg_debt),
        ExecuteMsg::Repay {
            position_id,
            position_owner,
            send_excess_to,
            debt_split,
        } => {
            let basket: Basket = BASKET.load(deps.storage)?;                        
            let credit_asset = assert_sent_native_token_balance(basket.credit_asset.info, &info)?;

            repay(
                deps.storage,
                deps.querier,
                deps.api,
                env,
                info,
                position_id,
                position_owner,
                credit_asset,
                send_excess_to,
                debt_split,
            )
        },
        ExecuteMsg::Accrue { position_owner, position_ids } => { external_accrue_call(deps.storage, deps.api, deps.querier, info, env, position_owner, position_ids) },
        // ExecuteMsg::RedeemCollateral { max_collateral_premium } => {
        //     redeem_for_collateral(
        //         deps,
        //         env,
        //         info,
        //         max_collateral_premium.unwrap_or(99u128)
        //     )
        // },
        // ExecuteMsg::EditRedeemability { position_ids, redeemable, premium, max_loan_repayment, restricted_collateral_assets } => {
        //     edit_redemption_info(
        //         deps.storage,
        //         info.sender,
        //         position_ids,
        //         redeemable,
        //         premium,
        //         max_loan_repayment,
        //         restricted_collateral_assets,
        //         false
        //     )
        // },
        // ExecuteMsg::LiqRepay {} => Err(ContractError::CustomError { val: String::from("LiqRepay removed") }),
        ExecuteMsg::EditcAsset {
            asset,
            max_borrow_LTV,
            max_LTV,
        } => edit_cAsset(deps, info, asset, max_borrow_LTV, max_LTV),
        ExecuteMsg::EditBasket(edit) => edit_basket(deps, env, info,edit),
        ExecuteMsg::Liquidate {
            position_id,
            position_owner,
        } => liquidate(
            deps.storage,
            deps.api,
            deps.querier,
            env,
            info,
            position_id,
            position_owner,
        ),
        ExecuteMsg::ClosePosition {
            position_id, close_percentage, max_spread, send_to,
        } => close_position(
            deps, 
            env,
            info,
            position_id,
            close_percentage,
            max_spread,
            send_to),
        ExecuteMsg::SetUserIntents { deployment_intent } => set_intents(deps, env, info, deployment_intent),
        ExecuteMsg::FulfillIntents { users } => fulfill_intents(deps, env, info, users),
        ExecuteMsg::SetDeploymentVenue { position_id, venue_address, initial_deployed_debt_amount } => {
            set_deployment_venue(deps, info, position_id, venue_address, initial_deployed_debt_amount)
        },
        ExecuteMsg::UpdateBasketLTVs {} => update_basket_ltvs(deps, env),
        ExecuteMsg::SetAffiliate { position_id, affiliate_address, affiliate_fee, label } => {
            set_affiliate(deps, env, info, position_id, affiliate_address, affiliate_fee, label)
        },
        ExecuteMsg::FulfillBadDebt { } => {
            fulfill_bad_debt(deps, env, info)
        },
        ExecuteMsg::TakeRevenue {} => {
            take_revenue(deps, env, info)
        },
        ExecuteMsg::CollateralRateAssurance { collateral_denoms } => {
            collateral_rate_assurance(deps, env, info, collateral_denoms)
        },
        ExecuteMsg::Callback(msg) => {
            if info.sender == env.contract.address {
                callback_handler(deps, env, msg)
            } else {
                Err(ContractError::Unauthorized { owner: env.contract.address.to_string() })
            }
        },
        ExecuteMsg::CheckAndClearDebtDelta { position_owner, position_id } => {
            check_and_clear_debt_delta(deps, info, position_owner, position_id)
        }
    }
}

/// Fulfill bad debt.
/// CDT is sent to the contract & burned to eliminate the accounted for and fulfilled bad debt.
/// We don't want to have stray CDT in the contract.
fn fulfill_bad_debt(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    //Load the state
    let config: Config = CONFIG.load(deps.storage)?;
    let mut basket: Basket = BASKET.load(deps.storage)?;
    let mut msgs: Vec<CosmosMsg> = vec![];

    //Check the msg info for a CDT coin amount
    let cdt_amount_opt = info.funds.iter().find(|coin| coin.denom == basket.credit_asset.info.to_string()).map(|c| c.amount);
    if cdt_amount_opt.is_none() {
        return Err(ContractError::CustomError { val: String::from("No CDT coin found in msg info") });
    }
    let cdt_amount = cdt_amount_opt.unwrap();

    //For TESTING 
    // basket.pending_bad_debt = cdt_amount - Uint128::new(10);
    
    //Calc the amount of bad debt fulfilled by this send & calc the excess if there is some
    let (fulfilled_bad_debt, excess) = {
        if basket.pending_bad_debt >= cdt_amount {
            (cdt_amount, Uint128::zero())
        } else {
            (basket.pending_bad_debt, cdt_amount - basket.pending_bad_debt)
        }
    };

    //Update basket pending bad debt
    basket.pending_bad_debt -= fulfilled_bad_debt;

    //Burn the CDT used to fulfill bad debt
    let burn_message = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.chain_proxy.unwrap().to_string(),
        msg: to_json_binary(&OsmoExecuteMsg::BurnTokens {
            denom: basket.credit_asset.info.to_string(),
            amount: fulfilled_bad_debt,
            burn_from_address: env.contract.address.to_string(),
        })?,
        funds: vec![],
    });
    msgs.push(burn_message);

    //Send back the excess CDT to the sender
    msgs.push(CosmosMsg::Bank(BankMsg::Send {
        to_address: info.clone().sender.to_string(),
        amount: vec![Coin {
            denom: basket.credit_asset.info.to_string(),
            amount: excess,
        }],
    }));
    
    //Update basket
    BASKET.save(deps.storage, &basket)?;

    Ok(Response::new().add_messages(msgs))
}

/// Take revenue from Basket's pending_revenue
/// Only callable by the revenue distributor contract
/// Takes ALL available revenue and maintains per-asset attribution
fn take_revenue(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;
    let mut basket: Basket = BASKET.load(deps.storage)?;
    
    // Validate caller is the revenue distributor contract
    let revenue_distributor = config.revenue_distributor
        .ok_or_else(|| ContractError::CustomError {
            val: "Revenue distributor not configured".to_string(),
        })?;
    
    if info.sender != revenue_distributor {
        return Err(ContractError::Unauthorized {
            owner: revenue_distributor.to_string(),
        });
    }
    
    // Take all available revenue
    let total_revenue = basket.pending_revenue.total_pending;
    
    // Clear pending revenue
    basket.pending_revenue.total_pending = Uint128::zero();
    basket.pending_revenue.per_asset_rev.clear();
    
    // Save updated basket
    BASKET.save(deps.storage, &basket)?;
    
    // Send CDT to revenue distributor
    let mut msgs: Vec<CosmosMsg> = vec![];
    if !total_revenue.is_zero() {
        msgs.push(CosmosMsg::Bank(BankMsg::Send {
            to_address: revenue_distributor.to_string(),
            amount: vec![Coin {
                denom: basket.credit_asset.info.to_string(),
                amount: total_revenue,
            }],
        }));
    }
    
    Ok(Response::new()
        .add_messages(msgs)
        .add_attribute("method", "take_revenue")
        .add_attribute("total_revenue", total_revenue.to_string()))
}

/// Check and clear debt delta for management points.
/// Called by Points contract to check if user qualifies for points.
/// Only points contract can call this.
fn check_and_clear_debt_delta(
    deps: DepsMut,
    info: MessageInfo,
    position_owner: String,
    position_id: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    // Only points contract can call this
    if config.points_contract.is_none() || info.sender != config.points_contract.clone().unwrap() {
        return Err(ContractError::Unauthorized {
            owner: config.points_contract.map(|a| a.to_string()).unwrap_or_else(|| "not set".to_string()),
        });
    }

    let owner_addr = deps.api.addr_validate(&position_owner)?;
    let (_position_index, mut position) = get_target_position(deps.storage, owner_addr.clone(), position_id)?;

    let qualifies = if let Some(initial_debt) = position.vol_window_initial_debt {
        let current_debt = get_total_position_debt(&position);
        // Negative delta = initial > current = repaid during volatility
        initial_debt > current_debt
    } else {
        false
    };

    // Clear debt delta
    position.vol_window_initial_debt = None;
    update_position(deps.storage, owner_addr, position)?;

    Ok(Response::new()
        .add_attribute("action", "check_and_clear_debt_delta")
        .add_attribute("position_owner", position_owner)
        .add_attribute("position_id", position_id.to_string())
        .add_attribute("qualifies_for_points", qualifies.to_string()))
}

/// Helper to align collateral_types and collateral_supply_caps by asset_info
fn align_basket_arrays(basket: &mut Basket) {
    // Sort both arrays by asset_info string
    let mut pairs: Vec<_> = basket.collateral_types.iter().zip(basket.collateral_supply_caps.iter()).collect();
    pairs.sort_by(|(a, _), (b, _)| a.asset.info.to_string().cmp(&b.asset.info.to_string()));
    let (types, caps): (Vec<_>, Vec<_>) = pairs.into_iter().map(|(a, b)| (a.clone(), b.clone())).unzip();
    basket.collateral_types = types;
    basket.collateral_supply_caps = caps;
}

/// Edit params for a cAsset in the basket
fn edit_cAsset(
    deps: DepsMut,
    info: MessageInfo,
    asset: AssetInfo,
    max_borrow_LTV: Option<Decimal>,
    max_LTV: Option<Decimal>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    //Assert Authority
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized { owner: config.owner.to_string() });
    }

    let mut basket: Basket = BASKET.load(deps.storage)?;
    let mut attrs = vec![
        attr("method", "edit_cAsset"),
    ];

    let mut msgs: Vec<CosmosMsg> = vec![];

    // Find the index of the asset to edit
    let idx = basket.collateral_types.iter().position(|c| c.asset.info.equal(&asset));
    if let Some(i) = idx {
        let mut asset = basket.collateral_types[i].clone();
        attrs.push(attr("asset", asset.asset.info.to_string()));

        if let Some(LTV) = max_LTV {
            //Enforce 1-100% range
            if LTV > Decimal::percent(100) || LTV < Decimal::percent(1) {
                return Err(ContractError::InvalidMaxLTV { max_LTV: LTV });
            }
            asset.max_LTV = LTV;

                //Edit the asset's liq_queue max_premium
                //Create Liquidation Queue for its assets
                if basket.clone().liq_queue.is_some() {
                    //Gets Liquidation Queue max premium.
                    //The premium has to be at most 5% less than the difference between max_LTV and 100%
                    //The ideal variable for the 5% is the avg caller_liq_fee during high traffic periods
                    let ltv_percent = (LTV * Decimal::from_ratio(Uint128::new(100u128), Uint128::one())).to_uint_floor();
                    let max_premium = match Uint128::new(95u128).checked_sub(ltv_percent) {
                        Ok( diff ) => diff,
                        //A default to 10 assuming that will be the highest sp_liq_fee
                        Err( _err ) => Uint128::new(10u128),
                    };

                    msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                        contract_addr: basket.clone().liq_queue.unwrap_or_else(|| Addr::unchecked("")).into_string(),
                        msg: to_json_binary(&LQ_ExecuteMsg::UpdateQueue {
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
                    asset.max_borrow_LTV = LTV;
                    attrs.push(attr("max_borrow_LTV", LTV.to_string()));
                } else {
                    return Err(ContractError::CustomError {
                        val:String::from("Invalid borrow LTV"),
                    })
                }
            }

        // Write the mutated asset back in place
        basket.collateral_types[i] = asset;
    } else {
        return Err(ContractError::CustomError {
            val:String::from("Collateral type doesn't exist in basket"),
        })
    }

    BASKET.save(deps.storage, &basket)?;

    Ok(Response::new().add_attributes(attrs).add_messages(msgs))
}

/// Update contract config
fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    update: UpdateConfig,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;
    let mut attrs = vec![
        attr("method", "update_config"),
    ];

    //Assert Authority
    if info.sender != config.owner {
        //Check if ownership transfer is in progress & transfer if so
        let new_owner = OWNERSHIP_TRANSFER.load(deps.storage)?;
        if info.sender == new_owner {
            config.owner = info.sender;
        } else {
            return Err(ContractError::Unauthorized { owner: new_owner.to_string() });
        }
    }
    
    if let Some(owner) = update.clone().owner {
        let valid_addr = deps.api.addr_validate(&owner)?;

        //Set owner transfer state
        OWNERSHIP_TRANSFER.save(deps.storage, &valid_addr)?; 
        attrs.push(attr("owner_transfer", valid_addr));
    }
    
    //Update Config
    update.update_config(deps.api, &mut config)?;

    //Save new Config
    CONFIG.save(deps.storage, &config)?;
    
    attrs.push(
        attr("updated_config", format!("{:?}", config)));
    Ok(Response::new().add_attributes(attrs))
}

/// Handle CallbackMsgs
pub fn callback_handler(
    deps: DepsMut,
    env: Env,
    msg: CallbackMsg,
) -> Result<Response, ContractError> {
    match msg {
        CallbackMsg::BadDebtCheck {
            position_owner,
            position_id,
        } => check_and_fulfill_bad_debt(deps, env, position_id, position_owner),
        CallbackMsg::ClosePositionCallback {
            position_id,
            position_owner,
        } => {
            // Handle close position callback after venue repayments
            handle_close_position_callback(deps, env, position_id, position_owner)
        }
    }
}

/// Handle close position callback after venue repayments
fn handle_close_position_callback(
    deps: DepsMut,
    env: Env,
    position_id: Uint128,
    position_owner: String,
) -> Result<Response, ContractError> {
    //Load Close Position Prop
    let state_propagation: ClosePositionPropagation = CLOSE_POSITION.load(deps.storage)?;

    //Create user info variables
    let valid_position_owner = deps.api.addr_validate(&position_owner)?;

    //Load State
    let basket: Basket = BASKET.load(deps.storage)?;
    let config: Config = CONFIG.load(deps.storage)?;

    //Query contract balance of the basket credit_asset
    let credit_asset_balance = get_contract_balances(
        deps.querier, 
        env.clone(), 
        vec![basket.credit_asset.info.clone()]
    )?[0];

    //Create repay_msg
    let repay_msg = ExecuteMsg::Repay {
        debt_split: None, 
        position_id, 
        position_owner: Some(valid_position_owner.clone().to_string()),
        send_excess_to: Some(valid_position_owner.clone().to_string()),
    };

    //Create repay_msg with queried funds
    //This works because the contract doesn't hold excess credit_asset, all repayments are burned & revenue isn't minted
    let repay_msg = CosmosMsg::Wasm(WasmMsg::Execute { 
        contract_addr: env.contract.address.to_string(), 
        msg: to_json_binary(&repay_msg)?, 
        funds: vec![asset_to_coin(
            Asset { 
                info: basket.credit_asset.info.clone(),
                amount: credit_asset_balance.clone(),
            })?]
    });

    //Update position claims for each asset withdrawn + sold
    for withdrawn_collateral in state_propagation.clone().withdrawn_assets {

        update_position_claims(
            deps.storage, 
            deps.querier, 
            env.clone(), 
            config.clone(),
            position_id,
            valid_position_owner.clone(), 
            withdrawn_collateral.info, 
            withdrawn_collateral.amount
        )?;
    }

    //Load position
    let (_i, target_position) = match get_target_position(
        deps.storage, 
        valid_position_owner.clone(), 
        position_id, 
    ){
        Ok(position) => position,
        Err(err) => return Err(ContractError::CustomError { val: err.to_string() })
    };

    //Withdrawing everything thats left
    let is_debt_zero = get_total_position_debt(&target_position).is_zero();
    let assets_to_withdraw: Vec<Asset> = target_position.collateral_assets
        .into_iter()
        .filter(|cAsset| cAsset.asset.amount > Uint128::zero())
        .map(|cAsset| cAsset.asset)
        .collect::<Vec<Asset>>();

    if assets_to_withdraw.len() > 0 && is_debt_zero {     
        //Create WithdrawMsg
        let withdraw_msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute { 
            contract_addr: env.contract.address.to_string(), 
            msg: to_json_binary(& ExecuteMsg::Withdraw { 
                position_id, 
                assets: assets_to_withdraw, 
                send_to: state_propagation.send_to, 
            })?, 
            funds: vec![],
        });

        //Response 
        Ok(Response::new()
            .add_message(repay_msg)
            .add_attribute("amount_repaid", credit_asset_balance)
            .add_message(withdraw_msg)
            .add_attribute("sold_assets", format!("{:?}", state_propagation.withdrawn_assets))
            .add_attribute("method", "close_position_callback")
        )
    } else {
        //Response 
        Ok(Response::new()
            .add_message(repay_msg)
            .add_attribute("amount_repaid", credit_asset_balance)
            .add_attribute("sold_assets", format!("{:?}", state_propagation.withdrawn_assets))
            .add_attribute("method", "close_position_callback")
        )
    }
}

/// Set affiliate for a Position.
/// Adds to current list of affiliations.
/// Affiliate fee is capped at the contract's affiliate fee max.
/// Fee can't be 0.
/// Only affiliate can change the fee.
fn set_affiliate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_id: Uint128,
    affiliate_address: String,
    affiliate_fee: Decimal,
    label: Option<String>,
) -> Result<Response, ContractError> {
    let mut attrs = vec![
        attr("method", "set_affiliate"),
        attr("position_id", position_id.to_string()),
        attr("affiliate_address", affiliate_address.clone()),
        attr("label", label.clone().unwrap_or_default()),
    ];
    let config: Config = CONFIG.load(deps.storage)?;

    //Validate address
    let _valid_addr = deps.api.addr_validate(&affiliate_address)?;

    //Validate fee
    if affiliate_fee > config.affiliate_fee_max {
        return Err(ContractError::CustomError { val: String::from("Affiliate fee exceeds max") });
    }

    //Validate fee
    // if affiliate_fee == Decimal::zero() {
    //     return Err(ContractError::CustomError { val: String::from("Affiliate fee can't be 0") });
    // }

    //Get target Position's affiliations
    let affiliations = AFFILIATES.load(deps.storage, position_id.to_string()).unwrap_or_else(|_| vec![]);

    //Add new affiliation
    if let Some(affiliation) = affiliations.iter().find(|a| a.affiliate_address == affiliate_address) {
        //Can only change fee if affiliate is the called
        if info.sender != deps.api.addr_validate(&affiliation.affiliate_address)? {
            return Err(ContractError::Unauthorized { owner: affiliation.affiliate_address.to_string() });
        }
        //Update fee (persist after branch)
        attrs.push(attr("affiliate_fee_updated", affiliate_fee.to_string()));
    } else {
        //Can't add more than 3 affiliations
        if affiliations.len() >= AFFILIATE_LIMIT {
            return Err(ContractError::CustomError { val: String::from("Can't add more than 3 affiliations but affiliations reset to only the latest one on repayments.") });
        }
        //Add new affiliation
        let mut new_affiliations = affiliations.clone();
        new_affiliations.push(AffiliateData {
            affiliate_address: affiliate_address.clone(),
            affiliate_fee,
            time_affiliated: env.block.time.seconds(),
            label: label.clone(),
        });
        attrs.push(attr("affiliate_fee", affiliate_fee.to_string()));
    }

    //Save affiliations
    if let Some(_existing) = affiliations.iter().find(|a| a.affiliate_address == affiliate_address) {
        // Replace updated fee by mapping and saving
        let updated: Vec<AffiliateData> = affiliations.into_iter().map(|mut a| {
            if a.affiliate_address == affiliate_address { a.affiliate_fee = affiliate_fee; }
            a
        }).collect();
        AFFILIATES.save(deps.storage, position_id.to_string(), &updated)?;
    } else {
        // Save the new affiliations list constructed above
        let mut new_affiliations = affiliations.clone();
        new_affiliations.push(AffiliateData {
            affiliate_address,
            affiliate_fee,
            time_affiliated: env.block.time.seconds(),
            label,
        });
        AFFILIATES.save(deps.storage, position_id.to_string(), &new_affiliations)?;
    }

        Ok(Response::new()
        .add_attributes(attrs)
    )
    
}

/// Check and recapitilize Bad Debt w/ revenue or MBRN auctions
fn check_and_fulfill_bad_debt(
    deps: DepsMut,
    _env: Env,
    position_id: Uint128,
    position_owner: Addr,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    let mut basket: Basket = BASKET.load(deps.storage)?;

    //Get target Position
    let (_i, mut target_position) = get_target_position(deps.storage, position_owner.clone(), position_id)?;

    //Load Liquidation Prop
    let liq_prop = LIQUIDATION.load(deps.storage)?;
    let cAsset_prices = liq_prop.cAsset_prices.clone();
    let cAsset_ratios = liq_prop.cAsset_ratios.clone();
    let collateral_assets = liq_prop.liquidated_assets.clone();

    //We check if the value left is > $1
    let total_asset_value: Decimal = target_position.clone()
        .collateral_assets
        .into_iter()
        .enumerate()
        .map(|(index, asset)| 
            {
                //Find asset's price
                let price = cAsset_prices[index].clone();
                //Return asset value
                price.get_value(asset.asset.amount).unwrap_or_else(|_| Decimal::zero())

            }
        )
        .collect::<Vec<Decimal>>()
        .iter()
        .sum();

    //We use > $1 bc full liquidations will leave rounding errors in the collateral assets so we just use $1 as a floor instead of $0
    if total_asset_value > Decimal::one() || get_total_position_debt(&target_position).is_zero() {
        Err(ContractError::PositionSolvent {})
    } else {
        let mut messages: Vec<CosmosMsg> = vec![];
        let mut bad_debt_amount = crate::rates::get_total_position_debt(&target_position);
        let mut attrs = vec![
            attr("method", "check_and_fulfill_bad_debt"),
            attr("bad_debt_amount", bad_debt_amount),
        ];

        //If the basket has revenue, "mint" and repay the bad debt
        if !basket.pending_revenue.total_pending.is_zero() {
            if bad_debt_amount >= basket.pending_revenue.total_pending {

                //Update bad_debt
                bad_debt_amount -= basket.pending_revenue.total_pending;

                //Update basket revenue
                basket.pending_revenue.total_pending = Uint128::zero();
            } else {                
                //Update basket revenue
                basket.pending_revenue.total_pending -= bad_debt_amount;

                //Set bad_debt to 0
                bad_debt_amount = Uint128::zero();

            }
        }

        //Set target_position.rate_segments to empty and add the leftover bad debt to pending_bad_debt
        target_position.rate_segments.clear();
        basket.pending_bad_debt += bad_debt_amount;
        
        //Save target_position w/ updated debt
        update_position(deps.storage, position_owner.clone(), target_position)?;

        
        //Send bad debt amount to the LTV Disco for each collateral asset
        for (num, cAsset) in collateral_assets.clone().iter().enumerate() {
            //Calc the bad debt amount per asset.
            //We do to_ceiling to prevent rounding errors to be sent to the auction.
            let bad_debt_amount_for_asset = decimal_multiplication(
                Decimal::from_ratio(bad_debt_amount, Uint128::one()), 
                cAsset_ratios[num]
            )?.to_uint_ceil();
            //Query to check if the LTV Disco can handle the bad debt amount for this asset
            let can_handle_bad_debt = deps.querier.query_wasm_smart::<bool>(
                config.ltv_disco.to_string(),
                &LTVDisco_QueryMsg::CanHandleBadDebt {
                    asset: cAsset.asset.info.to_string(),
                    amount: bad_debt_amount_for_asset,
                },
            )?;
            if can_handle_bad_debt {
                //Send the bad debt amount to the LTV Disco
                messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.ltv_disco.to_string(),
                    msg: to_json_binary(&LTVDisco_ExecuteMsg::AddBadDebt {
                        asset: cAsset.asset.info.to_string(),
                        amount: bad_debt_amount_for_asset,
                    })?,
                    funds: vec![],
                }));
                //Update remaining bad debt
                bad_debt_amount -= bad_debt_amount_for_asset;
            }
        }

        //Send remaining bad debt amount to the auction contract if greater than 0.
        //This will trigger a MBRN auction to sell MBRN to repay the bad debt.
        if config.debt_auction.is_some() && !bad_debt_amount.is_zero() {
            let auction_msg = AuctionExecuteMsg::StartAuction {
                repayment_position_info: Some(UserInfo {
                    position_id,
                    position_owner: position_owner.to_string(),
                }),
                auction_asset: Asset {
                    amount: bad_debt_amount,
                    info: basket.clone().credit_asset.info,
                },
                send_to: None,
                per_asset_distribution: None, // Bad debt auction doesn't track per-asset distribution
            };

            messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.debt_auction.unwrap_or_else(|| Addr::unchecked("")).to_string(),
                msg: to_json_binary(&auction_msg)?,
                funds: vec![],
            }));
        } else {
            return Err(ContractError::CustomError {
                val: String::from("Debt Auction contract not added to config"),
            });
        }

        //Save Basket w/ updated revenue & pending bad debt
        BASKET.save(deps.storage, &basket)?;
        
        attrs.push(
            attr("amount_sent_to_auction", bad_debt_amount)
        );

        Ok(Response::new()
            .add_messages(messages)
            .add_attributes(attrs))
    }
}

pub fn set_deployment_venue(
    deps: DepsMut,
    info: MessageInfo,
    position_id: Uint128,
    venue_address: String,
    initial_deployed_debt_amount: Option<Uint128>,
) -> Result<Response, ContractError> {
    // Get target position
    // Which also validates ownership
    let (_, mut target_position) = get_target_position(deps.storage, info.sender.clone(), position_id)?;
    
    // Validate venue address
    let venue_addr = deps.api.addr_validate(&venue_address)?;
    
    // If initial_deployed_debt_amount is None, query the deployable venue for RetrievableCDT
    let deployed_debt_amount = if let Some(amount) = initial_deployed_debt_amount {
        amount
    } else {
        // Query the deployable venue contract's RetrievableCDT query
        deps.querier.query::<Uint128>(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: venue_address.clone(),
            msg: to_json_binary(&DeployableVenue_QueryMsg::RetrievableCDT {
                user: info.sender.to_string(),
            })?,
        }))?
    };
    
    // Check if deployment venue already exists in position.deployed_to
    let is_new_venue = !target_position.deployed_to.iter().any(|venue| venue.address == venue_addr);
    
    if let Some(existing_venue) = target_position.deployed_to.iter_mut().find(|venue| venue.address == venue_addr) {
        // Update existing venue's deployed_debt_amount
        existing_venue.deployed_debt_amount = deployed_debt_amount;
    } else {
        // Add new deployment venue
        target_position.deployed_to.push(DeploymentVenue {
            address: venue_addr,
            deployed_debt_amount,
            failed_liquidation: false,
        });
        
        // Update active deployment venues for new venues
        set_active_deployment_venues(deps.storage, vec![StringEntry {
            entry: venue_address.clone(),
            remove: false,
        }])?;
    }
    
    // Save position
    update_position(deps.storage, info.sender.clone(), target_position)?;
    
    Ok(Response::new()
        .add_attributes(vec![
            attr("action", "set_deployment_venue"),
            attr("position_id", position_id.to_string()),
            attr("venue_address", venue_address),
            attr("deployed_debt_amount", deployed_debt_amount.to_string()),
        ]))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.id {
        LIQ_QUEUE_REPLY_ID => handle_liq_queue_reply(deps, msg, env),
        WITHDRAW_REPLY_ID => handle_withdraw_reply(deps, env, msg),
        REVENUE_REPLY_ID => handle_revenue_reply(deps, env, msg),
        CLOSE_POSITION_REPLY_ID => handle_close_position_reply(deps, env, msg),
        SELL_COLLATERAL_REPLY_ID => handle_sell_collateral_reply(deps, env, msg),
        DEPLOYABLE_VENUE_REPLY_ID => handle_deployable_venue_reply(deps, env, msg),
        // 99u64 => handle_rblp_query(deps, env, msg),
        BAD_DEBT_REPLY_ID => Ok(Response::new()),
        id => Err(StdError::generic_err(format!("invalid reply id: {}", id))),
    }
}

/// Handle RBLP query
// fn handle_rblp_query(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response>{
//     //Query target position
//     let target_position = match get_target_position(deps.storage, Addr::unchecked("osmo1988s5h45qwkaqch8km4ceagw2e08vdw28mwk4n"), Uint128::new(1u128)){
//         Ok((_i, pos)) => pos,
//         Err(_) => panic!("No target position found"),
//     };


//     //Query RBLP's UserIntentState to see if the user has funds sitting in the vault
//     let user_intents: Vec<UserIntentResponse> = match deps.querier
//         .query::<Vec<UserIntentResponse>>(&QueryRequest::Wasm(WasmQuery::Smart {
//             contract_addr: "osmo17rvvd6jc9javy3ytr0cjcypxs20ru22kkhrpwx7j3ym02znuz0vqa37ffx".to_string(),
//             msg: to_json_binary(&RBLP_QueryMsg::GetUserIntent { 
//                 start_after: None, 
//                 limit: None, 
//                 users: vec!["osmo1988s5h45qwkaqch8km4ceagw2e08vdw28mwk4n".to_string()],
//             })?,
//         })){
//             Ok(res) => res,
//             Err(_) => vec![],
//         };
//     let user_intent: UserIntentResponse = if user_intents.len() > 0 {user_intents[0].clone()} else {
//         panic!("UserIntent: {:?}, Target Position: {:?}", Vec::<UserIntentResponse>::new(), target_position);
//     };

//     panic!("UserIntent: {:?}, Target Position: {:?}", user_intent, target_position);

    
//     //Return response
//     Ok(Response::new())
// }

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        
        QueryMsg::GetBasketPositions {
            start_after,
            limit,
            user_info, 
            user,
        } => to_json_binary(&query_basket_positions(
            deps,
            env,
            start_after,
            limit,
            user_info, 
            user,
        )?),
        QueryMsg::GetBasket { } => to_json_binary(&BASKET.load(deps.storage)?),
        // QueryMsg::GetBasketRedeemability { position_owner, start_after, limit } => {
        //     to_json_binary(&query_basket_redeemability(deps, position_owner, start_after, limit)?)
        // }
        QueryMsg::GetCreditRate { } => {
            to_json_binary(&query_basket_credit_interest(deps, env)?)
        }
        QueryMsg::GetCollateralInterest { } => {
            to_json_binary(&query_collateral_rates(deps)?)
        },
        QueryMsg::SimulateMint { position_info, LTV } => {
            to_json_binary(&simulate_LTV_mint(deps, env, position_info, LTV)?)
        },
        QueryMsg::GetUserIntent { start_after, limit, users } => {
            to_json_binary(&query_user_intent_state(deps, env,  start_after, limit, users)?)
        }
        QueryMsg::GetAffiliates { position_id } => {
            to_json_binary(&AFFILIATES.load(deps.storage, position_id.to_string()).unwrap_or_else(|_| vec![]))
        }
        QueryMsg::GetLiquidationStats { start_after, limit } => {
            to_json_binary(&query_liquidation_stats(deps, start_after, limit)?)
        }
        QueryMsg::GetActiveDeploymentVenues { venue, start_after, limit } => {
            to_json_binary(&query_active_deployment_venues(deps, venue, start_after, limit)?)
        }
        QueryMsg::GetHistoricalOraclePrices { asset } => {
            to_json_binary(&query_historical_oracle_prices(deps, asset)?)
        },
        QueryMsg::GetHistoricalInterestRates { asset } => {
            to_json_binary(&query_historical_interest_rates(deps, asset)?)
        }
        QueryMsg::GetRates {} => {
            to_json_binary(&RATES.load(deps.storage)?)
        }
        QueryMsg::SimulateLiquidation { collateral_to_sell, target_denom } => {
            to_json_binary(&query_simulate_liquidation(deps, collateral_to_sell, target_denom)?)
        }
        QueryMsg::CheckVolatilityWindow { assets } => {
            to_json_binary(&query_volatility_window(deps, assets)?)
        }
        QueryMsg::GetHistoricalLTV { asset_denom, start_time, end_time, limit } => {
            to_json_binary(&query_historical_ltv(deps, asset_denom, start_time, end_time, limit)?)
        }
        QueryMsg::GetLTVShiftInfo { asset_denom } => {
            to_json_binary(&query_ltv_shift_info(deps, env, asset_denom)?)
        }
    }
}

/// Check for duplicate assets in a Vec<Asset>
fn duplicate_asset_check(assets: Vec<Asset>) -> Result<(), ContractError> {
    //No duplicates
    for (i, asset) in assets.clone().into_iter().enumerate() {
        let mut assets_copy = assets.clone();
        assets_copy.remove(i);

        if let Some(_asset) = assets_copy
            .into_iter()
            .find(|asset_clone| asset_clone.info.equal(&asset.info))
        {
            return Err(ContractError::CustomError { val: String::from("No duplicate assets in Asset object lists") } );
        }
    }

    Ok(())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, _env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {

    // let mut basket = BASKET.load(deps.storage)?;
    
    // //Align basket
    // align_basket_arrays(&mut basket);

    // // Reset basket from positions
    // reset_basket_from_positions(deps.storage, &mut basket);

    // // Save the basket
    // BASKET.save(deps.storage, &basket)?;

    Ok(Response::new())
}

/// Helper to reset collateral_types and supply cap data from all positions with non-zero credit_amount
fn reset_basket_from_positions(storage: &mut dyn cosmwasm_std::Storage, basket: &mut Basket) {
    use std::collections::HashMap;
    use cosmwasm_std::Uint128;

    // Map from asset_info string to running total
    let mut collateral_totals: HashMap<String, Uint128> = HashMap::new();

    // Reset all basket amounts to zero first
    for c in basket.collateral_types.iter_mut() {
        c.asset.amount = Uint128::zero();
    }
    for cap in basket.collateral_supply_caps.iter_mut() {
        cap.current_supply = Uint128::zero();
    }

    // Iterate through all POSITIONS
    let all_positions = POSITIONS.range(storage, None, None, cosmwasm_std::Order::Ascending);
    for item in all_positions {
        if let Ok((_owner, positions_vec)) = item {
            for position in positions_vec {
                if !get_total_position_debt(&position).is_zero() {
                    for casset in position.collateral_assets {
                        let key = casset.asset.info.to_string();
                        let entry = collateral_totals.entry(key).or_insert(Uint128::zero());
                        *entry += casset.asset.amount;
                    }
                }
            }
        }
    }

    // Set the basket collateral_types and supply_caps to the computed totals
    for c in basket.collateral_types.iter_mut() {
        let key = c.asset.info.to_string();
        if let Some(total) = collateral_totals.get(&key) {
            c.asset.amount = *total;
        }
    }
    for cap in basket.collateral_supply_caps.iter_mut() {
        let key = cap.asset_info.to_string();
        if let Some(total) = collateral_totals.get(&key) {
            cap.current_supply = *total;
        }
    }
}

/// Collateral rate assurance function to check rate consistency
pub fn collateral_rate_assurance(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    collateral_denoms: Option<Vec<String>>,
) -> Result<Response, ContractError> {
    // Error if not the contract calling
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized { owner: env.contract.address.to_string() });
    }

    let _config = CONFIG.load(deps.storage)?;
    let basket = BASKET.load(deps.storage)?;
    
    let mut attrs = vec![attr("method", "collateral_rate_assurance")];
    let mut errors = Vec::new();

    // Determine which collateral denoms to check
    let denoms_to_check = if let Some(denoms) = collateral_denoms {
        denoms
    } else {
        // Check all collateral types in the basket
        basket.collateral_types.iter()
            .map(|c_asset| c_asset.asset.info.to_string())
            .collect()
    };

    for denom in denoms_to_check {
        // Load the rate assurance state for this denom
        if let Ok(collateral_rate_assurance) = COLLATERAL_RATE_ASSURANCE.load(deps.storage, denom.clone()) {
            // Get current collateral balance
            let current_collateral = get_contract_balances(
                deps.querier,
                env.clone(),
                vec![AssetInfo::NativeToken { denom: denom.clone() }]
            )?[0];

            // Get current collateral state total from basket
            let collateral_state_total = basket.collateral_supply_caps.iter()
                .find(|cap| cap.asset_info.to_string() == denom)
                .map(|cap| cap.current_supply)
                .unwrap_or(Uint128::zero());

            // Calculate current rate (collateral_per_state)
            let current_collateral_per_one = if collateral_state_total.is_zero() {
                Uint128::zero()
            } else {
                calculate_base_tokens(
                    Uint128::new(1_000_000),
                    current_collateral,
                    collateral_state_total
                )?
            };

            // Check rate difference 
            let difference = if current_collateral_per_one > collateral_rate_assurance.pre_collateral_per_one {
                current_collateral_per_one.checked_sub(collateral_rate_assurance.pre_collateral_per_one).unwrap_or(Uint128::zero())
            } else {
                collateral_rate_assurance.pre_collateral_per_one.checked_sub(current_collateral_per_one).unwrap_or(Uint128::zero())
            };

            if difference > Uint128::from_str("1").unwrap_or(Uint128::zero()) {
                errors.push(format!(
                    "Collateral rate assurance failed for {}: pre: {} --- post: {}",
                    denom, collateral_rate_assurance.pre_collateral_per_one, current_collateral_per_one
                ));
            }

            attrs.push(attr(format!("{}_pre_rate", denom), collateral_rate_assurance.pre_collateral_per_one));
            attrs.push(attr(format!("{}_post_rate", denom), current_collateral_per_one));
        }
    }

    // Return error if any rate checks failed
    if !errors.is_empty() {
        return Err(ContractError::CustomError { val: errors.join("; ") });
    }

    Ok(Response::new().add_attributes(attrs))
}

/// Set active deployment venues
pub fn set_active_deployment_venues(
    storage: &mut dyn Storage,
    venues: Vec<StringEntry>,
) -> StdResult<()> {
    //load active deployment venues
    let mut active_deployment_venues = ACTIVE_DEPLOYMENT_VENUES.load(storage)?;
    //update active deployment venues
    for venue in venues {
        if venue.remove {
            active_deployment_venues.retain(|v| v != &venue.entry);
        } else {
            active_deployment_venues.push(venue.entry);
        }
    }
    //save active deployment venues
    ACTIVE_DEPLOYMENT_VENUES.save(storage, &active_deployment_venues)?;
    Ok(())
}