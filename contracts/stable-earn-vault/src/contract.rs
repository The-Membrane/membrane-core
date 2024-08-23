#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, to_json_binary, Addr, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env, MessageInfo, QuerierWrapper, Reply, Response, StdError, StdResult, Storage, SubMsg, Uint128, WasmMsg
};
use membrane::oracle::{self, PriceResponse};
use membrane::types::{Asset, AssetInfo, Basket, UserInfo};
use osmosis_std::types::osmosis;
use serde::de;
use std::cmp::{max, min};
use std::str::FromStr;
use std::vec;
use cw2::set_contract_version;
use membrane::math::{decimal_division, decimal_multiplication, decimal_subtraction};

use crate::error::TokenFactoryError;
use crate::state::{TOKEN_RATE_ASSURANCE, TokenRateAssurance, CONFIG, OWNERSHIP_TRANSFER, VAULT_TOKEN};
use membrane::stable_earn_vault::{Config, APRResponse, ExecuteMsg, InstantiateMsg, MigrateMsg, QueryMsg};
use membrane::mars_vault_token::{ExecuteMsg as Vault_ExecuteMsg, QueryMsg as Vault_QueryMsg};
use membrane::cdp::{BasketPositionsResponse, CollateralInterestResponse, ExecuteMsg as CDP_ExecuteMsg, InterestResponse, PositionResponse, QueryMsg as CDP_QueryMsg};
use membrane::osmosis_proxy::{ExecuteMsg as OP_ExecuteMsg};
use membrane::oracle::QueryMsg as Oracle_QueryMsg;
use membrane::stability_pool_vault::{
    calculate_base_tokens, calculate_vault_tokens, APRResponse as NoCost_APRResponse
};
use osmosis_std::types::osmosis::tokenfactory::v1beta1::{self as TokenFactory};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:stable-earn-vault";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Reply IDs
const ENTER_VAULT_REPLY_ID: u64 = 1u64;
const CDP_REPLY_ID: u64 = 2u64;
const LOOP_REPLY_ID: u64 = 3u64;

//Constants
const SECONDS_PER_DAY: u64 = 86_400u64;
const LOOP_MAX: u32 = 5u32;

////PROCEDURAL FLOW/NOTES////
// - There is a deposit and exit fee. 
// --The exit fee is added in manually thru the contract in get_total_deposits().
// -- The deposit fee is baked into the "liquid" valuation calc of the CDP position so deposits that don't get looped won't confer this fee to the vault.
// - We need to keep a buffer of vault tokens outside of the vault to allow for easy withdrawals (todo)
// -

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, TokenFactoryError> {
    let mut config = Config {
        owner: info.sender.clone(),
        cdt_denom: msg.clone().cdt_denom,
        vault_token: String::from("factory/".to_owned() + env.contract.address.as_str() + "/" + msg.clone().vault_subdenom.as_str()),
        deposit_token: msg.clone().deposit_token,
        cdp_contract_addr: deps.api.addr_validate(&msg.clone().cdp_contract_addr)?,
        osmosis_proxy_contract_addr: deps.api.addr_validate(&msg.clone().osmosis_proxy_contract_addr)?,
        oracle_contract_addr: deps.api.addr_validate(&msg.clone().oracle_contract_addr)?,
        withdrawal_buffer: Decimal::percent(10),
        total_nonleveraged_vault_tokens: Uint128::new(1_000_000_000_000), //from initial deposit
        cdp_position_id: Uint128::zero(),
        deposit_cap: Uint128::new(10_000_000_000),
        swap_slippage: Decimal::from_str("0.005").unwrap(), //0.5%
        vault_cost_index: 0,
    };
    //Validate the deposit token vault addr
    deps.api.addr_validate(&config.deposit_token.vault_addr.to_string())?;
    //Query the basket to find the index of the vault_token
    let basket: Basket = match deps.querier.query_wasm_smart::<Basket>(
        config.cdp_contract_addr.to_string(),
        &CDP_QueryMsg::GetBasket { },
    ){
        Ok(basket) => basket,
        Err(_) => return Err(TokenFactoryError::CustomError { val: String::from("Failed to query the CDP Basket") }),
    };
    //Find the index
    let mut saved_index: Option<u64> = None;
    for (index, asset) in basket.clone().collateral_types.into_iter().enumerate(){
        if asset.asset.info.to_string() == config.deposit_token.clone().vault_token {
            saved_index = Some(index as u64);
            break;
        }
    }
    if let Some(index) = saved_index {
        config.vault_cost_index = index as usize;
    } else {
        return Err(TokenFactoryError::CustomError { val: String::from("Failed to find the vault token in the CDP Basket") });
    }

    //Save initial state
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    //Query the underlying of the initial vault token deposit
    let underlying_deposit_token: Uint128 = match deps.querier.query_wasm_smart::<Uint128>(
        config.deposit_token.vault_addr.to_string(),
        &Vault_QueryMsg::VaultTokenUnderlying { vault_token_amount: Uint128::new(1_000_000_000_000) },
    ){
        Ok(underlying_deposit_token) => underlying_deposit_token,
        Err(_) => return Err(TokenFactoryError::CustomError { val: String::from("Failed to query the Mars Vault Token for the underlying deposit amount in instantiate") }),
    };

    //Set the initial vault token amount from the initial deposit
    let vault_tokens_to_distribute = calculate_vault_tokens(
        underlying_deposit_token,
        Uint128::zero(), 
        Uint128::zero()
    )?;
    CONFIG.save(deps.storage, &config)?;
    VAULT_TOKEN.save(deps.storage, &vault_tokens_to_distribute)?;  
    //Create Denom Msg
    let denom_msg = TokenFactory::MsgCreateDenom { sender: env.contract.address.to_string(), subdenom: msg.vault_subdenom.clone() };
    //Create CDP deposit msg to get the position ID
    //Instantiatoor must send a vault token
    let cdp_deposit_msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.cdp_contract_addr.to_string(),
        msg: to_json_binary(&CDP_ExecuteMsg::Deposit { position_id: None, position_owner: None })?,
        funds: vec![Coin {
            denom: config.deposit_token.vault_token.clone(),
            amount: Uint128::new(1_000_000_000_000),
        }],
    });
    let cdp_submsg = SubMsg::reply_on_success(cdp_deposit_msg, CDP_REPLY_ID);
    
    //Create Response
    let res = Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address)
        .add_attribute("sub_denom", msg.clone().vault_subdenom)
    //UNCOMMENT
        .add_message(denom_msg)
        .add_submessage(cdp_submsg);
    Ok(res)
}


#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, TokenFactoryError> {
    match msg {
        ExecuteMsg::UpdateConfig { 
            owner, 
            cdp_contract_addr, 
            mars_vault_addr,
            osmosis_proxy_contract_addr, 
            oracle_contract_addr, 
            withdrawal_buffer,
            deposit_cap, 
            swap_slippage,
            vault_cost_index
        } => update_config(deps, info, owner, cdp_contract_addr, mars_vault_addr, osmosis_proxy_contract_addr, oracle_contract_addr, withdrawal_buffer, deposit_cap, swap_slippage, vault_cost_index),
        ExecuteMsg::EnterVault { } => enter_vault(deps, env, info),
        ExecuteMsg::ExitVault {  } => exit_vault(deps, env, info),
        ExecuteMsg::UnloopCDP { desired_collateral_withdrawal } => unloop_cdp(deps, env, info, desired_collateral_withdrawal),
        ExecuteMsg::LoopCDP { } => loop_cdp(deps, env, info),
        ///CALLBACKS///
        ExecuteMsg::RateAssurance {  } => rate_assurance(deps, env, info),
        ExecuteMsg::PostLoopMaintenance {  } => post_loop(deps, env),
        ExecuteMsg::UnloopMaintenance {  } => post_unloop(deps, env),
    }
}



//LOOP NOTES: 
// - Loop to leave a 101 CDT LTV gap to allow easier unlooping under the minimum
// - Only loop if 7 day APR is profitable
// - Don't loop if CDT price is below 99% of peg
// - We don't loop the buffer of vault tokens in the contract
// todo!(); //Also do the unloop blocker above 101% of peg
//POST LOOP NOTES:
// - At the end of the loop in a submsg, we deposit all vault tokens in our contract in case we withdraw too many during the unloop or swap below the slippage limit in the loop.
// - If the vault is unprofitable post loop, we error, the caller needs to attempt a lower loop max.
// -- We need to avoid the vault getting farmed for swap fees
fn loop_cdp(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
) -> Result<Response, TokenFactoryError> {
    //Load config
    let config = CONFIG.load(deps.storage)?;
    let mut msgs = vec![];
    
    //Ensure price is above 99.5% of peg
    //We want to ensure loops keep redemptions at 99% of peg profitable
    test_looping_peg_price(deps.querier, config.clone(), Decimal::percent(98) + config.swap_slippage)?;

    let (
        running_credit_amount, 
        running_collateral_amount, 
        vt_price, 
        cdt_price
    ) = get_cdp_position_info(deps.as_ref(), env.clone(), config.clone())?;

    //Get deposit token price
    let prices: Vec<PriceResponse> = match deps.querier.query_wasm_smart::<Vec<PriceResponse>>(
        config.oracle_contract_addr.to_string(),
        &Oracle_QueryMsg::Price {
            asset_info: AssetInfo::NativeToken { denom: config.clone().deposit_token.deposit_token },
            twap_timeframe: 0, //We want current swap price
            oracle_time_limit: 0,
            basket_id: None
        },
    ){
        Ok(prices) => prices,
        Err(_) => return Err(TokenFactoryError::CustomError { val: String::from("Failed to query the deposit token price in loop") }),
    };
    let deposit_token_price: PriceResponse = prices[0].clone();

    let (_, _, amount_to_mint) = calc_mintable(
        config.clone().swap_slippage, 
        vt_price.clone(),
        deposit_token_price.clone(), 
        cdt_price.clone(), 
        running_collateral_amount, 
        running_credit_amount
    )?;
        
    //Leave a 101 CDT LTV gap to allow easier unlooping under the minimum debt (100)
    //$112.22 of LTV space is ~101 CDT at 90% borrow LTV
    // if min_deposit_value < Decimal::percent(112_22){
    //     break;
    // }

    //Create mint msg
    let mint_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.cdp_contract_addr.to_string(),
        msg: to_json_binary(&CDP_ExecuteMsg::IncreaseDebt { 
            position_id: config.cdp_position_id,
            amount: Some(amount_to_mint),
            LTV: None,
            mint_to_addr: None,
        })?,
        funds: vec![],
    });
    msgs.push(mint_msg);
    //Create swap msg
    let swap_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.osmosis_proxy_contract_addr.to_string(),
        msg: to_json_binary(&OP_ExecuteMsg::ExecuteSwaps { 
            token_out: config.deposit_token.deposit_token.clone(),
            max_slippage: config.swap_slippage,
        })?,
        funds: vec![
            Coin {
                denom: config.cdt_denom.clone(),
                amount: amount_to_mint,
            }
        ],
    });
    let submsg = SubMsg::reply_on_success(swap_msg, LOOP_REPLY_ID);
    
    /////What if we submsg the swap & do the next steps after the swap so we don't have to guess the deposit_value?////

    println!("end of loop_cdp");
    //Create Response
    let res = Response::new()
        .add_attribute("method", "loop_cdp")
        .add_attribute("current_collateral", running_collateral_amount)
        .add_attribute("current_debt", running_credit_amount)
        .add_messages(msgs)
        .add_submessage(submsg);

    Ok(res)
    
}


/// POST LOOP: Check loop profitablility & peg price
fn post_loop(
    deps: DepsMut,
    env: Env,
) -> Result<Response, TokenFactoryError>{
    //Load config
    let config = CONFIG.load(deps.storage)?;

    //Only loop if the 7 day APR is profitable.
    //This fn will error if the APR is profitable.
    // match test_7day_apr_profitability(deps.as_ref(), env.clone(), config.clone(), true){
    //     Ok(_) => return Err(TokenFactoryError::CustomError { val: String::from("Looped APR is less profitable than unleveraged APR, try a lower loop max to reduce CDP rates.") }),
    //     Err(_) => {},
    // };

    //Ensure price is still above 99% of peg
    let (cdt_market_price, cdt_peg_price) = test_looping_peg_price(deps.querier, config.clone(), Decimal::percent(98))?;

    //Create Response
    let res = Response::new()
        .add_attribute("method", "post_loop")
        .add_attribute("cdt_market_price", cdt_market_price.to_string())
        .add_attribute("cdt_peg_price", cdt_peg_price.to_string());

    Ok(res)
}

fn test_looping_peg_price(
    querier: QuerierWrapper,
    config: Config,
    desired_peg_price: Decimal,
) -> Result<(Decimal, Decimal), TokenFactoryError>{
    //Query basket for CDT peg price
    let basket: Basket = match  querier.query_wasm_smart::<Basket>(
        config.cdp_contract_addr.to_string(),
        &CDP_QueryMsg::GetBasket {  },
    ){
        Ok(basket) => basket,
        Err(_) => return Err(TokenFactoryError::CustomError { val: String::from("Failed to query the CDP basket in test_looping_peg_price") }),
    };
    let cdt_peg_price: Decimal = basket.credit_price.price;

    //Check that CDT market price is equal or above 99% of peg
    let prices: Vec<PriceResponse> = match querier.query_wasm_smart::<Vec<PriceResponse>>(
        config.oracle_contract_addr.to_string(),
        &Oracle_QueryMsg::Price {
            asset_info: AssetInfo::NativeToken { denom: config.clone().cdt_denom },
            twap_timeframe: 0, //We want current swap price
            oracle_time_limit: 0,
            basket_id: None
        },
    ){
        Ok(prices) => prices,
        Err(_) => return Err(TokenFactoryError::CustomError { val: String::from("Failed to query the cdt price in post unloop") }),
    };
    let cdt_market_price: Decimal = prices[0].clone().price;

    if decimal_division(cdt_market_price, max(cdt_peg_price, Decimal::one()))? < desired_peg_price {
        return Err(TokenFactoryError::CustomError { val: String::from("CDT price is below 99% of peg, can't loop. Try a lower loop_max to reduce sell pressure.") });
    }

    Ok((cdt_market_price, cdt_peg_price))
}

fn post_unloop(
    deps: DepsMut,
    env: Env,
) -> Result<Response, TokenFactoryError>{
    //Load config
    let config = CONFIG.load(deps.storage)?;

    //Query basket for CDT peg price
    let basket: Basket = match deps.querier.query_wasm_smart::<Basket>(
        config.cdp_contract_addr.to_string(),
        &CDP_QueryMsg::GetBasket {  },
    ){
        Ok(basket) => basket,
        Err(_) => return Err(TokenFactoryError::CustomError { val: String::from("Failed to query the CDP basket in unloop") }),
    };
    let cdt_peg_price: Decimal = basket.credit_price.price;

    //Check that CDT market price is equal or below 101% of peg
    let prices: Vec<PriceResponse> = match deps.querier.query_wasm_smart::<Vec<PriceResponse>>(
        config.oracle_contract_addr.to_string(),
        &Oracle_QueryMsg::Price {
            asset_info: AssetInfo::NativeToken { denom: config.clone().cdt_denom },
            twap_timeframe: 0, //We want current swap price
            oracle_time_limit: 0,
            basket_id: None
        },
    ){
        Ok(prices) => prices,
        Err(_) => return Err(TokenFactoryError::CustomError { val: String::from("Failed to query the cdt price in post unloop") }),
    };
    let cdt_market_price: Decimal = prices[0].clone().price;

    if decimal_division(cdt_market_price, max(cdt_peg_price, Decimal::one()))? > Decimal::percent(101){
        return Err(TokenFactoryError::CustomError { val: String::from("CDT price is above 101% of peg, can't unloop.") });
    }

    //Create Response
    let res = Response::new()
        .add_attribute("method", "post_unloop")
        .add_attribute("cdt_market_price", cdt_market_price.to_string())
        .add_attribute("cdt_peg_price", cdt_peg_price.to_string());


    Ok(res)
    
}
 
/// Calc mintable value & return new tokens to deposit, value & amount to mint
fn calc_mintable(
    swap_slippage: Decimal,
    vt_price: PriceResponse,
    deposit_token_price: PriceResponse,
    cdt_price: PriceResponse,
    vault_tokens: Uint128,
    debt: Uint128,
) -> StdResult<(Uint128, Decimal, Uint128)>{ 
    //Calc the value of the vault tokens
    let vault_tokens_value = vt_price.get_value(vault_tokens)?;
    //Calc the value of the CDT debt
    let debt_value = cdt_price.get_value(debt)?;
    //Calc LTV
    let ltv = decimal_division(debt_value, max(vault_tokens_value, Decimal::one()))?;
    //Calc the distance of the LTV to 90%
    let ltv_space_to_mint = match Decimal::percent(90).checked_sub(ltv){
        Ok(v) => v,
        Err(_) => return Err(StdError::GenericErr { msg: format!("LTV over 90%: {} > 0.9", ltv) }),
    };
    //Calc the value of the debt to mint
    let mintable_value = decimal_multiplication(vault_tokens_value, ltv_space_to_mint)?;
    //Calc the amount of vault tokens to mint
    let amount_to_mint = cdt_price.get_amount(mintable_value)?;
    //Calc the value of the mintable value with slippage
    let min_deposit_value = decimal_multiplication(mintable_value, decimal_subtraction(Decimal::one(), swap_slippage)?)?;
    //Calc the minimum amount of deposit token we will send to the vault 
    let min_new_deposit_token = deposit_token_price.get_amount(min_deposit_value)?;

    // println!("Mintable Value: {}, {}, {}, {}", mintable_value, min_deposit_value, min_new_deposit_token, swap_slippage);

    Ok((min_new_deposit_token, min_deposit_value, amount_to_mint))
}


//Unloop the vaults CDP position
//..to either withdraw for a user OR to fully close debt position
//NOTE: 
//- Accrue beforehand if trying to fully unloop
//- If the 7 day APR is unprofitable, anyone can call this fn with desired collateral withdrawal as None
// - If the 7 day APR is profitable, only the contract can call this fn
// - We only loop once per call if the desired collateral withdrawal is None bc we don't want to unloop too deep into profitability and have to loop again
fn unloop_cdp(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    desired_collateral_withdrawal: Uint128,
) -> Result<Response, TokenFactoryError> {
    //Load config
    let mut config = CONFIG.load(deps.storage)?;
    let mut msgs = vec![];

    //Error if not the contract calling
    if info.sender != env.contract.address {
        return Err(TokenFactoryError::Unauthorized {});
    }

    //Get running totals for CDP position & prices
    let (
        mut running_credit_amount, 
        mut running_collateral_amount, 
        vt_token_price, 
        cdt_price
    ) = get_cdp_position_info(deps.as_ref(), env.clone(), config.clone())?;

    panic!("running_credit_amount: {}, running_collateral_amount: {}", running_credit_amount, running_collateral_amount);

    //Initialize loop variables 
    let mut loops_count = 0;
    ////Loop: Create an unloop msg instance////
    while !running_credit_amount.is_zero() && loops_count < LOOP_MAX {
        //1) Withdraw as much vault token as possible
        let (withdrawable_collateral, withdrawable_value_w_slippage) = calc_withdrawable_collateral(
            config.clone().swap_slippage, 
            vt_token_price.clone(),
            cdt_price.clone(),
            running_collateral_amount,
            running_credit_amount,
        )?;
        //1a) If this withdraw hits the desired_collateral_withdrawal then we stop
        // - We'll have to stop early & withdraw less if its more than the desired_collateral_withdrawal
        if withdrawable_collateral >= desired_collateral_withdrawal.clone() {
            //Early return if we hit the desired collateral withdrawal
            let withdraw_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.cdp_contract_addr.to_string(),
                msg: to_json_binary(&CDP_ExecuteMsg::Withdraw { 
                    position_id: config.cdp_position_id,
                    assets: vec![
                        Asset {
                            info: AssetInfo::NativeToken {
                                denom: config.deposit_token.clone().vault_token,
                            },
                            amount: desired_collateral_withdrawal.clone(),
                        }
                    ],
                    send_to: None,
                })?,
                funds: vec![],
            });
            msgs.push(withdraw_msg);
            //The exit_vault fn handles the exit & withdrawal of the vault tokens to send the deposit_token to the user
                
            return Ok(Response::new()
            .add_attribute("method", "unloop_cdp")
            .add_attribute("withdrawn_collateral", withdrawable_collateral)
            .add_messages(msgs));
        } else {        
            let withdraw_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.cdp_contract_addr.to_string(),
                msg: to_json_binary(&CDP_ExecuteMsg::Withdraw { 
                    position_id: config.cdp_position_id,
                    assets: vec![
                        Asset {
                            info: AssetInfo::NativeToken {
                                denom: config.deposit_token.clone().vault_token,
                            },
                            amount: withdrawable_collateral,
                        }
                    ],
                    send_to: None,
                })?,
                funds: vec![],
            });
            msgs.push(withdraw_msg);
        }
        //2) - Query the amount of deposit tokens we'll receive
        // - Exit the vault
        // - sell the underlying token for CDT
        //Query the amount of deposit tokens we'll receive
        let underlying_deposit_token: Uint128 = match deps.querier.query_wasm_smart::<Uint128>(
            config.deposit_token.vault_addr.to_string(),
            &Vault_QueryMsg::VaultTokenUnderlying { vault_token_amount: withdrawable_collateral },
        ){
            Ok(underlying_deposit_token) => underlying_deposit_token,
            Err(_) => return Err(TokenFactoryError::CustomError { val: String::from("Failed to query the Mars Vault Token for the underlying deposit amount in unloop") }),
        };
        //Exit vault
        let exit_vault_strat =  CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.deposit_token.vault_addr.to_string(),
            msg: to_json_binary(&Vault_ExecuteMsg::ExitVault { })?,
            funds: vec![
                Coin {
                    denom: config.deposit_token.vault_token.clone(),
                    amount: withdrawable_collateral,
                }
            ],
        });
        msgs.push(exit_vault_strat);
        //Sell tokens for CDT
        let sell_deposit_token_for_CDT = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.osmosis_proxy_contract_addr.to_string(),
            msg: to_json_binary(&OP_ExecuteMsg::ExecuteSwaps { 
                token_out: config.cdt_denom.clone(),
                max_slippage: config.swap_slippage,
            })?,
            funds: vec![
                Coin {
                    denom: config.deposit_token.deposit_token.clone(),
                    amount: underlying_deposit_token,
                }
            ],
        });
        msgs.push(sell_deposit_token_for_CDT);
        //3) Repay the CDP loan
        //Calc the minimum amount of CDT received from the router
        let minimum_CDT = cdt_price.get_amount(withdrawable_value_w_slippage)?;
        //Create repay_msg
        let repay_CDP_loan = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.cdp_contract_addr.to_string(),
            msg: to_json_binary(&CDP_ExecuteMsg::Repay { 
                position_id: config.cdp_position_id,
                position_owner: None,
                send_excess_to: None,
            })?,
            funds: vec![
                Coin {
                    denom: config.cdt_denom.clone(),
                    amount: minimum_CDT,
                }
            ],
        });
        msgs.push(repay_CDP_loan);
        //Update running credit amount & collateral amount
        running_credit_amount = match running_credit_amount.checked_sub(minimum_CDT){
            Ok(v) => v,
            Err(_) => return Err(TokenFactoryError::CustomError { val: format!("Failed to subtract running credit amount: {} - {}", running_credit_amount, minimum_CDT) }),
        };
        running_collateral_amount = match running_collateral_amount.checked_sub(withdrawable_collateral){
            Ok(v) => v,
            Err(_) => return Err(TokenFactoryError::CustomError { val: format!("Failed to subtract running collateral amount: {} - {}", running_collateral_amount, withdrawable_collateral) }),
        };
        //4) - We reloop if this instance doesnt hit the desired_collateral_withdrawal OR we have non-zero debt
        loops_count += 1;
    }

    //If we didn't hit the desired collateral withdrawal after LOOP_MAX loops, check again then return an error
        let (withdrawable_collateral, withdrawable_value) = calc_withdrawable_collateral(
            config.clone().swap_slippage, 
            vt_token_price.clone(),
            cdt_price.clone(),
            running_collateral_amount,
            running_credit_amount,
        )?;
        //1a) If this withdraw hits the desired_collateral_withdrawal, we send 
        if withdrawable_collateral >= desired_collateral_withdrawal.clone(){
        //Send the desired collateral withdrawal at the end of the msgs
        let withdraw_msg = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.cdp_contract_addr.to_string(),
            msg: to_json_binary(&CDP_ExecuteMsg::Withdraw { 
                position_id: config.cdp_position_id,
                assets: vec![
                    Asset {
                        info: AssetInfo::NativeToken {
                            denom: config.deposit_token.clone().vault_token,
                        },
                        amount: desired_collateral_withdrawal.clone(),
                    }
                ],
                send_to: None,
            })?,
            funds: vec![],
        });
        msgs.push(withdraw_msg);
        //The exit_vault fn handles the exit & withdrawal of the vault tokens to send the deposit_token to the user

    } else {
        return Err(TokenFactoryError::CustomError { val: format!("Failed to hit the desired collateral withdrawal in unloop, most we can withdraw in 1 tx is {}", withdrawable_collateral) });
    }
    

    //Update config's total non-leveraged vault tokens if we reset to zero
    //This allows us to take into account swap fees & slippage
    if running_credit_amount.is_zero() {
        config.total_nonleveraged_vault_tokens = running_collateral_amount;
        println!("Total Non-Leveraged Vault Tokens: {}", config.total_nonleveraged_vault_tokens);
        //Save the updated config
        CONFIG.save(deps.storage, &config)?;
    }
    
    //Create Response
    let res = Response::new()
        .add_attribute("method", "unloop_cdp")
        .add_attribute("remaining_collateral", running_collateral_amount)
        .add_attribute("remaining_debt", running_credit_amount)
        .add_messages(msgs);

    Ok(res)
}

//Return CP position info
fn get_cdp_position_info(
    deps: Deps,
    env: Env,
    config: Config,
) -> Result<(Uint128,Uint128, PriceResponse, PriceResponse), TokenFactoryError> {
    //Query VT token price
    let prices: Vec<PriceResponse> = match deps.querier.query_wasm_smart::<Vec<PriceResponse>>(
        config.oracle_contract_addr.to_string(),
        &Oracle_QueryMsg::Price {
            asset_info: AssetInfo::NativeToken { denom: config.clone().deposit_token.vault_token },
            twap_timeframe: 0, //We want current swap price
            oracle_time_limit: 0,
            basket_id: None
        },
    ){
        Ok(prices) => prices,
        Err(_) => return Err(TokenFactoryError::CustomError { val: String::from("Failed to query the VT token price in get_cdp_position_info") }),
    };   
    let vt_token_price: PriceResponse = prices[0].clone();
    //Query basket for CDT price
    let basket: Basket = match deps.querier.query_wasm_smart::<Basket>(
        config.cdp_contract_addr.to_string(),
        &CDP_QueryMsg::GetBasket {  },
    ){
        Ok(basket) => basket,
        Err(_) => return Err(TokenFactoryError::CustomError { val: String::from("Failed to query the CDP basket in get_cdp_position_info") }),
    };
    let cdt_price: PriceResponse = basket.credit_price;
    
    //Query the CDP position for the amount of vault tokens we have
    //Query the CDP position for the amount of vault tokens we have as collateral
    let vault_position: Vec<BasketPositionsResponse> = match deps.querier.query_wasm_smart::<Vec<BasketPositionsResponse>>(
        config.cdp_contract_addr.to_string(),
        &CDP_QueryMsg::GetBasketPositions { 
            start_after: None, 
            user: None,
            user_info: Some(UserInfo {
                position_owner: env.contract.address.to_string(),
                position_id: config.cdp_position_id,
            }), 
            limit: None, 
        },
    ){
        Ok(vault_position) => vault_position,
        Err(err) => return Err(TokenFactoryError::CustomError { val: String::from("Failed to query the CDP Position for the vault token amount in get_cdp_position_info:") + &err.to_string() }),
    };
    let vault_position: PositionResponse = vault_position[0].positions[0].clone();

    //Set running credit amount 
    let running_credit_amount = vault_position.credit_amount;
    //Set running collateral amount
    let running_collateral_amount = vault_position.collateral_assets[0].asset.amount;

    Ok((running_credit_amount, running_collateral_amount, vt_token_price, cdt_price))   
}


//We want to loop if looped APR is greater than base APR 
// BUT unloop if looped APR is unprofitable
//Otherwise, returns Ok(()) if unprofitable
//So in this scenario, the unleveraged_apr is the buffer between loops and unloops.
// fn test_7day_apr_profitability(
//     deps: Deps,
//     env: Env,
//     config: Config,
//     is_loop: bool,
// ) -> Result<(), TokenFactoryError> {
//     //Query the vault's APR
//     let apr: APRResponse = query_apr(deps, env)?;

//     //Check that the 7 day APR is unprofitable.
//     //NOTE: The swap fees to loop & unloop is the only reason why we aren't also checking the APR-Cost difference is above the base APR.
//     //Users will withdraw if the base APR is better than the APR-Cost difference

//     let weekly_apr = apr.week_apr.unwrap_or_else(|| Decimal::zero());
//     let monthly_apr = apr.month_apr.unwrap_or_else(|| Decimal::zero());
//     let three_month_apr = apr.three_month_apr.unwrap_or_else(|| Decimal::zero());
//     let unleveraged_apr = decimal_division(weekly_apr, apr.leverage)?;
//     //Rate to lose max slippage in a month & 3 months
//     let month_rate_to_lose_swap_fee = decimal_multiplication(Decimal::percent(12_00), config.swap_slippage)?;
//     let three_month_rate_to_lose_swap_fee = decimal_multiplication(Decimal::percent(4_00), config.swap_slippage)?;



//     //We want to loop if looped APR is greater than base APR
//     // meaning we must *Error* to allow a loop
//     if is_loop && three_month_apr > apr.cost && decimal_subtraction(three_month_apr, apr.cost)? > unleveraged_apr 
//     ||
//     is_loop && apr.three_month_apr.is_none() && monthly_apr > apr.cost && decimal_subtraction(monthly_apr, apr.cost)? > unleveraged_apr 
//     || 
//     is_loop && (apr.three_month_apr.is_none() && apr.month_apr.is_none()) && weekly_apr > apr.cost && decimal_subtraction(weekly_apr, apr.cost)? > unleveraged_apr {
//         return Err(TokenFactoryError::CustomError { val: String::from("Looped APR is more profitable than unleveraged APR") });
//     }
//     //This stops UNLOOPS if the APR is profitable at all
//     //bc unlooping has a cost we don't want to unloop to optimize the APR.
//     else if !is_loop && weekly_apr > apr.cost {
//         return Err(TokenFactoryError::CustomError { val: String::from("Looped 7 day APR is profitable") });
//     }
//     ////////Both the 1 & 3 month need to be unprofitable to unloop/////////
//     //The idea is that yield is a long term game so we'll eat short term unprofitability for long term gains.
//     //Users who can't stomach short term losses will withdraw their funds which: 1) pays users staying & 2) reduces the vaults cost.
//     else if !is_loop && !monthly_apr.is_zero() && apr.cost > monthly_apr && decimal_subtraction(apr.cost, monthly_apr)? < month_rate_to_lose_swap_fee {
//         return Err(TokenFactoryError::CustomError { val: format!("Monthly cost didn't lose the vault {:?} this period so we can't unloop", config.swap_slippage) });
//     }
//     else if !is_loop && !three_month_apr.is_zero() && apr.cost > three_month_apr && decimal_subtraction(apr.cost, three_month_apr)? > three_month_rate_to_lose_swap_fee {
//         return Err(TokenFactoryError::CustomError { val: format!("3 month cost didn't lose the vault {:?} this period so we can't unloop", config.swap_slippage) });
//     }


//     //This return means we are allowing an unloop
//     Ok(())
// }

fn calc_withdrawable_collateral(
    swap_slippage: Decimal,
    vt_price: PriceResponse,
    cdt_price: PriceResponse,
    vault_tokens: Uint128,
    debt: Uint128,
) -> StdResult<(Uint128, Decimal)>{ //withdrawal_amount, withdraw_value
    //Calc the value of the vault tokens
    let vault_tokens_value = vt_price.get_value(vault_tokens)?;
    //Calc the value of the CDT debt
    let debt_value = cdt_price.get_value(debt)?;
    //Calc LTV
    let ltv = decimal_division(debt_value, max(vault_tokens_value, Decimal::one()))?;
    //Calc the distance of the LTV to 90%
    let ltv_space_to_withdraw = match Decimal::percent(90).checked_sub(ltv){
        Ok(v) => v,
        Err(_) => return Err(StdError::GenericErr { msg: format!("LTV over 90%: {} > 0.9", ltv) }),
    };
    //Calc the value of the vault tokens we withdraw
    //It's either clearing the debt or using the LTV space
    let mut withdrawable_value = min(decimal_multiplication(vault_tokens_value, ltv_space_to_withdraw)?, debt_value);

    //If withdrawable_value * slippage puts the debt value below 100 debt, withdraw the difference
    let minimum_debt_value = cdt_price.get_value(Uint128::new(100))?;
    let withdrawal_w_slippage = decimal_multiplication(withdrawable_value, decimal_subtraction(Decimal::one(), swap_slippage)?)?;
    if debt_value > withdrawal_w_slippage && decimal_subtraction(debt_value, withdrawal_w_slippage)? < minimum_debt_value {
        //Calc the difference but add one as a buffer
        let difference = decimal_subtraction(debt_value, minimum_debt_value)? + Decimal::one();
        //Subtract difference from withdrawable_value bc we want to withdraw less
        withdrawable_value = decimal_subtraction(withdrawable_value, difference)?;        
    } 
    
    //Return the amount of vault tokens we can withdraw
    let withdrawable_collateral = vt_price.get_amount(withdrawable_value)?;

    Ok((withdrawable_collateral, withdrawal_w_slippage))
}


///Rate assurance
/// Ensures that the conversion rate is static for deposits & withdrawals
/// We are trusting that Mars deposits will only go up.
fn rate_assurance(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, TokenFactoryError> {
    //Load config    
    let config = CONFIG.load(deps.storage)?;

    //Error if not the contract calling
    if info.sender != env.contract.address {
        return Err(TokenFactoryError::Unauthorized {});
    }

    //Load State
    let token_rate_assurance = TOKEN_RATE_ASSURANCE.load(deps.storage)?;
    let total_vault_tokens = VAULT_TOKEN.load(deps.storage)?;

    //Get total deposit tokens
    let total_deposit_tokens = get_total_deposit_tokens(deps.as_ref(), env.clone(), config)?;

    //Calc the rate of vault tokens to deposit tokens
    let btokens_per_one = calculate_base_tokens(
        Uint128::new(1_000_000_000_000), 
        total_deposit_tokens, 
        total_vault_tokens
    )?;

    //Check that the rates are static 
    if btokens_per_one != token_rate_assurance.pre_btokens_per_one {
        return Err(TokenFactoryError::CustomError { val: format!("Deposit or withdraw rate assurance failed. Deposit tokens per 1 post-tx: {:?} --- pre-tx: {:?}", btokens_per_one, token_rate_assurance.pre_btokens_per_one) });
    }

    Ok(Response::new())
}


/// Accepts USDC or USDT, deposits these to the respective Mars Supply Vault & sends user vault tokens
/// - SubMsg deposits all vault tokens into CDP contract
fn enter_vault(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, TokenFactoryError> {
    //Load State
    let config = CONFIG.load(deps.storage)?;
 
    //Assert the only token sent is the deposit token
    if info.funds.len() != 1 {
        return Err(TokenFactoryError::CustomError { val: format!("More than 1 asset was sent, this function only accepts the deposit token: {:?}", config.clone().deposit_token) });
    }
    if info.funds[0].denom != config.deposit_token.deposit_token {
        return Err(TokenFactoryError::CustomError { val: format!("The wrong asset was sent ({:?}), this function only accepts the deposit token: {:?}", info.funds[0].denom, config.clone().deposit_token) });
    }
    
    //Get the amount of deposit token sent
    let deposit_amount = info.funds[0].amount;

    //////Calculate the amount of vault tokens to mint////
    //Get total deposit tokens
    let total_deposit_tokens = get_total_deposit_tokens(deps.as_ref(), env.clone(), config.clone())?;

    //Get the total amount of vault tokens circulating
    let total_vault_tokens: Uint128 = VAULT_TOKEN.load(deps.storage)?;
    //Calc & save base token rates
    let pre_btokens_per_one = calculate_base_tokens(
        Uint128::new(1_000_000_000_000), 
        total_deposit_tokens, 
        total_vault_tokens
    )?;
    TOKEN_RATE_ASSURANCE.save(deps.storage, &TokenRateAssurance {
        pre_btokens_per_one,
    })?;
    //Calculate the amount of vault tokens to mint
    let vault_tokens_to_distribute = calculate_vault_tokens(
        deposit_amount, 
        total_deposit_tokens, 
        total_vault_tokens
    )?;
    // println!("vault_tokens_to_distribute: {:?}, {}, {}, {}", vault_tokens_to_distribute, total_deposit_tokens, deposit_amount, total_vault_tokens);
    ////////////////////////////////////////////////////

    let mut msgs: Vec<SubMsg> = vec![];
    //Mint vault tokens to the sender
    let mint_vault_tokens_msg: CosmosMsg = TokenFactory::MsgMint {
        sender: env.contract.address.to_string(), 
        amount: Some(osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: config.vault_token.clone(),
            amount: vault_tokens_to_distribute.to_string(),
        }), 
        mint_to_address: info.sender.to_string(),
    }.into();
    //UNCOMMENT
    msgs.push(SubMsg::new(mint_vault_tokens_msg));

    //Update the total token amounts
    VAULT_TOKEN.save(deps.storage, &(total_vault_tokens + vault_tokens_to_distribute))?;
    
    //Save the updated config
    CONFIG.save(deps.storage, &config)?;

    //Send the deposit tokens to the yield strategy
    let send_deposit_to_yield_msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.deposit_token.vault_addr.to_string(),
        msg: to_json_binary(&Vault_ExecuteMsg::EnterVault {  })?,
        funds: vec![Coin {
            denom: config.deposit_token.deposit_token.clone(),
            amount: deposit_amount,
        }],
    });
    msgs.push(
        SubMsg::reply_on_success(send_deposit_to_yield_msg, ENTER_VAULT_REPLY_ID)
    );
    

    //Add rate assurance callback msg
    if !total_deposit_tokens.is_zero() && !total_vault_tokens.is_zero() {
        //UNCOMMENT
        msgs.push(SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance { })?,
            funds: vec![],
        })));
    }

    //Create Response
    let res = Response::new()
        .add_attribute("method", "enter_vault")
        .add_attribute("deposit_amount", deposit_amount)
        .add_attribute("vault_tokens_distributed", vault_tokens_to_distribute)
        .add_attribute("vt_sent_to_cdp", deposit_amount)
        .add_submessages(msgs);

    Ok(res)
}

/// User sends vault_tokens to withdraw the deposit_token from the vault
/// We burn vault tokens & unstake whatever was withdrawn
fn exit_vault(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, TokenFactoryError> {
    let mut config = CONFIG.load(deps.storage)?;
    let mut msgs: Vec<CosmosMsg> = vec![];
    
    //Assert the only token sent is the vault token
    if info.funds.len() != 1 {
        return Err(TokenFactoryError::CustomError { val: format!("More than 1 asset was sent, this function only accepts the vault token: {:?}", config.clone().vault_token) });
    }
    if info.funds[0].denom != config.vault_token {
        return Err(TokenFactoryError::CustomError { val: format!("The wrong asset was sent ({:?}), this function only accepts the vault token: {:?}", info.funds[0].denom, config.clone().vault_token) });
    }

    //Get the amount of vault tokens sent
    let vault_tokens = info.funds[0].amount;
    if vault_tokens.is_zero() {
        return Err(TokenFactoryError::CustomError { val: String::from("Need to send more than 0 vault tokens") });
    }

    //////Calculate the amount of deposit tokens to withdraw////
    //Get total deposit tokens
    let total_deposit_tokens = get_total_deposit_tokens(deps.as_ref(), env.clone(), config.clone())?;
    //Get the total amount of vault tokens circulating
    let total_vault_tokens = VAULT_TOKEN.load(deps.storage)?;
    //Calc & save token rate
    let pre_btokens_per_one = calculate_base_tokens(
        Uint128::new(1_000_000_000_000), 
        total_deposit_tokens, 
        total_vault_tokens
    )?;
    TOKEN_RATE_ASSURANCE.save(deps.storage, &TokenRateAssurance {
        pre_btokens_per_one,
    })?;
    //Calculate the amount of deposit tokens to withdraw
    let deposit_tokens_to_withdraw = calculate_base_tokens(
        vault_tokens, 
        total_deposit_tokens, 
        total_vault_tokens
    )?;
    ////////////////////////////////////////////////////
    
    //Burn vault tokens
    let burn_vault_tokens_msg: CosmosMsg = TokenFactory::MsgBurn {
        sender: env.contract.address.to_string(), 
        amount: Some(osmosis_std::types::cosmos::base::v1beta1::Coin {
            denom: config.vault_token.clone(),
            amount: vault_tokens.to_string(),
        }), 
        burn_from_address: env.contract.address.to_string(),
    }.into();
    //UNCOMMENT
    msgs.push(burn_vault_tokens_msg);

    //Update the total vault tokens
    let new_vault_token_supply = match total_vault_tokens.checked_sub(vault_tokens){
        Ok(v) => v,
        Err(_) => return Err(TokenFactoryError::CustomError { val: format!("Failed to subtract vault token total supply: {} - {}", total_vault_tokens, vault_tokens) }),
    };
    VAULT_TOKEN.save(deps.storage, &new_vault_token_supply)?;

    //Query how many vault tokens we need to withdraw from the CDP Position for the deposit token withdrawal
    let mars_vault_tokens_to_withdraw: Uint128 = match deps.querier.query_wasm_smart::<Uint128>(
        config.deposit_token.vault_addr.to_string(),
        &Vault_QueryMsg::DepositTokenConversion { deposit_token_amount: deposit_tokens_to_withdraw },
    ){
        Ok(vault_tokens) => vault_tokens,
        Err(_) => return Err(TokenFactoryError::CustomError { val: String::from("Failed to query the Mars Vault Token for the deposit token conversion amount") }),
    };
    
    //Update config's tracking of non-leveraged vault tokens
    config.total_nonleveraged_vault_tokens = match config.total_nonleveraged_vault_tokens.checked_sub(mars_vault_tokens_to_withdraw){
        Ok(v) => v,
        Err(_) => return Err(TokenFactoryError::CustomError { val: format!("Failed to subtract config non-leveraged vault tokens: {} - {}", config.total_nonleveraged_vault_tokens, mars_vault_tokens_to_withdraw) }),
    };
    //Save the updated config
    CONFIG.save(deps.storage, &config)?;

    
    //Get the amount of vault tokens in the contract    
    let contract_balance_of_deposit_vault_tokens = deps.querier.query_balance(env.clone().contract.address.to_string(), config.deposit_token.clone().vault_token)?.amount;
    
    //Calc the amount of vt tokens to withdraw from the CDP position
    let vtokens_to_unloop_from_cdp = match mars_vault_tokens_to_withdraw.checked_sub(contract_balance_of_deposit_vault_tokens){
        Ok(v) => v,
        Err(_) => Uint128::zero(), //This means contract balance is larger and we don't need to unloop any extra
    };
    

    //Withdraw tokens from CDP Position
    //..which requires us to unloop
    let unloop_to_withdraw = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: env.contract.address.to_string(),
        msg: to_json_binary(&ExecuteMsg::UnloopCDP { 
            desired_collateral_withdrawal: vtokens_to_unloop_from_cdp,
            })?,
        funds: vec![],
    });
    // println!("deposit_tokens_to_withdraw: {:?}", deposit_tokens_to_withdraw);
    msgs.push(unloop_to_withdraw);

    //Withdraw deposit tokens from the yield strategy
    let withdraw_deposit_tokens_msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.deposit_token.vault_addr.to_string(),
        msg: to_json_binary(&Vault_ExecuteMsg::ExitVault { })?,
        funds: vec![
            Coin {
                denom: config.deposit_token.vault_token.clone(),
                amount: mars_vault_tokens_to_withdraw,
            }
        ],
    });
    msgs.push(withdraw_deposit_tokens_msg);

    //Send the deposit tokens to the user
    let send_deposit_to_user_msg: CosmosMsg = CosmosMsg::Bank(BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: vec![Coin {
            denom: config.deposit_token.deposit_token.clone(),
            amount: deposit_tokens_to_withdraw,
        }],
    });
    msgs.push(send_deposit_to_user_msg);
    
    //Add rate assurance callback msg if this withdrawal leaves other depositors with tokens to withdraw
    if !new_vault_token_supply.is_zero() && total_deposit_tokens > deposit_tokens_to_withdraw {
        msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: env.contract.address.to_string(),
            msg: to_json_binary(&ExecuteMsg::RateAssurance { })?,
            funds: vec![],
        }));
    }

    //Create Response 
    let res = Response::new()
        .add_attribute("method", "exit_vault")
        .add_attribute("vault_tokens", vault_tokens)
        .add_attribute("deposit_tokens_withdrawn", deposit_tokens_to_withdraw)
        .add_messages(msgs);

    Ok(res)
}
 
/// Update contract configuration
/// This function is only callable by an owner with non_token_contract_auth set to true
fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    cdp_contract_addr: Option<String>,
    mars_vault_addr: Option<String>,
    osmosis_proxy_contract_addr: Option<String>,
    oracle_contract_addr: Option<String>,
    withdrawal_buffer: Option<Decimal>,
    deposit_cap: Option<Uint128>,
    swap_slippage: Option<Decimal>,
    vault_cost_index: Option<()>,
) -> Result<Response, TokenFactoryError> {
    let mut config = CONFIG.load(deps.storage)?;

    //Assert Authority
    if info.sender != config.owner {
        //Check if ownership transfer is in progress & transfer if so
        if info.sender == OWNERSHIP_TRANSFER.load(deps.storage)? {
            config.owner = info.sender;
        } else {
            return Err(TokenFactoryError::Unauthorized {});
        }
    }

    let mut attrs = vec![attr("method", "update_config")];
    //Save optionals
    if let Some(addr) = owner {
        let valid_addr = deps.api.addr_validate(&addr)?;

        //Set owner transfer state
        OWNERSHIP_TRANSFER.save(deps.storage, &valid_addr)?;
        attrs.push(attr("owner_transfer", valid_addr));  
    }
    if let Some(addr) = cdp_contract_addr {
        config.cdp_contract_addr = deps.api.addr_validate(&addr)?;
        attrs.push(attr("updated_cdp_contract_addr", addr));
    }
    if let Some(addr) = mars_vault_addr {
        config.deposit_token.vault_addr = deps.api.addr_validate(&addr)?;
        attrs.push(attr("updated_mars_vault_addr", addr));
    }
    if let Some(addr) = osmosis_proxy_contract_addr {
        config.osmosis_proxy_contract_addr = deps.api.addr_validate(&addr)?;
        attrs.push(attr("updated_osmosis_proxy_contract_addr", addr));
    }
    if let Some(addr) = oracle_contract_addr {
        config.oracle_contract_addr = deps.api.addr_validate(&addr)?;
        attrs.push(attr("updated_oracle_contract_addr", addr));
    }
    if let Some(buffer) = withdrawal_buffer {
        config.withdrawal_buffer = buffer;
        attrs.push(attr("updated_withdrawal_buffer", buffer.to_string()));
    }
    if let Some(debt_cap) = deposit_cap {
        config.deposit_cap = debt_cap;
        attrs.push(attr("updated_deposit_cap", debt_cap));
    }
    if let Some(slippage) = swap_slippage {
        config.swap_slippage = slippage;
        attrs.push(attr("updated_swap_slippage", slippage.to_string()));
    }
    if let Some(_) = vault_cost_index {
        //Query the basket to find the index of the vault_token
        let basket: Basket = match deps.querier.query_wasm_smart::<Basket>(
            config.cdp_contract_addr.to_string(),
            &CDP_QueryMsg::GetBasket { },
        ){
            Ok(basket) => basket,
            Err(_) => return Err(TokenFactoryError::CustomError { val: String::from("Failed to query the CDP Basket") }),
        };
        //Find the index
        let mut saved_index: Option<u64> = None;
        for (index, asset) in basket.clone().collateral_types.into_iter().enumerate(){
            if asset.asset.info.to_string() == config.deposit_token.clone().vault_token {
                saved_index = Some(index as u64);
                break;
            }
        }
        if let Some(index) = saved_index {
            config.vault_cost_index = index as usize;
        } else {
            return Err(TokenFactoryError::CustomError { val: String::from("Failed to find the vault token in the CDP Basket") });
        }    
    }
    CONFIG.save(deps.storage, &config)?;
    attrs.push(attr("updated_config", format!("{:?}", config)));

    Ok(Response::new().add_attributes(attrs))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_json_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::VaultTokenUnderlying { vault_token_amount } => to_json_binary(&query_vault_token_underlying(deps, env, vault_token_amount)?),
        QueryMsg::APR {} => to_json_binary(&query_apr(deps, env)?),
    }
}

/// Return APR for the valid durations 7, 30, 90, 365 days using the Mars Vault's APR
/// & cost of the CDP
fn query_apr(
    deps: Deps,
    env: Env,
) -> StdResult<APRResponse> {
    let config = CONFIG.load(deps.storage)?;
    let mut aprs = APRResponse {
        week_apr: None,
        month_apr: None,
        three_month_apr: None,
        year_apr: None,
        leverage: Decimal::zero(),
        cost: Decimal::zero()
    };
    //Get total_deposit_tokens
    // let total_deposit_tokens = get_total_deposit_tokens(deps, env.clone(), config.clone())?;
    
    ////Find the leverage of the contract////
    //Query the collateral of the contract's CDP
    //Query the CDP position for the amount of vault tokens we have
    let vault_position: Vec<BasketPositionsResponse> = match deps.querier.query_wasm_smart::<Vec<BasketPositionsResponse>>(
        config.cdp_contract_addr.to_string(),
        &CDP_QueryMsg::GetBasketPositions { 
            start_after: None, 
            user: None,
            user_info: Some(UserInfo {
                position_owner: env.contract.address.to_string(),
                position_id: config.cdp_position_id,
            }), 
            limit: None, 
        },
    ){
        Ok(vault_position) => vault_position,
        Err(err) => return Err(StdError::GenericErr { msg: String::from("Failed to query the CDP Position for the vault token amount in get_total_deposit_tokens:") + &err.to_string() }),
    };
    let vault_position: PositionResponse = vault_position[0].positions[0].clone();
    //Set running collateral amount
    let vt_collateral_amount = vault_position.collateral_assets[0].asset.amount;
    //Find the leverage by dividing the collateral by the non-leveraged vault tokens
    let leverage = decimal_division(Decimal::from_ratio(vt_collateral_amount, Uint128::one()), Decimal::from_ratio(max(config.total_nonleveraged_vault_tokens, Uint128::one()), Uint128::one()))?;
    aprs.leverage = leverage;
    

    //Get ratio of tokens not in the CDP loop
    let ( ratio_of_tokens_in_contract, _, _, _ ) = get_buffer_amounts(
        deps.querier, 
        config.clone(),
        env.contract.address.to_string(),
    )?;
    let ratio_of_tokens_in_CDP = match Decimal::one().checked_sub(ratio_of_tokens_in_contract){
        Ok(v) => v,
        Err(_) => Decimal::one(),
    };
    
    //Query Mars Vault APR
    let apr: NoCost_APRResponse = match deps.querier.query_wasm_smart::<NoCost_APRResponse>(
        config.deposit_token.vault_addr.to_string(),
        &Vault_QueryMsg::APR { },
    ){
        Ok(apr) => apr,
        Err(_) => return Err(StdError::GenericErr { msg: format!("Failed to query the APR in query_apr") }),
    };
    //Add our leverage to the APR & buffered tokens to the APR
    if let Some(week_apr) = apr.week_apr {
        let apr = match week_apr.checked_mul(leverage){
            Ok(v) => v,
            Err(_) => return Err(StdError::GenericErr { msg: String::from("Failed to multiply the weekly APR by the leverage in query_apr") }),
        };
        let loop_apr = match decimal_multiplication(ratio_of_tokens_in_CDP, apr){
            Ok(v) => v,
            Err(_) => return Err(StdError::GenericErr { msg: String::from("Failed to multiply the weekly APR by the ratio of tokens in the CDP in query_apr") }),
        };
        let buffer_apr = match decimal_multiplication(ratio_of_tokens_in_contract, week_apr){
            Ok(v) => v,
            Err(_) => return Err(StdError::GenericErr { msg: String::from("Failed to multiply the weekly APR by the ratio of tokens in the contract in query_apr") }),
        };
        aprs.week_apr = Some(loop_apr.checked_add(buffer_apr)?);
    }
    if let Some(month_apr) = apr.month_apr {
        let apr = match month_apr.checked_mul(leverage){
            Ok(v) => v,
            Err(_) => return Err(StdError::GenericErr { msg: String::from("Failed to multiply the monthly APR by the leverage in query_apr") }),
        };
        let loop_apr = match decimal_multiplication(ratio_of_tokens_in_CDP, apr){
            Ok(v) => v,
            Err(_) => return Err(StdError::GenericErr { msg: String::from("Failed to multiply the monthly APR by the ratio of tokens in the CDP in query_apr") }),
        };
        let buffer_apr = match decimal_multiplication(ratio_of_tokens_in_contract, month_apr){
            Ok(v) => v,
            Err(_) => return Err(StdError::GenericErr { msg: String::from("Failed to multiply the monthly APR by the ratio of tokens in the contract in query_apr") }),
        };
        aprs.month_apr = Some(loop_apr.checked_add(buffer_apr)?);
    }
    if let Some(three_month_apr) = apr.three_month_apr {
        let apr = match three_month_apr.checked_mul(leverage){
            Ok(v) => v,
            Err(_) => return Err(StdError::GenericErr { msg: String::from("Failed to multiply the 3 month APR by the leverage in query_apr") }),
        };
        let loop_apr = match decimal_multiplication(ratio_of_tokens_in_CDP, apr){
            Ok(v) => v,
            Err(_) => return Err(StdError::GenericErr { msg: String::from("Failed to multiply the 3 month APR by the ratio of tokens in the CDP in query_apr") }),
        };
        let buffer_apr = match decimal_multiplication(ratio_of_tokens_in_contract, three_month_apr){
            Ok(v) => v,
            Err(_) => return Err(StdError::GenericErr { msg: String::from("Failed to multiply the 3 month APR by the ratio of tokens in the contract in query_apr") }),
        };
        aprs.three_month_apr = Some(loop_apr.checked_add(buffer_apr)?);
    }
    if let Some(year_apr) = apr.year_apr {
        let apr = match year_apr.checked_mul(leverage){
            Ok(v) => v,
            Err(_) => return Err(StdError::GenericErr { msg: String::from("Failed to multiply the yearly APR by the leverage in query_apr") }),
        };
        let loop_apr = match decimal_multiplication(ratio_of_tokens_in_CDP, apr){
            Ok(v) => v,
            Err(_) => return Err(StdError::GenericErr { msg: String::from("Failed to multiply the yearly APR by the ratio of tokens in the CDP in query_apr") }),
        };
        let buffer_apr = match decimal_multiplication(ratio_of_tokens_in_contract, year_apr){
            Ok(v) => v,
            Err(_) => return Err(StdError::GenericErr { msg: String::from("Failed to multiply the yearly APR by the ratio of tokens in the contract in query_apr") }),
        };
        aprs.year_apr = Some(loop_apr.checked_add(buffer_apr)?);
    }

    //Query the cost of the deposit vault's vault token
    let basket_interest: CollateralInterestResponse = match deps.querier.query_wasm_smart::<CollateralInterestResponse>(
        config.cdp_contract_addr.to_string(),
        &CDP_QueryMsg::GetCollateralInterest {  },
    ){
        Ok(basket_interest) => basket_interest,
        Err(_) => return Err(StdError::GenericErr { msg: String::from("Failed to query the CDP collateral interest rates in query_apr") }),
    };
    let vt_cost: Decimal = basket_interest.rates[config.vault_cost_index].clone();
    //Set cost
    aprs.cost = vt_cost;

    Ok(aprs)
}

/// Return underlying deposit token amount for an amount of vault tokens
fn query_vault_token_underlying(
    deps: Deps,
    env: Env,
    vault_token_amount: Uint128,
) -> StdResult<Uint128> {
    let config = CONFIG.load(deps.storage)?;
    let total_vault_tokens = VAULT_TOKEN.load(deps.storage)?;
    
    //Get total deposit tokens
    let total_deposit_tokens = get_total_deposit_tokens(deps, env.clone(), config.clone())?;
    //Calc the amount of deposit tokens the user owns pre-discount
    let users_base_tokens = calculate_base_tokens(
        vault_token_amount, 
        total_deposit_tokens, 
        total_vault_tokens
    )?;

    //Return the discounted amount
    Ok(users_base_tokens)
}

//This calcs the value of tokens backing the CDP minus the debt minus swap fees if the debt is non-zero
// The swap fees won't be detracted if the withdrawal is within the buffer
//..but calculate it anyway as a reward for the people not withdrawing (if the CDP is open with debt)
fn get_total_deposit_tokens(
    deps: Deps,
    env: Env,
    config: Config,
) -> StdResult<Uint128> {
    //Get CDT price
    let basket: Basket = match deps.querier.query_wasm_smart::<Basket>(
        config.cdp_contract_addr.to_string(),
        &CDP_QueryMsg::GetBasket {  },
    ){
        Ok(basket) => basket,
        Err(_) => return Err(StdError::GenericErr { msg: String::from("Failed to query the CDP basket in get_total_deposit_tokens") }),
    };
    let cdt_price: PriceResponse = basket.credit_price;
    //Get vault token price
    let prices: Vec<PriceResponse> = match deps.querier.query_wasm_smart::<Vec<PriceResponse>>(
        config.oracle_contract_addr.to_string(),
        &Oracle_QueryMsg::Price {
            asset_info: AssetInfo::NativeToken{ denom: config.clone().deposit_token.vault_token },
            twap_timeframe: 60, //We want the price the CDP will use
            oracle_time_limit: 600,
            basket_id: None
        },
    ){
        Ok(prices) => prices,
        Err(_) => return Err(StdError::GenericErr { msg: String::from("Failed to query the VT token price in get_total_deposit_tokens") }),
    };
    let vt_token_price: PriceResponse = prices[0].clone();
    //Query the CDP position for the amount of vault tokens we have as collateral
    let vault_position: Vec<BasketPositionsResponse> = match deps.querier.query_wasm_smart::<Vec<BasketPositionsResponse>>(
        config.cdp_contract_addr.to_string(),
        &CDP_QueryMsg::GetBasketPositions { 
            start_after: None, 
            user: None,
            user_info: Some(UserInfo {
                position_owner: env.contract.address.to_string(),
                position_id: config.cdp_position_id,
            }), 
            limit: None, 
        },
    ){
        Ok(vault_position) => vault_position,
        Err(err) => return Err(StdError::GenericErr { msg: String::from("Failed to query the CDP Position for the vault token amount in get_total_deposit_tokens:") + &err.to_string() }),
    };
    let vault_position: PositionResponse = vault_position[0].positions[0].clone();
    //Calc value of the debt
    let debt_value = cdt_price.get_value(vault_position.credit_amount)?;
    //Calc value of the collateral
    let collateral_value = vt_token_price.get_value(vault_position.collateral_assets[0].asset.amount)?;
    //Calc the value of the collateral minus the debt
    let mut liquid_value = match collateral_value.checked_sub(debt_value){
        Ok(v) => v,
        Err(_) => return Err(StdError::GenericErr { msg: format!("Failed to subtract the debt from the collateral in get_total_deposit_tokens, collateral value: {}, debt value: {}", collateral_value, debt_value) }),
    };
    //Only deduct slippage costs if the position has debt 
    if !debt_value.is_zero() {
        //Calc value minus slippage
        liquid_value = decimal_multiplication(liquid_value, decimal_subtraction(Decimal::one(), config.swap_slippage)?)?;
    }    
    //Calc the amount of vaulted deposit tokens
    let mut total_vaulted_deposit_tokens = vt_token_price.get_amount(liquid_value)?;

    //Query the contract balance for the amount of buffered vault tokens
    let vt_buffer = match deps.querier.query_balance(env.contract.address.to_string(), config.deposit_token.vault_token){
        Ok(balance) => balance.amount,
        Err(_) => Uint128::zero(),
    };    
    //Add buffered assets to the total deposit tokens
    total_vaulted_deposit_tokens += vt_buffer;

    //Query the underlying of the initial vault token deposit
    let underlying_deposit_token: Uint128 = match deps.querier.query_wasm_smart::<Uint128>(
        config.deposit_token.vault_addr.to_string(),
        &Vault_QueryMsg::VaultTokenUnderlying { vault_token_amount: total_vaulted_deposit_tokens },
    ){
        Ok(underlying_deposit_token) => underlying_deposit_token,
        Err(_) => return Err(StdError::GenericErr { msg: String::from("Failed to query the Mars Vault Token for the underlying deposit amount in instantiate") }),
    };

    Ok(underlying_deposit_token)
    
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(deps: DepsMut, env: Env, msg: Reply) -> StdResult<Response> {
    match msg.id {
        ENTER_VAULT_REPLY_ID => handle_enter_reply(deps, env, msg),
        CDP_REPLY_ID => handle_cdp_reply(deps, env, msg),
        LOOP_REPLY_ID => handle_loop_reply(deps, env, msg),
        id => Err(StdError::generic_err(format!("invalid reply id: {}", id))),
    }
} 


fn handle_loop_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(result) => {
            //Load config
            let config = CONFIG.load(deps.storage)?;  
            let mut msgs = vec![];
            panic!("start of swap reply: {:?}, {:?}, {:?}", env.contract.address.to_string(), config.clone().deposit_token.deposit_token, config.clone().deposit_token.vault_addr);
               
            //Query balances for the deposit token received from the swap
            let deposit_token_amount = match deps.querier.query_balance(env.contract.address.to_string(), config.clone().deposit_token.deposit_token){
                Ok(balance) => balance.amount,
                Err(_) => match deps.querier.query_all_balances(env.contract.address.to_string()).unwrap().iter().find(|coin| coin.denom == config.clone().deposit_token.deposit_token){
                    Some(coin) => coin.amount,
                    None => return Err(StdError::GenericErr { msg: String::from("Failed to query the deposit token amount in loop") }),
                },
            };
            
            //Query how many vault tokens we'll get for this deposit
            let vault_tokens: Uint128 = match deps.querier.query_wasm_smart::<Uint128>(
                config.deposit_token.vault_addr.to_string(),
                &Vault_QueryMsg::DepositTokenConversion { deposit_token_amount: deposit_token_amount.clone() },
            ){
                Ok(vault_tokens) => vault_tokens,
                Err(_) => return Err(StdError::GenericErr { msg: String::from("Failed to query the Mars Vault Token for the new deposit amount in loop") }),
            };
            
            //Create enter into vault msg
            let deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.deposit_token.vault_addr.to_string(),
                msg: to_json_binary(&Vault_ExecuteMsg::EnterVault { })?,
                funds: vec![
                    Coin {
                        denom: config.deposit_token.deposit_token.clone(),
                        amount: deposit_token_amount,
                    }
                ],
            });
            msgs.push(deposit_msg);
                     
            //Deposit any excess vtokens into the CDP
            let ( _, _, _, vt_sent_to_cdp ) = get_buffer_amounts(
                deps.querier, 
                config.clone(),
                env.contract.address.to_string(),
            )?;

            //Create deposit msg
            let deposit_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.cdp_contract_addr.to_string(),
                msg: to_json_binary(&CDP_ExecuteMsg::Deposit { 
                    position_id: Some(config.cdp_position_id),
                    position_owner: None,
                })?,
                funds: vec![
                    Coin {
                        denom: config.deposit_token.clone().vault_token,
                        amount: vault_tokens + vt_sent_to_cdp,
                    }
                ],
            });
            msgs.push(deposit_msg);
            
            //Add post loop maintenance msg 
            let post_loop_msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: env.contract.address.to_string(),
                msg: to_json_binary(&ExecuteMsg::PostLoopMaintenance { })?,
                funds: vec![],
            });
            msgs.push(post_loop_msg);

            //Create Response
            let res = Response::new()
                .add_attribute("method", "handle_loop_reply")
                .add_attribute("deposit_tokens_swapped_for", deposit_token_amount)
                .add_attribute("vault_tokens_sent_to_cdp", vault_tokens + vt_sent_to_cdp)
                .add_messages(msgs);

            return Ok(res);

        } //We only reply on success
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }
}

fn handle_cdp_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(result) => {
            let cdp_event = result
                .events
                .into_iter()
                .find(|e| e.attributes.iter().any(|attr| attr.key == "position_id"))
                .ok_or_else(|| StdError::GenericErr {  msg: String::from("unable to find cdp deposit event")})?;

                let position_id = &cdp_event
                .attributes
                .iter()
                .find(|attr| attr.key == "position_id")
                .unwrap()
                .value;
                let position_id = Uint128::from_str(position_id)?;
            //Load config
            let mut config = CONFIG.load(deps.storage)?;  
            //Save the position ID
            config.cdp_position_id = position_id;
            //Save Updated Config
            CONFIG.save(deps.storage, &config)?;


            //Set redemptions for the position
            let set_redemptions_msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.cdp_contract_addr.to_string(),
                msg: to_json_binary(&CDP_ExecuteMsg::EditRedeemability { 
                    position_ids: (vec![position_id]),
                    redeemable: Some(true), 
                    //Bc if we set it at 99% where the loop price floor is, redemptions will be unprofitable anytime we loop within (max_slippage) of the floor
                    premium: Some(2),  
                    max_loan_repayment: Some(Decimal::one()), 
                    restricted_collateral_assets: None 
                })?,
                funds: vec![],
            });

            //Create Response
            let res = Response::new()
                .add_attribute("method", "handle_initial_cdp_deposit_reply")
                .add_attribute("vault_position_id", position_id)
                .add_message(set_redemptions_msg);  

            return Ok(res);

        } //We only reply on success
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }
}

/// - Add the vault tokens received from the vault deposit into config state
/// - Deposit all vault tokens into CDP contract
fn handle_enter_reply(
    deps: DepsMut,
    env: Env,
    msg: Reply,
) -> StdResult<Response> {
    match msg.result.into_result() {
        Ok(result) => {
            //Load config
            let mut config = CONFIG.load(deps.storage)?;  
            let mut msgs = vec![];
            
            let ( _, contract_balance_of_deposit_vault_tokens, vt_kept, vt_sent_to_cdp ) = get_buffer_amounts(
                deps.querier, 
                config.clone(),
                env.contract.address.to_string(),
            )?;

            //Update the total deposit tokens
            config.total_nonleveraged_vault_tokens += contract_balance_of_deposit_vault_tokens;
            //Save Updated Config
            CONFIG.save(deps.storage, &config)?;

            
            //Deposit everything to the CDP Position
            if !vt_sent_to_cdp.is_zero() {
                let send_deposit_to_yield_msg: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.cdp_contract_addr.to_string(),
                    msg: to_json_binary(&CDP_ExecuteMsg::Deposit { 
                        position_id: Some(config.cdp_position_id),
                        position_owner: None,
                    })?,
                    funds: vec![Coin {
                        denom: config.deposit_token.clone().vault_token,
                        amount: vt_sent_to_cdp
                    }],
                });
                msgs.push(send_deposit_to_yield_msg);
            }

            //Create Response
            let res = Response::new()
                .add_attribute("method", "handle_enter_vault_reply")
                .add_attribute("vault_tokens_sent_to_cdp", vt_sent_to_cdp)
                .add_attribute("vault_tokens_kept", vt_kept)
                .add_messages(msgs);

            return Ok(res);

        } //We only reply on success
        Err(err) => return Err(StdError::GenericErr { msg: err }),
    }
}

fn get_buffer_amounts(
    querier: QuerierWrapper,
    config: Config,
    contract_address: String,
) -> StdResult<(Decimal, Uint128, Uint128, Uint128)> {
    /////Send the vault tokens to the yield strategy///
    //Find the vault token balance in the contract
    let contract_balance_of_deposit_vault_tokens = querier.query_balance(contract_address, config.deposit_token.clone().vault_token)?.amount;

    //Calculate ratio of vault tokens in the contract to the total vault tokens
    let ratio_of_tokens_in_contract = decimal_division(Decimal::from_ratio(contract_balance_of_deposit_vault_tokens, Uint128::one()), Decimal::from_ratio(max(config.total_nonleveraged_vault_tokens, Uint128::one()), Uint128::one()))?;
    
    //Calculate what is sent and what is kept
    let mut vt_sent_to_cdp: Uint128 = Uint128::zero();
    let mut vt_kept: Uint128 = Uint128::zero();            
    let desired_ratio_tokens = decimal_multiplication(Decimal::from_ratio(config.total_nonleveraged_vault_tokens, Uint128::one()), config.withdrawal_buffer)?;

    //If the ratio of tokens in the contract is less than the withdrawal_buffer, calculate the amount of vault tokens to send to the yield strategy
    if ratio_of_tokens_in_contract < config.withdrawal_buffer {
        //Calculate the amount of vault tokens that would make the ratio equal to the withdrawal buffer
        let tokens_to_fill_ratio = desired_ratio_tokens.to_uint_floor() - contract_balance_of_deposit_vault_tokens;
        //How much do we send to the yield strategy
        if tokens_to_fill_ratio >= contract_balance_of_deposit_vault_tokens {
            vt_kept = contract_balance_of_deposit_vault_tokens;
        } else {
            vt_sent_to_cdp = contract_balance_of_deposit_vault_tokens - tokens_to_fill_ratio;
            vt_kept = tokens_to_fill_ratio;
        }
    } else
    //If the ratio to keep is past the threshold then calculate how much to send to the CDP
    {
        //Calculate the amount of vault tokens that would make the ratio equal to the withdrawal buffer
        let tokens_to_lower_ratio = contract_balance_of_deposit_vault_tokens - desired_ratio_tokens.to_uint_floor() ;
        //How much do we send to the yield strategy
        if tokens_to_lower_ratio >= contract_balance_of_deposit_vault_tokens {
            vt_sent_to_cdp = contract_balance_of_deposit_vault_tokens;
        } else {
            vt_kept = contract_balance_of_deposit_vault_tokens - tokens_to_lower_ratio;
            vt_sent_to_cdp = tokens_to_lower_ratio;
        }                
    }

    Ok(( ratio_of_tokens_in_contract, contract_balance_of_deposit_vault_tokens, vt_kept, vt_sent_to_cdp ))
}


#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(deps: DepsMut, env: Env, _msg: MigrateMsg) -> Result<Response, TokenFactoryError> {
    // //Load the config
    // let config = CONFIG.load(deps.storage)?;
    // //Query the underlying of the initial vault token deposit
    // let underlying_deposit_token: Uint128 = match deps.querier.query_wasm_smart::<Uint128>(
    //     config.deposit_token.vault_addr.to_string(),
    //     &Vault_QueryMsg::VaultTokenUnderlying { vault_token_amount: Uint128::new(1_000_000_000_000) },
    // ){
    //     Ok(underlying_deposit_token) => underlying_deposit_token,
    //     Err(_) => return Err(TokenFactoryError::CustomError { val: String::from("Failed to query the Mars Vault Token for the underlying deposit amount in instantiate") }),
    // };

    // //Set the initial vault token amount from the initial deposit
    // let vault_tokens_to_distribute = calculate_vault_tokens(
    //     underlying_deposit_token,
    //     Uint128::zero(), 
    //     Uint128::zero()
    // )?;
    // VAULT_TOKEN.save(deps.storage, &vault_tokens_to_distribute)?;

    // //Mint vault tokens to the sender
    // let mint_vault_tokens_msg: CosmosMsg = TokenFactory::MsgMint {
    //     sender: env.contract.address.to_string(), 
    //     amount: Some(osmosis_std::types::cosmos::base::v1beta1::Coin {
    //         denom: config.vault_token.clone(),
    //         amount: vault_tokens_to_distribute.to_string(),
    //     }), 
    //     mint_to_address: "osmo13gu58hzw3e9aqpj25h67m7snwcjuccd7v4p55w".to_string(),
    // }.into();

    Ok(Response::default())
}