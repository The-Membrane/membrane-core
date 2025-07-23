use std::cmp::{max, min, Ordering};
use std::str::FromStr;

use cosmwasm_std::{attr, to_json_binary, Addr, Api, CosmosMsg, Decimal, DepsMut, Env, MessageInfo, Order, QuerierWrapper, Response, StdError, StdResult, Storage, Uint128, WasmMsg};

use membrane::managed_market::{Config, ExecuteMsg, MarketParams};
use membrane::stability_pool_vault::calculate_vault_tokens;
use membrane::system_discounts::{QueryMsg as DiscountQueryMsg, UserDiscountResponse};
use membrane::types::{cAsset, Basket, UserPosition, Rate, SupplyCap};
use membrane::helpers::get_asset_liquidity;
use membrane::math::{decimal_multiplication, decimal_division, decimal_subtraction};
use osmosis_std::types::osmosis::tokenfactory::v1beta1::{self as TokenFactory};

use crate::positions::{get_total_debt_tokens, get_total_vault_tokens, query_markets_manager_fee};
use crate::ContractError;
use crate::state::{CONFIG, DEBT_VAULT_TOKEN, JUNIOR_DEBT_VAULT_TOKEN, MARKET_PARAMS, POSITIONS};

//Constants
pub const SECONDS_PER_YEAR: u64 = 31_536_000u64;
const MINIMUM_LIQUIDITY: Uint128 = Uint128::new(2_000_000_000_000u128);

/// Accrue interest for a list of Positions
pub fn external_accrue_call(
    storage: &mut dyn Storage,
    api: &dyn Api,
    querier: QuerierWrapper,
    info: MessageInfo,
    env: Env,
    position_owner: String,
    collateral_denom: String,
) -> Result<Response, ContractError>{
    let mut config = CONFIG.load(storage)?;

    //Get the position owner
    let valid_position_owner = api.addr_validate(&position_owner)?;

    //Get the user position
    let mut user_position = POSITIONS.load(storage, (valid_position_owner.clone(), collateral_denom.clone()))?;
        

    //Get total vault tokens
    let total_vault_tokens = DEBT_VAULT_TOKEN.load(storage)?;

    //Initialize msgs
    let mut msgs: Vec<CosmosMsg> = vec![];

    //Previous debt amount
    let previous_debt_amount = user_position.debt_amount;
    

    //Get markets manager fee
    let markets_manager_fee = query_markets_manager_fee(querier, config.markets_manager_contract.to_string())?;
    //Accrue interest
    accrue(
        storage, 
        env.clone(),            
        &mut config.clone(),
        &mut user_position,
        &mut msgs,
        markets_manager_fee
    )?;

    //Save the updated config
    CONFIG.save(storage, &config)?;
    //Save the updated user position
    POSITIONS.save(storage, (valid_position_owner.clone(), collateral_denom.clone()), &user_position)?;

    //Calc the accrued interest
    let accrued_interest = user_position.debt_amount
        .checked_sub(previous_debt_amount)
        .map_err(|_| StdError::generic_err("Accrued interest is negative"))?;



    Ok(Response::new()
        .add_attributes(vec![
            attr("method", "accrue"),
            attr("position_owner", position_owner),
            attr("collateral_denom", collateral_denom),
            attr("accrued_interest", accrued_interest),
        ]).add_messages(msgs))
}

pub fn accumulate_interest_dec(decimal: Decimal, rate: Decimal, time_elapsed: u64) -> StdResult<Decimal> {

    let applied_rate = rate.checked_mul(Decimal::from_ratio(
        Uint128::from(time_elapsed),
        Uint128::from(SECONDS_PER_YEAR),
    ))?;

    decimal_multiplication(decimal, applied_rate)
}

pub fn get_interest_rate(
    market: MarketParams,
    config: Config,
) -> Result<Decimal, ContractError> {
    if let Some(rate_kink) = market.rate_params.rate_kink.clone() {
        // Kink params present: use utilization-based logic
        let debt_utilization_rate = decimal_division(
            Decimal::from_ratio(market.total_borrowed, Uint128::one()),
            Decimal::from_ratio(config.total_debt_tokens, Uint128::one()),
        )?;

        let kink = rate_kink.kink_starting_point_ratio;
        let pre_kink_rate = {
            let percent_into_kink = decimal_division(
                debt_utilization_rate,
                kink,
            )?;
            decimal_multiplication(
                market.rate_params.base_rate,
                percent_into_kink,
            )?
        };

        let kinked_rate = match debt_utilization_rate.checked_sub(kink) {
            Ok(percent_over_kink) => {
                let rate_multiplier = rate_kink.rate_mulitplier;
                let rate_to_add = decimal_multiplication(
                    percent_over_kink,
                    rate_multiplier,
                )?;
                pre_kink_rate.checked_add(rate_to_add)
                    .map_err(|_| ContractError::CustomError { val: format!("Failed to add rate: {} + {}", pre_kink_rate, rate_to_add) })?
            },
            Err(_) => pre_kink_rate,
        };

        Ok(min(kinked_rate, market.rate_params.rate_max))
    } else {
        // No kink params: fixed rate
        Ok(market.rate_params.base_rate)
    }
}

/// Get all market collateral types.
/// Inefficient: This loads the collateral types from the market params to then load them again later in a for loop.
pub fn get_market_collateral_types(
    storage: &mut dyn Storage
) -> Result<Vec<String>, ContractError> {

    let market_collateral: Vec<String> = MARKET_PARAMS
        .range(storage, None, None, Order::Ascending)
        .map(|item| {
            let (k, _) = item?;
            Ok(k)
        })
        .collect::<Result<Vec<String>, ContractError>>()?;

    Ok(market_collateral)
}

/// Accrue interest.
/// WARNING: The senior tranche will not accrue its target rate if...
/// either of the market's interest rates are below the target rate * 1 + (percentage supply surplus)
pub fn accrue(
    storage: &mut dyn Storage,
    env: Env,
    config: &mut Config,
    user_position: &mut UserPosition,
    msgs: &mut Vec<CosmosMsg>,
    fee_to_membrane: Decimal
) -> Result<(), ContractError> {
    //Early return if we have no debt tokens
    if config.total_debt_tokens.is_zero() && config.junior_debt_info.clone().unwrap().total_debt.is_zero() {
        return Ok(());
    }
    //println!("get_total_debt_tokens(config.clone(), Some(true))?: {}", get_total_debt_tokens(config.clone(), Some(true))?);

    //Calc Time-elapsed and update last_Accrued
    let mut time_elapsed = env.block.time.seconds() - config.global_rate_index.last_accrued;
    //Cap the time elapsed to 30 days
    // - This is to prevent the rate index from growing too large if the contract is inactive for a long time
    // - This is a bit arbitrary, but we can change it later if needed
    let max_time = 3600 * 24 * 30; // 30 days
    time_elapsed = min(time_elapsed, max_time);

    if config.global_rate_index.last_accrued == 0 {
        time_elapsed = 0;
    }
    config.global_rate_index.last_accrued = env.block.time.seconds();
    ///////////////////////////////////////////////////
    /// 
    //Map through ALL markets to accrue interest to the Market index & the global index
    // - Market index changes will accrue user debt while global index changes will accrue to the supplied debt
    // - If the user is in the market, we update their rate_index

    // - Either way, we update the market's rate_index & the global rate_index




    //Initialize manager revenue 
    let mut manager_revenue = Uint128::zero();
    let mut membrane_revenue = Uint128::zero();
    //Get Junior Vault Token Supply
    let junior_vault_token_supply = get_total_vault_tokens(storage, true)?;

    //Map through all markets to get the market rate index
    let global_collateral = get_market_collateral_types(storage)?;
    for market_collateral in global_collateral {
        //Get the market params
        let mut market_params = MARKET_PARAMS.load(storage, market_collateral.clone())?;
        

        /////Accrue interest to the debt/////
        
        
        /// Get interest rate //////
        let interest_rate = get_interest_rate(market_params.clone(), config.clone())?;
        //No negative rates
        if interest_rate < Decimal::zero() {
            return Err(ContractError::CustomError { val: format!("Interest rate is negative: {}", interest_rate) });
        }
        ////////////////////


        //Accumulate rate on the rate_index
        // let accrued_rate = accumulate_interest_dec(
        //     config.global_rate_index.rate_index,
        //     interest_rate,
        //     time_elapsed,
        // )?;
        // config.global_rate_index.rate_index += accrued_rate;
        //Accumulate rate on the market's rate_index
        let market_rate = accumulate_interest_dec(
            market_params.market_rate_index.rate_index,
            interest_rate,
            time_elapsed,
        )?;
        market_params.market_rate_index.rate_index += market_rate;


        //If the user is in THIS market, update their rate index & debt amount
        if user_position.collateral_denom == market_collateral {
            //If user's rate index is zero, we set it to the market's rate index
            if user_position.rate_index.is_zero() || user_position.debt_amount.is_zero() {
                user_position.rate_index = market_params.market_rate_index.rate_index;
            }
            //Calc rate_of_change for the position's credit amount
            let debt_rate_of_change = decimal_division(market_params.market_rate_index.rate_index, user_position.rate_index)?;
            //Update user's rate_index
            user_position.rate_index =  market_params.market_rate_index.rate_index;
            
            //Calc new_credit_amount
            let new_credit_amount = decimal_multiplication(
                Decimal::from_ratio(user_position.debt_amount, Uint128::one()), 
                debt_rate_of_change
            )?.to_uint_floor();

            if new_credit_amount > user_position.debt_amount {
                //Set position's debt to the debt + accrued_interest
                user_position.debt_amount = new_credit_amount;
            }
        }
        //println!("market_params.total_borrowed: {}", market_params.total_borrowed);
        //println!("market_rate: {}", market_rate);
        //Calculate market's total accrued interest
        let market_new_credit_amount = decimal_multiplication(
            Decimal::from_ratio(market_params.total_borrowed, Uint128::one()),
            market_rate
        )?.to_uint_floor();
        //println!("market_new_credit_amount: {}", market_new_credit_amount);
        //println!("market_params.total_borrowed: {}", market_params.total_borrowed);
        let total_accrued_interest = market_new_credit_amount;

        //Add accrued interest to market's total borrowed
        if !total_accrued_interest.is_zero() {
            market_params.total_borrowed += market_new_credit_amount;
        }

        //println!("get_total_debt_tokens(config.clone(), Some(true))?: {}", get_total_debt_tokens(config.clone(), Some(true))?);
        //Distribute yield between senior and junior tranches
        distribute_yield(config, total_accrued_interest, time_elapsed, market_params.total_borrowed, get_total_vault_tokens(storage, false)?, junior_vault_token_supply)?;

        //println!("get_total_debt_tokens(config.clone(), Some(true))?: {}", get_total_debt_tokens(config.clone(), Some(true))?);
        //println!("total_accrued_interest: {}", total_accrued_interest);

        //Calc manager revenue
        if !total_accrued_interest.is_zero() {
            let manager_fee = decimal_multiplication(
                Decimal::from_ratio(total_accrued_interest, Uint128::one()),
                config.manager_fee,
            )?;
            manager_revenue += manager_fee.to_uint_floor();

            //Calc revenue to membrane
            let membrane_fee = decimal_multiplication(
                Decimal::from_ratio(total_accrued_interest, Uint128::one()),
                fee_to_membrane,
            )?;
            membrane_revenue += membrane_fee.to_uint_floor();
        }

        //Save the updated market params
        MARKET_PARAMS.save(storage, market_collateral.clone(), &market_params)?;
    }



    //println!("manager_revenue: {}", manager_revenue);
    //println!("membrane_revenue: {}", membrane_revenue);

    /////Managers get Junior, Membrane gets Senior/////
    //Calculate the amount of vault tokens to mint to the manager as the fee
    if manager_revenue > Uint128::zero() || membrane_revenue > Uint128::zero() {
        //////////Manager Revenue//////////
        /// 
        //println!("junior_vault_token_supply: {}", junior_vault_token_supply);
        println!("manager_revenue: {}", manager_revenue);
        //println!("config {:?}", config);
        //println!("get_total_debt_tokens(config.clone(), Some(true))?: {}", get_total_debt_tokens(config.clone(), Some(true))?);
        let vt_to_mint_to_manager = calculate_vault_tokens(
            manager_revenue, 
            get_total_debt_tokens(config.clone(), Some(true))?, 
            junior_vault_token_supply
        )?;

        //println!("vt_to_mint_to_manager: {}", vt_to_mint_to_manager);
        //Mint the vault tokens to the manager
        if !vt_to_mint_to_manager.is_zero() {
            let mint_vault_tokens_msg: CosmosMsg = TokenFactory::MsgMint {
                sender: env.contract.address.to_string(), 
                amount: Some(osmosis_std::types::cosmos::base::v1beta1::Coin {
                    denom: config.junior_debt_supply_vault_token.clone().unwrap(),
                    amount: vt_to_mint_to_manager.to_string(),
                }), 
                mint_to_address: config.owner.clone().to_string(),
            }.into();
            msgs.push(mint_vault_tokens_msg);
        }


        //Update vault token supply
        let new_vault_token_supply = match junior_vault_token_supply.checked_add(vt_to_mint_to_manager){
            Ok(v) => v,
            Err(_) => return Err(ContractError::CustomError { val: format!("Failed to add vault token total supply: {} + {}", junior_vault_token_supply, vt_to_mint_to_manager) }),
        };

        //Save the updated junior vault token supply
        JUNIOR_DEBT_VAULT_TOKEN.save(storage, &new_vault_token_supply)?;

        //////////Protocol Revenue//////////
        //Get Senior Vault Token Supply
        let senior_vault_token_supply = get_total_vault_tokens(storage, false)?;
        //Calculate the amount of vault tokens to mint to the MarketsManager Contract
        let vt_to_mint_to_membrane = calculate_vault_tokens(
            membrane_revenue, 
            get_total_debt_tokens(config.clone(), Some(false))?, 
            senior_vault_token_supply
        )?;

        //Mint membrane revenue to MarketsManager Contract
        if vt_to_mint_to_membrane > Uint128::zero() {
            let mint_vault_tokens_msg: CosmosMsg = TokenFactory::MsgMint {
                sender: env.contract.address.to_string(), 
                amount: Some(osmosis_std::types::cosmos::base::v1beta1::Coin {
                    denom: config.debt_supply_vault_token.clone(),
                    amount: vt_to_mint_to_membrane.to_string(),
                }), 
                mint_to_address: config.markets_manager_contract.clone().to_string(),
            }.into();
            msgs.push(mint_vault_tokens_msg);
        }


        //Update vault token supply
        let new_vault_token_supply= match senior_vault_token_supply.checked_add(vt_to_mint_to_membrane){
            Ok(v) => v,
            Err(_) => return Err(ContractError::CustomError { val: format!("Failed to add vault token total supply: {} + {}", new_vault_token_supply, vt_to_mint_to_membrane) }),
        };


        //////////Save vault token supply//////////
        DEBT_VAULT_TOKEN.save(storage, &new_vault_token_supply)?;

        //println!("senior_vault_token_supply: {}", senior_vault_token_supply);
        //println!("vt_to_mint_to_membrane: {}", vt_to_mint_to_membrane);
        //Add rate assurance callback msg
        if !senior_vault_token_supply.is_zero() && !vt_to_mint_to_membrane.is_zero() {
            msgs.push(CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: env.contract.address.to_string(),
                msg: to_json_binary(&ExecuteMsg::RateAssurance { is_junior: false })?,
                funds: vec![],
            }));
        }

    }


    Ok(())
}

/// Distribute yield between senior and junior tranches based on target rates
pub fn distribute_yield(
    config: &mut Config,
    total_accrued_interest: Uint128,
    time_elapsed: u64,
    market_total_borrowed: Uint128,
    senior_vault_token_supply: Uint128,
    junior_vault_token_supply: Uint128,
) -> Result<(), ContractError> {
    // Early return if no yield to distribute
    if total_accrued_interest.is_zero() {
        return Ok(());
    }

    // Early return if no total debt tokens (prevents division by zero)
    if config.total_debt_tokens.is_zero() {
        return Ok(());
    }

    // Early return if this market has no borrowed amount
    if market_total_borrowed.is_zero() {
        return Ok(());
    }

    // Get senior yield target from config
    let senior_yield_target = match config.senior_debt_fixed_yield_target {
        Some(target) => target,
        None => return Ok(()), // No target set, skip distribution
    };

    // Calculate this market's share of the total borrowed
    let market_share_ratio = decimal_division(
        Decimal::from_ratio(market_total_borrowed, Uint128::one()),
        Decimal::from_ratio(config.total_debt_tokens, Uint128::one()),
    )?;

    // Calculate expected yearly senior yield using accumulate_interest_dec
    let expected_senior_yield = accumulate_interest_dec(
        Decimal::from_ratio(config.total_debt_tokens, Uint128::one()),
        senior_yield_target,
        time_elapsed,
    )?.to_uint_floor();

    // Scale the expected yield by this market's share
    let proportional_expected_yield = decimal_multiplication(
        Decimal::from_ratio(expected_senior_yield, Uint128::one()),
        market_share_ratio,
    )?.to_uint_floor();

    // Determine senior and junior portions
    let (mut senior_portion, mut junior_portion) = if total_accrued_interest > proportional_expected_yield {
        // Senior gets target amount, excess goes to junior
        (proportional_expected_yield, total_accrued_interest - proportional_expected_yield)
    } else {
        // 80% to senior, rest to junior
        let senior_portion = decimal_multiplication(
            Decimal::from_ratio(total_accrued_interest, Uint128::one()),
            Decimal::percent(80),
        )?.to_uint_floor();
        (senior_portion, total_accrued_interest - senior_portion)
    };

    //If senior vault tokens is 0:
    //- set junior portion to total accrued interest
    // - set senior portion to 0
    // vice versa
    if senior_vault_token_supply.is_zero() {
        junior_portion = total_accrued_interest;
        senior_portion = Uint128::zero();
    } else if junior_vault_token_supply.is_zero() {
        senior_portion = total_accrued_interest;
        junior_portion = Uint128::zero();
    }

    // Add senior portion to config.total_debt_tokens
    config.total_debt_tokens = config.total_debt_tokens.checked_add(senior_portion)
        .map_err(|_| ContractError::CustomError { val: format!("Failed to add senior portion to total debt tokens") })?;

    // Add junior portion to junior_debt_info
    if let Some(ref mut junior_debt_info) = config.junior_debt_info {
        junior_debt_info.total_debt = junior_debt_info.total_debt.checked_add(junior_portion)
            .map_err(|_| ContractError::CustomError { val: format!("Failed to add junior portion to junior debt info") })?;
    }


    Ok(())
}