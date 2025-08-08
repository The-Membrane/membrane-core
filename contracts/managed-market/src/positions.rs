use std::cmp::min;
use std::ops::Sub;
use std::str::FromStr;
use std::vec;

use cosmwasm_std::{
    attr, to_json_binary, Addr, Api, BankMsg, Coin, CosmosMsg, Decimal, DepsMut, Env, MessageInfo,
    QuerierWrapper, QueryRequest, Response, StdError, StdResult, Storage, SubMsg, Uint128, WasmMsg,
    WasmQuery,
};


use membrane::mars_vault_token::{ExecuteMsg as Vault_ExecuteMsg, QueryMsg as Vault_QueryMsg};
use membrane::helpers::{validate_position_owner, asset_to_coin, withdrawal_msg, get_contract_balances};
use membrane::math::{decimal_division, decimal_multiplication, Uint256, decimal_subtraction};
use membrane::oracle::PriceResponse;
use membrane::mm_swap::{ExecuteMsg as Swap_ExecuteMsg};
use membrane::tokenfactory::{ExecuteMsg as TokenFactory};
// use membrane::osmosis_proxy::ExecuteMsg as OP_ExecuteMsg;
use membrane::types::{
    Asset, AssetInfo, AutoCloseParams, BorrowOptions, LoopLTVParams, UXBoosts, UserPosition, VTClaimCheckpoint
};
use membrane::managed_market::{Config, DebtInfo, ExecuteMsg, MarketParams};
use membrane::stability_pool_vault::{
    calculate_base_tokens, calculate_vault_tokens
};
use membrane::market_manager::{Config as MarketManagerConfig, QueryMsg as MarketManagerQueryMsg};
use osmosis_std::types::osmosis::twap::v1beta1 as TWAP;
use osmosis_std::types::osmosis::poolmanager::v1beta1::{MsgSwapExactAmountIn, SwapAmountInRoute};
use osmosis_std::types::osmosis::poolmanager::v1beta1::{self as PoolManager, SwapAmountOutRoute};
use serde::de;

use crate::oracle::{get_asset_prices};
use crate::rates::accrue;
use crate::state::{ClosePositionPropagation, CollateralRateAssurance, LiquidationPropagation, LoopPropagation, TokenRateAssurance, ACTIONS_PAUSED, CLAIM_TRACKER, CLOSE_POSITION, COLLATERAL_RATE_ASSURANCE, COLLATERAL_STATE_TOTAL, DEBT_VAULT_TOKEN, JUNIOR_CLAIM_TRACKER, JUNIOR_DEBT_VAULT_TOKEN, LIQUIDATION, LOOP_POSITION, MARKET_PARAMS, POSITION_UX_BOOSTS, TOKEN_RATE_ASSURANCE};
// use crate::state::{get_target_position, update_position, update_position_claims, ClosePositionPropagation, CollateralVolatility, Timer, BASKET, CLOSE_POSITION, FREEZE_TIMER, REDEMPTION_OPT_IN, STORED_PRICES, VOLATILITY};
use crate::{
    state::{
        CONFIG, POSITIONS
    },
    ContractError,
};

use membrane::tokenfactory::{mint_msg, burn_msg};

//Liquidation reply ids
pub const LIQUIDATE_REPLY_ID: u64 = 1u64;
pub const CLOSE_POSITION_REPLY_ID: u64 = 2u64;
pub const LOOP_POSITION_REPLY_ID: u64 = 3u64;
pub const LTV_CHECK_REPLY_ID: u64 = 4u64;
pub const BAD_DEBT_REPLY_ID: u64 = 999999u64;


//Todo:
// - Close, Loop, SL, TP (DONE)
// - Move UX state to separate object (DONE)
// - Check TP logic to see if it stays after usage if position isn't removed (DONE)'
// - Save purchase prices and amounts (DONE)
// - Remove position data once closed (DONE)
// - Calc avg price & total bought post final loop (DONE)
// - Profit & Volume state (DONE)
// - Calculate profits on close if there were loops (DONE)

//Our Product roadmap is:
// 1) Exotic collateral 
// -- borrow fee
// -- per user debt cap
// -- keep max LTV and borrow LTV close so that liquidations are small but frequent nd don't cause the market to crash
// 2) Trading Strategies
// - Take initial out
// --- Strat struct implementation
// 3) Leveraged blue chips (Pyth oracles)
// -- Hands free leverage (SL & Loop intents) (*for borrower*)
// -- Use liquidatibility & volatiility to increase interest rates *for supplier*
// -- Fixed rate (*for borrower*), grants stability for strats
// 4) Fixed rate, low leverage market for users who want low leverage over a long period of time
// 5) Interest rate arbs (redemptions, vault oracles)

//TODO:
// - Add optional oracle contract 
// - Add optional swap contract. This will need checks that the contract returns the asset at appropriate slippage limits.
// - LTV ramping

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
// - per user debt cap (% of total supply)
/// 

///V3
/// - LTV Ramping
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
pub const NOBLE_USDC_DENOM: &str = "ibc/498A0751C798A0D9A389AA3691123DADA57DAA4FE165D5C75894505B876BA6E4";
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
    env: Env,
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

    //Set collateral amount 
    let new_collateral_amount = info.funds[0].amount;
    //Init attrs
    let mut attrs = vec![
        attr("method", "supply_collateral"),
        attr("collateral_amount", new_collateral_amount.to_string()),
        attr("collateral_denom", market.clone().collateral_params.collateral_asset),
        attr("owner", owner.to_string()),
    ];

        
    //Get collateral state total
    let mut collateral_state_total = COLLATERAL_STATE_TOTAL.load(deps.storage, market.collateral_params.collateral_asset.clone()).unwrap_or(Uint128::zero());

    //Get total collateral 
    let total_collateral = get_contract_balances(
        deps.querier, 
        env.clone(), 
        vec![AssetInfo::NativeToken { denom: market.collateral_params.collateral_asset.clone() }]
    )?[0];

    let pre_deposit_collateral = total_collateral - new_collateral_amount;
    if !pre_deposit_collateral.is_zero() {

        //Calc the rate of vault tokens to deposit tokens
        let btokens_per_one = calculate_base_tokens(
            Uint128::new(1_000_000), 
            pre_deposit_collateral, 
            collateral_state_total
        )?;


        //Create collateral rate assurance
        COLLATERAL_RATE_ASSURANCE.save(deps.storage, &CollateralRateAssurance {
            collateral_denom: market.collateral_params.collateral_asset.clone(),
            pre_collateral_per_one: btokens_per_one,
        })?;
    }

    //Centralised collateral accounting
    let user_position = adjust_position_collateral(
        deps.storage,
        owner.clone(),
        market.collateral_params.collateral_asset.clone(),
        new_collateral_amount,
        true,
        &mut collateral_state_total,
    )?;
    attrs.push(attr("user_state", format!("{:?}", user_position)));

    //collateral_state_total already updated by helper

    let mut msgs = vec![];
    //Create collateral rate assurance msg
    if !collateral_state_total.is_zero() && !pre_deposit_collateral.is_zero(){
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {    
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::CollateralRateAssurance {  })?,
            funds: vec![],
        }));
    }


    Ok(Response::new()
    .add_messages(msgs)
    .add_attributes(attrs))
}

fn get_mint_denom(
    config: Config, 
    is_junior: bool
) -> String {
    if is_junior {
        config.junior_debt_supply_vault_token.clone().unwrap()
    } else {
        config.debt_supply_vault_token.clone()
    }
}

fn update_config_debt_totals(
    config: &mut Config,
    is_junior: bool,
    amount: Uint128,
    add: bool,
) -> Result<(), ContractError> {
    if add {
        if is_junior {
            config.junior_debt_info.as_mut().unwrap().total_debt = match config.junior_debt_info.as_mut().unwrap().total_debt.checked_add(amount){
                Ok(v) => v,
                Err(_) => return Err(ContractError::CustomError { val: format!("Junior debt total + Amount: underflow error, {:?} + {}",  config.junior_debt_info, amount) }),
            };
        } else {
            config.total_debt_tokens = match config.total_debt_tokens.checked_add(amount){
                Ok(v) => v,
                Err(_) => return Err(ContractError::CustomError { val: format!("Total debt tokens + Amount: underflow error, {} + {}",  config.total_debt_tokens, amount) }),
            };
        }
    } else {        
        if is_junior {
            config.junior_debt_info.as_mut().unwrap().total_debt = match config.junior_debt_info.as_mut().unwrap().total_debt.checked_sub(amount){
                Ok(v) => v,
                Err(_) => return Err(ContractError::CustomError { val: format!("Junior debt total - Amount: underflow error, {:?} - {}",  config.junior_debt_info, amount) }),
            };
        } else {
            config.total_debt_tokens = match config.total_debt_tokens.checked_sub(amount){
                Ok(v) => v,
                Err(_) => return Err(ContractError::CustomError { val: format!("Total debt tokens - Amount: underflow error, {} - {}",  config.total_debt_tokens, amount) }),
            };
        }
    }
    Ok(())
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
    is_junior: bool,
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
    if info.funds.len() != 1 || info.funds[0].denom != config.debt_token.clone().unwrap() {
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

    ///Junior Tranche needs to be handled differently

    // //Get total_debt_tokens
    // let total_debt_tokens = get_total_debt_tokens(config.clone(), Some(is_junior))?;
    // //Get total_vault_tokens
    // let total_vault_tokens = get_total_vault_tokens(deps.storage, is_junior)?;

    //Ensure the deposit doesn't push the market over supply caps
    if let Some(debt_supply_cap) = config.debt_supply_cap {
        let total_debt = get_total_debt_tokens(config.clone(), None)? + supplied_amount;
        if total_debt > debt_supply_cap {
            return Err(ContractError::SupplyCapExceeded { balance: total_debt, cap: debt_supply_cap });
        }
    }

    //Get markets manager fee
    let markets_manager_fee = query_markets_manager_fee(deps.querier, config.markets_manager_contract.to_string())?;

    //Accrue to make sure current suppliers get their yield
    // This is done to ensure that config's total_debt_tokens is up to date
    accrue(
        deps.storage,
        env.clone(), 
        &mut config, 
        &mut UserPosition { 
            collateral_denom: String::from(""),
            collateral_amount: Uint128::zero(), 
            debt_amount: Uint128::zero(), 
            rate_index: Decimal::zero()
        },
        &mut msgs,
        markets_manager_fee
    )?;
    

    //Get total_debt_tokens
    let total_debt_tokens = get_total_debt_tokens(config.clone(), Some(is_junior))?;
    //Get total_vault_tokens
    let total_vault_tokens = get_total_vault_tokens(deps.storage, is_junior)?;

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

    //Get the mint denom 
    let mint_denom = get_mint_denom(config.clone(), is_junior);

    //Mint vault tokens to user
    if !vault_tokens_to_send.is_zero() {
        let mint_vault_tokens_msg = mint_msg(
            config.token_factory_contract.clone(),
            env.contract.address.as_str(),
            &mint_denom,
            vault_tokens_to_send,
            &send_to.to_string(),
        );
        msgs.push(mint_vault_tokens_msg);
    }

    //Update config state
    // println!("supplied_amount: {}", supplied_amount);
    // println!("is_junior: {}", is_junior);
    update_config_debt_totals(&mut config, is_junior, supplied_amount, true)?;
    CONFIG.save(deps.storage, &config)?;

    //Update vault token supply
    let new_vault_token_supply = match total_vault_tokens.checked_add(vault_tokens_to_send){
        Ok(v) => v,
        Err(_) => return Err(ContractError::CustomError { val: format!("Failed to add vault token total supply: {} + {}", total_vault_tokens, vault_tokens_to_send) }),
    };
    //Update vault token supply
    if is_junior {
        // println!("new_vault_token_supply: {}", new_vault_token_supply);
        JUNIOR_DEBT_VAULT_TOKEN.save(deps.storage, &new_vault_token_supply)?;
    } else {
        DEBT_VAULT_TOKEN.save(deps.storage, &new_vault_token_supply)?;
    }


    //Add rate assurance callback msg
    if !total_vault_tokens.is_zero() {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance { is_junior })?,
            funds: vec![],
        }));
    }


    Ok(Response::new()
    .add_attributes(vec![
        attr("method", "supply_debt"),
        attr("is_junior", is_junior.to_string()),
        attr("debt_amount", supplied_amount.to_string()),
        attr("vault_tokens_minted", vault_tokens_to_send.to_string()),
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

    //Get markets manager fee
    let markets_manager_fee = query_markets_manager_fee(deps.querier, config.markets_manager_contract.to_string())?;

    //Check & assert vault token
    //Assert the sender sent the vault token only
    if info.funds.len() != 1 || (info.funds[0].denom != config.debt_supply_vault_token && info.funds[0].denom != config.junior_debt_supply_vault_token.clone().unwrap()) {
        return Err(ContractError::CustomError { val: format!("Need to send one of the vault tokens only: {}, {}", config.debt_supply_vault_token, config.junior_debt_supply_vault_token.clone().unwrap()) });
    }

    //Check to see if the debt is junior or senior
    let is_junior = info.funds[0].denom == config.junior_debt_supply_vault_token.clone().unwrap();
    //Label vault tokens sent
    let vault_tokens_sent = info.funds[0].amount;
    if vault_tokens_sent.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    //Accrue to make sure withdrawing suppliers get their yield.
    //This is done to ensure that config's total_debt_tokens is up to date.

    accrue(
        deps.storage,
        env.clone(), 
        &mut config, 
        &mut UserPosition { 
            collateral_denom: String::from(""),
            collateral_amount: Uint128::zero(), 
            debt_amount: Uint128::zero(), 
            rate_index: Decimal::zero(),
        },
        &mut msgs,
        markets_manager_fee
    )?;
    
    //Get total_debt_tokens
    let total_debt_tokens = get_total_debt_tokens(config.clone(), Some(is_junior))?;
    //Get total_vault_tokens
    let total_vault_tokens = get_total_vault_tokens(deps.storage, is_junior)?;


    //Calc base token to user
    let base_tokens_to_send = calculate_base_tokens(
        vault_tokens_sent,
        total_debt_tokens,
        total_vault_tokens.clone()
    )?;

    //Get balance of debt tokens we have to send.
    let debt_token_balance = deps.querier.query_balance(env.clone().contract.address, config.debt_token.clone().unwrap())?;

    //If we have less debt tokens than being requested, we error.
    //Or if we have less of this tranche of debt tokens, we error.
    // This ensures that even if we have enough debt tokens, if its not part of the tranche requested, we error.
    // Ex: If we have 1000 debt tokens, but only 500 of the tranche & 501 are requested, we error.
    if debt_token_balance.amount < base_tokens_to_send || total_debt_tokens < base_tokens_to_send {
        return Err(ContractError::CustomError { val: format!("Not enough debt tokens to send, maximum: {}, requested: {}", min(debt_token_balance.amount, total_debt_tokens), base_tokens_to_send) });
    }

    //Get the mint denom
    let mint_denom = get_mint_denom(config.clone(), is_junior);

    //Burn vault tokens.
    //Send base tokens to user.
    if !base_tokens_to_send.is_zero() {
        //Burn vault tokens.
        let burn_vault_tokens_msg = burn_msg(
            config.token_factory_contract.clone(),
            env.contract.address.as_str(),
            &mint_denom,
            vault_tokens_sent,
            env.contract.address.as_str(),
        );
        msgs.push(burn_vault_tokens_msg);

        //Send base tokens to user.
        let send_base_tokens_msg: CosmosMsg = BankMsg::Send {
            to_address: send_to.to_string(),
            amount: vec![Coin {
                denom: config.debt_token.clone().unwrap(),
                amount: base_tokens_to_send,
            }],
        }.into();
        msgs.push(send_base_tokens_msg);
    }

    //Update config state
    update_config_debt_totals(&mut config, is_junior, base_tokens_to_send, false)?;
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
    if is_junior {
        JUNIOR_DEBT_VAULT_TOKEN.save(deps.storage, &new_vault_token_supply)?;
    } else {
        DEBT_VAULT_TOKEN.save(deps.storage, &new_vault_token_supply)?;
    }
    
    //Add rate assurance callback msg if this withdrawal leaves other depositors with tokens to withdraw.
    if !new_vault_token_supply.is_zero() {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance { is_junior })?,
            funds: vec![],
        }));
    }

    Ok(Response::new()
    .add_attributes(vec![
        attr("method", "withdraw_debt"),
        attr("is_junior", is_junior.to_string()),
        attr("vault_tokens_burnt", vault_tokens_sent),
        attr("base_tokens_withdrawn", base_tokens_to_send),
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

    let mut msgs = vec![];
    
    //Check if frozen.
    //This ensures that if Freezes weren't enabled from the jump, the contract can't be frozen.
    if let Ok(frozen) = ACTIONS_PAUSED.load(deps.storage) { 
        if frozen {return Err(ContractError::Frozen {  }) }
    }

    //Set position_owner
    let mut position_owner = info.sender.clone();

    //If the contract is withdrawing for a user (i.e. ClosePosition), set the position owner to the recipient
    if info.sender == env.contract.address && send_to.is_some(){
        position_owner = deps.api.addr_validate(&send_to.clone().unwrap())?.clone();
    } else if info.sender == env.contract.address && send_to.is_none(){
        return Err(ContractError::CustomError { val: "Can't withdraw for the contract".to_string() });
    }

    //Load user state
    let mut user_position = POSITIONS.load(deps.storage, (position_owner.clone(), collateral_denom.clone()))?;

    //Return early if no collateral
    if user_position.collateral_amount.is_zero() {
        return Err(ContractError::CustomError { val: "No collateral to withdraw".to_string() });
    }

    //Set send to
    let send_to = match send_to.clone() {
        Some(send_to) => deps.api.addr_validate(&send_to)?,
        None => info.sender.clone(),
    };

    //Get markets manager fee
    let markets_manager_fee = query_markets_manager_fee(deps.querier, config.markets_manager_contract.to_string())?;

    //Accrue if debt is owed
    accrue(
        deps.storage,
        env.clone(), 
        &mut config, 
        &mut user_position,
        &mut msgs,
        markets_manager_fee
    )?;
    //Load market post-accrue
    let market = match MARKET_PARAMS.load(deps.storage, collateral_denom.clone()){
        Ok(market) => market,
        Err(_) => return Err(ContractError::CustomError { val: format!("Collateral asset ({:?}) not supported", collateral_denom) }),
    };

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
            //Get prices
            let prices = get_asset_prices(
                deps.querier, 
                config.clone(), 
                env.contract.address.to_string(),
                true, 
                 vec![market.collateral_params.collateral_asset.clone(), config.debt_token.clone().unwrap()]
                )?;
            let collateral_price = prices[0].clone();
            let debt_price = prices[1].clone();
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

    //Update state for user is handled via helper

    //Load collateral state total prior to adjustment
    let mut collateral_state_total = COLLATERAL_STATE_TOTAL.load(deps.storage, collateral_denom.clone()).unwrap_or(Uint128::zero());
    //Get total collateral 
    let total_collateral = get_contract_balances(
        deps.querier, 
        env.clone(), 
        vec![AssetInfo::NativeToken { denom: market.collateral_params.collateral_asset.clone() }]
    )?[0];

    if !total_collateral.is_zero() && !collateral_state_total.is_zero(){
        //Calc the rate of vault tokens to deposit tokens
        let btokens_per_one = calculate_base_tokens(
            Uint128::new(1_000_000), 
            total_collateral, 
            collateral_state_total
        )?;


        //Create collateral rate assurance
        COLLATERAL_RATE_ASSURANCE.save(deps.storage, &CollateralRateAssurance {
            collateral_denom: market.collateral_params.collateral_asset.clone(),
            pre_collateral_per_one: btokens_per_one,
        })?;
    }
    //Centralised collateral accounting
    let user_position = adjust_position_collateral(
        deps.storage,
        position_owner.clone(),
        collateral_denom.clone(),
        withdrawable_amount,
        false,
        &mut collateral_state_total,
    )?;

    //Update state for config
    CONFIG.save(deps.storage, &config)?;

    //Send withdrawn assets
    let withdraw_coins = vec![Coin {
        denom: market.collateral_params.collateral_asset.clone(),
        amount: withdrawable_amount,
    }];
    let withdraw_collateral_message = CosmosMsg::Bank(BankMsg::Send {
        to_address: send_to.to_string(),
        amount: withdraw_coins,
    });
    msgs.push(withdraw_collateral_message.clone());

    //Create collateral rate assurance msg (conditional uses the new collateral state total)
    if !collateral_state_total.is_zero() {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {    
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::CollateralRateAssurance {  })?,
            funds: vec![],
        }));
    }

    Ok(Response::new()
    .add_messages(msgs)
    .add_attributes(vec![
        attr("method", "withdraw_collateral"),
        attr("withdrawn_amount", withdrawable_amount),
        attr("collateral_denom", market.collateral_params.collateral_asset),
        attr("position_owner", position_owner.to_string()),
        attr("send_to", send_to.to_string()),
        attr("new_position", format!("{:?}", user_position)),
    ])) 
}

pub fn edit_ux_boosts(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    collateral_denom: String,
    loop_ltv: Option<Option<LoopLTVParams>>,
    take_profit_params: Option<Option<AutoCloseParams>>,
    stop_loss_params: Option<Option<AutoCloseParams>>,
    arb_price: Option<Option<Decimal>>,
    collateral_value_fee_to_executor: Option<Decimal>, 
) -> Result<Response, ContractError> {
    //Load user state
    let mut user_position_ux_boosts = match POSITION_UX_BOOSTS.load(deps.storage, (info.sender.clone(), collateral_denom.to_string())) {
        Ok(val) => val,
        Err(_) => {
            // If not found, check if user has a position
            match POSITIONS.load(deps.storage, (info.sender.clone(), collateral_denom.to_string())) {
                Ok(_user_position) => {
                    // Set to default UXBoosts
                    UXBoosts {
                        collateral_value_fee_to_executor: Decimal::zero(),
                        loop_ltv: None,
                        take_profit_params: None,
                        stop_loss_params: None,
                        arb_price: None,
                        collateral_bought_from_loops: vec![],
                    }
                },
                Err(e) => return Err(ContractError::CustomError { val: format!("Failed to load user UX boosts and no position found: {}", e) }),
            }
        }
    };

    //Load market
    let market = match MARKET_PARAMS.load(deps.storage, collateral_denom.to_string()){
        Ok(market) => market,
        Err(_) => return Err(ContractError::CustomError { val: format!("Collateral asset ({:?}) not supported", collateral_denom) }),
    };

    //Set loop ltv
    if let Some(loop_ltv) = loop_ltv.clone() {
        //Can't be above max borrow ltv
        if let Some(loop_ltv) = loop_ltv.clone() {
            if loop_ltv.loop_ltv > market.collateral_params.max_borrow_LTV {
                return Err(ContractError::CustomError { val: format!("Loop ltv {} can't be higher than max borrow ltv {}", loop_ltv.loop_ltv, market.collateral_params.max_borrow_LTV) });
            }
        }
        user_position_ux_boosts.loop_ltv = loop_ltv;
    }
    //Set take profit ltv
    if let Some(take_profit_params) = take_profit_params.clone() {
        user_position_ux_boosts.take_profit_params = take_profit_params.clone();
        //If the close percentage leaves the position underneath the debt minimum, we error
        if let Some(take_profit_params) = take_profit_params.clone() {
            /////Calc debt left after close////
            //Get the inverse of the close percentage
            let inverse_close_percentage = match decimal_subtraction(Decimal::one(), take_profit_params.percent_to_close){
                Ok(val) => val,
                Err(_) => return Err(ContractError::CustomError { val: format!("Failed to calculate inverse close percentage: {:?} - {:?}", Decimal::one(), take_profit_params.percent_to_close) }),
            };
            //Load the user position
            let user_position = match POSITIONS.load(deps.storage, (info.sender.clone(), collateral_denom.to_string())){
                Ok(user_position) => user_position,
                Err(_) => return Err(ContractError::CustomError { val: format!("User position not found") }),
            };
            //Get remaining debt
            let remaining_debt = match decimal_multiplication(Decimal::from_ratio(user_position.debt_amount, Uint128::one()), inverse_close_percentage){
                Ok(val) => val,
                Err(_) => return Err(ContractError::CustomError { val: format!("Failed to calculate remaining debt: {:?} * {:?}", user_position.debt_amount, inverse_close_percentage) }),
            };

            //Assert that the remaining debt is above the minimum
            if remaining_debt.to_uint_floor() < market.debt_minimum && !inverse_close_percentage.is_zero() {
                return Err(ContractError::CustomError { val: format!("Take profit can't leave the position with less than the debt minimum") });
            }
        }
    }
    //Set stop loss ltv
    if let Some(stop_loss_params) = stop_loss_params.clone() {
        if let Some(stop_loss_params) = stop_loss_params.clone() {
            if stop_loss_params.ltv > Decimal::one() {
                return Err(ContractError::CustomError { val: "Stop loss can't be higher than 1".to_string() });
            }

            //Can't set stop loss higher than loop ltv
            if let Some(loop_ltv) = user_position_ux_boosts.loop_ltv.clone() {
                if stop_loss_params.ltv > loop_ltv.loop_ltv {
                    return Err(ContractError::CustomError { val: "Stop loss can't be higher than loop ltv".to_string() });
                }
            }
            //Can't set stop loss higher than take profit ltv
            if let Some(take_profit_params) = user_position_ux_boosts.take_profit_params.clone() {
                if stop_loss_params.ltv > take_profit_params.ltv {
                    return Err(ContractError::CustomError { val: "Stop loss can't be higher than take profit ltv".to_string() });
                }
            }
            //Can't be above liquidation ltv
            if stop_loss_params.ltv > market.collateral_params.liquidation_LTV {
                return Err(ContractError::CustomError { val: "Stop loss can't be higher than liquidation ltv".to_string() });
            }


            //If the close percentage leaves the position underneath the debt minimum, we error
            /////Calc debt left after close////
            //Get the inverse of the close percentage
            let inverse_close_percentage = match decimal_subtraction(Decimal::one(), stop_loss_params.percent_to_close){
                Ok(val) => val,
                Err(_) => return Err(ContractError::CustomError { val: format!("Failed to calculate inverse close percentage: {:?} - {:?}", Decimal::one(), stop_loss_params.percent_to_close) }),
            };
            //Load the user position
            let user_position = match POSITIONS.load(deps.storage, (info.sender.clone(), collateral_denom.to_string())){
                Ok(user_position) => user_position,
                Err(_) => return Err(ContractError::CustomError { val: format!("User position not found") }),
            };
            //Get remaining debt
            let remaining_debt = match decimal_multiplication(Decimal::from_ratio(user_position.debt_amount, Uint128::one()), inverse_close_percentage){
                Ok(val) => val,
                Err(_) => return Err(ContractError::CustomError { val: format!("Failed to calculate remaining debt: {:?} * {:?}", user_position.debt_amount, inverse_close_percentage) }),
            };

            //Assert that the remaining debt is above the minimum
            if remaining_debt.to_uint_floor() < market.debt_minimum  {
                return Err(ContractError::CustomError { val: format!("Take profit can't leave the position with less than the debt minimum") });
            }

        }
        //Set stop loss params
        user_position_ux_boosts.stop_loss_params = stop_loss_params;
    }
    //Set collateral value fee to executor
    if let Some(collateral_value_fee_to_executor) = collateral_value_fee_to_executor {
        user_position_ux_boosts.collateral_value_fee_to_executor = collateral_value_fee_to_executor;
    }
    //Set arb price
    if let Some(arb_price) = arb_price {
        user_position_ux_boosts.arb_price = arb_price;
    }

    //Save user state
    POSITION_UX_BOOSTS.save(deps.storage, (info.sender.clone(), collateral_denom.to_string()), &user_position_ux_boosts)?;

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "edit_ux_boosts"),
            attr("loop_ltv", format!("{:?}", loop_ltv)),
            attr("take_profit_params", format!("{:?}", take_profit_params)),
            attr("stop_loss_params", format!("{:?}", stop_loss_params)),
            attr("collate   ral_value_fee_to_executor", format!("{:?}", collateral_value_fee_to_executor)),
            attr("arb_price", format!("{:?}", arb_price)),
            attr("collateral_bought_from_loops", format!("{:?}", user_position_ux_boosts.collateral_bought_from_loops )),

        ]))
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
    let mut msgs = vec![];

    //Check if frozen
    //This ensures that if Freezes weren't enabled from the jump, the contract can't be frozen.
    if let Ok(frozen) = ACTIONS_PAUSED.load(deps.storage) { 
        if frozen {return Err(ContractError::Frozen {  }) }
    }

    //Set position_owner
    let mut position_owner = info.sender.clone();

    //If the contract is borrowing for a user (i.e. LoopPosition), set the position owner to the recipient
    if info.sender == env.contract.address && send_to.is_some(){
        position_owner = deps.api.addr_validate(&send_to.clone().unwrap())?.clone();
    } else if info.sender == env.contract.address && send_to.is_none(){
        return Err(ContractError::CustomError { val: "Can't borrow for the contract".to_string() });
    }

    //Load user state
    let mut user_position = POSITIONS.load(deps.storage, (position_owner.clone(), collateral_denom.clone()))?;

    //Set send to
    let mut send_to = match send_to {
        Some(send_to) => deps.api.addr_validate(&send_to)?,
        None => info.sender.clone(),
    };
    //If its the contract, set it to the contract address
    if info.sender == env.contract.address {
        send_to = env.contract.address.clone();
    }

    //Get markets manager fee
    let markets_manager_fee = query_markets_manager_fee(deps.querier, config.markets_manager_contract.to_string())?;

    //Accrue.
    //Even without debt, this keeps everyones's state up to date
    accrue(
        deps.storage,
        env.clone(), 
        &mut config,
        &mut user_position,
        &mut msgs,
        markets_manager_fee
    )?;

    //Load market post-accrue
    let mut market = match MARKET_PARAMS.load(deps.storage, collateral_denom.clone()){
        Ok(market) => market,
        Err(_) => return Err(ContractError::CustomError { val: format!("Collateral asset ({:?}) not supported", collateral_denom) }),
    };

    
    //Get prices
    let prices = get_asset_prices(
        deps.querier,
        config.clone(), 
        env.contract.address.to_string(),
        true, 
        vec![market.collateral_params.collateral_asset.clone(), config.debt_token.clone().unwrap()]
    )?;
    let collateral_price = prices[0].clone();
    let debt_price = prices[1].clone();

    //Assert borrow is valid. 
    //Borrowable amount is capped by current LTV & borrowable LTV & borrow cap.
    let (borrowable_amount, borrow_fee) = calc_borrowable_amount(
        deps.querier,
        env.clone(),
        borrow_options.clone(),
        market.clone(),
        collateral_price.clone(),
        debt_price.clone(),
        user_position.clone(),
        config.debt_token.clone().unwrap()
    )?;

    //Query the estimated swap amount for the new total borrowed amount to make sure it can be liquidated.
    let new_total_borrowed = match market.total_borrowed.checked_add(borrowable_amount){
        Ok(val) => val,
        Err(_) => return Err(ContractError::CustomError { val: format!("Total Borrowed: {} + Borrow Amount: {}, underflow error", market.total_borrowed, borrowable_amount) }),
    };
    //Update config's total borrowed
    config.total_borrowed = match config.total_borrowed.clone().unwrap().checked_add(borrowable_amount){
        Ok(val) => Some(val),
        Err(_) => return Err(ContractError::CustomError { val: format!("Total Borrowed: {:?} + Borrow Amount: {}, underflow error", config.total_borrowed, borrowable_amount) }),
    };
    //Should this be a market toggle? Yes.
    if market.borrow_cap.cap_borrows_by_liquidity {
        // check_debt_liquidatibility(deps.querier, market.clone(), 
        //     new_total_borrowed, 
        //     get_contract_balances(
        //         deps.querier,
        //         env.clone(),
        //         vec![AssetInfo::NativeToken { denom: market.clone().collateral_params.collateral_asset }],
        //     )?[0],
        // )?;
    }

    //////Update state for user/////
    user_position.debt_amount = match user_position.debt_amount.checked_add(borrowable_amount){
        Ok(val) => val,
        Err(_) => return Err(ContractError::CustomError { val: format!("User Debt Amount: {} + Borrow Amount: {}, underflow error", user_position.debt_amount, borrowable_amount) }),
    };
    //If the debt isn't > debt_minimum, error
    if user_position.debt_amount < market.debt_minimum {
        return Err(ContractError::CustomError { val: format!("User Debt Amount: {} < Debt Minimum: {}", user_position.debt_amount, market.debt_minimum) });
    }
    //Save user state
    POSITIONS.save(deps.storage, (position_owner.clone(), collateral_denom.clone()), &user_position)?;

    //Update state for config
    market.total_borrowed = new_total_borrowed;
    MARKET_PARAMS.save(deps.storage, collateral_denom.clone(), &market)?;
    

    if !JUNIOR_DEBT_VAULT_TOKEN.load(deps.storage)?.is_zero() {
        //Add borrow fee to the junior tranche
        config.junior_debt_info.as_mut().unwrap().total_debt = match config.junior_debt_info.as_mut().unwrap().total_debt.checked_add(borrow_fee){
            Ok(val) => val,
            Err(_) => return Err(ContractError::CustomError { val: format!("Total Debt Tokens: {} + Borrow Fee: {}, underflow error", config.junior_debt_info.as_mut().unwrap().total_debt, borrow_fee) }),
        };
    } else {
        config.total_debt_tokens = match config.total_debt_tokens.checked_add(borrow_fee){
            Ok(val) => val,
            Err(_) => return Err(ContractError::CustomError { val: format!("Total Debt Tokens: {} + Borrow Fee: {}, underflow error", config.total_debt_tokens, borrow_fee) }),
        };
    }
    //Update config state
    CONFIG.save(deps.storage, &config)?;

    //Send borrowed CDT
    let borrow_coins = vec![Coin {
        denom: config.debt_token.clone().unwrap(),
        amount: borrowable_amount,
    }];
    let borrow_cdt_message = CosmosMsg::Bank(BankMsg::Send {
        to_address: send_to.to_string(),
        amount: borrow_coins,
    });
    msgs.push(borrow_cdt_message.clone());



    Ok(Response::new()
    .add_attributes(vec![
        attr("method", "borrow_cdt"),
        attr("borrowed_amount", borrowable_amount),
        attr("borrow_fee", borrow_fee),
    ])
.add_messages(msgs))
}

pub fn calc_borrowable_amount(
    querier: QuerierWrapper,
    env: Env,
    borrow_options: BorrowOptions,
    market: MarketParams,
    collateral_price: PriceResponse,
    debt_price: PriceResponse,
    user_position: UserPosition,
    debt_token: String
) -> Result<(Uint128, Uint128), ContractError> {
    //Assert borrow is valid. 
    //Borrowable amount is capped by current LTV & borrowable LTV & borrow cap.
    let mut borrowable_amount = {
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
        // // println!("borrowable_amount: {:?}", borrowable_amount);
        // // println!("market.borrow_cap.fixed_cap: {:?}", market.borrow_cap.fixed_cap);
        //Does this market have a fixed borrow cap?
        let space_to_borrow = match market.borrow_cap.fixed_cap {
            Some(cap) => {
                //Calc space to borrow within the cap
                let space = cap.checked_sub(market.total_borrowed).unwrap_or(Uint128::zero());
                space
            },
            None => borrowable_amount
        };
        // // println!("space_to_borrow: {:?}", space_to_borrow);

        let capped_borrow = min(theoretical_borrow, space_to_borrow);
        //If the market has a per user borrow cap, check it
        let capped_borrow = match market.per_user_debt_cap {
            Some(cap) => {
                //Calc space to borrow within the cap
                let space = cap.checked_sub(user_position.debt_amount).unwrap_or(Uint128::zero());
                min(capped_borrow, space)
            },
            None => capped_borrow
        };
        //Check contract balances for actual borrow amounts
        let contract_balance_of_debt = get_contract_balances(
            querier,
            env.clone(),
            vec![AssetInfo::NativeToken { denom: debt_token }],
        )?[0];
        // // println!("capped_borrow:1] {:?}", capped_borrow);
        // // println!("contract_balance_of_debt:1 {:?}", contract_balance_of_debt);
        let actual_borrow = min(capped_borrow, contract_balance_of_debt);
        // // println!("borrow_cap: {:?}", market.borrow_cap);
        // // println!("actual_borrow: {:?}", actual_borrow);
        actual_borrow
    };


    //Check borrow amount is non-zero
    if borrowable_amount.is_zero() {
        return Err(ContractError::CustomError { val: "No space to borrow".to_string() });
    }


    //If there is a borrow fee, subtract it from the borrowable amount
    let borrow_fee = match market.borrow_fee.is_zero() {
        false => {
            let fee_amount = decimal_multiplication(
                Decimal::from_ratio(borrowable_amount, Uint128::one()), 
                market.borrow_fee
            )?.to_uint_floor();
            borrowable_amount = borrowable_amount.checked_sub(fee_amount).unwrap_or(Uint128::zero());

            fee_amount
        },
        true => Uint128::zero()
    };

    Ok((borrowable_amount, borrow_fee))
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
    send_excess_to: Option<String>,
) -> Result<Response, ContractError> {    
    let mut config = CONFIG.load(deps.storage)?;

    //Check if frozen
    //This ensures that if Freezes weren't enabled from the jump, the contract can't be frozen.
    if let Ok(frozen) = ACTIONS_PAUSED.load(deps.storage) { 
        if frozen {return Err(ContractError::Frozen {  }) }
    }


    //Check & assert deposit asset
    //Assert the sender sent the deposit asset only
    if info.funds.len() != 1 || info.funds[0].denom != config.debt_token.clone().unwrap() {
        return Err(ContractError::CustomError { val: format!("Need to send the debt asset only: {}", config.debt_token.clone().unwrap()) });
    }

    //Label repayment amount 
    let mut repay_amount = info.funds[0].amount;
    let mut excess_repayment = Uint128::zero();
    let mut msgs = vec![];

    //Set position owner
    let position_owner = match info.sender != env.contract.address {
        true => info.sender.clone(),
        false => deps.api.addr_validate(&send_excess_to.clone().unwrap())?,
    };

    //Load user state
    let mut user_position = POSITIONS.load(deps.storage, (position_owner.clone(), collateral_denom.clone()))?;

    //Get markets manager fee
    let markets_manager_fee = query_markets_manager_fee(deps.querier, config.markets_manager_contract.to_string())?;

    //Accrue.
    //Even without debt, this keeps everyones's state up to date
    accrue(
        deps.storage,
        env.clone(), 
        &mut config, 
        &mut user_position,
        &mut msgs,
        markets_manager_fee
    )?;    

    //Load market post-accrue
    let mut market = match MARKET_PARAMS.load(deps.storage, collateral_denom.clone()){
        Ok(market) => market,
        Err(_) => return Err(ContractError::CustomError { val: format!("Collateral asset ({:?}) not supported", collateral_denom.clone()) }),
    };

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
    //If the debt is under the minimmum, error
    if user_position.debt_amount < market.debt_minimum && user_position.debt_amount > Uint128::zero() {
        return Err(ContractError::CustomError { val: format!("User Debt Amount: {} < Debt Minimum: {}", user_position.debt_amount, market.debt_minimum) });
    }
    POSITIONS.save(deps.storage, (position_owner.clone(), collateral_denom.clone()), &user_position)?;

     //Send back excess repayment, defaults to the repaying address
     if !excess_repayment.is_zero() {
        //Set send_excess_to
        let send_excess_to = match send_excess_to {
            Some(send_excess_to) => deps.api.addr_validate(&send_excess_to)?,
            None => position_owner.clone(),
        };
        //Send excess repayment
        let excess_repayment_coins = vec![Coin {
            denom: config.debt_token.clone().unwrap(),
            amount: excess_repayment,
        }];
        let excess_repayment_message = CosmosMsg::Bank(BankMsg::Send {
            to_address: send_excess_to.to_string(),
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
    //Update config's total borrowed
    config.total_borrowed = match config.total_borrowed.clone().unwrap().checked_sub(repay_amount){
        Ok(val) => Some(val),
        Err(_) => return Err(ContractError::CustomError { val: format!("Total Borrowed: {:?} - Repay Amount: {}, underflow error", config.total_borrowed, repay_amount) }),
    };
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


// pub fn check_debt_liquidatibility(
//     querier: QuerierWrapper,
//     market: MarketParams,
//     debt_amount: Uint128,
//     collateral_amount: Uint128,
// ) -> Result<(), ContractError> {

//     //Get swap routes
//     let routes = get_swap_out_routes_to_cdt(market.clone())?;

//     //Get debt value
//     // let debt_value = debt_price.get_value(debt_amount)?;

//     //Estimate swap
//     let res: PoolManager::EstimateSwapExactAmountOutResponse = PoolManager::PoolmanagerQuerier::new(&querier).estimate_swap_exact_amount_out(
//         0u64,  //id doesn't matter here
//         routes, //routes are the oracle pool plus 1268 the CDT pool
//         debt_amount.to_string(),
//     )?;
//     //This doesn't account for individual position liquidatibility
//     if Uint128::from_str(&res.token_in_amount).unwrap() > collateral_amount {
//         return Err(ContractError::NoLiquidatibility {  });
//     }

//     Ok(())
// }

// fn get_swap_out_routes_to_cdt(
//     market: MarketParams,
// ) -> Result<Vec<SwapAmountOutRoute>, ContractError> {
    
//     let mut routes = vec![];

//     //Get the oracle pool
//     let oracle_pool = market.pool_for_oracle_and_liquidations;

//     //Convert the oracle pool to a swap route
//     oracle_pool.pools_for_osmo_twap.into_iter().for_each(|pool| {
//         routes.push(SwapAmountOutRoute {
//             pool_id: pool.pool_id,
//             token_in_denom: pool.base_asset_denom,
//         });
//     });

//     //Add the CDT pool
//     routes.push(SwapAmountOutRoute {
//         pool_id: 1268u64,
//         token_in_denom: NOBLE_USDC_DENOM.to_string(),
//     });

//     Ok(routes)
// }

// fn get_swap_in_routes_to_cdt(
//     market: MarketParams,
//     debt_token: String
// ) -> Result<Vec<SwapAmountInRoute>, ContractError> {
    
//     let mut routes = vec![];

//     //Get the oracle pool
//     let oracle_pool = market.pool_for_oracle_and_liquidations;

//     //Convert the oracle pool to a swap route
//     oracle_pool.pools_for_osmo_twap.into_iter().for_each(|pool| {
//         routes.push(SwapAmountInRoute {
//             pool_id: pool.pool_id,
//             token_out_denom: pool.quote_asset_denom,
//         });
//     });

//     //Add the CDT pool
//     routes.push(SwapAmountInRoute {
//         pool_id: 1268u64,
//         token_out_denom: debt_token.clone(),
//     });

//     Ok(routes)
// }


fn create_swap_to_debt_token_msg(
    config: Config,
    env: Env,
    collateral_denom: String,
    collateral_amount: Uint128,
    max_slippage: Decimal,
    is_liquidation: bool,
) -> Result<Vec<SubMsg>, ContractError> {
    let mut msgs = vec![];

    //If its a vault token:
    // - Withdraw from the vault
    // - Query how much we expect to receive - 1
    // // - Set that amount as the collateral amount for the swap 
    // if let Some(vault_info) = market.pool_for_oracle_and_liquidations.vault_info {

    //     //Withdraw from the vault
    //     let withdraw_msg = CosmosMsg::Wasm(WasmMsg::Execute {
    //         contract_addr: vault_info.vault_contract.clone(),
    //         msg: to_json_binary(&Vault_ExecuteMsg::ExitVault {})?,
    //         funds: vec![
    //             Coin {
    //                 denom: collateral_denom.clone(),
    //                 amount: collateral_amount,
    //             }
    //         ],
    //     });
    //     msgs.push(SubMsg::new(withdraw_msg));

    //     //Query how much we expect to receive - 1
    //     let underlying_deposit_token: Uint128 = match querier.query_wasm_smart::<Uint128>(
    //         vault_info.vault_contract,
    //         &Vault_QueryMsg::VaultTokenUnderlying { vault_token_amount: collateral_amount },
    //     ){
    //         Ok(underlying_deposit_token) => underlying_deposit_token,
    //         Err(_) => return Err(ContractError::CustomError { val: String::from("Failed to query the Mars Vault Token for the underlying deposit amount in instantiate") }),
    //     };

    //     //Set the collateral amount for the swap
    //     collateral_amount = underlying_deposit_token - Uint128::one();

    //     //Set the collateral denom to the underlying deposit token
    //     if market.pool_for_oracle_and_liquidations.pools_for_osmo_twap.len() == 0 {
    //         collateral_denom = NOBLE_USDC_DENOM.to_string();
    //     } else {
    //         collateral_denom = market.pool_for_oracle_and_liquidations.pools_for_osmo_twap[0].base_asset_denom.clone();
    //     }
    // }
        

    // //Get token_in & token_out prices
    // let token_in_price = collateral_price.clone();
    // let token_out_price = debt_price.clone();

    // //Calculate min amount out
    // let token_in_value = token_in_price.get_value(collateral_amount)?;
    // let token_out_min_value = decimal_multiplication(token_in_value, Decimal::one() - max_slippage)?;
    // let token_out_min_amount = token_out_price.get_amount(token_out_min_value)?;

    // //Create Msg
    // let msg: CosmosMsg = MsgSwapExactAmountIn {
    //     sender: env.contract.address.to_string(),
    //     routes,
    //     token_in: Some(osmosis_std::types::cosmos::base::v1beta1::Coin {
    //         amount: collateral_amount.to_string(),
    //         denom: collateral_denom
    //     }),
    //     token_out_min_amount: token_out_min_amount.to_string(),
        
    // }.into();

    //Create swap msg from swap contract
    let debt_token = config.debt_token.clone().unwrap().to_string();
    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.swap_contract.unwrap().to_string(),
        msg: to_json_binary(&Swap_ExecuteMsg::Swap {
            caller: env.contract.address.to_string(),
            token_in: collateral_denom.clone(),
            token_out: debt_token.clone(),
            max_slippage: max_slippage,
        })?,
        funds: vec![
            Coin {
                denom: collateral_denom.clone(),
                amount: collateral_amount,
            }
        ],
    });
    //Reply on success to edit the user position based on the CDT that was swapped for & edit the config's total borrowed.
    let sub_msg = SubMsg::reply_on_success(msg, if is_liquidation { LIQUIDATE_REPLY_ID } else { CLOSE_POSITION_REPLY_ID });
    msgs.push(sub_msg);

    Ok(msgs)
}

// fn get_swap_in_routes_to_collateral(
//     market: MarketParams,
// ) -> Result<Vec<SwapAmountInRoute>, ContractError> {
    
//     let mut routes = vec![];

//     // Add the CDT pool first (we're starting from CDT now)
//     routes.push(SwapAmountInRoute {
//         pool_id: 1268u64,
//         token_out_denom: NOBLE_USDC_DENOM.to_string(),
//     });

//     // Get the oracle pool
//     let oracle_pool = market.pool_for_oracle_and_liquidations;

//     // Reverse the oracle pool route (simulating CDT -> collateral)
//     oracle_pool.pools_for_osmo_twap.into_iter().rev().enumerate().for_each(|(i, pool)| {
//         routes.push(SwapAmountInRoute {
//             pool_id: pool.pool_id,
//             token_out_denom: pool.base_asset_denom.clone(),
//         });
//     });

//     Ok(routes)
// }

fn create_swap_to_collateral_msg(
    config: Config,
    env: Env,
    collateral_denom: String,
    debt_amount: Uint128,
    max_slippage: Decimal,
) -> Result<CosmosMsg, ContractError> {
    // //Get token_in & token_out prices
    // let token_out_price = collateral_price.clone();
    // let token_in_price = debt_price.clone();

    // //Calculate min amount out
    // let token_in_value = token_in_price.get_value(debt_amount)?;
    // let token_out_min_value = decimal_multiplication(token_in_value, Decimal::one() - max_slippage)?;
    // let token_out_min_amount = token_out_price.get_amount(token_out_min_value)?;

    // //Create Msg
    // let msg: CosmosMsg = MsgSwapExactAmountIn {
    //     sender: env.contract.address.to_string(),
    //     routes,
    //     token_in: Some(osmosis_std::types::cosmos::base::v1beta1::Coin {
    //         amount: debt_amount.to_string(),
    //         denom: debt_denom
    //     }),
    //     token_out_min_amount: token_out_min_amount.to_string(),
        
    // }.into();

    //Create swap msg from swap contract
    let debt_token = config.debt_token.clone().unwrap().to_string();
    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.swap_contract.unwrap().to_string(),
        msg: to_json_binary(&Swap_ExecuteMsg::Swap {
            caller: env.contract.address.to_string(),
            token_in: debt_token.clone(),
            token_out: collateral_denom.clone(),
            max_slippage: max_slippage,
        })?,
        funds: vec![
            Coin {
                denom: debt_token,
                amount: debt_amount,
            }
        ],
    });

    Ok(msg)
}
//How do we socialize bad debt?
//On liquidation we'll have to check for bad debt and add it to the config.
pub fn get_total_debt_tokens(
    config: Config,
    is_junior: Option<bool>,
) -> StdResult<Uint128> {
    //If is_junior, return junior debt tokens
    if let Some(is_junior) = is_junior {
        if is_junior {
            return Ok(config.junior_debt_info.clone().unwrap().total_debt.checked_sub(config.junior_debt_info.unwrap().bad_debt).unwrap_or(Uint128::zero()));
        } else {
            return Ok(config.total_debt_tokens.checked_sub(config.bad_debt).unwrap_or(Uint128::zero()));
        }
    } 
    //If no is_junior, return total debt tokens by adding total, junior_total and subtracting bad_debt from both 
    else {
        //Just all the fn twice so there are no discrepancies
        let total_debt_tokens = 
            get_total_debt_tokens(config.clone(), Some(false))?
            .checked_add(get_total_debt_tokens(config.clone(), Some(true))?)
            .unwrap_or(Uint128::zero());

        Ok(total_debt_tokens)
     }
}

pub fn get_total_vault_tokens(
    storage: &dyn Storage,
    is_junior: bool,
    ) -> StdResult<Uint128> {
    if is_junior {
        return Ok(JUNIOR_DEBT_VAULT_TOKEN.load(storage)?);
    } else {
        return Ok(DEBT_VAULT_TOKEN.load(storage)?);
    }
}

///Rate assurance
/// Ensures that the conversion rate is static for debt deposits & withdrawals
pub fn rate_assurance(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    is_junior: bool,
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
    let total_vault_tokens = get_total_vault_tokens(deps.storage, is_junior)?;

    //Get total_debt_tokens
    let total_debt_tokens = get_total_debt_tokens(config.clone(), Some(is_junior))?;

    //Calc the rate of vault tokens to deposit tokens
    let btokens_per_one = calculate_base_tokens(
        Uint128::new(1_000_000_000_000), 
        total_debt_tokens, 
        total_vault_tokens
    )?;

    //For deposit or withdraw, check that the rates are at most, off by 1
    let difference = if btokens_per_one > token_rate_assurance.pre_btokens_per_one {
        btokens_per_one.checked_sub(token_rate_assurance.pre_btokens_per_one).unwrap_or(Uint128::zero())
    } else {
        token_rate_assurance.pre_btokens_per_one.checked_sub(btokens_per_one).unwrap_or(Uint128::zero())
    };
    
    if difference > Uint128::from_str("1").unwrap_or(Uint128::zero()) {
        return Err(ContractError::CustomError { val: format!("Deposit or withdraw rate assurance failed for base token conversion. pre: {:?} --- post: {:?}", token_rate_assurance.pre_btokens_per_one, btokens_per_one) });
    }

    Ok(Response::new())
}

/// Collateral Rate Assurance
/// Ensures that the conversion rate is static for collateral deposits & withdrawals
pub fn collateral_rate_assurance(
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
    let collateral_rate_assurance = COLLATERAL_RATE_ASSURANCE.load(deps.storage)?;

    let collateral_state_total = COLLATERAL_STATE_TOTAL.load(deps.storage, collateral_rate_assurance.collateral_denom.clone())?;

    //Get total collateral 
    let total_collateral = get_contract_balances(
        deps.querier, 
        env.clone(), 
        vec![AssetInfo::NativeToken { denom: collateral_rate_assurance.collateral_denom.clone() }]
    )?[0];

    //Calc the rate of vault tokens to deposit tokens
    let btokens_per_one = calculate_base_tokens(
        Uint128::new(1_000_000), 
        total_collateral, 
        collateral_state_total
    )?;

    //For deposit or withdraw, check that the rates are at most, off by 1
    let difference = if btokens_per_one > collateral_rate_assurance.pre_collateral_per_one {
        btokens_per_one.checked_sub(collateral_rate_assurance.pre_collateral_per_one).unwrap_or(Uint128::zero())
    } else {
        collateral_rate_assurance.pre_collateral_per_one.checked_sub(btokens_per_one).unwrap_or(Uint128::zero())
    };
    
    if difference > Uint128::from_str("1").unwrap_or(Uint128::zero()) {
        return Err(ContractError::CustomError { val: format!("Deposit or withdraw rate assurance failed for deposit token state checks. pre: {:?} --- post: {:?}", collateral_rate_assurance.pre_collateral_per_one, btokens_per_one) });
    }

    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "collateral_rate_assurance"),
            attr("collateral_denom", collateral_rate_assurance.collateral_denom),
            attr("pre_collateral_per_one", collateral_rate_assurance.pre_collateral_per_one),
            attr("post_collateral_per_one", btokens_per_one),
        ]))
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

    //Get markets manager fee
    let markets_manager_fee = query_markets_manager_fee(deps.querier, config.markets_manager_contract.to_string())?;

    //Accrue.
    //Even without debt, this keeps everyones's state up to date
    accrue(
        deps.storage,
        env.clone(), 
        &mut config, 
        &mut user_position,
        &mut msgs,
        markets_manager_fee
    )?;    

    //Reload market post-accrue
    let market = match MARKET_PARAMS.load(deps.storage, collateral_denom.clone()){
        Ok(market) => market,
        Err(_) => return Err(ContractError::CustomError { val: format!("Collateral asset ({:?}) not supported", collateral_denom) }),
    };

    //Check if the position is insolvent
    //Get prices
    let prices = get_asset_prices(
        deps.querier,
        config.clone(), 
        env.contract.address.to_string(),
        true,
         vec![market.collateral_params.collateral_asset.clone(), config.debt_token.clone().unwrap()]
        )?;
    let collateral_price = prices[0].clone();
    let debt_price = prices[1].clone();
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

    //Centralised collateral accounting for liquidated collateral + fee
    let mut collateral_state_total_liq = COLLATERAL_STATE_TOTAL.load(deps.storage, collateral_denom.clone()).unwrap_or(Uint128::zero());
    adjust_position_collateral(
        deps.storage,
        position_owner.clone(),
        collateral_denom.clone(),
        collateral_amount_to_liquidate + fee_amount,
        false,
        &mut collateral_state_total_liq,
    )?;

    //Create swap msg for liquidations
    let swap_msgs = create_swap_to_debt_token_msg(
        config.clone(),
        env.clone(),
        collateral_denom.clone(), 
        collateral_amount_to_liquidate, 
        max_slippage,
        true
    )?;

    //Save pre liquidation CDT balance 
    let cdt_balance = deps.querier.query_balance(env.clone().contract.address, config.debt_token.clone().unwrap())?.amount;
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
        .add_submessages(swap_msgs)
        .add_messages(msgs)
        .add_submessage(SubMsg::reply_always(check_bad_debt_msg, BAD_DEBT_REPLY_ID))
        .add_attributes(vec![
            attr("method", "liquidate"),
            attr("user", position_owner),
            attr("collateral_liquidated", collateral_amount_to_liquidate),
            attr("fee_liquidated", fee_amount),
        ])) 

}


/// Distribute bad debt between junior and senior tranches according to risk waterfall.
/// Returns (junior_bad_debt_added, senior_bad_debt_added)
pub(crate) fn distribute_bad_debt(
    config: &mut Config,
    mut amount: Uint128,
) -> Result<(Uint128, Uint128), ContractError> {
    let mut junior_added = Uint128::zero();
    let mut senior_added = Uint128::zero();
    // If junior tranche exists and has room for bad debt
    if let Some(ref mut junior_info) = config.junior_debt_info {
        //Get junior room to underwrite bad debt
        let junior_room = junior_info.total_debt.checked_sub(junior_info.bad_debt)
            .map_err(|_| ContractError::CustomError { val: "Junior bad debt underflow".to_string() })?;
        //Get amount to add to junior bad debt
        let to_junior = min(amount, junior_room);

        //If there is a bad debt to add
        if !to_junior.is_zero() {
            //Update junior bad debt
            junior_info.bad_debt = junior_info.bad_debt.checked_add(to_junior)
                .map_err(|_| ContractError::CustomError { val: "Junior bad debt overflow".to_string() })?;
            //Update junior added
            junior_added = to_junior;
            //Update amount
            amount = amount.checked_sub(to_junior)
                .map_err(|_| ContractError::CustomError { val: "Bad debt subtraction underflow".to_string() })?;
        }
    }
    // Remainder goes to senior (unlabeled) bad debt
    if !amount.is_zero() {
        //Update senior bad debt
        config.bad_debt = config.bad_debt.checked_add(amount)
            .map_err(|_| ContractError::CustomError { val: "Senior bad debt overflow".to_string() })?;
        //Update senior added
        senior_added = amount;
    }
    Ok((junior_added, senior_added))
}

/// Check and recapitilize Bad Debt w/ revenue or MBRN auctions
pub fn check_and_fulfill_bad_debt(
    deps: DepsMut,
    env: Env,
) -> Result<Response, ContractError> {
    let mut config: Config = CONFIG.load(deps.storage)?;
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
    
    //Get prices
    let prices = get_asset_prices(
        deps.querier,
        config.clone(), 
        env.contract.address.to_string(),
        true, 
        vec![market.collateral_params.collateral_asset.clone()]
    )?;
    let collateral_price = prices[0].clone();
    //Get position's collateral asset value
    let collateral_value = collateral_price.get_value(liquidated_position.collateral_amount)?;
    //We check if the value left is > $1.
    //We use > $1 bc full liquidations will leave rounding errors in the collateral assets so we just use $1 as a floor instead of $0
    if collateral_value > Decimal::one() || liquidated_position.debt_amount.is_zero() {
        Err(ContractError::PositionSolvent {})
    } else {
        // Distribute bad debt according to tranches
        let (junior_added, senior_added) = distribute_bad_debt(&mut config, liquidated_position.debt_amount)?;
        //Update config state
        CONFIG.save(deps.storage, &config)?;
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
                attr("junior_bad_debt_added", junior_added),
                attr("senior_bad_debt_added", senior_added),
                attr("liquidated_position_debt", liquidated_position.debt_amount),
                attr("liquidated_position_collateral", liquidated_position.collateral_amount),
            ]))
    }
}


pub fn crank_realized_apr(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    is_junior: bool,
) -> Result<Response, ContractError> {
    //Load state
    let config = CONFIG.load(deps.storage)?; 
    let total_vault_tokens = if is_junior {
        JUNIOR_DEBT_VAULT_TOKEN.load(deps.storage)?
    } else {
        DEBT_VAULT_TOKEN.load(deps.storage)?
    };

    //Update Claim tracker
    let mut claim_tracker = if is_junior {
        JUNIOR_CLAIM_TRACKER.load(deps.storage)?
    } else {
        CLAIM_TRACKER.load(deps.storage)?
    };
    //Calculate time since last claim
    let time_since_last_checkpoint = env.block.time.seconds() - claim_tracker.last_updated;
    //Get the total deposit tokens
    let total_debt_tokens = get_total_debt_tokens(config.clone(), Some(is_junior))?;
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
        if is_junior {
            JUNIOR_CLAIM_TRACKER.save(deps.storage, &claim_tracker)?;
        } else {
            CLAIM_TRACKER.save(deps.storage, &claim_tracker)?;
        }

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
    if is_junior {
        JUNIOR_CLAIM_TRACKER.save(deps.storage, &claim_tracker)?;
    } else {
        CLAIM_TRACKER.save(deps.storage, &claim_tracker)?;
    }

    Ok(Response::new().add_attributes(vec![
        attr("method", "crank_realized_apr"),
        attr("new_base_token_conversion_rate", btokens_per_one),
        attr("time_since_last_checkpoint", time_since_last_checkpoint.to_string())
    ]))
}

pub fn query_markets_manager_fee(
    querier: QuerierWrapper,
    markets_manager_contract: String,
) -> Result<Decimal, ContractError> {
    let config: MarketManagerConfig = querier.query_wasm_smart::<MarketManagerConfig>(markets_manager_contract, &MarketManagerQueryMsg::Config {})?;

    Ok(config.managed_market_fee)
    
}
 
/// Sell position collateral to repay any % of debt.
/// Max spread is used to ensure the full debt is repaid in lieu of slippage.
pub fn close_position(
    deps: DepsMut, 
    env: Env,
    info: MessageInfo,
    collateral_denom: String,
    close_percentage: Option<Decimal>,
    mut max_spread: Decimal,
    mut send_to: Option<String>,
    //Close for a user that is not the sender. For SL, TP, or arb.
    position_owner: Option<String>,
) -> Result<Response, ContractError>{
    //Load global state
    let mut config: Config = CONFIG.load(deps.storage)?;
    let market = match MARKET_PARAMS.load(deps.storage, collateral_denom.clone()){
        Ok(market) => market,
        Err(_) => return Err(ContractError::CustomError { val: format!("Collateral asset ({:?}) not supported", collateral_denom) }),
    };

    //Initialize msgs
    let mut msgs: Vec<CosmosMsg> = vec![];

    //Get prices
    let prices = get_asset_prices(
        deps.querier, 
        config.clone(), 
        env.contract.address.to_string(),
        false, 
        vec![market.collateral_params.collateral_asset.clone(), config.debt_token.clone().unwrap()]
    )?;
    let collateral_price = prices[0].clone();
    let debt_price = prices[1].clone();

    //Set close_percentage
    let mut close_percentage = match close_percentage {
        Some(close_percentage) => min(close_percentage, Decimal::one()),
        None => Decimal::one(),
    };

    //Set position owner
    let position_owner = match position_owner {
        Some(position_owner) => deps.api.addr_validate(&position_owner)?,
        None => info.sender.clone(),
    };

    //Mutate max spread.
    //if the sender isn't the user, we don't go over the max slippage
    if info.sender != position_owner {
        max_spread = min(max_spread, market.max_slippage)
    }

    //Load target_position
    let mut target_position = match POSITIONS.may_load(deps.storage, (position_owner.clone(), collateral_denom.clone()))? {
        Some(target_position) => target_position,
        None => return Err(ContractError::CustomError { val: format!("Position not found for user {} in the {} collateral market", position_owner, collateral_denom) }),
    };

    //Get markets manager fee
    let markets_manager_fee = query_markets_manager_fee(deps.querier, config.markets_manager_contract.to_string())?;

    //Accrue interest
    accrue(
        deps.storage, 
        env.clone(), 
        &mut config, 
        &mut target_position, 
        &mut msgs, 
        markets_manager_fee
    )?;



    //If position owner is not the sender, make sure the position has SL and TP params ready to execute.
    if position_owner != info.sender {
        //Load Position's UX Boosts
        let mut target_position_ux_boosts = match POSITION_UX_BOOSTS.load(deps.storage, (position_owner.clone(), collateral_denom.clone())){
            Ok(target_position_ux_boosts) => target_position_ux_boosts,
            Err(_) => return Err(ContractError::CustomError { val: format!("Position owner {} has no UX Boosts set", position_owner) }),
        };
        //Check if the position has SL, TP, or arb price params set
        if target_position_ux_boosts.stop_loss_params.is_none() && target_position_ux_boosts.take_profit_params.is_none() && target_position_ux_boosts.arb_price.is_none() {
            return Err(ContractError::CustomError { val: format!("Position owner {} has no SL or TP params set", position_owner) });
        }

        //Get position LTV 
        let collateral_value = collateral_price.get_value(target_position.collateral_amount)?;
        let debt_value = debt_price.get_value(target_position.debt_amount)?;
        let position_LTV = decimal_division(debt_value, collateral_value)?;

        ////If either are some, check that the set price has hit the target price.///
        //SL
        if let Some(stop_loss_params) = target_position_ux_boosts.stop_loss_params.clone() {
            if position_LTV > stop_loss_params.ltv {
                return Err(ContractError::CustomError { val: format!("Position owner {} has not hit the stop loss ltv of {}. Current ltv is {}", position_owner, stop_loss_params.ltv, position_LTV) });
            }
            //Update close percentage to the auto close params
            close_percentage = min(stop_loss_params.percent_to_close, Decimal::one());
            //Update the send_to to the auot close params
            send_to = match stop_loss_params.send_to {
                Some(send_to) => Some(send_to),
                None => Some(position_owner.to_string()),
            };

            //Remove the stop loss params if they are not perpetual
            if !stop_loss_params.perpetual {
                target_position_ux_boosts.stop_loss_params = None;
            }   
        } else
        //TP 
        if let Some(take_profit_params) = target_position_ux_boosts.take_profit_params.clone() {
            if position_LTV < take_profit_params.ltv {
                return Err(ContractError::CustomError { val: format!("Position owner {} has not hit the take profit ltv of {}. Current ltv is {}", position_owner, take_profit_params.ltv, position_LTV) });
            }
            //Update close percentage to the auto close params
            close_percentage = min(take_profit_params.percent_to_close, Decimal::one());
            //Update the send_to to the auot close params
            send_to = match take_profit_params.send_to {
                Some(send_to) => Some(send_to),
                None => Some(position_owner.to_string()),
            };

            //Remove the take profit params if they are not perpetual
            if !take_profit_params.perpetual {
                target_position_ux_boosts.take_profit_params = None;
            }
        } else
        //Arb price
        if let Some(arb_price) = target_position_ux_boosts.arb_price.clone() {
            if debt_price.price > arb_price {
                return Err(ContractError::CustomError { val: format!("Debt price is not less than or equal to the arb price for {}", position_owner) });
            }
            //Bc its an arb, any viable arb is profitable so the close percentage can be whatever the caller wants.
            //Granted this restricts flexibility but its fine for what we need it for.

            //Set send_to to the position owner
            send_to = Some(position_owner.to_string());
        }

        ////Send the executor the fee///
        //Calculate the amount of collateral to send
        let mut fee_collateral_amount = collateral_price.get_amount(target_position_ux_boosts.collateral_value_fee_to_executor * close_percentage)?;
        //Update the position's state, subtract fee amount 
        target_position.collateral_amount = match target_position.collateral_amount.checked_sub(fee_collateral_amount){
            Ok(val) => val,
            Err(_) => {
                //If fee is greater, set the fee to 1% of the collateral
                fee_collateral_amount = collateral_price.get_amount(decimal_multiplication(
                    Decimal::from_ratio(target_position.collateral_amount, Uint128::one()),
                    Decimal::percent(1))?
                )?;
                //Subtract the fee from the position
                match target_position.collateral_amount.checked_sub(fee_collateral_amount){
                    Ok(val) => val,
                    Err(_) => return Err(ContractError::CustomError { val: format!("2nd Layer: Collateral amount to send: {} > User collateral amount: {}", fee_collateral_amount, target_position.collateral_amount) }),
                }

            }
        };
        

        //Send the fee to the executor
        if !fee_collateral_amount.is_zero() {
            let fee_message = CosmosMsg::Bank(BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: vec![Coin {
                    denom: collateral_denom.clone(),
                    amount: fee_collateral_amount,
                }],
            });
            msgs.push(fee_message);
        }

        //Save the updated position_ux_boosts
        POSITION_UX_BOOSTS.save(deps.storage, (position_owner.clone(), collateral_denom.clone()), &target_position_ux_boosts)?;

    }

    //Set send_to for withdrawal in Reply
    if send_to.is_none() {
        send_to = Some(position_owner.to_string());
    }


    //Set close_amount
    let close_amount = target_position.debt_amount * close_percentage;

    //Calc collateral to sell
    //credit_amount * credit_price * (1 + max_spread)
    let total_collateral_value_to_sell = {
            decimal_multiplication(
                debt_price.get_value(close_amount)?, 
                (max_spread + Decimal::one())
            )?
    };
    //Max_spread is added to the collateral amount to ensure enough credit is purchased
    //Excess debt token gets sent back to the position_owner during repayment

    //Calc collateral_amount_to_sell
    let mut collateral_amount_to_sell = {

        let collateral_value_to_sell = total_collateral_value_to_sell;

        let post_normalized_amount: Uint128 = match collateral_price.get_amount(collateral_value_to_sell){
            Ok(amount) => amount,
            Err(_e) => return Err(ContractError::CustomError { val: String::from("Collateral value to sell is too high to calculate an amount for due to the max spread creating an out of bounds error") })
        };

        post_normalized_amount
    };

    //Collateral to sell can't be more than the position owns
    if collateral_amount_to_sell > target_position.collateral_amount {
        collateral_amount_to_sell = target_position.collateral_amount;
    }

    //Centralised collateral accounting for the collateral we are selling
    let mut collateral_state_total = COLLATERAL_STATE_TOTAL.load(deps.storage, collateral_denom.clone()).unwrap_or(Uint128::zero());
    adjust_position_collateral(
        deps.storage,
        position_owner.clone(),
        collateral_denom.clone(),
        collateral_amount_to_sell,
        false,
        &mut collateral_state_total,
    )?;

    //Create swap subMsg to sell, create repay & withdraw msgs in reply on success
    let swap_msgs = create_swap_to_debt_token_msg(
        config.clone(),
        env.clone(),
        collateral_denom.clone(), 
        collateral_amount_to_sell, 
        max_spread,
        false
    )?; 

    //Save CLOSE_POSITION_PROPAGATION
    CLOSE_POSITION.save(deps.storage, &ClosePositionPropagation {
        position_owner: position_owner.to_string(),
        collateral_denom: collateral_denom.clone(),
        send_to,
        pre_close_debt_balance: get_contract_balances(
            deps.querier, 
            env.clone(), 
            vec![AssetInfo::NativeToken { denom: config.debt_token.clone().unwrap() }])?[0],
        collateral_swapped: collateral_amount_to_sell,
    })?;

    Ok(Response::new()
        .add_messages(msgs)
        .add_submessages(swap_msgs)
        .add_attributes(vec![
        attr("collateral_denom", collateral_denom),
        attr("msg_executor", info.sender),
        attr("position_owner", position_owner),
        attr("collateral_amount_to_sell", collateral_amount_to_sell),
        attr("debt_amount_to_repay", close_amount),
        attr("max_spread", max_spread.to_string()),
    ])) 
}

pub fn loop_position(
    deps: DepsMut, 
    env: Env,
    info: MessageInfo,
    collateral_denom: String,   
    //Loop for a user that is not the sender. For managed intents positions.
    position_owner: Option<String>,
    //Max slippage
    max_slippage: Option<Decimal>,
) -> Result<Response, ContractError>{
    //Load global state
    let config: Config = CONFIG.load(deps.storage)?;
    let market = match MARKET_PARAMS.load(deps.storage, collateral_denom.clone()){
        Ok(market) => market,
        Err(_) => return Err(ContractError::CustomError { val: format!("Collateral asset ({:?}) not supported", collateral_denom) }),
    };

    //Initialize msgs
    let mut msgs: Vec<CosmosMsg> = vec![];

    //Get prices
    let prices = get_asset_prices(
        deps.querier, 
        config.clone(), 
        env.contract.address.to_string(),
        false, 
        vec![market.collateral_params.collateral_asset.clone(), config.debt_token.clone().unwrap()]
    )?;
    let collateral_price = prices[0].clone();
    let debt_price = prices[1].clone();

    //Set position owner
    let position_owner = match position_owner {
        Some(position_owner) => deps.api.addr_validate(&position_owner)?,
        None => info.sender.clone(),
    };

    //Load target_position
    let target_position = match POSITIONS.may_load(deps.storage, (position_owner.clone(), collateral_denom.clone()))? {
        Some(target_position) => target_position,
        None => return Err(ContractError::CustomError { val: format!("Position not found for user {} in the {} collateral market", position_owner, collateral_denom) }),
    };

    //Load target_position_ux_boosts
    let target_position_ux_boosts = match POSITION_UX_BOOSTS.load(deps.storage, (position_owner.clone(), collateral_denom.clone())){
        Ok(target_position_ux_boosts) => target_position_ux_boosts,
        Err(_) => return Err(ContractError::CustomError { val: format!("Position owner {} has no UX Boosts set", position_owner) }),
    };

    //Set UX boost params
    let mut loop_params = target_position_ux_boosts.clone();

    //Get user's intended LTV
    let intended_LTV = match loop_params.loop_ltv.clone() {
        Some(intended_LTV) => min(intended_LTV.loop_ltv, market.collateral_params.max_borrow_LTV),
        None => return Err(ContractError::CustomError { val: format!("Position owner {} has no loop params set", position_owner) }),
    };

    //Transform LTV intent to multiplier intent
    //ex: 60% LTV = 2.5x 
    let intended_multiplier = match decimal_division(
        Decimal::one(), 
        Decimal::one() - intended_LTV
    ){
        Ok(val) => val,
        Err(_) => return Err(ContractError::CustomError { val: format!("Failed to calculate multiplier from LTV") }),
    };

    //Get the sum of collateral bought from loops
    let total_bought_from_loops = loop_params.collateral_bought_from_loops.iter().map(|collateral| collateral.amount_purchased).sum::<Uint128>();

    //Calculate the looped exposure the position currently has
    let current_multiplier = decimal_division(
        Decimal::from_ratio(total_bought_from_loops + target_position.collateral_amount, Uint128::one()),
        Decimal::from_ratio(target_position.collateral_amount, Uint128::one())
    )?;

    //Only loop if the multiplier is not within 3% of the intended
    if current_multiplier < decimal_multiplication(intended_multiplier, Decimal::percent(97))? {
        //Calc the current LTV
        let collateral_value = collateral_price.get_value(target_position.collateral_amount)?;
        let debt_value = debt_price.get_value(target_position.debt_amount)?;
        let position_LTV = decimal_division(debt_value, collateral_value)?;

        //Calc LTV space to loop
        let LTV_space_to_loop = match decimal_subtraction(intended_LTV, position_LTV){
            Ok(val) => { val },
            Err(_) => return Err(ContractError::CustomError { val: format!("Failed to calculate LTV space to loop") }),
        };
        
        //Calc amount of debt to mint
        let debt_value_to_loop = decimal_multiplication(collateral_value, LTV_space_to_loop)?;
        let debt_amount_to_mint = debt_price.get_amount(debt_value_to_loop)?;

        //Create mint msg
        let internal_mint_msg = ExecuteMsg::Borrow { 
            collateral_denom: collateral_denom.clone(), 
            send_to: Some(position_owner.to_string()), 
            borrow_amount: BorrowOptions { amount: Some(debt_amount_to_mint), ltv: None }
        };
        let mint_msg = WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&internal_mint_msg)?,
            funds: vec![],
        };
        msgs.push(mint_msg.into());

        //Simulate borrow amount
        let (borrowable_amount, _) = calc_borrowable_amount(
            deps.querier.clone(),
            env.clone(),
            BorrowOptions { amount: Some(debt_amount_to_mint), ltv: None },
            market.clone(),
            collateral_price.clone(),
            debt_price.clone(),
            target_position.clone(),
            config.debt_token.clone().unwrap()
        )?;
        //Set max slippage
        let max_slippage = match max_slippage {
            Some(max_slippage) => { 
                //if the sender isn't the user, we don't go over the max slippage
                if info.sender != position_owner {
                    min(max_slippage, market.max_slippage)
                } else {
                    max_slippage
                }
            },
            None => market.max_slippage,
        };

        //Create swap to collateral SubMsg
        let swap_msg = create_swap_to_collateral_msg(
            config.clone(), 
            env.clone(),
            collateral_denom.clone(), 
            borrowable_amount, 
            max_slippage,
        )?;

        let sub_msg = SubMsg::reply_on_success(swap_msg, LOOP_POSITION_REPLY_ID);

        //Save LOOP_POSITION_PROPAGATION
        LOOP_POSITION.save(deps.storage, &LoopPropagation {
            position_owner: position_owner.to_string(),
            collateral_denom: collateral_denom.clone(),
            pre_loop_collateral_balance: get_contract_balances(
                deps.querier, 
                env.clone(), 
                vec![AssetInfo::NativeToken { denom: collateral_denom.to_string() }])?[0],
            intended_multiplier,
        })?;

        //Return
        Ok(Response::new()
            .add_messages(msgs)
            .add_submessage(sub_msg)
            .add_attributes(vec![
                attr("collateral_denom", collateral_denom),
                attr("msg_executor", info.sender),
                attr("position_owner", position_owner),
                attr("debt_amount_to_mint", debt_amount_to_mint),
                attr("max_slippage", max_slippage.to_string()),
            ]))

    } else {
        //If the multiplier is within 3% of the intended, remove the LTV intent if its not perpetual
        if let Some(intended_LTV) = loop_params.loop_ltv.clone() {
            if !intended_LTV.perpetual {
                loop_params.loop_ltv = None;
            }
            //Save the updated loop_params
            POSITION_UX_BOOSTS.save(deps.storage, (position_owner.clone(), collateral_denom.clone()), &loop_params)?;
        }

        //Return
        Ok(Response::new()
            .add_attributes(vec![
                attr("collateral_denom", collateral_denom),
                attr("msg_executor", info.sender),
                attr("position_owner", position_owner),
            ]))
        // return Err(ContractError::CustomError { val: format!("Current multiplier {} is within 3% of the intended multiplier {}", current_multiplier, intended_multiplier) });
    }

}

// -----------------------------------------------------------------------------
// Helper: centralised collateral accounting
// -----------------------------------------------------------------------------
/// Adjust the global collateral state total **and** the user position's collateral
/// in a single, reusable place.
///
/// * `add == true` – increase collateral (e.g. deposits/supply).
/// * `add == false` – decrease collateral (e.g. withdraw, close, liquidate).
///
/// The helper takes care of:
///   1. Updating `COLLATERAL_STATE_TOTAL`.
///   2. Updating / removing the entry in `POSITIONS`.
///   3. Cleaning up `POSITION_UX_BOOSTS` when a position has no collateral left.
/// It returns the up-to-date `UserPosition` after the mutation (this will be a
/// zero-collateral position when it has been removed from storage).
fn adjust_position_collateral(
    storage: &mut dyn Storage,
    position_owner: Addr,
    collateral_denom: String,
    amount: Uint128,
    add: bool,
    collateral_state_total: &mut Uint128,
) -> Result<UserPosition, ContractError> {
    if amount.is_zero() {
        return Err(ContractError::ZeroAmount {});
    }

    // Update the global collateral tally via the mutable reference
    if add {
        *collateral_state_total = collateral_state_total
            .checked_add(amount)
            .map_err(|_| ContractError::CustomError { val: format!("Failed to add collateral state total: {} + {}", *collateral_state_total, amount) })?;
    } else {
        *collateral_state_total = collateral_state_total
            .checked_sub(amount)
            .map_err(|_| ContractError::CustomError { val: format!("Failed to subtract collateral state total: {} - {}", *collateral_state_total, amount) })?;
    }
    // Persist new total
    COLLATERAL_STATE_TOTAL.save(storage, collateral_denom.clone(), collateral_state_total)?;

    // --- user position --------------------------------------------------------
    let mut position = POSITIONS
        .may_load(storage, (position_owner.clone(), collateral_denom.clone()))?
        .unwrap_or(UserPosition {
            collateral_denom: collateral_denom.clone(),
            collateral_amount: Uint128::zero(),
            debt_amount: Uint128::zero(),
            rate_index: Decimal::zero(),
        });

    if add {
        position.collateral_amount = position
            .collateral_amount
            .checked_add(amount)
            .map_err(|_| ContractError::CustomError { val: format!("Collateral overflow {} + {}", position.collateral_amount, amount) })?;
    } else {
        // Ensure sufficient collateral
        if amount > position.collateral_amount {
            return Err(ContractError::CustomError { val: "Insufficient collateral to remove".to_string() });
        }
        position.collateral_amount = position
            .collateral_amount
            .checked_sub(amount)
            .map_err(|_| ContractError::CustomError { val: format!("Failed to subtract collateral amount: {} - {}", position.collateral_amount, amount) })?;
    }

    // Persist / clean-up
    if position.collateral_amount.is_zero() && position.debt_amount.is_zero() {
        // Remove empty position & its UX boosts
        POSITIONS.remove(storage, (position_owner.clone(), collateral_denom.clone()));
        POSITION_UX_BOOSTS.remove(storage, (position_owner.clone(), collateral_denom.clone()));
    } else {
        POSITIONS.save(storage, (position_owner.clone(), collateral_denom.clone()), &position)?;
    }

    Ok(position)
}
// -----------------------------------------------------------------------------



