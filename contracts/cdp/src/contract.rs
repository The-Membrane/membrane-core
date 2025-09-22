use std::env;
use std::str::FromStr;
use std::collections::HashMap;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, to_json_binary,SubMsg, QueryRequest, WasmQuery, Addr, Binary, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdError, StdResult, Uint128, WasmMsg
};

use membrane::auction::ExecuteMsg as AuctionExecuteMsg;
use membrane::helpers::{assert_sent_native_token_balance};
use membrane::liq_queue::ExecuteMsg as LQ_ExecuteMsg;
use membrane::cdp::{Config, CallbackMsg, ExecuteMsg, InstantiateMsg, QueryMsg, UpdateConfig, MigrateMsg};
use membrane::types::{
    cAsset, AffiliateData, Asset, AssetInfo, Basket, Position, UserInfo
};

use crate::error::ContractError;
use crate::rates::{external_accrue_call};
use crate::risk_engine::assert_basket_assets;
use crate::positions::{
    close_position, create_basket, deposit, edit_basket, edit_redemption_info, fulfill_intents, increase_debt, liq_repay, redeem_for_collateral, repay, set_intents, withdraw, BAD_DEBT_REPLY_ID, CLOSE_POSITION_REPLY_ID, LIQ_QUEUE_REPLY_ID, REVENUE_REPLY_ID, WITHDRAW_REPLY_ID
};
use crate::query::{
    query_basket_credit_interest, query_basket_positions, query_basket_redeemability, query_collateral_rates, simulate_LTV_mint, query_user_intent_state
};
use crate::liquidations::liquidate;
use crate::reply::{handle_close_position_reply, handle_liq_queue_reply, handle_revenue_reply, handle_withdraw_reply};
use crate::state::{ get_target_position, update_position, ContractVersion, BASKET, AFFILIATES, CONFIG, CONTRACT, LIQUIDATION, OWNERSHIP_TRANSFER, POSITIONS};

use membrane::range_bound_lp_vault::{QueryMsg as RBLP_QueryMsg, UserIntentResponse};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:cdp";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

const AFFILIATE_LIMIT: usize = 3;

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
        stability_pool: None,
        dex_router: None,
        staking_contract: None,
        oracle_contract: None,
        chain_proxy: None,
        debt_auction: None,
        liquidity_contract: None,
        discounts_contract: None,
        revenue_distributor: None,
        oracle_time_limit: msg.oracle_time_limit,
        cpc_multiplier: Decimal::one(), 
        rate_slope_multiplier: msg.rate_slope_multiplier,
        debt_minimum: msg.debt_minimum,
        base_debt_cap_multiplier: msg.base_debt_cap_multiplier,
        collateral_twap_timeframe: msg.collateral_twap_timeframe,
        credit_twap_timeframe: msg.credit_twap_timeframe,
        rate_hike_rate: Some(Decimal::percent(30)),
        redemption_fee: Decimal::from_str("0.005").unwrap(), //0.5%
        affiliate_fee_max: Decimal::percent(10), //10%
        skip_credit_price_accrual: true,
    };

    //Set optional config parameters
    if let Some(address) = msg.owner {
        config.owner = deps.api.addr_validate(&address)?;
    };
    if let Some(address) = msg.stability_pool {
        config.stability_pool = Some(deps.api.addr_validate(&address)?)
    };
    if let Some(address) = msg.dex_router {
        config.dex_router = Some(deps.api.addr_validate(&address)?)
    };
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
        ExecuteMsg::Deposit { position_owner, position_id} => {
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

            deposit(deps, env, info, position_owner, position_id, cAssets)
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
            mint_intent
        } => increase_debt(deps, env, info, position_id, amount, LTV, mint_to_addr, mint_intent),
        ExecuteMsg::Repay {
            position_id,
            position_owner,
            send_excess_to,
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
            )
        },
        ExecuteMsg::Accrue { position_owner, position_ids } => { external_accrue_call(deps.storage, deps.api, deps.querier, info, env, position_owner, position_ids) },
        ExecuteMsg::RedeemCollateral { max_collateral_premium } => {
            redeem_for_collateral(
                deps, 
                env, 
                info, 
                max_collateral_premium.unwrap_or(99u128)
            )
        },
        ExecuteMsg::EditRedeemability { position_ids, redeemable, premium, max_loan_repayment, restricted_collateral_assets } => {
            edit_redemption_info(
                deps.storage,
                info.sender, 
                position_ids, 
                redeemable, 
                premium, 
                max_loan_repayment,
                restricted_collateral_assets,
                false
            )
        },
        ExecuteMsg::LiqRepay {} => {
            if !info.funds.is_empty() {
                let credit_asset = Asset {
                    info: AssetInfo::NativeToken {
                        denom: info.funds[0].clone().denom,
                    },
                    amount: info.funds[0].amount,
                };
                liq_repay(deps, env, info, credit_asset)
            } else { //This is checked more specifically in repay(). This is solely to guarantee only one asset is checked.
                 Err(ContractError::InvalidCredit {})
            }
        },
        ExecuteMsg::EditcAsset {
            asset,
            max_borrow_LTV,
            max_LTV,
            hike_rates,
        } => edit_cAsset(deps, info, asset, max_borrow_LTV, max_LTV, hike_rates),
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
        ExecuteMsg::SetUserIntents { mint_intent } => set_intents(deps, env, info, mint_intent),
        ExecuteMsg::FulfillIntents { users } => fulfill_intents(deps, env, info, users),
        ExecuteMsg::SetAffiliate { position_id, affiliate_address, affiliate_fee } => {
            set_affiliate(deps, env, info, position_id, affiliate_address, affiliate_fee)
        },
        ExecuteMsg::Callback(msg) => {
            if info.sender == env.contract.address {
                callback_handler(deps, env, msg)
            } else {
                Err(ContractError::Unauthorized { owner: env.contract.address.to_string() })
            }
        }
    }
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
    rate_hiked: Option<bool>,
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
                    let max_premium = match Uint128::new(95u128).checked_sub( LTV * Uint128::new(100u128) ){
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

        if let Some(rate_hiked) = rate_hiked {
            asset.hike_rates = Some(rate_hiked);
            attrs.push(attr("rate_hiked", rate_hiked.to_string()));
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
) -> Result<Response, ContractError> {
    let mut attrs = vec![
        attr("method", "set_affiliate"),
        attr("position_id", position_id.to_string()),
        attr("affiliate_address", affiliate_address.clone()),
    ];
    let config: Config = CONFIG.load(deps.storage)?;

    //Validate address
    let valid_addr = deps.api.addr_validate(&affiliate_address)?;

    //Validate fee
    if affiliate_fee > config.affiliate_fee_max {
        return Err(ContractError::CustomError { val: String::from("Affiliate fee exceeds max") });
    }

    //Validate fee
    if affiliate_fee == Decimal::zero() {
        return Err(ContractError::CustomError { val: String::from("Affiliate fee can't be 0") });
    }

    //Get target Position's affiliations
    let mut affiliations = AFFILIATES.load(deps.storage, position_id.to_string()).unwrap_or_else(|_| vec![]);

    //Add new affiliation
    if let Some(mut affiliation) = affiliations.iter_mut().find(|a| a.affiliate_address == affiliate_address) {
        //Can only change fee if affiliate is the called
        if info.sender != affiliation.affiliate_address {
            return Err(ContractError::Unauthorized { owner: affiliation.affiliate_address.to_string() });
        }
        //Update fee
        affiliation.affiliate_fee = affiliate_fee;
        attrs.push(attr("affiliate_fee_updated", affiliate_fee.to_string()));
    } else {
        //Can't add more than 3 affiliations
        if affiliations.len() >= AFFILIATE_LIMIT {
            return Err(ContractError::CustomError { val: String::from("Can't add more than 3 affiliations") });
        }
        //Add new affiliation
        affiliations.push(AffiliateData {
            affiliate_address,
            affiliate_fee,
            time_affiliated: env.block.time.seconds(),
        });
        attrs.push(attr("affiliate_fee", affiliate_fee.to_string()));
    }

    //Save affiliations
    AFFILIATES.save(deps.storage, position_id.to_string(), &affiliations)?;

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
    let cAsset_prices = LIQUIDATION.load(deps.storage)?.cAsset_prices;

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
    if total_asset_value > Decimal::one() || target_position.credit_amount.is_zero() {
        Err(ContractError::PositionSolvent {})
    } else {
        let mut messages: Vec<CosmosMsg> = vec![];
        let mut bad_debt_amount = target_position.credit_amount;
        let mut attrs = vec![
            attr("method", "check_and_fulfill_bad_debt"),
            attr("bad_debt_amount", bad_debt_amount),
        ];

        //If the basket has revenue, mint and repay the bad debt
        if !basket.pending_revenue.is_zero() {
            if bad_debt_amount >= basket.pending_revenue {

                //Update bad_debt
                bad_debt_amount -= basket.pending_revenue;

                //Update basket revenue
                basket.pending_revenue = Uint128::zero();
            } else {                
                //Update basket revenue
                basket.pending_revenue -= bad_debt_amount;

                //Set bad_debt to 0
                bad_debt_amount = Uint128::zero();

            }
        }

        //Set target_position.credit_amount to the leftover bad debt
        target_position.credit_amount = bad_debt_amount;
        
        //Save target_position w/ updated debt
        update_position(deps.storage, position_owner.clone(), target_position)?;

        //Send bad debt amount to the auction contract if greater than 0
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

        //Save Basket w/ updated revenue
        BASKET.save(deps.storage, &basket)?;
        
        attrs.push(
            attr("amount_sent_to_auction", bad_debt_amount)
        );

        Ok(Response::new()
            .add_messages(messages)
            .add_attributes(attrs))
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.id {
        LIQ_QUEUE_REPLY_ID => handle_liq_queue_reply(deps, msg, env),
        WITHDRAW_REPLY_ID => handle_withdraw_reply(deps, env, msg),
        REVENUE_REPLY_ID => handle_revenue_reply(deps, env, msg),
        CLOSE_POSITION_REPLY_ID => handle_close_position_reply(deps, env, msg),
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
        QueryMsg::GetBasketRedeemability { position_owner, start_after, limit } => {
            to_json_binary(&query_basket_redeemability(deps, position_owner, start_after, limit)?)
        }
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
pub fn migrate(deps: DepsMut, env: Env, _msg: MigrateMsg) -> Result<Response, ContractError> {

    let mut basket = BASKET.load(deps.storage)?;
    
    //Align basket
    align_basket_arrays(&mut basket);

    // Reset basket from positions
    reset_basket_from_positions(deps.storage, &mut basket);

    // Save the basket
    BASKET.save(deps.storage, &basket)?;

    Ok(Response::new())
}

/// Helper to reset collateral_types and supply cap data from all positions with non-zero credit_amount
fn reset_basket_from_positions(storage: &mut dyn cosmwasm_std::Storage, basket: &mut Basket) {
    use std::collections::HashMap;
    use membrane::types::AssetInfo;
    use membrane::types::cAsset;
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
                if !position.credit_amount.is_zero() {
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
