use std::env;
use std::str::FromStr;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, to_binary, Addr, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps,
    DepsMut, Env, MessageInfo, Response, StdError, StdResult, QuerierWrapper,
    Storage, Uint128, WasmMsg, coin,
};
use cw2::set_contract_version;
use cw_coins::Coins;

use membrane::cdp::{ExecuteMsg as CDP_ExecuteMsg, QueryMsg as CDP_QueryMsg};
use membrane::oracle::PriceResponse;
use membrane::stability_pool::{
    Config, ExecuteMsg, InstantiateMsg, QueryMsg, UpdateConfig,
};
use membrane::osmosis_proxy::ExecuteMsg as OsmosisProxy_ExecuteMsg;
use membrane::types::{
    Asset, AssetInfo, AssetPool, Deposit, User, UserInfo, UserRatio, Basket,
};
use membrane::helpers::{validate_position_owner, withdrawal_msg, assert_sent_native_token_balance, asset_to_coin, accumulate_interest, accrue_user_positions, query_asset_price, query_basket};
use membrane::math::{decimal_division, decimal_multiplication, decimal_subtraction};

use crate::error::ContractError;
use crate::query::{query_user_incentives, query_liquidatible, query_user_claims, query_capital_ahead_of_deposits, query_asset_pool};
use crate::state::{Propagation, ASSET, CONFIG, INCENTIVES, PROP, USERS, OWNERSHIP_TRANSFER};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:stability-pool";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Timeframe constants
const SECONDS_PER_DAY: u64 = 86_400u64;

//FIFO Stability Pool
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let mut config = Config {
        owner: info.sender,
        incentive_rate: msg.incentive_rate.unwrap_or_else(|| Decimal::percent(9)),
        max_incentives: msg
            .max_incentives
            .unwrap_or_else(|| Uint128::new(10_000_000_000_000)),
        unstaking_period: 1u64,
        minimum_deposit_amount: msg.minimum_deposit_amount,
        mbrn_denom: msg.mbrn_denom,
        osmosis_proxy: deps.api.addr_validate(&msg.osmosis_proxy)?,
        positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
        oracle_contract: deps.api.addr_validate(&msg.oracle_contract)?,
    };

    //Set optional config parameters
    if let Some(owner) = msg.owner {
        config.owner = deps.api.addr_validate(&owner)?;
    }

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;

    //Initialize the propagation object
    PROP.save(
        deps.storage,
        &Propagation {
            repaid_amount: Uint128::zero(),
        },
    )?;

    //Initialize Incentive Total
    INCENTIVES.save(deps.storage, &Uint128::zero())?;

    //Initialize Asset Pool
    let mut pool = msg.asset_pool;
    pool.deposits = vec![];

    ASSET.save(deps.storage, &pool)?;

    Ok(Response::new().add_attributes(vec![
        attr("method", "instantiate"),
        attr("config", format!("{:?}", config)),
    ])
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
        ExecuteMsg::UpdateConfig(update) => update_config(deps, info, update),
        ExecuteMsg::Deposit { user } => {
            //Outputs asset objects w/ correct amounts
            let valid_assets = validate_assets(deps.storage, vec![AssetInfo::NativeToken { denom: info.clone().funds[0].clone().denom }], info.clone(), true)?;
            if valid_assets.is_empty() || info.clone().funds.len() > 1 {
                return Err(ContractError::CustomError {
                    val: "No valid asset or more than one asset sent".to_string(),
                });
            }
			
            deposit(deps, env, info, user, valid_assets[0].clone())
        }
        ExecuteMsg::Withdraw { amount } => withdraw(deps, env, info, amount),
        ExecuteMsg::Restake { restake_amount } => restake(deps, env, info, restake_amount),
        ExecuteMsg::Liquidate { liq_amount } => liquidate(deps, info, liq_amount),
        ExecuteMsg::ClaimRewards {} => claim(deps, env, info),
        ExecuteMsg::Distribute {
            distribution_assets,
            distribution_asset_ratios,
            distribute_for,
        } => distribute_funds(
            deps,
            info,
            env,
            distribution_assets,
            distribution_asset_ratios,
            distribute_for,
        ),
        ExecuteMsg::Repay {
            user_info,
            repayment,
        } => repay(deps, env, info, user_info, repayment),
    }
}

/// Update contract configuration
fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    update: UpdateConfig,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    //Assert Authority
    if info.sender != config.owner {
        //Check if ownership transfer is in progress & transfer if so
        if info.sender == OWNERSHIP_TRANSFER.load(deps.storage)? {
            config.owner = info.sender;
        } else {
            return Err(ContractError::Unauthorized {});
        }
    }

    let mut attrs = vec![attr("method", "update_config")];

    //Match Optionals
    if let Some(owner) = update.owner {
        let valid_addr = deps.api.addr_validate(&owner)?;

        //Set owner transfer state
        OWNERSHIP_TRANSFER.save(deps.storage, &valid_addr)?;
        attrs.push(attr("owner_transfer", valid_addr));
    }
    if let Some(mbrn_denom) = update.mbrn_denom {
        config.mbrn_denom = mbrn_denom.clone();
    }
    if let Some(osmosis_proxy) = update.osmosis_proxy {
        config.osmosis_proxy = deps.api.addr_validate(&osmosis_proxy)?;
    }
    if let Some(positions_contract) = update.positions_contract {
        config.positions_contract = deps.api.addr_validate(&positions_contract)?;
    }
    if let Some(oracle_contract) = update.oracle_contract {
        config.oracle_contract = deps.api.addr_validate(&oracle_contract)?;
        attrs.push(attr("new_oracle_contract", oracle_contract));
    }
    if let Some(incentive_rate) = update.incentive_rate {
        //Enforce incentive rate range of 0-20%
        if incentive_rate > Decimal::percent(20) {
            return Err(ContractError::CustomError {
                val: "Incentive rate cannot be greater than 20%".to_string(),
            });
        }
        config.incentive_rate = incentive_rate;
    }
    if let Some(max_incentives) = update.max_incentives {
        //Enforce max incentive range of 1M - 10M
        if max_incentives < Uint128::from(1_000_000u128) || max_incentives > Uint128::from(10_000_000u128) {
            return Err(ContractError::CustomError {
                val: "Max incentives must be between 1M and 10M".to_string(),
            });
        }
        config.max_incentives = max_incentives;
    }
    if let Some(minimum_deposit_amount) = update.minimum_deposit_amount {
        config.minimum_deposit_amount = minimum_deposit_amount;
    }
    if let Some(new_unstaking_period) = update.unstaking_period {
        //Enforce unstaking period range of 1-7 days
        if new_unstaking_period < 1 || new_unstaking_period > 7 {
            return Err(ContractError::CustomError {
                val: "Unstaking period must be between 1 and 7 days".to_string(),
            });
        }
        config.unstaking_period = new_unstaking_period;
    }

    //Save new Config
    CONFIG.save(deps.storage, &config)?;
    attrs.push(attr("updated_config", format!("{:?}", config)));

    Ok(Response::new().add_attributes(attrs))
}

/// Deposit debt tokens into the contract
pub fn deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_owner: Option<String>,
    asset: Asset,
) -> Result<Response, ContractError> {
    //Load Config
    let config = CONFIG.load(deps.storage)?;

    //Assert minimum deposit amount
    if asset.amount < config.minimum_deposit_amount {
        return Err(ContractError::MinimumDeposit { min: config.minimum_deposit_amount });
    }

    let valid_owner_addr = validate_position_owner(deps.api, info, position_owner)?;

    //Adding to Asset_Pool totals and deposit's list
    let mut asset_pool = ASSET.load(deps.storage)?;

    let deposit = Deposit {
        user: valid_owner_addr.clone(),
        amount: Decimal::from_ratio(asset.amount, Uint128::new(1u128)),
        deposit_time: env.block.time.seconds(),
        last_accrued: env.block.time.seconds(),
        unstake_time: None,
    };

    if asset_pool.credit_asset.info.equal(&asset.info){
        //Add user deposit to Pool totals
        asset_pool.credit_asset.amount += asset.amount;
        //Add user deposit to deposits list
        asset_pool.deposits.push(deposit);

        ASSET.save(deps.storage, &asset_pool)?;            
    } else { return Err(ContractError::InvalidAsset {  }) }

    //Response build
    let response = Response::new();
    Ok(response.add_attributes(vec![
        attr("method", "deposit"),
        attr("position_owner", valid_owner_addr.to_string()),
        attr("deposited_asset", format!("{:?}", asset)),
    ]))
}

/// Return accrued amount.
/// Assert max incentives limit.
fn accrue_incentives(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    stake: Uint128,
    deposit: &mut Deposit,
) -> StdResult<Uint128> {    
    //Time elapsed starting from now or unstake time
    let time_elapsed = match deposit.unstake_time {
        Some(_) => {            
            //Set last_accrued
            deposit.last_accrued = env.block.time.seconds();

            //Set time elapsed
            //If its unstaking, there are no rewards
            0

        },
        None => {
            let last_accrued = deposit.last_accrued;
            
            //Set last_accrued
            deposit.last_accrued = env.block.time.seconds();

            //Calculate time elapsed
            env.block.time.seconds() - last_accrued
        },
    };    

    let rate: Decimal = config.clone().incentive_rate;
    
    //This calcs the amount of CDT to incentivize so the rate is acting as if MBRN = CDT (1:1) 
    let mut incentives = accumulate_interest(stake, rate, time_elapsed)?;   

    //Get CDT Price
    let basket: Basket = match query_basket(querier, config.clone().positions_contract.to_string()){
        Ok(basket) => basket,
        Err(_) => {
            querier.query_wasm_smart::<Basket>(
            config.clone().positions_contract,
            &CDP_QueryMsg::GetBasket {}
            )?
        },
    };
    let cdt_price: PriceResponse = basket.credit_price;

    //Get MBRN price
    let mbrn_price: Decimal = match query_asset_price(
        querier, 
        config.clone().oracle_contract.into(), 
        AssetInfo::NativeToken { denom: config.clone().mbrn_denom },
        60,
        None,
    ){
        Ok(price) => price,
        Err(_) => cdt_price.price, //We default to CDT repayment price in the first hour of incentives
    };

    //Transmute CDT amount to MBRN incentive amount
    incentives = decimal_division(
        cdt_price.get_value(incentives)?
        , mbrn_price)? * Uint128::one();

    let mut total_incentives = INCENTIVES.load(storage)?;

    //Assert that incentives aren't over max, set to remaining cap if so.
    if total_incentives + incentives > config.max_incentives {
        incentives = config.max_incentives - total_incentives;
        INCENTIVES.save(storage, &config.max_incentives)?;
    } else {
        total_incentives += incentives;
        INCENTIVES.save(storage, &total_incentives)?;
    }

    Ok(incentives)
}

/// Withdraw / Unstake, capital can be used for liquidations while unstaking
pub fn withdraw(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    amount: Uint128,
) -> Result<Response, ContractError> {    
    let config = CONFIG.load(deps.storage)?;

    let message: CosmosMsg;
    let mut msgs = vec![];
    let mut attrs = vec![
        attr("method", "withdraw"),
        attr("position_owner", info.sender.to_string()),
    ];

    let asset_pool = ASSET.load(deps.storage)?;        
    
    //This forces withdrawals to be done by the info.sender
    //so no need to check if the withdrawal is done by the position owner
    let user_deposits: Vec<Deposit> = asset_pool.clone().deposits
        .into_iter()
        .filter(|deposit| deposit.user == info.sender)
        .collect::<Vec<Deposit>>();

    let total_user_deposits: Decimal = user_deposits
        .iter()
        .map(|user_deposit| user_deposit.amount)
        .collect::<Vec<Decimal>>()
        .into_iter()
        .sum();

    //Cant withdraw more than the total deposit amount
    if total_user_deposits < Decimal::from_ratio(amount, Uint128::new(1u128)) {
        return Err(ContractError::InvalidWithdrawal {});
    } else {
        let mut skip_unstaking = false;
        //If unstaking time is 0, skip unstaking
        if config.unstaking_period == 0 { skip_unstaking = true; }

        //Go thru each deposit and withdraw request from state
        let (withdrawable, new_pool) = withdrawal_from_state(
            deps.storage,
            deps.querier,
            env.clone(),
            config.clone(),
            info.clone().sender,
            Decimal::from_ratio(amount, Uint128::new(1u128)),
            asset_pool.clone(),
            skip_unstaking,
        )?;

        //Update pool
        ASSET.save(deps.storage, &new_pool)?;

        //If there is a withdrawable amount
        if !withdrawable.is_zero() {
            //Create Position accrual msgs to lock in user discounts before withdrawing
            let accrual_msg = accrue_user_positions(
                deps.querier, 
                config.positions_contract.to_string(),
                info.sender.clone().to_string(), 
                32,
            )?;
            msgs.push(accrual_msg);

            let withdrawable_asset = Asset {
                amount: withdrawable,
                ..asset_pool.clone().credit_asset
            };

            attrs.push(attr("withdrawn_asset", withdrawable_asset.to_string()));

            //Create withdrawal msg
            message = withdrawal_msg(withdrawable_asset, info.sender.clone())?;
            msgs.push(message);
        }
    }    

    Ok(Response::new().add_attributes(attrs).add_messages(msgs))
}

/// Unstake or withdraw tokens from Deposits & update state.
/// Add any claimables to user claims.
fn withdrawal_from_state(
    storage: &mut dyn Storage,
    querier: QuerierWrapper,
    env: Env,
    config: Config,
    user: Addr,
    mut withdrawal_amount: Decimal,
    mut pool: AssetPool,
    skip_unstaking: bool,
) -> Result<(Uint128, AssetPool), ContractError> {
    
    let mut mbrn_incentives = Uint128::zero();

    let mut error: Option<StdError> = None;
    let mut is_user = false;
    let mut withdrawable = false;
    let mut withdrawable_amount = Uint128::zero();
    let mut returning_deposit: Option<Deposit> = None;

    let mut new_deposits: Vec<Deposit> = pool
        .clone()
        .deposits
        .into_iter()
        .map(|mut deposit_item| {
            //Only edit user deposits
            if deposit_item.user == user {
                is_user = true;
                
                //Calc incentives for the deposit
                let accrued_incentives = match accrue_incentives(
                    storage,
                    querier,
                    env.clone(),
                    config.clone(),
                    deposit_item.amount * Uint128::new(1u128),
                    &mut deposit_item,
                ){
                    Ok(incentive) => incentive,
                    Err(err) => {
                        error = Some(err);
                        Uint128::zero()
                    }
                };
                mbrn_incentives += accrued_incentives;

                /////Check if deposit is withdrawable
                if !skip_unstaking {
                    //If deposit has been "unstaked" ie previously withdrawn, assert the unstaking period has passed before withdrawing
                    if deposit_item.unstake_time.is_some() {
                        //If time_elapsed is >= unstaking period
                        if env.block.time.seconds() - deposit_item.unstake_time.unwrap()
                            >= (config.unstaking_period * SECONDS_PER_DAY)
                        {
                            withdrawable = true;
                        } //If unstaking period hasn't passed do nothing

                    } else {
                        //Set unstaking time for the amount getting withdrawn
                        //Create a Deposit object for the amount getting unstaked so the original deposit doesn't lose its position
                        if deposit_item.amount > withdrawal_amount
                            && withdrawal_amount != Decimal::zero()
                        {
                            //If withdrawal amount is less than minimum deposit amount, set withdrawal amount to minimum deposit amount
                            //This ensures all Deposits are at least the minimum deposit amount
                            if withdrawal_amount * Uint128::new(1u128) < config.minimum_deposit_amount {
                                //If withdrawal amount is less than minimum deposit amount, set withdrawal amount to minimum deposit amount
                                withdrawal_amount = Decimal::from_ratio(config.minimum_deposit_amount, Uint128::one());

                                //If the resulting deposit amount is less than minimum deposit amount, withdraw it all
                                if deposit_item.amount - withdrawal_amount < withdrawal_amount {
                                    withdrawal_amount = deposit_item.amount;
                                }
                            }

                            //Set new deposit
                            returning_deposit = Some(Deposit {
                                amount: deposit_item.amount - withdrawal_amount,
                                unstake_time: None,
                                ..deposit_item.clone()
                            });

                            //Update existing deposit state
                            deposit_item.amount = withdrawal_amount;
                            deposit_item.unstake_time = Some(env.block.time.seconds());

                            //Set withdrawal_amount to 0
                            withdrawal_amount = Decimal::zero();

                        } else if withdrawal_amount != Decimal::zero() {
                            //Set unstaking time
                            deposit_item.unstake_time = Some(env.block.time.seconds());
                            //Subtract from withdrawal_amount 
                            withdrawal_amount -= deposit_item.amount;
                        }                        
                    }
                } else {
                    //Allow regular withdraws if from CDP Repay fn
                    deposit_item.unstake_time = Some( env.block.time.seconds() );
                    //Withdraws from state
                    withdrawable = true;
                }

                //Subtract from each deposit until there is none left to withdraw
                //If not withdrawable we only edit withdraw amount to make sure the deposits...
                //..that would get parsed through in a valid withdrawal get edited
                if withdrawal_amount != Decimal::zero() && deposit_item.amount > withdrawal_amount && (skip_unstaking || withdrawable) {

                    withdrawable_amount += withdrawal_amount * Uint128::new(1u128);

                    //Subtract from deposit.amount
                    deposit_item.amount -= withdrawal_amount;

                    //Check if deposit is below minimum
                    if deposit_item.amount * Uint128::new(1u128) < config.minimum_deposit_amount {
                        //If it is, add to withdrawable
                        withdrawable_amount += deposit_item.amount * Uint128::new(1u128);
                        //Set deposit amount to 0
                        deposit_item.amount = Decimal::zero();
                    }                      
                    

                    //Calc incentives
                    let accrued_incentives = match accrue_incentives(
                        storage,
                        querier,
                        env.clone(),
                        config.clone(),
                        withdrawal_amount * Uint128::new(1u128),
                        &mut deposit_item,
                    ){
                        Ok(incentive) => incentive,
                        Err(err) => {
                            error = Some(err);
                            Uint128::zero()
                        }
                    };

                    mbrn_incentives += accrued_incentives;
                    
                    withdrawal_amount = Decimal::zero();

                } else if withdrawal_amount != Decimal::zero() && deposit_item.amount <= withdrawal_amount {
                    //If deposit.amount less than withdrawal_amount, subtract it from the withdrawal amount
                    withdrawal_amount -= deposit_item.amount;  
                    
                    //Add to withdrawable_amount
                    withdrawable_amount += deposit_item.amount * Uint128::new(1u128);  
                    //Set deposit amount to 0      
                    deposit_item.amount = Decimal::zero();                
                    
                }

                withdrawable = false;
            }                    
            
            deposit_item
        })
        .collect::<Vec<Deposit>>()
        .into_iter()
        .filter(|deposit| deposit.amount != Decimal::zero())
        .collect::<Vec<Deposit>>();

    //Sets returning_deposit to the back of the line, if some
    if let Some(deposit) = returning_deposit {
        if deposit.amount != Decimal::zero() {
            new_deposits.push(deposit);
        }
    }//Set new deposits
    pool.deposits = new_deposits;
    //Subtract withdrawable from total pool amount
    pool.credit_asset.amount = pool.credit_asset.amount.checked_sub(withdrawable_amount).unwrap();

    if error.is_some() {
        return Err(ContractError::CustomError {
            val: error.unwrap().to_string(),
        });
    }
    
    //If there are incentives
    if !mbrn_incentives.is_zero() {
        //Add incentives to User Claims
        USERS.update(
            storage,
            user,
            |user_claims| -> Result<User, ContractError> {
                match user_claims {
                    Some(mut user) => {
                        user.claimable_assets.add(&coin(mbrn_incentives.u128(), config.clone().mbrn_denom))?;
                                
                        Ok(user)
                    }
                    None => {
                        if is_user {
                            Ok(User {
                                claimable_assets: Coins::from_str(&coin(mbrn_incentives.u128(), config.clone().mbrn_denom).to_string())?,
                            })
                        } else {
                            Err(ContractError::CustomError {
                                val: String::from("Invalid user"),
                            })
                }}}},
        )?;
    }

    Ok((withdrawable_amount, pool))
}

/// Restake unstaking deposits for a user
fn restake(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    mut restake_amount: Decimal,
) -> Result<Response, ContractError> {
    //Initialize variables
    let initial_restake = restake_amount;
    let mut incentives = Uint128::zero();
    let mut error: Option<StdError> = None;

    let mut asset_pool = ASSET.load(deps.storage)?;
    let config = CONFIG.load(deps.storage)?;
    
    //Attempt restaking 
    asset_pool.deposits = asset_pool
        .deposits
        .into_iter()
        .map(|mut deposit| {
            if deposit.user == info.clone().sender && !restake_amount.is_zero() && deposit.unstake_time.is_some(){

                //Accrue the deposit's incentives
                incentives += match accrue_incentives(
                    deps.storage, 
                    deps.querier,
                    env.clone(), 
                    config.clone(),
                    deposit.amount * Uint128::new(1u128), 
                    &mut deposit){
                        Ok(incentive) => incentive,
                        Err(err) => {
                            error = Some(err);
                            Uint128::zero()
                        }
                    };

                if deposit.amount >= restake_amount {
                    //Zero restake_amount
                    restake_amount = Decimal::zero();

                    //Restake
                    deposit.unstake_time = None;
                    deposit.deposit_time = env.block.time.seconds();
                } else if deposit.amount < restake_amount {
                    //Sub from restake_amount
                    restake_amount -= deposit.amount;

                    //Restake
                    deposit.unstake_time = None;
                    deposit.deposit_time = env.block.time.seconds();
                }
            }
            deposit
        })
        .collect::<Vec<Deposit>>();

    //Return error from the accrue_incentives function if Some()
    if let Some(error) = error {
        return Err(ContractError::CustomError {
            val: error.to_string(),
        });
    }

    //Save accrued incentives to user claims
    if !incentives.is_zero(){
        USERS.update(
            deps.storage,
            info.sender,
            |user_claims| -> Result<User, ContractError> {
                match user_claims {
                    Some(mut user) => {
                        user.claimable_assets.add(&coin(incentives.u128(), config.clone().mbrn_denom))?;

                        Ok(user)
                    }
                    None => {
                        Ok(User {
                            claimable_assets: Coins::from_str(&coin(incentives.u128(), config.clone().mbrn_denom).to_string())?,
                })}}},
        )?;
    }

    //Save new Deposits
    ASSET.save(deps.storage, &asset_pool)?;

    Ok(Response::new().add_attributes(vec![
        attr("method", "restake"),
        attr("restake_amount", initial_restake.to_string()),
    ]))
}

/// Send repayments for the Positions contract.
/// Positions contract sends back a distribute msg.
pub fn liquidate(
    deps: DepsMut,
    info: MessageInfo,
    credit_amount: Decimal,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    if info.sender != config.positions_contract {
        return Err(ContractError::Unauthorized {});
    }

    let mut asset_pool = ASSET.load(deps.storage)?;

    let liq_amount = credit_amount;
    //Assert repay amount or pay as much as possible
    let mut repay_asset = Asset {
        info: asset_pool.credit_asset.info.clone(),
        amount: Uint128::new(0u128),
    };
    let mut leftover = Decimal::zero();

    if liq_amount > Decimal::from_ratio(asset_pool.credit_asset.amount, Uint128::new(1u128)) {
        //If greater then repay what's possible
        repay_asset.amount = asset_pool.credit_asset.amount;
        leftover =
            liq_amount - Decimal::from_ratio(asset_pool.credit_asset.amount, Uint128::new(1u128));
    } else {
        //Pay what's being asked
        repay_asset.amount = liq_amount * Uint128::new(1u128); // * 1
    }

    //Save Repaid amount to Propagate
    let mut prop = PROP.load(deps.storage)?;
    prop.repaid_amount += repay_asset.amount;
    PROP.save(deps.storage, &prop)?;

    //Repay for the user
    let repay_msg = CDP_ExecuteMsg::LiqRepay {};

    let coin: Coin = asset_to_coin(repay_asset.clone())?;

    let message = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.positions_contract.to_string(),
        msg: to_binary(&repay_msg)?,
        funds: vec![coin],
    });

    //Subtract repaid_amount from totals
    asset_pool.credit_asset.amount -= repay_asset.amount;
    //Save updated Pool
    ASSET.save(deps.storage, &asset_pool)?;
    
    Ok(Response::new().add_message(message).add_attributes(vec![
        attr("method", "liquidate"),
        attr(
            "leftover_repayment",
           leftover.to_string(),
        ),
    ]))
}

/// Calculate which and how much each user gets distributed from the liquidation.
/// Distributions are done in order of the Deposit list, not deposit_time.
pub fn distribute_funds(
    deps: DepsMut,
    info: MessageInfo,
    env: Env,
    distribution_assets: Vec<Asset>,
    distribution_asset_ratios: Vec<Decimal>,
    distribute_for: Uint128, //How much repayment is this distributing for
) -> Result<Response, ContractError> {
    //Load State
    let mut asset_pool = ASSET.load(deps.storage)?;   
    let config = CONFIG.load(deps.storage)?;

    //Can only be called by the positions contract
    if info.sender != config.positions_contract {
        return Err(ContractError::Unauthorized {});
    } 

    //Set repaid_amount
    let repaid_amount: Uint128 = distribute_for;

    ///Calculate the user distributions
    let mut pool_parse = asset_pool.clone().deposits.into_iter();
    let mut distribution_list: Vec<Deposit> = vec![];
    let mut current_repay_total: Decimal = Decimal::percent(0);
    let repaid_amount_decimal = Decimal::from_ratio(repaid_amount, Uint128::new(1u128));

    //Create distribution list
    while current_repay_total < repaid_amount_decimal {
        match pool_parse.next() {
            Some(mut deposit) => {

                //If greater, only add what's necessary and edit the deposit
                if (current_repay_total + deposit.amount) > repaid_amount_decimal {
                    //Subtract to calc what's left to repay
                    let remaining_repayment = repaid_amount_decimal - current_repay_total;

                    deposit.amount -= remaining_repayment;
                    current_repay_total += remaining_repayment;

                    //Add Deposit w/ amount = to remaining_repayment
                    //Splits original Deposit amount between both Vecs
                    distribution_list.push(Deposit {
                        amount: remaining_repayment,
                        ..deposit.clone()
                    });

                    //Calc MBRN incentives
                    if env.block.time.seconds() > deposit.last_accrued {
                        let accrued_incentives = accrue_incentives(
                            deps.storage,
                            deps.querier,
                            env.clone(),
                            config.clone(),
                            remaining_repayment * Uint128::new(1u128),
                            &mut deposit,
                        )?;

                        if !accrued_incentives.is_zero() {                 
                            //Add incentives to User Claims
                            add_to_user_claims(deps.storage, deposit.user, AssetInfo::NativeToken { denom: config.clone().mbrn_denom }, accrued_incentives)?;
                        }
                    }
                } else {
                    //Else, keep adding
                    current_repay_total += deposit.amount;
                    distribution_list.push(deposit.clone());
                    
                    if env.block.time.seconds() > deposit.last_accrued { 
                        //Calc MBRN incentives
                        let accrued_incentives = accrue_incentives(
                            deps.storage,
                            deps.querier,
                            env.clone(),
                            config.clone(),
                            deposit.amount * Uint128::new(1u128),
                            &mut deposit,
                        )?;

                        if !accrued_incentives.is_zero() {                            
                            //Add incentives to User Claims
                            add_to_user_claims(deps.storage, deposit.user, AssetInfo::NativeToken { denom: config.clone().mbrn_denom }, accrued_incentives)?;
                        }
                    }
                }
            }
            None => {
                //End of deposit list
                //If it gets here and the repaid amount != current_repay_total, the state was mismanaged previously
                //since by now the funds have already been sent.
                //For safety sake we'll set the values equal, as their job was to act as a limiter for the distribution list.
                current_repay_total = repaid_amount_decimal;
            }
        }
    }

    //This doesn't filter partial uses
    let mut edited_deposits: Vec<Deposit> = asset_pool
        .clone()
        .deposits
        .into_iter()
        .filter(|deposit| !deposit.equal(&distribution_list))
        .collect::<Vec<Deposit>>();
        
    //If there is an overlap between the lists, meaning there was a partial usage, account for it
    if distribution_list.len() + edited_deposits.len() > asset_pool.deposits.len() {
        edited_deposits[0].amount -= distribution_list[distribution_list.len() - 1].amount;
    }

    //Set deposits
    asset_pool.deposits = edited_deposits;

    //Save pool w/ edited deposits to state
    ASSET.save(deps.storage, &asset_pool)?;

    //Calc user ratios and distribute collateral based on them
    //Distribute 1 collateral at a time (not pro-rata) for gas and UX optimizations (ie if a user wants to sell they won't have to sell on 4 different pairs)
    let (ratios, user_deposits) = get_distribution_ratios(distribution_list.clone())?;

    let distribution_ratios: Vec<UserRatio> = user_deposits
        .into_iter()
        .enumerate()
        .map(|(index, deposit)| UserRatio {
            user: deposit.user,
            ratio: ratios[index],
        })
        .collect::<Vec<UserRatio>>();

    //1) Calc cAsset's ratios of total value
    let cAsset_ratios = distribution_asset_ratios;
    
    //2) Split assets to users
    split_assets_to_users(deps.storage, cAsset_ratios, distribution_assets.clone(), distribution_ratios)?;

    //Response Builder
    let res = Response::new();
    Ok(res.add_attributes(vec![
        attr("method", "distribute"),
        attr("credit_asset", asset_pool.credit_asset.info.to_string()),
        attr("distribution_assets", format!("{:?}", distribution_assets)),
    ]))
}

/// Repay for a user in the Positions contract
fn repay(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    user_info: UserInfo,
    repayment: Asset,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    //Assert Authority
    if info.sender != config.positions_contract {
        return Err(ContractError::Unauthorized {});
    }

    let mut msgs = vec![];
    let attrs = vec![
        attr("method", "repay"),
        attr("user_info", user_info.to_string()),
    ];
    let asset_pool = ASSET.load(deps.storage)?;

    if asset_pool.credit_asset.info.equal(&repayment.info){
        let position_owner = deps.api.addr_validate(&user_info.position_owner)?;

        //This forces repayments to be done by the position_owner
        //so no need to check if the withdrawal is done by the position owner
        let user_deposits: Vec<Deposit> = asset_pool
            .clone()
            .deposits
            .into_iter()
            .filter(|deposit| deposit.user == position_owner)
            .collect::<Vec<Deposit>>();

        let total_user_deposits: Decimal = user_deposits
            .iter()
            .map(|user_deposit| user_deposit.amount)
            .collect::<Vec<Decimal>>()
            .into_iter()
            .sum();

        //Cant repay more than the total deposit amount
        if total_user_deposits < Decimal::from_ratio(repayment.amount, Uint128::new(1u128)) {
            return Err(ContractError::InvalidWithdrawal {});
        } else if total_user_deposits.is_zero() {
            return Err(ContractError::InvalidWithdrawal {});
        } else {
            //Go thru each deposit and withdraw request from state
            let (_withdrawable, new_pool) = withdrawal_from_state(
                deps.storage,
                deps.querier,
                env,
                config.clone(),
                position_owner.clone(),
                Decimal::from_ratio(repayment.amount, Uint128::new(1u128)),
                asset_pool,
                true,
            )?;
            
            //Update pool
            ASSET.save(deps.storage, &new_pool)?;

            /////This is where the function differs from withdraw()
            //Add Positions RepayMsg
            let repay_msg = CDP_ExecuteMsg::Repay {
                position_id: user_info.position_id,
                position_owner: Some(user_info.clone().position_owner),
                send_excess_to: Some(user_info.clone().position_owner),
            };

            let coin: Coin = asset_to_coin(repayment.clone())?;           

            let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.positions_contract.to_string(),
                msg: to_binary(&repay_msg)?,
                funds: vec![coin],
            });
            msgs.push(msg);
        }
    } else {
        return Err(ContractError::InvalidAsset {});
    }

    Ok(Response::new().add_attributes(attrs).add_messages(msgs))
}

/// Sends available claims to info.sender
pub fn claim(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;    

    let mut accrued_incentives = Uint128::zero();
    let asset_pool = ASSET.load(deps.storage)?;
    //Add newly accrued incentives to claimables
    accrued_incentives += get_user_incentives(deps.storage, env.clone(), info.clone().sender, asset_pool, config.clone().incentive_rate)?;    

    if !accrued_incentives.is_zero(){
        //Add incentives to User Claims
        add_to_user_claims(deps.storage, info.clone().sender, AssetInfo::NativeToken { denom: config.clone().mbrn_denom }, accrued_incentives)?;
    }
    
    //Create claim msgs
    let (messages, claimables) = user_claims_msgs(
        deps.storage,
        info.clone(),
    )?;

    let res = Response::new()
        .add_attribute("method", "claim")
        .add_attribute("user", info.sender)
        .add_attribute("claimables", format!("{:?}", claimables));

    Ok(res.add_messages(messages))
}

/// Build claim messages for a user & clear claims
fn user_claims_msgs(
    storage: &mut dyn Storage,
    info: MessageInfo,
) -> Result<(Vec<CosmosMsg>, Vec<Coin>), ContractError> {
    let user = USERS.load(storage, info.clone().sender)?;
    let config = CONFIG.load(storage)?;
    let mut messages: Vec<CosmosMsg> = vec![];
    let mut native_claims: Vec<Coin> = vec![];

    //Aggregate native token sends
    for asset in user.clone().claimable_assets.to_vec() {
        //if asset is MBRN, add a MBRN mint message
        if asset.denom == config.clone().mbrn_denom {
            let mint_msg = OsmosisProxy_ExecuteMsg::MintTokens {
                denom: config.clone().mbrn_denom,
                mint_to_address: info.sender.to_string(),
                amount: asset.amount,
            };
            let msg = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.osmosis_proxy.to_string(),
                msg: to_binary(&mint_msg)?,
                funds: vec![],
            });
            messages.push(msg);
        } else {
            //Add to native list
            native_claims.push(asset.clone());  
        }
    }    

    if native_claims != vec![] {
        let msg = CosmosMsg::Bank(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: native_claims.clone(),
        });
        messages.push(msg);
    }

    //Remove User's claims
    //We can fully remove because all claims will be native tokens 
    USERS.remove(storage, info.sender);

    Ok((messages, native_claims))
}

/// Split distribution assets to users based on ratios
fn split_assets_to_users(
    storage: &mut dyn Storage,
    mut cAsset_ratios: Vec<Decimal>,
    mut distribution_assets: Vec<Asset>,
    distribution_ratios: Vec<UserRatio>,
) -> Result<(), ContractError>{
    
    for mut user_ratio in distribution_ratios {
        for (index, mut cAsset_ratio) in cAsset_ratios.clone().into_iter().enumerate() {
            if cAsset_ratio == Decimal::zero() {
                continue;
            }

            if user_ratio.ratio == cAsset_ratio {
                //Allocate the full ratio worth of asset to the User
                let send_amount = distribution_assets[index].amount;

                //Set distribution_asset amount to difference
                distribution_assets[index].amount = Uint128::zero();

                //Add all of this asset to existing claims
                //Add to existing user claims
                add_to_user_claims(storage, user_ratio.clone().user, distribution_assets[index].clone().info, send_amount)?;

                //Set cAsset_ratios[index] to 0
                cAsset_ratios[index] = Decimal::zero();

                break;
            } else if user_ratio.ratio < cAsset_ratio {

                //Allocate full user ratio of the asset
                let send_ratio = decimal_division(user_ratio.ratio, cAsset_ratio)?;
                let send_amount = decimal_multiplication(
                    send_ratio,
                    Decimal::from_ratio(distribution_assets[index].amount, Uint128::new(1u128)),
                )? * Uint128::new(1u128);

                //Set distribution_asset amount to difference
                distribution_assets[index].amount -= send_amount;
                                
                //Add to existing user claims
                add_to_user_claims(storage, user_ratio.clone().user, distribution_assets[index].clone().info, send_amount)?;

                //Set cAsset_ratio to the difference
                cAsset_ratio = decimal_subtraction(cAsset_ratio, user_ratio.ratio)?;
                cAsset_ratios[index] = cAsset_ratio;

                break;
            } else if user_ratio.ratio > cAsset_ratio {

                //Allocate the full ratio worth of asset to the User
                let send_amount = distribution_assets[index].amount;

                //Set distribution_asset amount to difference
                distribution_assets[index].amount = Uint128::zero();

                //Add to existing user claims
                add_to_user_claims(storage, user_ratio.clone().user, distribution_assets[index].clone().info, send_amount)?;

                //Set user_ratio as leftover
                user_ratio.ratio = decimal_subtraction(user_ratio.ratio, cAsset_ratio)?;                                

                //Set cAsset_ratio to 0
                cAsset_ratios[index] = Decimal::zero();
                //continue loop
            }
        }
    }

    Ok(())
}

/// Add assets to user claims
fn add_to_user_claims(
    storage: &mut dyn Storage,
    user: Addr,
    distribution_asset: AssetInfo,
    send_amount: Uint128,
) -> StdResult<()>{
        if !send_amount.is_zero(){
        //Add to existing user claims
        USERS.update(
            storage,
            user,
            |user| -> StdResult<User> {
                match user {
                    Some(mut user) => {
                        //Add Coin to user claims
                        user.claimable_assets.add(&coin(send_amount.u128(), distribution_asset.to_string()))?;

                        Ok(user)
                    }
                    None => {
                        //Create object for user
                        Ok(User {
                            claimable_assets: Coins::from_str(&coin(send_amount.u128(), distribution_asset.to_string()).to_string())?,
                        })
                    }
                }
            },
        )?;
    }

    Ok(())
}

/// Get user ratios of deposits from a list of deposits
pub fn get_distribution_ratios(deposits: Vec<Deposit>) -> StdResult<(Vec<Decimal>, Vec<Deposit>)> {
    let mut user_deposits: Vec<Deposit> = vec![];
    let mut total_amount: Decimal = Decimal::percent(0);
    let mut new_deposits: Vec<Deposit> = vec![];

    //For each Deposit, create a condensed Deposit for its user.
    //Add to an existing one if found.
    for deposit in deposits.into_iter() {
        match user_deposits
            .clone()
            .into_iter()
            .find(|user_deposit| user_deposit.user == deposit.user)
        {
            Some(mut user_deposit) => {
                user_deposit.amount += deposit.amount;

                //Recreating edited user deposits due to lifetime issues
                new_deposits = user_deposits
                    .into_iter()
                    .filter(|deposit| deposit.user != user_deposit.user)
                    .collect::<Vec<Deposit>>();

                new_deposits.push(user_deposit);
                total_amount += deposit.amount;
            }
            None => {
                new_deposits.push(Deposit { ..deposit });
                total_amount += deposit.amount;
            }
        }
        user_deposits = new_deposits.clone();
    }

    //Getting each user's % of total amount
    let mut user_ratios: Vec<Decimal> = vec![];
    for deposit in user_deposits.iter() {
        user_ratios.push(decimal_division(deposit.amount, total_amount)?);
    }

    Ok((user_ratios, user_deposits))
}

/// Calculate a user's incentives from each deposit
fn get_user_incentives(
    storage: &mut dyn Storage,
    env: Env,
    user: Addr,
    mut asset_pool: AssetPool,
    rate: Decimal,
) -> StdResult<Uint128>{
    let mut total_user_incentives = Uint128::zero();
    let mut error: Option<StdError> = None;

    //Calc and add new_incentives
    //Update deposit.last_accrued time
    let new_deposits: Vec<Deposit> = asset_pool.clone().deposits.into_iter().map(|mut deposit| {

        if deposit.user == user {
            match deposit.unstake_time {
                Some(unstake_time) => {
                    let time_elapsed = unstake_time - deposit.last_accrued;
                    let stake = deposit.amount * Uint128::one();
    
                    if time_elapsed != 0 {
                        //Add accrued incentives
                        total_user_incentives += match accumulate_interest(stake, rate, time_elapsed){
                            Ok(incentives) => incentives,
                            Err(err) => {
                                error = Some(err);
                                Uint128::zero()
                            },
                        };
                    }                    
                    
                    deposit.last_accrued = unstake_time;
                },
                None => {
                    let time_elapsed = env.block.time.seconds() - deposit.last_accrued;
                    let stake = deposit.amount * Uint128::one();
    
                    if time_elapsed != 0 {
                        //Add accrued incentives
                        total_user_incentives += match accumulate_interest(stake, rate, time_elapsed){
                            Ok(incentives) => incentives,
                            Err(err) => {
                                error = Some(err);
                                Uint128::zero()
                            },
                        };
                    }
                    
                    deposit.last_accrued = env.block.time.seconds();
                },
            }
        }

        deposit
    }).collect::<Vec<Deposit>>();

    //Return error if any
    if let Some(err) = error {
        return Err(err);
    }

    //Set new deposits
    asset_pool.deposits = new_deposits;

    //Save pool
    ASSET.save( storage, &asset_pool )?;

    let mut total_incentives = INCENTIVES.load(storage)?;
    let config = CONFIG.load(storage)?;
    //Assert that incentives aren't over max, set to remaining cap if so.
    if total_incentives + total_user_incentives > config.max_incentives {
        total_user_incentives = config.max_incentives - total_incentives;
        INCENTIVES.save(storage, &config.max_incentives)?;
    } else {
        total_incentives += total_user_incentives;
        INCENTIVES.save(storage, &total_incentives)?;
    }

    Ok(total_user_incentives)
}


#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::UnclaimedIncentives { user } => to_binary(&query_user_incentives(deps, env, user)?),
        QueryMsg::CapitalAheadOfDeposit { user } => to_binary(&query_capital_ahead_of_deposits(deps, user)?),
        QueryMsg::CheckLiquidatible { amount } => to_binary(&query_liquidatible(deps, amount)?),
        QueryMsg::UserClaims { user } => to_binary(&query_user_claims(deps, user)?),
        QueryMsg::AssetPool { user, deposit_limit , start_after} => to_binary(&query_asset_pool(deps, user, deposit_limit, start_after)?),
    }
}

/// Note: This fails if an asset total is sent in two separate Asset objects. Both will be invalidated.
pub fn validate_assets(
    deps: &dyn Storage,
    assets: Vec<AssetInfo>,
    info: MessageInfo,
    in_pool: bool,
) -> Result<Vec<Asset>, ContractError> {
    let mut valid_assets: Vec<Asset> = vec![];

    if in_pool {
        //Validate sent assets against accepted assets
        let asset_pool = ASSET.load(deps)?;

        for asset in assets {
            //Validate its balance
            if asset_pool.credit_asset.info.equal(&asset){
                if let Ok(valid_asset) = assert_sent_native_token_balance(asset, &info) {
                    valid_assets.push(valid_asset);
                }
            }                
        };
    } else {
        for asset in assets {
            if let AssetInfo::NativeToken { denom: _ } = asset {
                if let Ok(valid_asset) = assert_sent_native_token_balance(asset, &info) {
                    valid_assets.push(valid_asset);
                }
            }            
        }
    }

    Ok(valid_assets)
}
