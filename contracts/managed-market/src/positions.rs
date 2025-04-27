use std::cmp::min;
use std::ops::Sub;
use std::str::FromStr;
use std::vec;

use cosmwasm_std::{
    attr, to_json_binary, Addr, Api, BankMsg, Coin, CosmosMsg, Decimal, DepsMut, Env, MessageInfo,
    QuerierWrapper, QueryRequest, Response, StdError, StdResult, Storage, SubMsg, Uint128, WasmMsg,
    WasmQuery,
};

use membrane::helpers::{validate_position_owner, asset_to_coin, withdrawal_msg, get_contract_balances};
use membrane::math::{decimal_division, decimal_multiplication, Uint256, decimal_subtraction};
use membrane::oracle::PriceResponse;
use membrane::osmosis_proxy::ExecuteMsg as OP_ExecuteMsg;
use membrane::types::{
    AssetInfo, BorrowOptions, UserPosition, VTClaimCheckpoint
};
use membrane::managed_market::{Config, ExecuteMsg, MarketParams};
use membrane::stability_pool_vault::{
    calculate_base_tokens, calculate_vault_tokens
};

use osmosis_std::types::osmosis::twap::v1beta1 as TWAP;
use osmosis_std::types::osmosis::tokenfactory::v1beta1::{self as TokenFactory};
use osmosis_std::types::osmosis::poolmanager::v1beta1::{MsgSwapExactAmountIn, SwapAmountInRoute};
use osmosis_std::types::osmosis::poolmanager::v1beta1::{self as PoolManager, SwapAmountOutRoute};
use serde::de;


use crate::rates::accrue;
use crate::state::{LiquidationPropagation, TokenRateAssurance, ACTIONS_PAUSED, CLAIM_TRACKER, DEBT_VAULT_TOKEN, LIQUIDATION, MARKET_PARAMS, TOKEN_RATE_ASSURANCE};
// use crate::state::{get_target_position, update_position, update_position_claims, ClosePositionPropagation, CollateralVolatility, Timer, BASKET, CLOSE_POSITION, FREEZE_TIMER, REDEMPTION_OPT_IN, STORED_PRICES, VOLATILITY};
use crate::{
    state::{
        CONFIG, POSITIONS
    },
    ContractError,
};

//Liquidation reply ids
pub const LIQUIDATE_REPLY_ID: u64 = 1u64;
pub const BAD_DEBT_REPLY_ID: u64 = 999999u64;


//Todo:
// - 

//Our Product roadmap is:
// Exotic collateral 
// -- borrow fee
// -- per user debt cap
// -- keep max LTV and borrow LTV close so that liquidations are small and don't cause the market to crash
// Leveraged blue chips (Pyth oracles)
// -- Hands free leverage (SL & Loop intents) (*for borrower*)
// -- Use liquidatibility & volatiility to increase interest rates *for borrower*
// -- Fixed rate (*for borrower*), grants stability for strats
// Interest rate arbs (redemptions, vault oracles)


///Launch markets
/// - CULT
/// - LAB
/// - KOPI

///V1
/// - Add state sanity checks for collateral usage
/// - Borrow fee for high risk collateral
/// - Immutability possibilities (No manager, no management fee, debt cap is based on liquidatibility, no debt minimum is safe bc the contract is upgradeable, anything else???)
/// -- The problem with immut is that it puts the responsibility on the contract admin

///V2: 
/// - STOP LOSS
/// - LTV Ramping
/// - Close Position
/// - Borrow Fee

///V3
/// - Liquidators can bring in their own CDT
/// - Use the liquidation queue for liquidations & add the collateral to the queue on instantiation.
/// 
///V4
/// - Multiple collateral types (if there is demand for it). Market params become a Vec<Config>. No duplicate collateral.
///
/// 
///V5
/// - Arbitrary debt types (Not a fan, we'd just be doing this for revenue)
/// - New oracle types (Pyth)
/// 
/// V6
/// - Set a threshold (100k CDT), where revenue for any debt passed gets split to Membrane.
// To reduce this split you buy MBRN.
// The threshold should be high enough to pay over management costs. So small profit + management costs.
/// 
/// 

//Constants
const NOBLE_USDC_DENOM: &str = "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4";
pub const CDT_DENOM: &str = "factory/osmo1s794h9rxggytja3a4pmwul53u98k06zy2qtrdvjnfuxruh7s8yjs6cyxgd/ucdt";


/// Deposit collateral to receive receipt tokens.
    /// Assert:
    /// - Only one asset is sent (error)
    /// - A market exists for that asset (error)
    /// - The contract isn't frozen (error)
    /// - The owner is whitelisted (error)
    /// - State is updated to reflect the deposit
pub fn supply_collateral(
    deps: DepsMut, 
    _env: Env,
    info: MessageInfo,
    owner: Option<String>,
) -> Result<Response, ContractError> {    
    let config = CONFIG.load(deps.storage)?;

    //Assert the sender sent only 1 asset
    if info.funds.len() != 1 {
        return Err(ContractError::CustomError { val: String::from("Need to send one collateral asset only") });
    }

    let market = match MARKET_PARAMS.load(deps.storage, info.funds[0].denom.clone()){
        Ok(market) => market,
        Err(_) => return Err(ContractError::CustomError { val: format!("Collateral asset ({:?}) not supported", info.funds[0].denom) }),
    };


    //Check if frozen
    //This ensures that if Freezes weren't enabled from the jump, the contract can't be frozen.
    if let Ok(frozen) = ACTIONS_PAUSED.load(deps.storage) { 
        if frozen {return Err(ContractError::Frozen {  }) }
    }

    //Set owner
    let owner = match owner {
        Some(owner) => deps.api.addr_validate(&owner)?,
        None => info.sender.clone(),
    };

    //Ensure owner is whitelisted if so
    if let Some(whitelisted_collateral_suppliers) = market.clone().whitelisted_collateral_suppliers {
        if let None = whitelisted_collateral_suppliers.into_iter().find(|addr| addr == &info.sender.to_string()){
            return Err(ContractError::CustomError { val: format!("Sender ({:?}) not whitelisted to supply collateral", info.sender.to_string()) });
        };
    }

    //Update user state 
    //Check & assert deposit asset
    POSITIONS.update(deps.storage, (owner.clone(), info.funds[0].denom.clone()), |position: Option<UserPosition>| -> Result<UserPosition, ContractError> {
        match position {
                Some(mut position) => {
                    position.collateral_amount += info.funds[0].amount;
                    return Ok(position)
                },
                None => {
                    return Ok(UserPosition {
                        collateral_denom: market.collateral_params.collateral_asset,
                        collateral_amount: info.funds[0].amount,
                        debt_amount: Uint128::zero(),
                        rate_index: Decimal::zero(),
                    })
                }
        }

    })?;

    Ok(Response::new()
    .add_attributes(vec![
        attr("method", "supply_collateral"),
    ]))
}

/// Deposit debt to receive receipt tokens.
    /// Assert:
    /// - The contract isn't frozen (error)
    /// - The owner is whitelisted to supply debt(error)
    /// - Only one asset is sent & its the debt token (error)
    /// - The amount sent is non-zero (error)
    /// - The amount sent doesn't break the global debt supply cap (error)
    /// - User is minted vault tokens if non-zero
    /// - Update the config.total_debt_tokens to reflect the deposit
    /// - Update vault token supply to reflect the mint & send
    /// - The debt vault token conversion rate is static (post-submsg error)
pub fn supply_debt(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    send_to: Option<String>,
) -> Result<Response, ContractError> {    
    let mut config = CONFIG.load(deps.storage)?;

    //Check if frozen.
    //This ensures that if Freezes weren't enabled from the jump, the contract can't be frozen.
    if let Ok(frozen) = ACTIONS_PAUSED.load(deps.storage) { 
        if frozen {return Err(ContractError::Frozen {  }) }
    }

    let mut msgs = vec![];

    //Ensure send_to is whitelisted if so
    if let Some(whitelisted_debt_suppliers) = config.clone().whitelisted_debt_suppliers {
        if let None = whitelisted_debt_suppliers.into_iter().find(|addr| addr == &info.sender.to_string()){
            return Err(ContractError::CustomError { val: format!("Sender ({:?}) not whitelisted to supply debt", info.sender.to_string()) });
        };
    }
// panic!("{:?}", config.clone().whitelisted_debt_suppliers );

    //Check & assert deposit asset
    //Assert the sender sent the deposit asset only
    if info.funds.len() != 1 || info.funds[0].denom != CDT_DENOM.to_string() {
        return Err(ContractError::CustomError { val: String::from("Need to send the debt asset only: factory/osmo1s794h9rxggytja3a4pmwul53u98k06zy2qtrdvjnfuxruh7s8yjs6cyxgd/ucdt") });
    }

    //Label supplied amount
    let supplied_amount = info.funds[0].amount;
    if supplied_amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    //Set send_to
    let send_to = match send_to {
        Some(send_to) => deps.api.addr_validate(&send_to)?,
        None => info.sender.clone(),
    };

    //Get total_debt_tokens
    let total_debt_tokens = get_total_debt_tokens(config.clone())?;
    //Get total_vault_tokens
    // let total_vault_tokens = DEBT_VAULT_TOKEN.load(deps.storage)?;

    //Ensure the deposit doesn't push the market over supply caps
    if let Some(debt_supply_cap) = config.debt_supply_cap {
        let total_debt = config.total_debt_tokens + supplied_amount;
        if total_debt > debt_supply_cap {
            return Err(ContractError::SupplyCapExceeded { balance: total_debt, cap: debt_supply_cap });
        }
    }

    //Accrue to make sure current suppliers get their yield
    //This is done to ensure that config's total_debt_tokens is up to date
    // accrue(
    //     deps.storage,
    //     total_debt_tokens,
    //     total_vault_tokens,
    //     env.clone(), 
    //     &mut config, 
    //     &mut UserPosition { 
    //         collateral_denom: String::from(""),
    //         collateral_amount: Uint128::zero(), 
    //         debt_amount: Uint128::zero(), 
    //         rate_index: Decimal::zero()
    //     },
    //     &mut msgs
    // )?;
    
    //Get total_vault_tokens
    let total_vault_tokens = DEBT_VAULT_TOKEN.load(deps.storage)?;

    //Calc & save token rates
    let pre_btokens_per_one = calculate_base_tokens(
        Uint128::new(1_000_000_000_000), 
        total_debt_tokens, //we don't subtract the supplied amount bc the total doesn't use contract balances
        total_vault_tokens
    )?;
    TOKEN_RATE_ASSURANCE.save(deps.storage, &TokenRateAssurance {
        pre_btokens_per_one,
    })?;


    //Calc vault token to user
    let vault_tokens_to_send = calculate_vault_tokens(
        supplied_amount,
        total_debt_tokens, //we don't subtract the supplied amount bc the total doesn't use contract balances
        total_vault_tokens.clone()
    )?;
    //Mint vault tokens to user
    if !vault_tokens_to_send.is_zero() {
        let mint_vault_tokens_msg: CosmosMsg = TokenFactory::MsgMint {
            sender: env.contract.address.to_string(), 
            amount: Some(osmosis_std::types::cosmos::base::v1beta1::Coin {
                denom: config.debt_supply_vault_token.clone(),
                amount: vault_tokens_to_send.to_string(),
            }), 
            mint_to_address: send_to.clone().to_string(),
        }.into();
        msgs.push(mint_vault_tokens_msg);
    }

    //Update config state
    config.total_debt_tokens += supplied_amount;
    CONFIG.save(deps.storage, &config)?;

    //Update vault token supply
    let new_vault_token_supply = match total_vault_tokens.checked_add(vault_tokens_to_send){
        Ok(v) => v,
        Err(_) => return Err(ContractError::CustomError { val: format!("Failed to add vault token total supply: {} + {}", total_vault_tokens, vault_tokens_to_send) }),
    };
    //Update vault token supply
    DEBT_VAULT_TOKEN.save(deps.storage, &new_vault_token_supply)?;


    //Add rate assurance callback msg
    if !total_vault_tokens.is_zero() {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance { })?,
            funds: vec![],
        }));
    }


    Ok(Response::new()
    .add_attributes(vec![
        attr("method", "supply_debt"),
    ]).add_messages(msgs))
}


/// Withdraw debt by sending vault receipt tokens
    /// Assert:
    /// - The contract isn't frozen (error)
    /// - Only one asset is sent & its the debt vault token (error)
    /// - The contract has enough debt token to send for the withdrawal (error)
    /// - Burn the sent vault tokens and send the underlying debt tokens
    /// - Update the config.total_debt_tokens to reflect the withdrawal
    /// - Update vault token supply to reflect the burn & send
    /// - The debt vault token conversion rate is static (post-submsg error)
pub fn withdraw_debt(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    send_to: Option<String>,
) -> Result<Response, ContractError> {    
    let mut config = CONFIG.load(deps.storage)?;

    //Check if frozen.
    //This ensures that if Freezes weren't enabled from the jump, the contract can't be frozen.
    if let Ok(frozen) = ACTIONS_PAUSED.load(deps.storage) { 
        if frozen {return Err(ContractError::Frozen {  }) }
    }

    let mut msgs = vec![];

    //Set send_to
    let send_to = match send_to {
        Some(send_to) => deps.api.addr_validate(&send_to)?,
        None => info.sender.clone(),
    };

    //Ensure sender is whitelisted
    if let Some(whitelisted_debt_suppliers) = config.clone().whitelisted_debt_suppliers {
        if let None = whitelisted_debt_suppliers.into_iter().find(|addr| addr == &info.sender.to_string()){
            return Err(ContractError::CustomError { val: format!("Sender ({:?}) not whitelisted to supply debt, so they can't withdraw either.", info.sender.to_string()) });
        };
    }

    //Accrue to make sure withdrawing suppliers get their yield.
    //This is done to ensure that config's total_debt_tokens is up to date.
    // let total_vault_tokens = DEBT_VAULT_TOKEN.load(deps.storage)?;
    // accrue(
    //     deps.storage,
    //     get_total_debt_tokens(config.clone())?,
    //     total_vault_tokens,
    //     env.clone(), 
    //     &mut config, 
    //     &mut UserPosition { 
    //         collateral_denom: String::from(""),
    //         collateral_amount: Uint128::zero(), 
    //         debt_amount: Uint128::zero(), 
    //         rate_index: Decimal::zero(),
    //     },
    //     &mut msgs
    // )?;
    //Check & assert vault token
    //Assert the sender sent the vault token only
    if info.funds.len() != 1 || info.funds[0].denom != config.debt_supply_vault_token {
        return Err(ContractError::CustomError { val: format!("Need to send the vault token only: {}", config.debt_supply_vault_token) });
    }
    //Label vault tokens sent
    let vault_tokens_sent = info.funds[0].amount;
    if vault_tokens_sent.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }
    
    //Get total_debt_tokens
    let total_debt_tokens = get_total_debt_tokens(config.clone())?;
    //Get total_vault_tokens
    let total_vault_tokens = DEBT_VAULT_TOKEN.load(deps.storage)?;


    //Calc base token to user
    let base_tokens_to_send = calculate_base_tokens(
        vault_tokens_sent,
        total_debt_tokens,
        total_vault_tokens.clone()
    )?;

    //Get balance of debt tokens we have to send.
    let debt_token_balance = deps.querier.query_balance(env.clone().contract.address, CDT_DENOM.to_string())?;


    //If we have less debt tokens than being requested, we error.
    if debt_token_balance.amount < base_tokens_to_send {
        return Err(ContractError::CustomError { val: format!("Not enough debt tokens to send, maximum: {}", debt_token_balance.amount ) });
    }

    //Burn vault tokens.
    //Send base tokens to user.
    if !base_tokens_to_send.is_zero() {
        //Burn vault tokens.
        let burn_vault_tokens_msg: CosmosMsg = TokenFactory::MsgBurn {
            sender: env.contract.address.to_string(), 
            amount: Some(osmosis_std::types::cosmos::base::v1beta1::Coin {
                denom: config.debt_supply_vault_token.clone(),
                amount: vault_tokens_sent.to_string(),
            }), 
            burn_from_address: env.contract.address.to_string(),
        }.into();
        msgs.push(burn_vault_tokens_msg);

        //Send base tokens to user.
        let send_base_tokens_msg: CosmosMsg = BankMsg::Send {
            to_address: send_to.to_string(),
            amount: vec![Coin {
                denom: CDT_DENOM.to_string(),
                amount: base_tokens_to_send,
            }],
        }.into();
        msgs.push(send_base_tokens_msg);
    }

    //Update config state
    config.total_debt_tokens = match config.total_debt_tokens.checked_sub(base_tokens_to_send){
        Ok(val) => val,
        Err(_) => return Err(ContractError::CustomError { val: "Total Debt - Debt to Send: underflow error".to_string() }),
    };
    CONFIG.save(deps.storage, &config)?;
    


    //Calc & save token rates
    let pre_btokens_per_one = calculate_base_tokens(
        Uint128::new(1_000_000_000_000), 
        total_debt_tokens, 
        total_vault_tokens
    )?;
    TOKEN_RATE_ASSURANCE.save(deps.storage, &TokenRateAssurance {
        pre_btokens_per_one,
    })?;


    //Update the total vault tokens
    let new_vault_token_supply = match total_vault_tokens.checked_sub(vault_tokens_sent){
        Ok(v) => v,
        Err(_) => return Err(ContractError::CustomError { val: format!("Failed to subtract vault token total supply: {} - {}", total_vault_tokens, vault_tokens_sent) }),
    };

    //Update vault token supply
    DEBT_VAULT_TOKEN.save(deps.storage, &new_vault_token_supply)?;
    
    //Add rate assurance callback msg if this withdrawal leaves other depositors with tokens to withdraw.
    if !new_vault_token_supply.is_zero() {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance { })?,
            funds: vec![],
        }));
    }

    Ok(Response::new()
    .add_attributes(vec![
        attr("method", "withdraw_debt"),
        attr("vault_tokens_burnt", vault_tokens_sent),
        attr("base_tokens_withdrawn", base_tokens_to_send)
    ]).add_messages(msgs))
}


/// Withdraw collateral to receive deposited tokens back
pub fn withdraw_collateral(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    send_to: Option<String>,
    collateral_denom: String,
    withdraw_amount: Option<Uint128>,
) -> Result<Response, ContractError> {    
    let mut config = CONFIG.load(deps.storage)?;
    let market = match MARKET_PARAMS.load(deps.storage, collateral_denom.clone()){
        Ok(market) => market,
        Err(_) => return Err(ContractError::CustomError { val: format!("Collateral asset ({:?}) not supported", collateral_denom) }),
    };

    let mut msgs = vec![];
    
    //Check if frozen.
    //This ensures that if Freezes weren't enabled from the jump, the contract can't be frozen.
    if let Ok(frozen) = ACTIONS_PAUSED.load(deps.storage) { 
        if frozen {return Err(ContractError::Frozen {  }) }
    }

    //Load user state
    let mut user_position = POSITIONS.load(deps.storage, (info.sender.clone(), collateral_denom.clone()))?;

    //Return early if no collateral
    if user_position.collateral_amount.is_zero() {
        return Err(ContractError::CustomError { val: "No collateral to withdraw".to_string() });
    }

    //Set send to
    let send_to = match send_to {
        Some(send_to) => deps.api.addr_validate(&send_to)?,
        None => info.sender.clone(),
    };

    let total_vault_tokens = DEBT_VAULT_TOKEN.load(deps.storage)?;

    //Accrue if debt is owed
    accrue(
        deps.storage,
        get_total_debt_tokens(config.clone())?,
        total_vault_tokens,
        env.clone(), 
        &mut config, 
        &mut user_position,
        &mut msgs
    )?;

    //Assert withdraw is valid. 
    //Withdrawable amount is capped by user's state & current LTV.
    let withdrawable_amount = match user_position.debt_amount.is_zero() {
        true => {
            //Do nothing
            let withdraw_amount = withdraw_amount.unwrap_or_else(|| user_position.collateral_amount);
            min(withdraw_amount, user_position.collateral_amount)
        },
        false => {
            //If the user has debt, they can only withdraw up to the borrow LTV
            let collateral_price = get_collateral_price(deps.storage, deps.querier, env.clone(), market.clone())?;
            let debt_price = get_cdt_price(deps.querier, env.clone())?;
            let collateral_value = collateral_price.get_value(user_position.collateral_amount)?;
            let debt_value = debt_price.get_value(user_position.debt_amount)?;

            let borrow_LTV = market.collateral_params.max_borrow_LTV;

            //Calc withdrawable amount////
            let min_collateral_value = match decimal_division(debt_value, borrow_LTV){
                Ok(val) => val,
                Err(_) => return Err(ContractError::CustomError { val: format!("Failed to calculate min collateral value: {:?} / {:?}", debt_value, borrow_LTV) }),
            };
            if collateral_value <= min_collateral_value {
                return Err(ContractError::CustomError { val: "No space to withdraw".to_string() });
            }
            let withdrawable_value = match decimal_subtraction(collateral_value, min_collateral_value){
                Ok(val) => val,
                Err(_) => return Err(ContractError::CustomError { val: format!("Failed to calculate withdrawable value: {:?} - {:?}", collateral_value, min_collateral_value) }),
            };
            let withdrawable_amount = collateral_price.get_amount(withdrawable_value)?;
            //////
            min(withdrawable_amount, user_position.collateral_amount)
        }
    };


    //Update state for user
    user_position.collateral_amount -= withdrawable_amount;
    POSITIONS.save(deps.storage, (info.sender.clone(), collateral_denom.clone()), &user_position)?;

    //Update state for config
    CONFIG.save(deps.storage, &config)?;

    //Send withdrawn assets
    let withdraw_coins = vec![Coin {
        denom: market.collateral_params.collateral_asset,
        amount: withdrawable_amount,
    }];
    let withdraw_collateral_message = CosmosMsg::Bank(BankMsg::Send {
        to_address: send_to.to_string(),
        amount: withdraw_coins,
    });

    Ok(Response::new()
    .add_attributes(vec![
        attr("method", "withdraw_collateral"),
    ])
    .add_message(withdraw_collateral_message)
.add_messages(msgs))
}

pub fn get_collateral_price(
    storage: &dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    market: MarketParams,
) -> Result<PriceResponse, ContractError> {
    //Load state
    let config: Config = CONFIG.load(storage)?;
    let asset_oracle_info = market.pool_for_oracle_and_liquidations;


    //twap_timeframe = MINUTES * SECONDS_PER_MINUTE
    let twap_timeframe: u64 = (60 * 60);
    let start_time: u64 = env.block.time.seconds() - twap_timeframe;

    let mut asset_price_in_lp_steps = vec![];


    //Query prices from the TWAP sources
    //This can use multiple pools to calculate our price
    for pool in asset_oracle_info.pools_for_osmo_twap.clone() {

        let res: TWAP::GeometricTwapToNowResponse = TWAP::TwapQuerier::new(&querier).geometric_twap_to_now(
            pool.clone().pool_id, 
            pool.clone().base_asset_denom, 
            pool.clone().quote_asset_denom, 
            Some(osmosis_std::shim::Timestamp {
                seconds:  start_time as i64,
                nanos: 0,
            }),
        )?;

        //Push TWAP
        asset_price_in_lp_steps.push(Decimal::from_str(&res.geometric_twap)?);
    }

    //Multiply prices to denominate in USDC
    let asset_price_in_usdc = {
        let mut final_price = Decimal::one();
        //If no prices were queried, return error
        if asset_price_in_lp_steps.len() == 0 {
            return Err(ContractError::CustomError {
                val: String::from("No TWAP prices found"),
            });
        }

        //Find asset price in USDC
        //Multiply prices to get the desired Quote
        for price in asset_price_in_lp_steps {
            final_price = decimal_multiplication(final_price, price)?;
        } 
        //Results in slight error: (https://medium.com/reflexer-labs/analysis-of-the-rai-twap-oracle-20a01af2e49d)

        final_price
    };

    Ok(PriceResponse { 
        prices: vec![], 
        price: asset_price_in_usdc, 
        decimals: asset_oracle_info.decimals.clone() })
}

pub fn get_cdt_price(
    querier: QuerierWrapper,
    env: Env,
) -> Result<PriceResponse, ContractError> {

    //twap_timeframe = MINUTES * SECONDS_PER_MINUTE
    let twap_timeframe: u64 = (60 * 60);
    let start_time: u64 = env.block.time.seconds() - twap_timeframe;

    //Query CDT/USDC price

        let res: TWAP::GeometricTwapToNowResponse = TWAP::TwapQuerier::new(&querier).geometric_twap_to_now(
            1268, 
            CDT_DENOM.to_string(), 
            NOBLE_USDC_DENOM.to_string(), 
            Some(osmosis_std::shim::Timestamp {
                seconds:  start_time as i64,
                nanos: 0,
            }),
        )?;

    //Price in USDC
    let asset_price_in_usdc = Decimal::from_str(&res.geometric_twap)?;

    Ok(PriceResponse { 
        prices: vec![], 
        price: asset_price_in_usdc, 
        decimals: 6 })
}

/// Borrow CDT from the market & add it as debt to the user's position
    /// Assert:
    /// - The contract isn't frozen (error)
    /// - The user has a position in the market (error)
    /// - The borrow doesn't break the debt cap (forced minimum)
    /// - The user has enough collateral to borrow the requested amount (forced minimum)
    /// - User state is updated to reflect the borrow
    /// - The market state is updated to reflect the borrow
    /// - The borrowed amount is sent to the user
pub fn borrow_cdt(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    send_to: Option<String>,
    collateral_denom: String,
    borrow_options: BorrowOptions,
) -> Result<Response, ContractError> {    
    let mut config = CONFIG.load(deps.storage)?;
    let mut market = match MARKET_PARAMS.load(deps.storage, collateral_denom.clone()){
        Ok(market) => market,
        Err(_) => return Err(ContractError::CustomError { val: format!("Collateral asset ({:?}) not supported", collateral_denom) }),
    };
    let mut msgs = vec![];

    //Check if frozen
    //This ensures that if Freezes weren't enabled from the jump, the contract can't be frozen.
    if let Ok(frozen) = ACTIONS_PAUSED.load(deps.storage) { 
        if frozen {return Err(ContractError::Frozen {  }) }
    }
    

    //Load user state
    let mut user_position = POSITIONS.load(deps.storage, (info.sender.clone(), collateral_denom.clone()))?;

    //Set send to
    let send_to = match send_to {
        Some(send_to) => deps.api.addr_validate(&send_to)?,
        None => info.sender.clone(),
    };

    let total_vault_tokens = DEBT_VAULT_TOKEN.load(deps.storage)?;

    //Accrue.
    //Even without debt, this keeps everyones's state up to date
    accrue(
        deps.storage,
        get_total_debt_tokens(config.clone())?,
        total_vault_tokens,
        env.clone(), 
        &mut config,
        &mut user_position,
        &mut msgs
    )?;

    let debt_price = get_cdt_price(deps.querier, env.clone())?;
    

    //Assert borrow is valid. 
    //Borrowable amount is capped by current LTV & borrowable LTV & borrow cap.
    let borrowable_amount = {
        let collateral_price = get_collateral_price(deps.storage, deps.querier, env.clone(), market.clone())?;
        let collateral_value = collateral_price.get_value(user_position.collateral_amount)?;
        let debt_value: Decimal = debt_price.get_value(user_position.debt_amount)?;

        let borrow_LTV = market.collateral_params.max_borrow_LTV;

        //Calc borrowable amount////
        let max_borrowable_value = decimal_multiplication(collateral_value, borrow_LTV)?;
        let borrowable_value = match decimal_subtraction(max_borrowable_value, debt_value){
            Ok(val) => val,
            Err(_) => return Err(ContractError::CustomError { val: format!("Failed to calculate borrowable value: {:?} - {:?}", max_borrowable_value, debt_value) }),
        };
        let borrowable_amount = debt_price.get_amount(borrowable_value)?;
        //////
        //Calc current ltv
        let current_ltv = match !collateral_value.is_zero() {
            true => decimal_division(debt_value, collateral_value)?,
            false => return Err(ContractError::CustomError { val: "No collateral value".to_string() }),
        };
        //Calc borrow amount
        let borrow_amount = calc_borrow_amount(
            borrow_options.clone(),
            market.collateral_params.max_borrow_LTV,
            current_ltv,
            debt_price.clone(),
            collateral_value
        )?;
        //Get pre-capped borrow amount
        let theoretical_borrow = min(borrow_amount, borrowable_amount);

        //Does this market have a fixed borrow cap?
        let space_to_borrow = match market.borrow_cap.fixed_cap {
            Some(cap) => {
                //Calc space to borrow within the cap
                cap.checked_sub(market.total_borrowed).unwrap_or(Uint128::zero())
            },
            None => borrowable_amount
        };
        let capped_borrow = min(theoretical_borrow, space_to_borrow);
        //If the market has a per user borrow cap, check it
        let capped_borrow = match market.per_user_debt_cap {
            Some(cap) => {
                //Calc space to borrow within the cap
                cap.checked_sub(user_position.debt_amount).unwrap_or(Uint128::zero())
            },
            None => capped_borrow
        };
        //Check contract balances for actual borrow amounts
        let contract_balance_of_debt = get_contract_balances(
            deps.querier,
            env.clone(),
            vec![AssetInfo::NativeToken { denom: CDT_DENOM.to_string() }],
        )?[0];
        let actual_borrow = min(capped_borrow, contract_balance_of_debt);

        actual_borrow
    };

    //Check borrow amount is non-zero
    if borrowable_amount.is_zero() {
        return Err(ContractError::CustomError { val: "No space to borrow".to_string() });
    }

    //Query the estimated swap amount for the new total borrowed amount to make sure it can be liquidated.
    let new_total_borrowed = match market.total_borrowed.checked_add(borrowable_amount){
        Ok(val) => val,
        Err(_) => return Err(ContractError::CustomError { val: format!("Total Borrowed: {} + Borrow Amount: {}, underflow error", market.total_borrowed, borrowable_amount) }),
    };
    //Should this be a market toggle? Yes.
    if market.borrow_cap.cap_borrows_by_liquidity {
        check_debt_liquidatibility(deps.querier, market.clone(), 
            new_total_borrowed, 
            get_contract_balances(
                deps.querier,
                env.clone(),
                vec![AssetInfo::NativeToken { denom: market.clone().collateral_params.collateral_asset }],
            )?[0],
        )?;
    }

    //Update state for user
    user_position.debt_amount = match user_position.debt_amount.checked_add(borrowable_amount){
        Ok(val) => val,
        Err(_) => return Err(ContractError::CustomError { val: format!("User Debt Amount: {} + Borrow Amount: {}, underflow error", user_position.debt_amount, borrowable_amount) }),
    };
    POSITIONS.save(deps.storage, (info.sender.clone(), collateral_denom.clone()), &user_position)?;

    //Update state for config
    market.total_borrowed = match market.total_borrowed.checked_add(borrowable_amount){
        Ok(val) => val,
        Err(_) => return Err(ContractError::CustomError { val: format!("Total Borrowed: {} + Borrow Amount: {}, underflow error", market.total_borrowed, borrowable_amount) }),
    };
    MARKET_PARAMS.save(deps.storage, collateral_denom.clone(), &market)?;
    CONFIG.save(deps.storage, &config)?;

    //Send borrowed CDT
    let borrow_coins = vec![Coin {
        denom: CDT_DENOM.to_string(),
        amount: borrowable_amount,
    }];
    let borrow_cdt_message = CosmosMsg::Bank(BankMsg::Send {
        to_address: send_to.to_string(),
        amount: borrow_coins,
    });



    Ok(Response::new()
    .add_attributes(vec![
        attr("method", "borrow_cdt"),
    ])
    .add_message(borrow_cdt_message)
.add_messages(msgs))
}

fn calc_borrow_amount(
    borrow_options: BorrowOptions,
    max_borrowable_ltv: Decimal,
    current_ltv: Decimal,
    debt_price: PriceResponse,
    collateral_value: Decimal,
) -> Result<Uint128, ContractError> {

    //Calculate borrow amount
    let borrow_amount = if let Some(borrow_amount) = borrow_options.amount {
        borrow_amount
    } else if let Some(borrow_ltv) = borrow_options.ltv {
        if borrow_ltv > max_borrowable_ltv {
            return Err(ContractError::CustomError { val: "Borrow LTV greater than max borrowable LTV".to_string() });
        }
        if borrow_ltv < current_ltv {
            return Err(ContractError::CustomError { val: "Borrow LTV less than current LTV".to_string() });
        }

        //Calculate borrow amount based on LTV
        let borrowable_ltv = match decimal_subtraction(
            borrow_ltv,
            current_ltv,
        ){
            Ok(val) => val,
            Err(_) => return Err(ContractError::CustomError { val: format!("Failed to calculate borrowable LTV: {:?} - {:?}", borrow_ltv, current_ltv) }),
        };

        let borrowable_value = decimal_multiplication(
            borrowable_ltv,
            collateral_value,
        )?;
        let borrowable_amount = debt_price.get_amount(borrowable_value)?;

        borrowable_amount

    } else {
        return Err(ContractError::CustomError { val: "No borrow amount specified".to_string() });
    };

    //Check borrow amount is valid
    if borrow_amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    Ok(borrow_amount)
}

/// Repay CDT to your position and add liquidity to the market.
/// No debt minimum because the manager will liquidate any unprofitable positions.
pub fn repay_cdt(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    collateral_denom: String,
) -> Result<Response, ContractError> {    
    let mut config = CONFIG.load(deps.storage)?;
    let mut market = match MARKET_PARAMS.load(deps.storage, collateral_denom.clone()){
        Ok(market) => market,
        Err(_) => return Err(ContractError::CustomError { val: format!("Collateral asset ({:?}) not supported", collateral_denom.clone()) }),
    };

    //Check if frozen
    //This ensures that if Freezes weren't enabled from the jump, the contract can't be frozen.
    if let Ok(frozen) = ACTIONS_PAUSED.load(deps.storage) { 
        if frozen {return Err(ContractError::Frozen {  }) }
    }


    //Check & assert deposit asset
    //Assert the sender sent the deposit asset only
    if info.funds.len() != 1 || info.funds[0].denom != CDT_DENOM.to_string() {
        return Err(ContractError::CustomError { val: format!("Need to send the debt asset only: {}", CDT_DENOM) });
    }

    //Label repayment amount 
    let mut repay_amount = info.funds[0].amount;
    let mut excess_repayment = Uint128::zero();
    let mut msgs = vec![];

    //Load user state
    let mut user_position = POSITIONS.load(deps.storage, (info.sender.clone(), collateral_denom.clone()))?;

    let total_vault_tokens = DEBT_VAULT_TOKEN.load(deps.storage)?;

    //Accrue.
    //Even without debt, this keeps everyones's state up to date
    accrue(
        deps.storage,
        get_total_debt_tokens(config.clone())?,
        total_vault_tokens,
        env.clone(), 
        &mut config, 
        &mut user_position,
        &mut msgs
    )?;    


    //Update state for user
    //Calculate the amount of debt that can be repaid
    user_position.debt_amount = match user_position.debt_amount.checked_sub(repay_amount){
        Ok(difference) => difference,
        Err(_err) => {
            //Set excess_repayment
            excess_repayment = repay_amount - user_position.debt_amount;
            //Set new repay_amount
            repay_amount = user_position.debt_amount;
            
            Uint128::zero()
        },
    };
    POSITIONS.save(deps.storage, (info.sender.clone(), collateral_denom.clone()), &user_position)?;


     //Send back excess repayment, defaults to the repaying address
     if !excess_repayment.is_zero() {
        let excess_repayment_coins = vec![Coin {
            denom: CDT_DENOM.to_string(),
            amount: excess_repayment,
        }];
        let excess_repayment_message = CosmosMsg::Bank(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: excess_repayment_coins,
        });
        msgs.push(excess_repayment_message);
    }


    //Update state for config
    market.total_borrowed = match market.total_borrowed.checked_sub(repay_amount){
        Ok(val) => val,
        Err(_) => return Err(ContractError::CustomError { val: format!("Total Borrowed: {} - Repay Amount: {}, underflow error", market.total_borrowed, repay_amount) }),
    };
    //Save market
    MARKET_PARAMS.save(deps.storage, collateral_denom.clone(), &market)?;
    //Update config state
    CONFIG.save(deps.storage, &config)?;


    Ok(Response::new()
    .add_attributes(vec![
        attr("method", "repay_cdt"),
        attr("repaid_amount", repay_amount),
        attr("excess_repayment", excess_repayment),
    ])
    .add_messages(msgs))
}


pub fn check_debt_liquidatibility(
    querier: QuerierWrapper,
    market: MarketParams,
    debt_amount: Uint128,
    collateral_amount: Uint128,
) -> Result<(), ContractError> {

    //Get swap routes
    let routes = get_swap_out_routes(market.clone())?;

    //Get debt value
    // let debt_value = debt_price.get_value(debt_amount)?;

    //Estimate swap
    let res: PoolManager::EstimateSwapExactAmountOutResponse = PoolManager::PoolmanagerQuerier::new(&querier).estimate_swap_exact_amount_out(
        0u64,  //id doesn't matter here
        routes, //routes are the oracle pool plus 1268 the CDT pool
        debt_amount.to_string(),
    )?;
    
    //This doesn't account for individual position liquidatibility
    if Uint128::from_str(&res.token_in_amount).unwrap() > collateral_amount {
        return Err(ContractError::NoLiquidatibility {  });
    }

    Ok(())
}

fn get_swap_out_routes(
    market: MarketParams,
) -> Result<Vec<SwapAmountOutRoute>, ContractError> {
    
    let mut routes = vec![];

    //Get the oracle pool
    let oracle_pool = market.pool_for_oracle_and_liquidations;

    //Convert the oracle pool to a swap route
    oracle_pool.pools_for_osmo_twap.into_iter().for_each(|pool| {
        routes.push(SwapAmountOutRoute {
            pool_id: pool.pool_id,
            token_in_denom: pool.base_asset_denom,
        });
    });

    //Add the CDT pool
    routes.push(SwapAmountOutRoute {
        pool_id: 1268u64,
        token_in_denom: NOBLE_USDC_DENOM.to_string(),
    });

    Ok(routes)
}

fn get_swap_in_routes(
    market: MarketParams,
) -> Result<Vec<SwapAmountInRoute>, ContractError> {
    
    let mut routes = vec![];

    //Get the oracle pool
    let oracle_pool = market.pool_for_oracle_and_liquidations;

    //Convert the oracle pool to a swap route
    oracle_pool.pools_for_osmo_twap.into_iter().for_each(|pool| {
        routes.push(SwapAmountInRoute {
            pool_id: pool.pool_id,
            token_out_denom: pool.quote_asset_denom,
        });
    });

    //Add the CDT pool
    routes.push(SwapAmountInRoute {
        pool_id: 1268u64,
        token_out_denom: CDT_DENOM.to_string(),
    });

    Ok(routes)
}


fn create_swap_msg(
    env: Env,
    collateral_denom: String,
    collateral_amount: Uint128,
    collateral_price: PriceResponse,
    debt_price: PriceResponse,
    routes: Vec<SwapAmountInRoute>,
    max_slippage: Decimal,
) -> Result<CosmosMsg, ContractError> {
    //Get token_in & token_out prices
    let token_in_price = collateral_price.clone();
    let token_out_price = debt_price.clone();

    //Calculate min amount out
    let token_in_value = token_in_price.get_value(collateral_amount)?;
    let token_out_min_value = decimal_multiplication(token_in_value, Decimal::one() - max_slippage)?;
    let token_out_min_amount = token_out_price.get_amount(token_out_min_value)?;

    //Create Msg
    let msg: CosmosMsg = MsgSwapExactAmountIn {
        sender: env.contract.address.to_string(),
        routes,
        token_in: Some(osmosis_std::types::cosmos::base::v1beta1::Coin {
            amount: collateral_amount.to_string(),
            denom: collateral_denom
        }),
        token_out_min_amount: token_out_min_amount.to_string(),
        
    }.into();

    Ok(msg)
}
//How do we socialize bad debt?
//On liquidation we'll have to check for bad debt and add it to the config.
pub fn get_total_debt_tokens(
    config: Config,
) -> StdResult<Uint128> {

    Ok(config.total_debt_tokens.checked_sub(config.bad_debt).unwrap_or(Uint128::zero()))

}
///Rate assurance
/// Ensures that the conversion rate is static for debt deposits & withdrawals
pub fn rate_assurance(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    //Load config    
    let config = CONFIG.load(deps.storage)?;

    //Error if not the contract calling
    if info.sender != env.contract.address {
        return Err(ContractError::Unauthorized { owner: env.contract.address.to_string() });
    }

    //Load Token Assurance State
    let token_rate_assurance = TOKEN_RATE_ASSURANCE.load(deps.storage)?;

    //Load Vault token supply
    let total_vault_tokens = DEBT_VAULT_TOKEN.load(deps.storage)?;

    //Get total_debt_tokens
    let total_debt_tokens = get_total_debt_tokens(config.clone())?;

    //Calc the rate of vault tokens to deposit tokens
    let btokens_per_one = calculate_base_tokens(
        Uint128::new(1_000_000_000_000), 
        total_debt_tokens, 
        total_vault_tokens
    )?;

    //For deposit or withdraw, check that the rates are static 
    if btokens_per_one != token_rate_assurance.pre_btokens_per_one {
        return Err(ContractError::CustomError { val: format!("Deposit or withdraw rate assurance failed for base token conversion. pre: {:?} --- post: {:?}", token_rate_assurance.pre_btokens_per_one, btokens_per_one) });
    }

    Ok(Response::new())
}

//Liquidate
/// Liquidate an insolvent position to repay debts by selling collateral.
// - we'll have to check for bad debt and add it to the config.
pub fn liquidate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    collateral_denom: String,
    position_owner: String,
    take_fee: bool,
    max_slippage: Option<Decimal>
)-> Result<Response, ContractError>{

    //Load Config
    let mut config: Config = CONFIG.load(deps.storage)?;
    let market = match MARKET_PARAMS.load(deps.storage, collateral_denom.clone()){
        Ok(market) => market,
        Err(_) => return Err(ContractError::CustomError { val: format!("Collateral asset ({:?}) not supported", collateral_denom) }),
    };
    let mut msgs = vec![];

    //Check if frozen.
    //This ensures that if Freezes weren't enabled from the jump, the contract can't be frozen.
    if let Ok(frozen) = ACTIONS_PAUSED.load(deps.storage) { 
        if frozen {return Err(ContractError::Frozen {  }) }
    }

    //Set slippage
    let max_slippage = match max_slippage {
        Some(max_slippage) => max_slippage,
        None => market.max_slippage,
    };

    //Validate position owner
    let position_owner = deps.api.addr_validate(&position_owner)?;

    //Load user position
    let mut user_position = POSITIONS.load(deps.storage, (position_owner.clone(), collateral_denom.clone()))?;

    let total_vault_tokens = DEBT_VAULT_TOKEN.load(deps.storage)?;

    //Accrue.
    //Even without debt, this keeps everyones's state up to date
    accrue(
        deps.storage,
        get_total_debt_tokens(config.clone())?,
        total_vault_tokens, 
        env.clone(), 
        &mut config, 
        &mut user_position,
        &mut msgs
    )?;    


    //Check if the position is insolvent
    let collateral_price = get_collateral_price(deps.storage, deps.querier, env.clone(), market.clone())?;
    let debt_price = get_cdt_price(deps.querier, env.clone())?;
    let collateral_value = collateral_price.get_value(user_position.collateral_amount)?;
    if collateral_value.is_zero() {
        return Err(ContractError::CustomError { val: "Collateral value is zero; cannot calculate LTV".to_string() });
    }
    let debt_value = debt_price.get_value(user_position.debt_amount)?;
    let position_LTV = decimal_division(debt_value, collateral_value)?;
    if position_LTV < market.collateral_params.liquidation_LTV {
        return Err(ContractError::CustomError { val: format!("Position is solvent. Position LTV: {} is below the liquidation LTV: {}", position_LTV, market.collateral_params.liquidation_LTV) });
    }
    ///Because the position is insolvent, we'll liquidate it///
    
    //Get the amount we need to liquidate based on the LTV distance
    //...between the current LTV and the max borrow LTV
    let LTV_space_to_liquidate = match decimal_subtraction(position_LTV, market.collateral_params.max_borrow_LTV){
        Ok(val) => { val },
        Err(_) => return Err(ContractError::CustomError { val: format!("Failed to calculate LTV space to liquidate") }),
    };

    //Get the amount of collateral to liquidate
    let collateral_value_to_liquidate = decimal_multiplication(collateral_value, LTV_space_to_liquidate)?;
    let collateral_amount_to_liquidate = min(
        collateral_price.get_amount(collateral_value_to_liquidate)?,
         user_position.collateral_amount
    );
    if collateral_amount_to_liquidate.is_zero() {
        return Err(ContractError::CustomError { val: "Calculated zero collateral to liquidate".to_string() });
    }

    //Calculate the fee for the caller based on the distance between the current LTV and the liquidation LTV
    //in %.
    let liquidation_fee = match decimal_subtraction(position_LTV, market.collateral_params.liquidation_LTV){
        Ok(val) => { val },
        Err(_) => return Err(ContractError::CustomError { val: format!("Failed to calculate liquidation fee") }),
    };
    //Calc fee token amount
    let fee_value   = decimal_multiplication(collateral_value_to_liquidate, liquidation_fee)?;
    let mut fee_amount = collateral_price.get_amount(fee_value)?;

    //Set fee to 0 if the caller doesn't want to charge fees for themself (manager or aligned contract)
    if !take_fee {
        fee_amount = Uint128::zero();
    }

    //Max amount to liquidate + fee, is the user's collateral amount.
    //We edit the liquidation amount, not the fee amount.
    if collateral_amount_to_liquidate + fee_amount > user_position.collateral_amount {
        fee_amount = min(
            match user_position.collateral_amount.checked_sub(collateral_amount_to_liquidate) {
                Ok(val) => val,
                Err(_) => return Err(ContractError::CustomError { val: format!("Fee amount: {} + Collateral amount to liquidate: {} > User collateral amount: {}", fee_amount, collateral_amount_to_liquidate, user_position.collateral_amount) }),
            },
        fee_amount
    );
    }

    //Remove liquidated collateral amount from user state 
    user_position.collateral_amount -= collateral_amount_to_liquidate + fee_amount;


    //Create swap msg for liquidations
    let swap_msg = create_swap_msg(
        env.clone(), 
        market.clone().collateral_params.collateral_asset, 
        collateral_amount_to_liquidate, 
        collateral_price, 
        debt_price, 
        get_swap_in_routes(market.clone())?, 
        max_slippage,
    )?;
    //Reply on success to edit the user position based on the CDT that was swapped for & edit the config's total borrowed.
    let sub_msg = SubMsg::reply_on_success(swap_msg, LIQUIDATE_REPLY_ID);
    //Save pre liquidation CDT balance 
    let cdt_balance = deps.querier.query_balance(env.clone().contract.address, CDT_DENOM.to_string())?.amount;
    LIQUIDATION.save(deps.storage, & LiquidationPropagation {
        collateral_denom: collateral_denom.clone(),
        position_owner: position_owner.clone(),
        pre_liquidation_cdt_balance: cdt_balance,
    })?;


    //Create fee msg
    if !fee_amount.is_zero(){
        let fee_msg: CosmosMsg = CosmosMsg::Bank(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: vec![Coin {
                denom: market.clone().collateral_params.collateral_asset,
                amount: fee_amount,
            }],
        });
        msgs.push(fee_msg);
    }

    //Save user position
    POSITIONS.save(deps.storage, (position_owner.clone(), collateral_denom.clone()), &user_position)?;

    //Create msg to check for bad debt post liquidation
    let check_bad_debt_msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&ExecuteMsg::CheckBadDebt { })?,
        funds: vec![],
    });

    //Update config state
    CONFIG.save(deps.storage, &config)?;

    //Create response
    Ok(Response::new()
        .add_submessage(sub_msg)
        .add_messages(msgs)
        .add_submessage(SubMsg::reply_always(check_bad_debt_msg, BAD_DEBT_REPLY_ID))
        .add_attributes(vec![
            attr("method", "liquidate"),
            attr("user", position_owner),
            attr("collateral_liquidated", collateral_amount_to_liquidate),
            attr("fee_liquidated", fee_amount),
        ])) 

}


/// Check and recapitilize Bad Debt w/ revenue or MBRN auctions
pub fn check_and_fulfill_bad_debt(
    deps: DepsMut,
    _env: Env,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    //Load Liquidation Prop
    let liq_prop = LIQUIDATION.load(deps.storage)?;

    //Load market
    let market = match MARKET_PARAMS.load(deps.storage, liq_prop.collateral_denom.clone()){
        Ok(market) => market,
        Err(_) => return Err(ContractError::CustomError { val: format!("Collateral asset ({:?}) not supported", liq_prop.collateral_denom) }),
    };

    //Load user position
    let mut liquidated_position = POSITIONS.load(deps.storage, (liq_prop.position_owner.clone(), liq_prop.collateral_denom.clone()))?;
    
    //Get collateral price
    let collateral_price = get_collateral_price(deps.storage, deps.querier, _env.clone(), market.clone())?;
    
    //Get position's collateral asset value
    let collateral_value = collateral_price.get_value(liquidated_position.collateral_amount)?;

    //We check if the value left is > $1.
    //We use > $1 bc full liquidations will leave rounding errors in the collateral assets so we just use $1 as a floor instead of $0
    if collateral_value > Decimal::one() || liquidated_position.debt_amount.is_zero() {
        Err(ContractError::PositionSolvent {})
    } else {
        //Add the positions's debt amount to the bad debt tracker
        let bad_debt = match config.bad_debt.checked_add(liquidated_position.debt_amount){
            Ok(val) => val,
            Err(_) => return Err(ContractError::CustomError { val: "Bad debt underflow error".to_string() }),
        };
        //Update config state
        CONFIG.save(deps.storage, &Config {
            bad_debt,
            ..config
        })?;
        //Update the position's state
        liquidated_position.debt_amount = Uint128::zero();
        liquidated_position.collateral_amount = Uint128::zero();

        //Save the position's state
        POSITIONS.save(deps.storage, (liq_prop.position_owner.clone(), liq_prop.collateral_denom.clone()), &liquidated_position)?;
        
        //Remove liquidation state
        LIQUIDATION.remove(deps.storage);

        //Create response
        Ok(Response::new()
            .add_attributes(vec![
                attr("method", "check_and_fulfill_bad_debt"),
                attr("position_owner", liq_prop.position_owner),
                attr("bad_debt", config.bad_debt),
                attr("liquidated_position_debt", liquidated_position.debt_amount),
                attr("liquidated_position_collateral", liquidated_position.collateral_amount),
            ]))
    }
}


pub fn crank_realized_apr(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
) -> Result<Response, ContractError> {
    //Load state
    let config = CONFIG.load(deps.storage)?; 
    let total_vault_tokens = DEBT_VAULT_TOKEN.load(deps.storage)?;

    //Update Claim tracker
    let mut claim_tracker = CLAIM_TRACKER.load(deps.storage)?;
    //Calculate time since last claim
    let time_since_last_checkpoint = env.block.time.seconds() - claim_tracker.last_updated;
    //Get the total deposit tokens
    let total_debt_tokens = get_total_debt_tokens(config.clone())?;
    //Calc the rate of vault tokens to deposit tokens
    let btokens_per_one = calculate_base_tokens(
        Uint128::new(1_000_000_000_000), 
        total_debt_tokens, 
        total_vault_tokens
    )?;

    
    //If the current rate is the same as the last rate, update the time since last checkpoint & return 
    if claim_tracker.vt_claim_checkpoints.len() > 0 && claim_tracker.vt_claim_checkpoints.last().unwrap().vt_claim_of_checkpoint == btokens_per_one {
        //Update time since last checkpoint
        claim_tracker.vt_claim_checkpoints.last_mut().unwrap().time_since_last_checkpoint += time_since_last_checkpoint;               
        //Update last updated time
        claim_tracker.last_updated = env.block.time.seconds();
        //Save Claim Tracker
        CLAIM_TRACKER.save(deps.storage, &claim_tracker)?;

        return Ok(Response::new().add_attributes(vec![
            attr("method", "crank_realized_apr"),
            attr("no_change_to_conversion_rate", btokens_per_one),
            attr("added_time_to_checkpoint", time_since_last_checkpoint.to_string())
        ]));
    }

    //If the trackers total time is over a year, remove the first instance
    // if claim_tracker.vt_claim_checkpoints.len() > 0 && claim_tracker.vt_claim_checkpoints.iter().map(|claim_checkpoint| claim_checkpoint.time_since_last_checkpoint).sum::<u64>() > SECONDS_PER_DAY * 365 {
    //     claim_tracker.vt_claim_checkpoints.remove(0);
    // }
    //Push new instance
    claim_tracker.vt_claim_checkpoints.push(VTClaimCheckpoint {
        vt_claim_of_checkpoint: btokens_per_one,
        time_since_last_checkpoint,
    });
    //Update last updated time
    claim_tracker.last_updated = env.block.time.seconds();
    //Save Claim Tracker
    CLAIM_TRACKER.save(deps.storage, &claim_tracker)?;

    Ok(Response::new().add_attributes(vec![
        attr("method", "crank_realized_apr"),
        attr("new_base_token_conversion_rate", btokens_per_one),
        attr("time_since_last_checkpoint", time_since_last_checkpoint.to_string())
    ]))
}


// Sell position collateral to fully repay debts.
// Max spread is used to ensure the full debt is repaid in lieu of slippage.
// pub fn close_position(
//     deps: DepsMut, 
//     env: Env,
//     info: MessageInfo,
//     position_owner: String,
//     close_percentage: Option<Decimal>,
//     max_spread: Decimal,
//     mut send_to: Option<String>,
// ) -> Result<Response, ContractError>{
    //Load Config
//     let config: Config = CONFIG.load(deps.storage)?;


//     //Check if frozen.
//     //This ensures that if Freezes weren't enabled from the jump, the contract can't be frozen.
//     if let Ok(frozen) = ACTIONS_PAUSED.load(deps.storage) { 
//         if frozen {return Err(ContractError::Frozen {  }) }
//     }


//     //Load Basket
//     let basket: Basket = BASKET.load(deps.storage)?;

//     //Set close_percentage
//     let close_percentage = match close_percentage {
//         Some(close_percentage) => min(close_percentage, Decimal::one()),
//         None => Decimal::one(),
//     };

//     //Load target_position, restrict to owner
//     let (_i, target_position) = get_target_position(deps.storage, info.clone().sender, position_id)?;

//     //Set close_amount
//     let close_amount = target_position.credit_amount * close_percentage;

//     //Calc collateral to sell
//     //credit_amount * credit_price * (1 + max_spread)
//     let total_collateral_value_to_sell = {
//             decimal_multiplication(
//                 basket.clone().credit_price.get_value(close_amount)?, 
//                 (max_spread + Decimal::one())
//             )?
//     };

//     //Max_spread is added to the collateral amount to ensure enough credit is purchased
//     //Excess debt token gets sent back to the position_owner during repayment

//     //Get cAsset_ratios for the target_position
//     let (cAsset_ratios, cAsset_prices) = get_cAsset_ratios(deps.storage, env.clone(), deps.querier, target_position.clone().collateral_assets, config.clone(), Some(basket.clone()))?;

//     let mut router_messages = vec![];
//     let mut withdrawn_assets = vec![];

//     //Calc collateral_amount_to_sell per asset & create router msg
//     for (i, _collateral_ratio) in cAsset_ratios.clone().into_iter().enumerate(){

//         //Calc collateral_amount_to_sell
//         let mut collateral_amount_to_sell = {

//             let collateral_value_to_sell = decimal_multiplication(total_collateral_value_to_sell, cAsset_ratios[i])?;

//             let post_normalized_amount: Uint128 = match cAsset_prices[i].get_amount(collateral_value_to_sell){
//                 Ok(amount) => amount,
//                 Err(_e) => return Err(ContractError::CustomError { val: String::from("Collateral value to sell is too high to calculate an amount for due to the max spread creating an out of bounds error") })
//             };

//             post_normalized_amount
//         };

//         //Collateral to sell can't be more than the position owns
//         if collateral_amount_to_sell > target_position.collateral_assets.clone()[i].asset.amount {
//             collateral_amount_to_sell = target_position.collateral_assets.clone()[i].asset.amount;
//         }

//         //Set collateral asset
//         let collateral_asset = target_position.clone().collateral_assets[i].clone().asset;

//         //Add collateral_amount to list for propagation
//         withdrawn_assets.push(Asset{
//             amount: collateral_amount_to_sell,
//             ..collateral_asset.clone()
//         });

//         //Create router subMsg to sell, repay in reply on success
//         let router_msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
//             contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
//             msg: to_json_binary(&OsmoExecuteMsg::ExecuteSwaps { 
//                 token_out: basket.clone().credit_asset.info.to_string(),
//                 max_slippage: max_spread,
//             })?,
//             funds: vec![
//                 Coin {
//                     denom: collateral_asset.clone().info.to_string(),
//                     amount: collateral_amount_to_sell,
//                 }
//             ],
//         });
//         router_messages.push(router_msg);
//     }

//     //Set send_to for WithdrawMsg in Reply
//     if send_to.is_none() {
//         send_to = Some(info.sender.to_string());
//     }

//     //Save CLOSE_POSITION_PROPAGATION
//     CLOSE_POSITION.save(deps.storage, &ClosePositionPropagation {
//         withdrawn_assets,
//         position_info: UserInfo { 
//             position_id, 
//             position_owner: info.sender.to_string(),
//         },
//         send_to,
//     })?;

//     //The last router message is updated to a CLOSE_POSITION_REPLY to close the position after all sales and repayments are done.
//     let sub_msg = SubMsg::reply_on_success(router_messages.pop().unwrap(), CLOSE_POSITION_REPLY_ID);    
//     //Transform Router Msgs into SubMsgs so they run after LP Withdrawals
//     let router_messages = router_messages.into_iter().map(|msg| SubMsg::new(msg)).collect::<Vec<SubMsg>>();

//     Ok(Response::new()
//         .add_submessages(router_messages)
//         .add_submessage(sub_msg)
//         .add_attributes(vec![
//         attr("position_id", position_id),
//         attr("user", info.sender),
//     ])) //If the sale incurred slippage and couldn't repay through the debt minimum, the subsequent withdraw msg will error and revert state 
// }
 