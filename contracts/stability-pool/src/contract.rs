use std::env;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, to_binary, Addr, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps,
    DepsMut, Env, MessageInfo, Response, StdError, StdResult,
    Storage, Uint128, WasmMsg,
};
use cw2::set_contract_version;

use membrane::positions::ExecuteMsg as CDP_ExecuteMsg;
use membrane::stability_pool::{
    Config, ExecuteMsg, InstantiateMsg, QueryMsg, UpdateConfig,
};
use membrane::types::{
    Asset, AssetInfo, AssetPool, Deposit, User, UserInfo, UserRatio,
};
use membrane::helpers::{validate_position_owner, withdrawal_msg, assert_sent_native_token_balance, asset_to_coin, accumulate_interest};
use membrane::math::{decimal_division, decimal_multiplication, decimal_subtraction};

use crate::error::ContractError;
use crate::query::{query_user_incentives, query_liquidatible, query_user_claims, query_capital_ahead_of_deposits, query_asset_pool};
use crate::state::{Propagation, ASSET, CONFIG, INCENTIVES, PROP, USERS};

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
        incentive_rate: msg.incentive_rate.unwrap_or_else(|| Decimal::percent(10)),
        max_incentives: msg
            .max_incentives
            .unwrap_or_else(|| Uint128::new(10_000_000_000_000)),
        unstaking_period: 1u64,
        mbrn_denom: msg.mbrn_denom,
        osmosis_proxy: deps.api.addr_validate(&msg.osmosis_proxy)?,
        positions_contract: deps.api.addr_validate(&msg.positions_contract)?,
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
            if valid_assets.is_empty() {
                return Err(ContractError::CustomError {
                    val: "No valid assets".to_string(),
                });
            }

            deposit(deps, env, info, user, valid_assets[0].clone())
        }
        ExecuteMsg::Withdraw { amount } => withdraw(deps, env, info, amount),
        ExecuteMsg::Restake { restake_amount } => restake(deps, env, info, restake_amount),
        ExecuteMsg::Liquidate { liq_amount } => liquidate(deps, info, liq_amount),
        ExecuteMsg::Claim {} => claim(deps, env, info),
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

fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    update: UpdateConfig,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    //Assert Authority
    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let mut attrs = vec![attr("method", "update_config")];

    //Match Optionals
    if let Some(owner) = update.owner {
        config.owner = deps.api.addr_validate(&owner)?;
        attrs.push(attr("new_owner", owner));
    }
    if let Some(mbrn_denom) = update.mbrn_denom {
        config.mbrn_denom = mbrn_denom.clone();
        attrs.push(attr("new_mbrn_denom", mbrn_denom));
    }
    if let Some(osmosis_proxy) = update.osmosis_proxy {
        config.osmosis_proxy = deps.api.addr_validate(&osmosis_proxy)?;
        attrs.push(attr("new_osmosis_proxy", osmosis_proxy));
    }
    if let Some(positions_contract) = update.positions_contract {
        config.positions_contract = deps.api.addr_validate(&positions_contract)?;
        attrs.push(attr("new_positions_contract", positions_contract));
    }
    if let Some(incentive_rate) = update.incentive_rate {
        config.incentive_rate = incentive_rate;
        attrs.push(attr("new_incentive_rate", incentive_rate.to_string()));
    }
    if let Some(max_incentives) = update.max_incentives {
        config.max_incentives = max_incentives;
        attrs.push(attr("new_max_incentives", max_incentives.to_string()));
    }
    if let Some(new_unstaking_period) = update.unstaking_period {
        config.unstaking_period = new_unstaking_period;
        attrs.push(attr("new_unstaking_period", new_unstaking_period.to_string()));
    }

    //Save new Config
    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attributes(attrs))
}

pub fn deposit(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    position_owner: Option<String>,
    asset: Asset,
) -> Result<Response, ContractError> {
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

//Get incentive rate and return accrued amount
fn accrue_incentives(
    storage: &mut dyn Storage,
    env: Env,
    config: Config,
    stake: Uint128,
    deposit: &mut Deposit,
) -> StdResult<Uint128> {    
    //Time elapsed starting from now or unstake time
    let time_elapsed = match deposit.unstake_time {
        Some( unstake_time ) => {
            unstake_time - deposit.last_accrued
        },
        None => {
            env.block.time.seconds() - deposit.last_accrued
        },
    };    
    let rate: Decimal = config.clone().incentive_rate;

    //Set last_accrued
    deposit.last_accrued = env.block.time.seconds();

    let mut incentives = accumulate_interest(stake, rate, time_elapsed)?;
    let mut total_incentives = INCENTIVES.load(storage)?;

    //Assert that incentives aren't over max, set 0 if so.
    if total_incentives + incentives > config.max_incentives {
        incentives = Uint128::zero();
    } else {
        total_incentives += incentives;
        INCENTIVES.save(storage, &total_incentives)?;
    }

    Ok(incentives)
}

//Withdraw / Unstake
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
        //Go thru each deposit and withdraw request from state
        let (withdrawable, new_pool) = withdrawal_from_state(
            deps.storage,
            env.clone(),
            config.clone(),
            info.clone().sender,
            Decimal::from_ratio(amount, Uint128::new(1u128)),
            asset_pool.clone(),
            false,
        )?;

        //Update pool
        ASSET.save(deps.storage, &new_pool)?;

        //If there is a withdrwable amount
        if !withdrawable.is_zero() {
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

fn withdrawal_from_state(
    storage: &mut dyn Storage,
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

                /////Check if deposit is withdrawable
                if !skip_unstaking {
                    //If deposit has been "unstaked" ie previously withdrawn, assert the unstaking period has passed before withdrawing
                    if deposit_item.unstake_time.is_some() {
                        //If time_elapsed is >= unstaking period
                        if env.block.time.seconds() - deposit_item.unstake_time.unwrap()
                            >= (config.unstaking_period * SECONDS_PER_DAY)
                        {
                            withdrawable = true;
                        }
                        //If unstaking period hasn't passed do nothing
                    } else {
                        //Set unstaking time for the amount getting withdrawn
                        //Create a Deposit object for the amount not getting unstaked
                        if deposit_item.amount > withdrawal_amount
                            && withdrawal_amount != Decimal::zero()
                        {
                            //Set new deposit
                            returning_deposit = Some(Deposit {
                                amount: deposit_item.amount - withdrawal_amount,
                                unstake_time: None,
                                ..deposit_item.clone()
                            });

                            //Set new deposit amount
                            deposit_item.amount = withdrawal_amount;
                        }

                        deposit_item.unstake_time = Some(env.block.time.seconds());
                    }
                } else {
                    //Allow regular withdraws if from CDP Repay fn
                    deposit_item.unstake_time = Some( env.block.time.seconds() );
                    //Withdraws from state
                    withdrawable = true;
                }

                //Subtract from each deposit until there is none left to withdraw
                //If not withdrawable we only edit withdraw amount to make sure the deposits...
                //..that would get parsed through in a valid withdrawal get their unstaking_time set/checked
                if withdrawal_amount != Decimal::zero() && deposit_item.amount > withdrawal_amount {
                    if withdrawable {
                        //Add to withdrawable
                        withdrawable_amount += withdrawal_amount * Uint128::new(1u128);

                        //Subtract from deposit.amount
                        deposit_item.amount -= withdrawal_amount;                        
                    }

                    //Calc incentives
                    let accrued_incentives = match accrue_incentives(
                        storage,
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

                } else if withdrawal_amount != Decimal::zero() && deposit_item.amount <= withdrawal_amount{
                    //If it's less than amount, 0 the deposit and substract it from the withdrawal amount
                    withdrawal_amount -= deposit_item.amount;

                    if withdrawable {
                        //Add to withdrawable_amount
                        withdrawable_amount += deposit_item.amount * Uint128::new(1u128);                        

                        deposit_item.amount = Decimal::zero();
                    }

                    //Calc incentives
                    let accrued_incentives = match accrue_incentives(
                        storage,
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
                }

                withdrawable = false;
            }
            deposit_item
        })
        .collect::<Vec<Deposit>>()
        .into_iter()
        .filter(|deposit| deposit.amount != Decimal::zero())
        .collect::<Vec<Deposit>>();

    //Push returning_deposit if some
    if let Some(deposit) = returning_deposit {
        new_deposits.push(deposit);
    }//Set new deposits
    pool.deposits = new_deposits;
    //Subtract withdrawable from total pool amount
    pool.credit_asset.amount -= withdrawable_amount;

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
                        user.claimable_assets.push(Asset {
                            info: AssetInfo::NativeToken {
                                denom: config.clone().mbrn_denom,
                            },
                            amount: mbrn_incentives,
                        });
                        Ok(user)
                    }
                    None => {
                        if is_user {
                            Ok(User {
                                claimable_assets: vec![Asset {
                                    info: AssetInfo::NativeToken {
                                        denom: config.clone().mbrn_denom,
                                    },
                                    amount: mbrn_incentives,
                                }],
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

//Restake unstaking deposits for a user
fn restake(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    mut restake_amount: Decimal,
) -> Result<Response, ContractError> {
    let initial_restake = restake_amount;

    let mut asset_pool = ASSET.load(deps.storage)?;

    //Attempt restaking 
    asset_pool.deposits = asset_pool
        .deposits
        .into_iter()
        .map(|mut deposit| {
            if deposit.user == info.clone().sender && !restake_amount.is_zero() {
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


    //Save new Deposits
    ASSET.save(deps.storage, &asset_pool)?;

    Ok(Response::new().add_attributes(vec![
        attr("method", "restake"),
        attr("restake_amount", initial_restake.to_string()),
    ]))
}

//- send repayments for the Positions contract
//- Positions contract sends back a distribute msg
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
            format!("{} {}", leftover, asset_pool.credit_asset.info),
        ),
    ]))
}

//Calculate which and how much each user gets distributed from the liquidation
pub fn distribute_funds(
    deps: DepsMut,
    info: MessageInfo,
    env: Env,
    mut distribution_assets: Vec<Asset>,
    distribution_asset_ratios: Vec<Decimal>,
    distribute_for: Uint128, //How much repayment is this distributing for
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    //Can only be called by the positions contract
    if info.sender != config.positions_contract {
        return Err(ContractError::Unauthorized {});
    }
    //Assert correct parameters
    if distribution_assets.is_empty() {
        return Err(ContractError::InsufficientFunds {});
    }

    let mut asset_pool = ASSET.load(deps.storage)?;

    //Assert that the distributed assets were sent
    let assets: Vec<AssetInfo> = distribution_assets
        .clone()
        .into_iter()
        .map(|asset| asset.info)
        .collect::<Vec<AssetInfo>>();

    let valid_assets = validate_assets(deps.storage, assets.clone(), info, false)?;

    if valid_assets.len() != distribution_assets.len() {
        return Err(ContractError::InvalidAssetObject {});
    }
    //Set distribution_assets to the validated assets
    distribution_assets = valid_assets;
    

    //Load repaid_amount
    //Liquidations are one msg at a time and PROP is always saved to first
    //so we can propagate without worry
    let mut prop = PROP.load(deps.storage)?;
    let repaid_amount: Uint128;
    //If this distribution is more than what was repaid, error
    if distribute_for <= prop.repaid_amount {
        repaid_amount = distribute_for;
        prop.repaid_amount -= distribute_for;
        PROP.save(deps.storage, &prop)?;
    } else {
        return Err(ContractError::CustomError {
            val: format!(
                "Distribution attempting to distribute_for too much ( {} > {} )",
                distribute_for, prop.repaid_amount
            ),
        });
    }

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
    //Also bc native tokens come in batches, CW20s come separately
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

//Repay for a user in the CDP contract
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
                env,
                config.clone(),
                position_owner,
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

            //Add Accrue Msg before Repayment to accrue discounted rates
            let msg = CosmosMsg::Wasm(WasmMsg::Execute { 
                contract_addr: config.clone().positions_contract.to_string(),
                msg: to_binary(&CDP_ExecuteMsg::Accrue { 
                    position_owner: Some(user_info.clone().position_owner),
                    position_id: user_info.clone().position_id 
                })?, 
                funds: vec![],
            });
            msgs.push(msg);

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

//Sends available claims to info.sender
//If claim_as is passed, the claims will be sent as said asset
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

fn user_claims_msgs(
    storage: &mut dyn Storage,
    info: MessageInfo,
) -> Result<(Vec<CosmosMsg>, Vec<Asset>), ContractError> {
    let user = USERS.load(storage, info.clone().sender)?;
    let mut messages: Vec<CosmosMsg> = vec![];
    let mut native_claims = vec![];

    //Aggregate native token sends
    for asset in user.clone().claimable_assets {
        if let AssetInfo::NativeToken { denom: _ } = asset.clone().info{
            native_claims.push(asset_to_coin(asset)?);
        }
    }    

    if native_claims != vec![] {
        let msg = CosmosMsg::Bank(BankMsg::Send {
            to_address: info.sender.to_string(),
            amount: native_claims,
        });
        messages.push(msg);
    }

    //Remove User's claims
    USERS.update(storage, info.sender, |user| -> StdResult<User> {
        match user {
            Some(mut user) => {
                user.claimable_assets = vec![];
                Ok(user)
            },
            None => {
                Err(StdError::GenericErr { msg: "No User found".to_string() })
            },
        }
    })?;

    Ok((messages, user.claimable_assets))
}


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
                distribution_assets[index].amount -= send_amount;

                //Add all of this asset to existing claims
                //Add to existing user claims
                add_to_user_claims(storage, user_ratio.clone().user, distribution_assets[index].clone().info, send_amount)?;

                //Set cAsset_ratios[index] to 0
                cAsset_ratios[index] = Decimal::zero();

                break;
            } else if user_ratio.ratio < cAsset_ratio {

                //Allocate full user ratio of the asset
                let send_ratio = decimal_division(user_ratio.ratio, cAsset_ratio);
                let send_amount = decimal_multiplication(
                    send_ratio,
                    Decimal::from_ratio(distribution_assets[index].amount, Uint128::new(1u128)),
                ) * Uint128::new(1u128);

                //Set distribution_asset amount to difference
                distribution_assets[index].amount -= send_amount;
                                
                //Add to existing user claims
                add_to_user_claims(storage, user_ratio.clone().user, distribution_assets[index].clone().info, send_amount)?;

                //Set cAsset_ratio to the difference
                cAsset_ratio = decimal_subtraction(cAsset_ratio, user_ratio.ratio);
                cAsset_ratios[index] = cAsset_ratio;

                break;
            } else if user_ratio.ratio > cAsset_ratio {

                //Allocate the full ratio worth of asset to the User
                let send_amount = distribution_assets[index].amount;

                //Set distribution_asset amount to difference
                distribution_assets[index].amount -= send_amount;

                //Add to existing user claims
                add_to_user_claims(storage, user_ratio.clone().user, distribution_assets[index].clone().info, send_amount)?;

                //Set user_ratio as leftover
                user_ratio.ratio = decimal_subtraction(user_ratio.ratio, cAsset_ratio);                                

                //Set cAsset_ratio to 0
                cAsset_ratios[index] = Decimal::zero();
                //continue loop
            }
        }
    }

    Ok(())
}

fn add_to_user_claims(
    storage: &mut dyn Storage,
    user: Addr,
    distribution_asset: AssetInfo,
    send_amount: Uint128,
) -> StdResult<()>{
    //Add to existing user claims
    USERS.update(
        storage,
        user,
        |user| -> StdResult<User> {
            match user {
                Some(mut user) => {
                    //Find Asset in user state
                    match user.clone().claimable_assets
                        .into_iter()
                        .enumerate()
                        .find(|(_i, asset)| {
                        asset.info.equal(&distribution_asset)
                    }) {
                        Some((i, _asset)) => {
                            //Add amount
                            user.claimable_assets[i].amount += send_amount;
                        }
                        None => {
                            user.claimable_assets.push(Asset {
                                amount: send_amount,
                                info: distribution_asset,
                            });
                        }
                    }

                    Ok(user)
                }
                None => {
                    //Create object for user
                    Ok(User {
                        claimable_assets: vec![Asset {
                            amount: send_amount,
                            info: distribution_asset,
                        }],
                    })
                }
            }
        },
    )?;

    Ok(())
}

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

    //getting each user's % of total amount
    let mut user_ratios: Vec<Decimal> = vec![];
    for deposit in user_deposits.iter() {
        user_ratios.push(decimal_division(deposit.amount, total_amount));
    }

    Ok((user_ratios, user_deposits))
}

//Calc a user's incentives from each deposit
fn get_user_incentives(
    storage: &mut dyn Storage,
    env: Env,
    user: Addr,
    mut asset_pool: AssetPool,
    rate: Decimal,
) -> StdResult<Uint128>{
    let mut total_incentives = Uint128::zero();
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
                        total_incentives += match accumulate_interest(stake, rate, time_elapsed){
                            Ok(incentives) => incentives,
                            Err(err) => {
                                error = Some(err);
                                Uint128::zero()
                            },
                        };
                    }                    
                    
                    deposit.last_accrued = env.block.time.seconds();
                },
                None => {
                    let time_elapsed = env.block.time.seconds() - deposit.last_accrued;
                    let stake = deposit.amount * Uint128::one();
    
                    if time_elapsed != 0 {
                        //Add accrued incentives
                        total_incentives += match accumulate_interest(stake, rate, time_elapsed){
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
    //Set new deposits
    asset_pool.deposits = new_deposits;

    //Save pool
    ASSET.save( storage, &asset_pool )?;

    Ok(total_incentives)
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

//Note: This fails if an asset total is sent in two separate Asset objects. Both will be invalidated.
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
