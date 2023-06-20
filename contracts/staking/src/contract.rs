#[cfg(not(feature = "library"))]
use std::env;


use cosmwasm_std::{entry_point, Coin};
use cosmwasm_std::{
    attr, coin, to_binary, Addr, Api, BankMsg, Binary, CosmosMsg, Decimal, Deps,
    DepsMut, Env, MessageInfo, Response, StdError, StdResult, Storage, Uint128, WasmMsg, QueryRequest, WasmQuery, QuerierWrapper,
};
use cw2::set_contract_version;

use membrane::governance::{QueryMsg as Gov_QueryMsg, ProposalListResponse, ProposalStatus};
use membrane::helpers::{assert_sent_native_token_balance, validate_position_owner, asset_to_coin, accrue_user_positions};
use membrane::osmosis_proxy::ExecuteMsg as OsmoExecuteMsg;
use membrane::cdp::QueryMsg as CDP_QueryMsg;
use membrane::auction::ExecuteMsg as AuctionExecuteMsg;
use membrane::staking::{ Config, ExecuteMsg, InstantiateMsg, QueryMsg };
use membrane::vesting::{QueryMsg as Vesting_QueryMsg, RecipientsResponse};
use membrane::types::{Asset, AssetInfo, FeeEvent, LiqAsset, StakeDeposit, StakeDistributionLog, StakeDistribution, Basket, Delegation, DelegationInfo};
use membrane::math::{decimal_division, decimal_multiplication};

use crate::error::ContractError;
use crate::query::{query_user_stake, query_staker_rewards, query_staked, query_fee_events, query_totals, query_delegations};
use crate::state::{Totals, CONFIG, FEE_EVENTS, STAKED, STAKING_TOTALS, INCENTIVE_SCHEDULING, OWNERSHIP_TRANSFER, DELEGATIONS};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:staking";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Constants
const SECONDS_PER_YEAR: u64 = 31_536_000u64;
pub const SECONDS_PER_DAY: u64 = 86_400u64;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let mut config: Config;

    if msg.owner.is_some() {
        config = Config {
            owner: deps.api.addr_validate(&msg.owner.unwrap())?,
            positions_contract: None,
            auction_contract: None,
            vesting_contract: None,
            governance_contract: None,
            osmosis_proxy: None,
            incentive_schedule: msg.incentive_schedule.unwrap_or_else(|| StakeDistribution {
                rate: Decimal::percent(123),
                duration: 240,
            }),
            fee_wait_period: msg.fee_wait_period.unwrap_or(3u64),
            unstaking_period: msg.unstaking_period.unwrap_or(3u64),
            mbrn_denom: msg.mbrn_denom,
        };
    } else {
        config = Config {
            owner: info.sender,
            positions_contract: None,
            auction_contract: None,
            vesting_contract: None,
            governance_contract: None,
            osmosis_proxy: None,
            incentive_schedule: msg.incentive_schedule.unwrap_or_else(|| StakeDistribution {
                rate: Decimal::percent(123),
                duration: 240,
            }),
            fee_wait_period: msg.fee_wait_period.unwrap_or(3u64),
            unstaking_period: msg.unstaking_period.unwrap_or(3u64),
            mbrn_denom: msg.mbrn_denom,
        };
    }

    //Set optional config parameters
    if let Some(vesting_contract) = msg.vesting_contract {        
        config.vesting_contract = Some(deps.api.addr_validate(&vesting_contract)?);
    };
    if let Some(positions_contract) = msg.positions_contract {
        config.positions_contract = Some(deps.api.addr_validate(&positions_contract)?);
    };
    if let Some(auction_contract) = msg.auction_contract {
        config.auction_contract = Some(deps.api.addr_validate(&auction_contract)?);
    };
    if let Some(governance_contract) = msg.governance_contract {
        config.governance_contract = Some(deps.api.addr_validate(&governance_contract)?);
    };
    if let Some(osmosis_proxy) = msg.osmosis_proxy {
        config.osmosis_proxy = Some(deps.api.addr_validate(&osmosis_proxy)?);
    };

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;

    //Initialize stake Totals
    STAKING_TOTALS.save(
        deps.storage,
        &Totals {
            stakers: Uint128::zero(),
            vesting_contract: Uint128::zero(),
        },
    )?;
    //Initialize fee events
    FEE_EVENTS.save(deps.storage, &vec![])?;

    //Initialize INCENTIVE_SCHEDULING
    INCENTIVE_SCHEDULING.save(deps.storage, &StakeDistributionLog {
        ownership_distribution: config.clone().incentive_schedule,
        start_time: env.block.time.seconds(),
    })?;

    
    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("config", format!("{:?}", config))
        .add_attribute("contract_address", env.contract.address))
}

/// Return total MBRN vesting
fn get_total_vesting(
    querier: QuerierWrapper,    
    vesting_contract: String,
) -> StdResult<Uint128>{

    let recipients = querier.query::<RecipientsResponse>(&QueryRequest::Wasm(WasmQuery::Smart { 
        contract_addr: vesting_contract, 
        msg: to_binary(&Vesting_QueryMsg::Recipients { })?
    }))?;    

    Ok(recipients.get_total_vesting())
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
            mbrn_denom,
            vesting_contract,
            governance_contract,
            osmosis_proxy,
            positions_contract,
            auction_contract,
            incentive_schedule,
            fee_wait_period,
            unstaking_period,
        } => update_config(
            deps,
            info,
            env,
            owner,
            positions_contract,
            auction_contract,
            vesting_contract,
            governance_contract,
            osmosis_proxy,
            mbrn_denom,
            incentive_schedule,
            fee_wait_period,
            unstaking_period,
        ),
        ExecuteMsg::Stake { user } => stake(deps, env, info, user),
        ExecuteMsg::Unstake { mbrn_amount } => unstake(deps, env, info, mbrn_amount),
        ExecuteMsg::UpdateDelegations { governator_addr, mbrn_amount, delegate, fluid, commission } => update_delegations(
            deps,
            env,
            info,
            governator_addr,
            mbrn_amount,
            fluid,
            delegate,
            commission,
        ),
        ExecuteMsg::DelegateFluidDelegations { governator_addr, mbrn_amount } => delegate_fluid_delegations(
            deps,
            env,
            info,
            governator_addr,
            mbrn_amount,
        ),
        ExecuteMsg::Restake { mbrn_amount } => restake(deps, env, info, mbrn_amount),
        ExecuteMsg::ClaimRewards {
            send_to,
            restake,
        } => claim_rewards(
            deps,
            env,
            info,
            send_to,
            restake,
        ),
        ExecuteMsg::DepositFee {} => {
            let config = CONFIG.load(deps.storage)?;

            if info.sender != config.positions_contract.unwrap() {
                return Err(ContractError::Unauthorized {});
            }

            //Take fee_assets from sent_assets
            let fee_assets = {
                info.clone()
                    .funds
                    .into_iter()
                    .map(|coin| Asset {
                        info: AssetInfo::NativeToken { denom: coin.denom },
                        amount: coin.amount,
                    })
                    .collect::<Vec<Asset>>()
            };

            deposit_fee(deps, env, fee_assets)
        },
        ExecuteMsg::TrimFeeEvents {  } => trim_fee_events(deps.storage, info),
    }
}

/// Update contract configuration
fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    env: Env,
    owner: Option<String>,
    positions_contract: Option<String>,
    auction_contract: Option<String>,
    vesting_contract: Option<String>,
    governance_contract: Option<String>,
    osmosis_proxy: Option<String>,
    mbrn_denom: Option<String>,
    incentive_schedule: Option<StakeDistribution>,
    fee_wait_period: Option<u64>,
    unstaking_period: Option<u64>,
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
    if let Some(owner) = owner {
        let valid_addr = deps.api.addr_validate(&owner)?;

        //Set owner transfer state
        OWNERSHIP_TRANSFER.save(deps.storage, &valid_addr)?;
        attrs.push(attr("owner_transfer", valid_addr));     
    };
    if let Some(incentive_schedule) = incentive_schedule {
        //Update incentive schedule
        config.incentive_schedule = incentive_schedule.clone();

        //Set Scheduling
        INCENTIVE_SCHEDULING.save(deps.storage, 
            &StakeDistributionLog { 
                ownership_distribution: incentive_schedule, 
                start_time: env.block.time.seconds(),
        })?;
    };
    if let Some(unstaking_period) = unstaking_period {
        config.unstaking_period = unstaking_period;
    };
    if let Some(fee_wait_period) = fee_wait_period {
        config.fee_wait_period = fee_wait_period;
    };
    if let Some(mbrn_denom) = mbrn_denom {
        config.mbrn_denom = mbrn_denom.clone();
    };
    if let Some(vesting_contract) = vesting_contract {
        config.vesting_contract = Some(deps.api.addr_validate(&vesting_contract)?);
    };
    if let Some(positions_contract) = positions_contract {
        config.positions_contract = Some(deps.api.addr_validate(&positions_contract)?);
    };
    if let Some(auction_contract) = auction_contract {
        config.auction_contract = Some(deps.api.addr_validate(&auction_contract)?);
    };
    if let Some(governance_contract) = governance_contract {
        config.governance_contract = Some(deps.api.addr_validate(&governance_contract)?);
    };
    if let Some(osmosis_proxy) = osmosis_proxy {
            config.osmosis_proxy = Some(deps.api.addr_validate(&osmosis_proxy)?);
    };

    //Save new Config
    CONFIG.save(deps.storage, &config)?;
    attrs.push(attr("updated_config", format!("{:?}", config)));
    
    Ok(Response::new().add_attributes(attrs))
}

/// Stake MBRN
pub fn stake(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    user: Option<String>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let valid_asset: Asset;
    //Assert only MBRN was sent
    if info.funds.len() == 1 && info.funds[0].denom == config.mbrn_denom {
        valid_asset = assert_sent_native_token_balance(
            AssetInfo::NativeToken {
                denom: config.clone().mbrn_denom,
            },
            &info,
        )?;
    } else {
        return Err(ContractError::CustomError {
            val: "No valid assets".to_string(),
        });
    }

    //Set valid address
    let valid_owner_addr = validate_position_owner(deps.api, info.clone(), user)?;

    //Add new deposit to staker's list of StakeDeposits
    STAKED.update(deps.storage, valid_owner_addr.clone(), |current_deposits| -> StdResult<_> {
        match current_deposits {
            Some(mut deposits) => {
                deposits.push(StakeDeposit {
                    staker: valid_owner_addr.clone(),
                    amount: valid_asset.amount,
                    stake_time: env.block.time.seconds(),
                    unstake_start_time: None,
                });
                Ok(deposits)
            }
            None => {
                let mut deposits = vec![];
                deposits.push(StakeDeposit {
                    staker: valid_owner_addr.clone(),
                    amount: valid_asset.amount,
                    stake_time: env.block.time.seconds(),
                    unstake_start_time: None,
                });
                Ok(deposits)
            }
        }
    })?;

    //Add to Totals
    let mut totals = STAKING_TOTALS.load(deps.storage)?;
    if let Some(vesting_contract) = config.clone().vesting_contract{
        if info.clone().sender == vesting_contract {
            totals.vesting_contract += valid_asset.amount;
        } else {
            totals.stakers += valid_asset.amount;
        }
    } else {
        totals.stakers += valid_asset.amount;
    }
    STAKING_TOTALS.save(deps.storage, &totals)?;    

    //Response build
    let response = Response::new();
    let attrs = vec![
        attr("method", "stake"),
        attr("staker", valid_owner_addr.to_string()),
        attr("amount", valid_asset.amount.to_string()),
    ];

    Ok(response.add_attributes(attrs))
}

/// First call is an unstake
/// 2nd call after unstake period is a withdrawal
pub fn unstake(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    mbrn_withdraw_amount: Option<Uint128>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let fee_events = FEE_EVENTS.load(deps.storage)?;

    //Restrict unstaking
    // can_this_addr_unstake(deps.querier, info.clone().sender, config.clone())?;

    //Get total Stake
    let total_stake = {
        let staker_deposits: Vec<StakeDeposit> = match STAKED.load(deps.storage, info.sender.clone()){
            Ok(deposits) => deposits,
            Err(_) => return Err(ContractError::CustomError {
                val: String::from("User has no stake"),
            }),
        };

        let total_staker_deposits: Uint128 = staker_deposits
            .into_iter()
            .map(|deposit| deposit.amount)
            .collect::<Vec<Uint128>>()
            .into_iter()
            .sum();

        total_staker_deposits
    };

    //Assert valid stake
    let withdraw_amount = mbrn_withdraw_amount.unwrap_or(total_stake);
    if withdraw_amount > total_stake {
        return Err(ContractError::CustomError {
            val: String::from("Invalid withdrawal amount"),
        });
    }


    //info.sender is user
    let (claimables, accrued_interest, withdrawable_amount) = withdraw_from_state(
        deps.storage,
        env,
        info.clone().sender,
        withdraw_amount,
        fee_events,
    )?;

    //Initialize variables
    let mut native_claims = vec![];
    let mut msgs = vec![];

    //If user can withdraw, accrue their positions and add to native_claims
    //Also update delegations
    if !withdrawable_amount.is_zero() {
        //Create Position accrual msgs to lock in user discounts before withdrawing
        // let accrual_msg = accrue_user_positions(
        //     deps.querier, 
        //     config.clone().positions_contract.unwrap_or_else(|| Addr::unchecked("")).to_string(),
        //     info.sender.clone().to_string(), 
        //     32,
        // )?;
        // msgs.push(accrual_msg);

        //Push to native claims list
        native_claims.push(asset_to_coin(Asset {
            info: AssetInfo::NativeToken {
                denom: config.clone().mbrn_denom,
            },
            amount: withdrawable_amount,
        })?);     

        //Get user's delegation info
        if let Ok(mut staker_delegation_info) = DELEGATIONS.load(deps.storage, info.sender.clone()){
            //Get user's delegated stake
            let total_delegations: Uint128 = staker_delegation_info.clone()
                .delegated_to
                .into_iter()
                .map(|delegation| delegation.amount)
                .collect::<Vec<Uint128>>()
                .into_iter()
                .sum();

            //If withdrawing more than is not delegated, undelegate the excess
            if withdrawable_amount > total_stake - total_delegations {
                let mut undelegate_amount = withdrawable_amount - (total_stake - total_delegations);
                for (i, delegation) in staker_delegation_info.clone().delegated_to.into_iter().enumerate() {
                    
                    //If undelegate amount is greater than the current delegation, undelegate the whole delegation & update undelegate amount
                    if undelegate_amount > delegation.amount {
                        undelegate_amount -= delegation.amount;
                        
                        //Remove staker delegation
                        staker_delegation_info.delegated_to.remove(i);

                        //Remove delegate delegation
                        let mut delegate_delegation_info = DELEGATIONS.load(deps.storage, delegation.delegator.clone())?;
                        for (i, delegate_delegation) in delegate_delegation_info.clone().delegated.into_iter().enumerate() {
                            if delegate_delegation.delegator == info.sender.clone() {
                                delegate_delegation_info.delegated.remove(i);
                                break;
                            }
                        }
                        DELEGATIONS.save(deps.storage, delegation.delegator.clone(), &delegate_delegation_info)?;
                    } else {
                        //If undelegate amount is less than the current delegation, undelegate the undelegate amount & break
                        staker_delegation_info.delegated_to[i].amount -= undelegate_amount;

                        //Update delegate delegation
                        let mut delegate_delegation_info = DELEGATIONS.load(deps.storage, delegation.delegator.clone())?;
                        for delegate_delegation in delegate_delegation_info.clone().delegated.into_iter() {
                            if delegate_delegation.delegator == info.sender.clone() {
                                delegate_delegation_info.delegated[i].amount -= undelegate_amount;
                                break;
                            }
                        }
                        DELEGATIONS.save(deps.storage, delegation.delegator.clone(), &delegate_delegation_info)?;
                        break;
                    }
                }

                //Save updated delegation info
                DELEGATIONS.save(deps.storage, info.sender.clone(), &staker_delegation_info)?;
            }
        };
        
    }

    //Create claimable msgs
    let claims_msgs = create_rewards_msgs(
        config.clone(), 
        claimables.clone(), 
        accrued_interest.clone(),
        info.clone().sender.to_string(),
        native_claims,
    )?;
    msgs.extend(claims_msgs);

    //Update Totals
    let mut totals = STAKING_TOTALS.load(deps.storage)?;
    if let Some(vesting_contract) = config.clone().vesting_contract{
        if info.clone().sender == vesting_contract{
            totals.vesting_contract -= withdrawable_amount;
        } else {
            totals.stakers -= withdrawable_amount;
        }
    } else {
        totals.stakers -= withdrawable_amount;
    }
    STAKING_TOTALS.save(deps.storage, &totals)?;

    //Response builder
    let response = Response::new();
    let attrs = vec![
        attr("method", "unstake"),
        attr("staker", info.sender.to_string()),
        attr("unstake_amount", withdrawable_amount.to_string()),
    ];

    Ok(response.add_attributes(attrs).add_messages(msgs))
}

/// (Un)Delegate MBRN to a Governator
/// If mbrn_amount is None, then act on the user's total stake
/// Only edits delegations for the user's stake, not their fluid delegated stake
fn update_delegations(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    governator_addr: String,
    mbrn_amount: Option<Uint128>,
    fluid: Option<bool>,
    delegate: Option<bool>,
    commission: Option<Decimal>,
) -> Result<Response, ContractError> {
    //Validate Governator, doesn't need to be a staker but can't be the user
    let valid_gov_addr = deps.api.addr_validate(&governator_addr)?;
    if valid_gov_addr == info.clone().sender {
        return Err(ContractError::CustomError {
            val: String::from("Delegate cannot be the user"),
        });
    }

    //Assert its a staker
    let staker_deposits: Vec<StakeDeposit> = STAKED.load(deps.storage, info.sender.clone())?;
    
    //Calc total stake
    let total_staker_deposits: Uint128 = staker_deposits
        .into_iter()
        .map(|deposit| deposit.amount)
        .collect::<Vec<Uint128>>()
        .into_iter()
        .sum();

    //Get staker's delegations
    let mut staker_delegation_info = match DELEGATIONS.load(deps.storage, info.clone().sender.clone()){
        Ok(delegations) => delegations,
        Err(_) => DelegationInfo {
            delegated: vec![],
            delegated_to: vec![],
            commission: commission.unwrap_or(Decimal::zero()),
        },
    };

    //Set total_delegatible_amount
    let total_delegatible_amount = total_staker_deposits.clone() - staker_delegation_info.delegated_to
        .iter()
        .map(|delegation| delegation.amount)
        .collect::<Vec<Uint128>>()
        .into_iter()
        .sum::<Uint128>();

    //Validate MBRN amount
    let mbrn_amount = mbrn_amount.unwrap_or(total_delegatible_amount);
        

    /////Act on Optionals/////
    //Delegations
    if let Some(delegate) = delegate {
        //If delegating, add to staker's delegated_to & delegates delegated
        if delegate {
            //If mbrn_amount is greater than total delegatible amount, return error
            if mbrn_amount > total_delegatible_amount {
                return Err(ContractError::CustomError {
                    val: String::from("MBRN amount exceeds delegatible amount"),
                });
            }
            //If no delegatible amount, return error
            if total_delegatible_amount.is_zero() {
                return Err(ContractError::CustomError {
                    val: String::from("No delegatible amount"),
                });
            }

            //Load delegate's info
            let mut delegates_delegations = match DELEGATIONS.load(deps.storage, valid_gov_addr.clone()){
                Ok(delegations) => delegations,
                Err(_) => DelegationInfo {
                    delegated: vec![],
                    delegated_to: vec![],
                    commission: Decimal::zero(),
                }
            };
            //Add to existing delegation from the Staker or add new Delegation object 
            match delegates_delegations.delegated.iter().enumerate().find(|(_i, delegation)| delegation.delegator == info.sender.clone()){
                Some((index, _)) => delegates_delegations.delegated[index].amount += mbrn_amount,
                None => {
                    delegates_delegations.delegated.push(Delegation {
                        delegator: info.sender.clone(),
                        amount: mbrn_amount,
                        fluidity: fluid.unwrap_or(false),
                        time_of_delegation: env.block.time.seconds(),
                    });
                }
            };
            //Save delegate's info           
            DELEGATIONS.save(deps.storage, valid_gov_addr.clone(), &delegates_delegations)?;

            //Add to staker's delegated_to
            //Add to existing delegation or add new Delegation object 
            match staker_delegation_info.delegated_to.iter().enumerate().find(|(_i, delegation)| delegation.delegator == valid_gov_addr.clone()){
                Some((index, _)) => staker_delegation_info.delegated_to[index].amount += mbrn_amount,
                None => {
                    staker_delegation_info.delegated_to.push(Delegation {
                        delegator: valid_gov_addr.clone(),
                        amount: mbrn_amount,
                        fluidity: fluid.unwrap_or(false),
                        time_of_delegation: env.block.time.seconds(),
                    });
                }
            };
            //Save staker's info
            DELEGATIONS.save(deps.storage, info.sender.clone(), &staker_delegation_info)?;
        } else {
            //If undelegating, remove from staker's delegations & delegates delegations
            //Remove from delegate's
            let mut delegates_delegations = DELEGATIONS.load(deps.storage, valid_gov_addr.clone())?;
            match delegates_delegations.delegated.iter().enumerate().find(|(_i, delegation)| delegation.delegator == info.clone().sender){
                Some((index, _)) => match delegates_delegations.delegated[index].amount.checked_sub(mbrn_amount){
                    Ok(new_amount) => delegates_delegations.delegated[index].amount = new_amount,
                    Err(_) => {
                        //If more than delegated, remove from delegate's delegated
                        delegates_delegations.delegated.remove(index);
                    }
                },
                None => {
                    return Err(ContractError::CustomError {
                        val: String::from("Delegator not found in delegated's delegated"),
                    });
                }
            };

            //Remove if empty, save otherwise
            if delegates_delegations.delegated.is_empty() && delegates_delegations.delegated_to.is_empty() {
                DELEGATIONS.remove(deps.storage, valid_gov_addr.clone());
            } else {
                DELEGATIONS.save(deps.storage, valid_gov_addr.clone(), &delegates_delegations)?;
            }

            //Subtract from staker's delegated_to
            match staker_delegation_info.delegated_to.iter().enumerate().find(|(_i, delegation)| delegation.delegator == valid_gov_addr.clone()){
                Some((index, _)) => match staker_delegation_info.delegated_to[index].amount.checked_sub(mbrn_amount){
                    Ok(new_amount) => staker_delegation_info.delegated_to[index].amount = new_amount,
                    Err(_) => {
                        //If more than delegated, remove from staker's delegated_to
                        staker_delegation_info.delegated_to.remove(index);
                    }
                },
                None => {
                    return Err(ContractError::CustomError {
                        val: String::from("Delegate not found in staker's delegated_to"),
                    });
                }
            };
            
            //Remove if empty, save otherwise
            if staker_delegation_info.delegated.is_empty() && staker_delegation_info.delegated_to.is_empty() {
                DELEGATIONS.remove(deps.storage, info.clone().sender);
            } else {
                DELEGATIONS.save(deps.storage, info.clone().sender, &staker_delegation_info)?;
            }
        }
    }

    
    //Edit & save staker's commission
    if let Some(commission) = commission {
        if let Ok(mut staker_delegation_info) = DELEGATIONS.load(deps.storage, info.sender.clone()){
            staker_delegation_info.commission = commission;
            DELEGATIONS.save(deps.storage, info.sender.clone(), &staker_delegation_info)?;
        }
    }

    //Update fluidity for both staker & delegate info if fluidity is Some
    if let Some(fluid) = fluid {
        //Staker's delegations
        if let Ok(mut staker_delegation_info) = DELEGATIONS.load(deps.storage, info.sender.clone()){
            staker_delegation_info.delegated_to = staker_delegation_info.delegated_to.clone()
                .into_iter()
                .map(|delegation| {
                    if delegation.delegator == valid_gov_addr.clone() {
                        Delegation {
                            fluidity: fluid,
                            ..delegation
                        }
                    } else {
                        delegation
                    }
                })
                .collect::<Vec<Delegation>>();
            DELEGATIONS.save(deps.storage, info.sender.clone(), &staker_delegation_info)?;
        };

        //Delegate's delegations
        if let Ok(mut delegates_delegations) = DELEGATIONS.load(deps.storage, valid_gov_addr.clone()){
            delegates_delegations.delegated = delegates_delegations.delegated.clone()
                .into_iter()
                .map(|delegation| {
                    if delegation.delegator == info.sender.clone() {
                        Delegation {
                            fluidity: fluid,
                            ..delegation
                        }
                    } else {
                        delegation
                    }
                })
                .collect::<Vec<Delegation>>();
            DELEGATIONS.save(deps.storage, valid_gov_addr.clone(), &delegates_delegations)?;
        };        
    }
    
    Ok(Response::new().add_attributes(vec![
        attr("action", "update_delegations"),
        attr("delegator", info.sender),
        attr("delegate", valid_gov_addr),
        attr("amount", mbrn_amount),
    ]))
}

/// Delegating Fluid delegatations
/// Delegates don't need to be stakers
/// Delegator loses control over the delegated amount, i.e. the initial staker retains  control over all delegated amounts
fn delegate_fluid_delegations(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    governator_addr: String,
    mbrn_amount: Option<Uint128>,
) -> Result<Response, ContractError>{    
    //Validate Governator, doesn't need to be a staker but can't be the user
    let valid_gov_addr = deps.api.addr_validate(&governator_addr)?;
    if valid_gov_addr == info.clone().sender {
        return Err(ContractError::CustomError {
            val: String::from("Delegate cannot be the user"),
        });
    }

    //Get delegate's delegations, assert they are a delegate
    let mut delegator_delegation_info = DELEGATIONS.load(deps.storage, info.clone().sender.clone())?;

    //Set delegation_info variants
    let mut fluid_delegations: Vec<Delegation> = delegator_delegation_info.delegated.clone()
        .into_iter()
        .filter(|delegation| delegation.fluidity)
        .collect();
    let non_fluid_delegations: Vec<Delegation> = delegator_delegation_info.delegated.clone()
        .into_iter()
        .filter(|delegation| !delegation.fluidity)
        .collect();

    //Set total_fluid_delegatible_amount
    let total_fluid_delegatible_amount = delegator_delegation_info.delegated
        .iter()
        .filter(|delegation| delegation.fluidity)
        .map(|delegation| delegation.amount)
        .collect::<Vec<Uint128>>()
        .into_iter()
        .sum::<Uint128>();
    //Validate MBRN amount
    let mut mbrn_amount = mbrn_amount.unwrap_or(total_fluid_delegatible_amount);
    
    if total_fluid_delegatible_amount < mbrn_amount {
        return Err(ContractError::CustomError {
            val: String::from("MBRN amount exceeds total fluid delegatible amount"),
        });
    }
    if total_fluid_delegatible_amount.is_zero() {
        return Err(ContractError::CustomError {
            val: String::from("No fluid delegations to delegate"),
        });
    }
 
    //Parse through delegator's fluid delegations
    for (i, delegation) in fluid_delegations.clone().into_iter().enumerate() {
        //If delegation amount is less than mbrn_amount, remove delegation from delegator's delegated
        let delegation_amount = if delegation.amount <= mbrn_amount {
            fluid_delegations.remove(i);
            //Subtract delegation amount from mbrn_amount
            mbrn_amount -= delegation.amount;

            delegation.amount
        } else {
            //If delegation amount is greater than mbrn_amount, subtract mbrn_amount from delegation amount
            fluid_delegations[i].amount -= mbrn_amount;

            let delegation_amount = mbrn_amount;

            //Set mbrn_amount to 0
            mbrn_amount = Uint128::zero();

            delegation_amount
        };

        //Delegate delegation_amount to governator
        let mut delegate_delegation_info = match DELEGATIONS.load(deps.storage, valid_gov_addr.clone()){
            Ok(delegation_info) => delegation_info,
            Err(_) => DelegationInfo {
                delegated_to: vec![],
                delegated: vec![],
                commission: Decimal::zero(),
            }
        };
        match delegate_delegation_info.delegated.iter().enumerate().find(|(_i, listed_delegation)| listed_delegation.delegator == delegation.delegator.clone()){
            Some((index, _)) => delegate_delegation_info.delegated[index].amount += delegation_amount,
            None => {
                delegate_delegation_info.delegated.push(Delegation {
                    delegator: delegation.delegator.clone(),
                    amount: delegation_amount,
                    fluidity: true,
                    time_of_delegation: env.block.time.seconds(),
                });
            }
        };
        //Save delegate's info           
        DELEGATIONS.save(deps.storage, valid_gov_addr.clone(), &delegate_delegation_info)?;

        //Add delegation_amount to initial delegator's delegated_to
        let mut initial_delegator_delegation_info = DELEGATIONS.load(deps.storage, delegation.delegator.clone())?;
        match initial_delegator_delegation_info.delegated_to.iter().enumerate().find(|(_i, listed_delegation)| listed_delegation.delegator == valid_gov_addr.clone()){
            Some((index, _)) => initial_delegator_delegation_info.delegated_to[index].amount += delegation_amount,
            None => {
                initial_delegator_delegation_info.delegated_to.push(Delegation {
                    delegator: valid_gov_addr.clone(),
                    amount: delegation_amount,
                    fluidity: true,
                    time_of_delegation: env.block.time.seconds(),
                });
            }
        };
        //Subtract delegation_amount from initial delegator's delegated_to from the executing delegator
        if let Some((index, _)) = initial_delegator_delegation_info.delegated_to.iter().enumerate().find(|(_i, listed_delegation)| listed_delegation.delegator == info.clone().sender){
            //Subtract delegation_amount
            initial_delegator_delegation_info.delegated_to[index].amount -= delegation_amount;
        } else {
            //This should be unreachable
            return Err(ContractError::CustomError {
                val: String::from("Delegator is not a delegate of the initial delegator"),
            });
        };
        //Save initial delegator's info
        DELEGATIONS.save(deps.storage, delegation.delegator.clone(), &initial_delegator_delegation_info)?;

        //If mbrn_amount is 0, break
        if mbrn_amount == Uint128::zero() {
            break;
        }
    }

    //Update delegator's delegations
    fluid_delegations.extend(non_fluid_delegations);
    delegator_delegation_info.delegated = fluid_delegations;
    DELEGATIONS.save(deps.storage, info.clone().sender.clone(), &delegator_delegation_info)?;

    
    Ok(Response::new().add_attributes(vec![
        attr("action", "delegate_fluid_delegations"),
        attr("delegator", info.sender),
        attr("delegate", valid_gov_addr),
        attr("amount", mbrn_amount),
    ]))
}

/// Restake unstaking deposits for a user
fn restake(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    mut restake_amount: Uint128,
) -> Result<Response, ContractError> {
    //Load state
    let config = CONFIG.load(deps.storage)?;
    let incentive_schedule = INCENTIVE_SCHEDULING.load(deps.storage)?;
    let fee_events = FEE_EVENTS.load(deps.storage)?;
    let DelegationInfo { delegated, delegated_to, commission: _ } = match DELEGATIONS.load(deps.storage, info.sender.clone()){
        Ok(delegation_info) => delegation_info,
        Err(_) => DelegationInfo {
            delegated: vec![],
            delegated_to: vec![],
            commission: Decimal::zero(),
        }
    };

    //Initialize variables
    let mut claimables: Vec<Asset> = vec![];
    let mut accrued_interest = Uint128::zero();
    let initial_restake = restake_amount;
    let mut error: Option<StdError> = None;

    //Load deposits
    let deposits = STAKED.load(deps.storage, info.clone().sender)?;
    //Calc total deposits past fee wait period
    let total_rewarding_stake: Uint128 = deposits.clone()
        .into_iter()
        .filter(|deposit| deposit.stake_time + (config.fee_wait_period * SECONDS_PER_DAY) >= env.block.time.seconds())
        .map(|deposit| deposit.amount)
        .sum();

    //Iterate through deposits
    let restaked_deposits: Vec<StakeDeposit> = deposits.clone()
        .into_iter()
        .map(|mut deposit| {
            if !restake_amount.is_zero() {
                if deposit.amount >= restake_amount {
                    //Zero restake_amount
                    restake_amount = Uint128::zero();

                    //Add claimables from this deposit
                    match add_deposit_claimables(
                        deps.storage,
                        config.clone(),
                        incentive_schedule.clone(),
                        env.clone(),
                        fee_events.clone(),
                        deposit.clone(),
                        delegated_to.clone(),
                        delegated.clone(),
                        &mut claimables,
                        &mut accrued_interest,
                        total_rewarding_stake,
                    ) {
                        Ok(res) => res,
                        Err(err) => 
                            error = Some(err)                        
                    };
                    
                    //Restake
                    deposit.unstake_start_time = None;
                    deposit.stake_time = env.block.time.seconds();
                } else if deposit.amount < restake_amount {
                    //Sub from restake_amount
                    restake_amount -= deposit.amount;

                    //Add claimables from this deposit
                    match add_deposit_claimables(
                        deps.storage,
                        config.clone(),
                        incentive_schedule.clone(),
                        env.clone(),
                        fee_events.clone(),
                        deposit.clone(),
                        delegated_to.clone(),
                        delegated.clone(),
                        &mut claimables,
                        &mut accrued_interest,
                        total_rewarding_stake,
                    ) {
                        Ok(res) => res,
                        Err(err) => 
                            error = Some(err)                        
                    };
                    

                    //Restake
                    deposit.unstake_start_time = None;
                    deposit.stake_time = env.block.time.seconds();
                }
            }
            deposit
        })
        .collect::<Vec<StakeDeposit>>();

    //Return error if any
    if let Some(err) = error {
        return Err(ContractError::Std(err));
    }

    //Create rewards msgs
    let rewards_msgs = create_rewards_msgs(
        config.clone(),
        claimables,
        accrued_interest,
        info.clone().sender.to_string(),
        vec![],
    )?;

    //Save new Deposits
    STAKED.save(deps.storage, info.clone().sender,&restaked_deposits)?;

    Ok(Response::new().add_messages(rewards_msgs).add_attributes(vec![
        attr("method", "restake"),
        attr("restake_amount", initial_restake),
    ]))
}

/// Sends available claims to info.sender or as specified in send_to.
/// If claim_as is passed, the claims will be sent as said asset.
/// If restake is true, the accrued ownership will be restaked.
pub fn claim_rewards(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    send_to: Option<String>,
    restake: bool,
) -> Result<Response, ContractError> {
    let config: Config = CONFIG.load(deps.storage)?;

    let mut messages: Vec<CosmosMsg>;
    let accrued_interest: Uint128;
    let user_claimables: Vec<Asset>;

    //Get user claim msgs and accrued interest
    (messages, user_claimables, accrued_interest) = user_claims(
        deps.storage,
        deps.api,
        env.clone(),
        config.clone(),
        info.clone(),
        send_to.clone(),
    )?;

    //Create MBRN Mint Msg
    if config.osmosis_proxy.is_some() {
        //Vesting contract gets no MBRN inflation
        if info.sender != config.clone().vesting_contract.unwrap_or_else(|| Addr::unchecked("")) && !accrued_interest.is_zero() {
            //Who to send to?
            if send_to.is_some() {
                let valid_recipient = deps.api.addr_validate(&send_to.clone().unwrap())?;

                let message = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
                    msg: to_binary(&OsmoExecuteMsg::MintTokens {
                        denom: config.mbrn_denom,
                        amount: accrued_interest,
                        mint_to_address: valid_recipient.to_string(),
                    })?,
                    funds: vec![],
                });
                messages.push(message);
            } else if restake {
                //Mint to contract
                let message = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
                    msg: to_binary(&OsmoExecuteMsg::MintTokens {
                        denom: config.clone().mbrn_denom,
                        amount: accrued_interest,
                        mint_to_address: env.contract.address.to_string(),
                    })?,
                    funds: vec![],
                });
                messages.push(message);
                //Stake for user
                let message = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: env.contract.address.to_string(),
                    msg: to_binary(&ExecuteMsg::Stake {
                        user: Some(info.sender.to_string()),
                    })?,
                    funds: vec![coin(accrued_interest.u128(), config.mbrn_denom)],
                });
                messages.push(message);
            } else {
                //Send stake to sender
                let message = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: config.osmosis_proxy.unwrap().to_string(),
                    msg: to_binary(&OsmoExecuteMsg::MintTokens {
                        denom: config.mbrn_denom,
                        amount: accrued_interest,
                        mint_to_address: info.sender.to_string(),
                    })?,
                    funds: vec![],
                });
                messages.push(message);
            }
        }
    } else {
        return Err(ContractError::CustomError {
            val: String::from("No proxy contract setup"),
        });
    }

    //Error if there is nothing to claim
    if messages.is_empty() {
        return Err(ContractError::CustomError {
            val: String::from("Nothing to claim"),
        });
    }

    let user_claimables_string: Vec<String> = user_claimables
        .into_iter()
        .map(|claims| claims.to_string())
        .collect::<Vec<String>>();

    let res = Response::new()
        .add_attribute("method", "claim")
        .add_attribute("user", info.sender)
        .add_attribute("send_to", send_to.unwrap_or_else(|| String::from("None")))
        .add_attribute("restake", restake.to_string())
        .add_attribute("mbrn_rewards", accrued_interest.to_string())
        .add_attribute("fee_rewards", format!("{:?}", user_claimables_string));

    Ok(res.add_messages(messages))
}

/// Deposit assets for staking rewards
fn deposit_fee(
    deps: DepsMut,
    env: Env,
    fee_assets: Vec<Asset>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let mut messages: Vec<CosmosMsg> = vec![];

    //Create response attribute
    let string_fee_assets = fee_assets.clone()
        .into_iter()
        .map(|asset| asset.to_string())
        .collect::<Vec<String>>();

    //Get CDT denom
    let basket: Basket = deps.querier.query_wasm_smart(
        config.clone().positions_contract.unwrap_or_else(|| Addr::unchecked("")), 
        &CDP_QueryMsg::GetBasket{ })?;
    let cdt_denom = basket.credit_asset.info;

    //If fee asset isn't CDT, send to Fee Auction if the contract is set
    let non_CDT_assets = fee_assets.clone()
        .into_iter()
        .filter(|fee_asset| fee_asset.info != cdt_denom)
        .collect::<Vec<Asset>>();
    
    //Act if there are non-CDT assets
    if non_CDT_assets.len() != 0 {
        if let Some(auction_contract) = config.clone().auction_contract {
            //Create auction msgs
            for asset in non_CDT_assets.clone() {
                let message: CosmosMsg = CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: auction_contract.to_string(),
                    msg: to_binary(&AuctionExecuteMsg::StartAuction { 
                        repayment_position_info: None, 
                        send_to: None, 
                        auction_asset: asset.clone(),
                    })?,
                    funds: vec![asset_to_coin(asset)?],
                });

                messages.push(message);
            }
        }
    }

    //Remove non-CDT assets from fee assets
    let CDT_assets = fee_assets.clone()
        .into_iter()
        .filter(|fee_asset| fee_asset.info == cdt_denom)
        .collect::<Vec<Asset>>();

    //Load Fee Events
    let mut fee_events = FEE_EVENTS.load(deps.storage)?;

    //Load Total staked
    let mut totals = STAKING_TOTALS.load(deps.storage)?;

    //Update vesting total
    if let Some(vesting_contract) = config.clone().vesting_contract {        
        let vesting_total = get_total_vesting(deps.querier, vesting_contract.to_string())?;

        totals.vesting_contract = vesting_total;
        STAKING_TOTALS.save(deps.storage, &totals)?;
    }

    //Set total
    let mut total: Uint128 = totals.vesting_contract + totals.stakers;
    if total.is_zero() {
        total = Uint128::new(1u128)
    }
    let decimal_total = Decimal::from_ratio(total, Uint128::new(1u128));
    
    //Add new Fee Event
    for asset in CDT_assets.clone() {        
        let amount = Decimal::from_ratio(asset.amount, Uint128::new(1u128));
        
        fee_events.push(FeeEvent {
            //We add the fee wait period so that the Fee distribution amount is correct
            //since deposits don't become eligible until the wait period is over yet they are added to the deposit_total at deposit
            time_of_event: env.block.time.seconds() + (config.clone().fee_wait_period * SECONDS_PER_DAY),
            fee: LiqAsset {
                //Amount = Amount per Staked MBRN
                info: asset.info,
                amount: decimal_division(amount, decimal_total)?,
            },
        });
    }

    FEE_EVENTS.save(deps.storage, &fee_events)?;
    
    Ok(Response::new().add_messages(messages).add_attributes(vec![
        attr("method", "deposit_fee"),
        attr("fee_assets", format!("{:?}", string_fee_assets)),
    ]))
}

/// Create rewards msgs from claimables and accrued interest
fn create_rewards_msgs(
    config: Config,
    claimables: Vec<Asset>,
    accrued_interest: Uint128,
    user: String,
    mut native_claims: Vec<Coin>,
) -> StdResult<Vec<CosmosMsg>>{

    let mut msgs: Vec<CosmosMsg> = vec![];

    //Create msg for claimable fees
    if claimables != vec![] {
        //Aggregate native tokens
        for asset in claimables {
            match asset.clone().info {
                AssetInfo::Token { address: _ } => {
                    return Err(StdError::GenericErr { msg: String::from("Non-native token unclaimable") })
                }
                AssetInfo::NativeToken { denom: _ } => {
                    native_claims.push(asset_to_coin(asset)?);
                }
            }
        }
    }

    if native_claims != vec![] {
        let msg = CosmosMsg::Bank(BankMsg::Send {
            to_address: user.clone(),
            amount: native_claims,
        });
        msgs.push(msg);
    }

    //Create msg to mint accrued interest
    if !accrued_interest.is_zero() {
        let message = CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: config.clone().osmosis_proxy.unwrap().to_string(),
            msg: to_binary(&OsmoExecuteMsg::MintTokens {
                denom: config.clone().mbrn_denom,
                amount: accrued_interest,
                mint_to_address: user,
            })?,
            funds: vec![],
        });
        msgs.push(message);
    }

    Ok(msgs)
}

/// Get deposit claims and add to list of claims/total interest
fn add_deposit_claimables(
    storage: &mut dyn Storage,
    config: Config,
    incentive_schedule: StakeDistributionLog,
    env: Env,
    fee_events: Vec<FeeEvent>,
    deposit: StakeDeposit,
    delegated_to: Vec<Delegation>,
    delegated: Vec<Delegation>,
    claimables: &mut Vec<Asset>,
    accrued_interest: &mut Uint128,
    total_rewarding_stake: Uint128, //stake thats being rewarded
) -> StdResult<()>{
    //Calc claimables from this deposit
    let (deposit_claimables, deposit_interest) = get_deposit_claimables(
        storage,
        config.clone(),
        incentive_schedule.clone(),
        env.clone(),
        fee_events.clone(),
        deposit.clone(),
        delegated.clone(),
        delegated_to.clone(),
        total_rewarding_stake,
    )?;
    *accrued_interest += deposit_interest;

    //Condense like Assets
    for claim_asset in deposit_claimables {
        //Check if asset is already in the list of claimables and add according
        match claimables
            .clone()
            .into_iter()
            .enumerate()
            .find(|(_i, asset)| asset.info == claim_asset.info)
        {
            Some((index, _asset)) => claimables[index].amount += claim_asset.amount,
            None => claimables.push(claim_asset),
        }
    }

    Ok(())
}
/// Can't Unstake if...
/// 1. There is an active proposal by the address
/// 2. The address has voted for a proposal that has passed but not yet executed
pub fn can_this_addr_unstake(
    querier: QuerierWrapper,
    user: Addr,
    config: Config,
) -> Result<(), ContractError> {
    
    //Can't unstake if there is an active proposal by user
    let proposal_list: ProposalListResponse = querier.query(&QueryRequest::Wasm(WasmQuery::Smart { 
        contract_addr: config.clone().governance_contract.unwrap().to_string(), 
        msg: to_binary(&Gov_QueryMsg::Proposals { start: None, limit: None })?
    }))?;

    for proposal in proposal_list.clone().proposal_list {
        if proposal.submitter == user && proposal.status == ProposalStatus::Active {
            return Err(ContractError::CustomError { val: String::from("Can't unstake while your proposal is active") })
        }
    }

    //Can't unstake if the user has voted for a proposal that has passed but not yet executed    
    //Get list of proposals that have passed & have executables
    for proposal in proposal_list.proposal_list {
        if proposal.status == ProposalStatus::Passed && proposal.messages.is_some() {
            //Get list of voters for this proposal
            let _voters: Vec<Addr> = querier.query_wasm_smart(
                config.clone().governance_contract.unwrap().to_string(), 
                &Gov_QueryMsg::ProposalVoters { 
                    proposal_id: proposal.proposal_id.into(), 
                    vote_option: membrane::governance::ProposalVoteOption::For, 
                    start: None, 
                    limit: None,
                    specific_user: Some(user.to_string())
                }
            )?;
            // if the query doesn't error then the user has voted For this proposal
            return Err(ContractError::CustomError { val: String::from("Can't unstake if the proposal you helped pass hasn't executed its messages yet") })
        }
    }

    Ok(())
}

/// Update deposits being withdrawn from.
/// Returns claimable assets, accrued interest, withdrawable amount.
fn withdraw_from_state(
    storage: &mut dyn Storage,
    env: Env,
    staker: Addr,
    mut withdrawal_amount: Uint128,
    fee_events: Vec<FeeEvent>,
) -> StdResult<(Vec<Asset>, Uint128, Uint128)> {
    let config = CONFIG.load(storage)?;
    let incentive_schedule = INCENTIVE_SCHEDULING.load(storage)?;
    let deposits = STAKED.load(storage, staker.clone())?;
    let DelegationInfo { delegated, delegated_to, commission: _ } = match DELEGATIONS.load(storage, staker.clone()){
        Ok(delegation_info) => delegation_info,
        Err(_) => DelegationInfo {
            delegated: vec![],
            delegated_to: vec![],
            commission: Decimal::zero(),
        }
    };

    let mut new_deposit_total = Uint128::zero();
    let mut accrued_interest = Uint128::zero();
    let mut withdrawable_amount = Uint128::zero();
    
    let mut claimables: Vec<Asset> = vec![];
    let mut error: Option<StdError> = None;
    let mut this_deposit_is_withdrawable = false;

    let mut returning_deposit: Option<StakeDeposit> = None;

    //Calc total deposits past fee wait period
    let total_rewarding_stake: Uint128 = deposits.clone()
        .into_iter()
        .filter(|deposit| deposit.stake_time + (config.fee_wait_period * SECONDS_PER_DAY) >= env.block.time.seconds())
        .map(|deposit| deposit.amount)
        .sum();

    //Iterate through deposits
    let mut new_deposits: Vec<StakeDeposit> = deposits
        .into_iter()
        .map(|mut deposit| {
            
            //If the deposit has started unstaking
            if let Some(deposit_unstake_start) = deposit.unstake_start_time {
                //If the unstake period has been fulfilled
                if env.block.time.seconds() - deposit_unstake_start
                    >= config.unstaking_period
                {
                    this_deposit_is_withdrawable = true;
                }
            }

            //Subtract from each deposit until there is none left to withdraw
            if withdrawal_amount != Uint128::zero() && deposit.amount > withdrawal_amount {
                
                //Only add claims if the deposit has passed the fee wait period
                if deposit.stake_time + (config.fee_wait_period * SECONDS_PER_DAY) <= env.block.time.seconds() {
                    //Add claimables from this deposit
                    match add_deposit_claimables(
                        storage,
                        config.clone(),
                        incentive_schedule.clone(),
                        env.clone(),
                        fee_events.clone(),
                        deposit.clone(),
                        delegated_to.clone(),
                        delegated.clone(),
                        &mut claimables,
                        &mut accrued_interest,
                        total_rewarding_stake,
                    ) {
                        Ok(res) => res,
                        Err(err) => 
                            error = Some(err)                        
                    };
                }

                //If withdrawable...
                //Set partial deposit total
                //Set current deposit to 0
                //Add withdrawal_amount to withdrawable_amount
                if this_deposit_is_withdrawable {
                    new_deposit_total = deposit.amount - withdrawal_amount;
                    withdrawable_amount += deposit.amount;
                    deposit.amount = Uint128::zero();

                    this_deposit_is_withdrawable = false;
                } else {
                    
                    //Since we claimed rewards
                    deposit.stake_time = env.block.time.seconds();                        
                    
                    //Set unstaking time for the amount getting withdrawn
                    //Create a StakeDeposit object for the amount not getting unstaked
                    //Set new deposit
                    returning_deposit = Some(StakeDeposit {
                        amount: deposit.amount - withdrawal_amount,
                        unstake_start_time: None,
                        ..deposit.clone()
                    });
                    
                    //Set new deposit amount
                    deposit.amount = withdrawal_amount;                       

                    //Set the unstaking_start_time and stake_time to now
                    deposit.unstake_start_time = Some(env.block.time.seconds());
                }

                //Zero withdrawal_amount since the deposit total fulfills the withdrawal
                withdrawal_amount = Uint128::zero();

            } else if withdrawal_amount != Uint128::zero() && deposit.amount <= withdrawal_amount {

                //Only add claims if the deposit has passed the fee wait period
                if deposit.stake_time + (config.fee_wait_period * SECONDS_PER_DAY) <= env.block.time.seconds() {
                    //Add claimables from this deposit
                    match add_deposit_claimables(
                        storage,
                        config.clone(),
                        incentive_schedule.clone(),
                        env.clone(),
                        fee_events.clone(),
                        deposit.clone(),
                        delegated_to.clone(),
                        delegated.clone(),
                        &mut claimables,
                        &mut accrued_interest,
                        total_rewarding_stake,
                    ) {
                        Ok(res) => res,
                        Err(err) => 
                            error = Some(err)                        
                    };
                }

                //Since it's less than the Deposit amount, substract it from the withdrawal amount
                withdrawal_amount -= deposit.amount;

                //If withdrawable...
                //Add deposit amount to withdrawable_amount
                //Set current deposit to 0
                if this_deposit_is_withdrawable {
                    withdrawable_amount += deposit.amount;
                    deposit.amount = Uint128::zero();

                    this_deposit_is_withdrawable = false;
                } else {
                    //Else, Set the unstaking_start_time and stake_time to now
                    deposit.unstake_start_time = Some(env.block.time.seconds());
                    //Since we claimed rewards
                    deposit.stake_time = env.block.time.seconds();
                }
        
            }
            deposit
        })
        .collect::<Vec<StakeDeposit>>()
        .into_iter()
        .filter(|deposit| deposit.amount != Uint128::zero())
        .collect::<Vec<StakeDeposit>>();

    if withdrawal_amount != Uint128::zero() {
        return Err(StdError::GenericErr {
            msg: format!(
                "Attempting to withdraw {} MBRN over ( {} )'s total deposit",
                withdrawal_amount, staker
            ),
        });
    }

    if error.is_some() {
        return Err(error.unwrap());
    }

    //Push returning_deposit if Some
    //This can be done outside the loop bc it can only happen once
    if let Some(deposit) = returning_deposit {
        new_deposits.push(deposit);
    }

    //We set any edited deposit to zero and push any partial withdrawals back to the list here
    if !new_deposit_total.is_zero() {
        new_deposits.push(StakeDeposit {
            staker: staker.clone(),
            amount: new_deposit_total,
            stake_time: env.block.time.seconds(),
            unstake_start_time: None,
        });
    }
    //Save new deposit stack
    STAKED.save(storage, staker.clone(), &new_deposits)?;

    Ok((claimables, accrued_interest, withdrawable_amount))
}


/// Calculates the accrued interest for a given stake
fn accumulate_interest(stake: Uint128, rate: Decimal, time_elapsed: u64) -> StdResult<Uint128> {
    let applied_rate = rate.checked_mul(Decimal::from_ratio(
        Uint128::from(time_elapsed),
        Uint128::from(SECONDS_PER_YEAR),
    ))?;

    let accrued_interest = stake * applied_rate;

    Ok(accrued_interest)
}

/// Return claim messages for a given user 
fn user_claims(
    storage: &mut dyn Storage,
    api: &dyn Api,
    env: Env,
    config: Config,
    info: MessageInfo,
    send_to: Option<String>,
) -> StdResult<(Vec<CosmosMsg>, Vec<Asset>, Uint128)> {
    //Can only claim for oneself (info.sender)
    let (user_claimables, accrued_interest) =
        get_user_claimables(storage, env, info.clone().sender)?;

    //Claim the available assets///
    //If we are sending to the sender
    if send_to.clone().is_none() {                
        //Send to sender
        let rewards_msgs = create_rewards_msgs(
            config.clone(), 
            user_claimables.clone(), 
            Uint128::zero(), //Dont send interest here
            info.clone().sender.to_string(), 
            vec![],
        )?;
        
        return Ok((rewards_msgs, user_claimables, accrued_interest))
    } else {
        //Validate recipient
        let valid_recipient = api.addr_validate(&send_to.clone().unwrap())?;

        //Send to recipient
        let rewards_msgs = create_rewards_msgs(
            config.clone(), 
            user_claimables.clone(), 
            Uint128::zero(), //Dont send interest here
            valid_recipient.to_string(), 
            vec![],
        )?;

        return Ok((rewards_msgs, user_claimables, accrued_interest))
    }  
}

/// Return user claimables for a given user
fn get_user_claimables(
    storage: &mut dyn Storage,
    env: Env,
    staker: Addr,
) -> StdResult<(Vec<Asset>, Uint128)> {

    //Load state
    let config = CONFIG.load(storage)?;
    let incentive_schedule = INCENTIVE_SCHEDULING.load(storage)?;
    let deposits: Vec<StakeDeposit> = STAKED.load(storage, staker.clone())?;
    let DelegationInfo { delegated, delegated_to, commission: _ } = match DELEGATIONS.load(storage, staker.clone()){
        Ok(res) => res,
        Err(_) => DelegationInfo {
            delegated: vec![],
            delegated_to: vec![],
            commission: Decimal::zero(),
        },
    };

    if deposits == vec![] {
        return Err(StdError::GenericErr {
            msg: String::from("User has no stake"),
        });
    }

    //Load Fee events
    let fee_events = FEE_EVENTS.load(storage)?;

    let mut claimables: Vec<Asset> = vec![];
    let mut total_deposits = Uint128::zero();
    let mut accrued_interest = Uint128::zero();
    
    //Filter for deposits past the fee wait period
    let deposits = deposits
        .into_iter()
        .filter(|deposit| {
            deposit.stake_time + (config.fee_wait_period * SECONDS_PER_DAY) <= env.block.time.seconds()
        })
        .collect::<Vec<StakeDeposit>>();
    
    //Calc total deposits past fee wait period
    let total_rewarding_stake: Uint128 = deposits.clone()
        .into_iter()
        .filter(|deposit| deposit.stake_time + (config.fee_wait_period * SECONDS_PER_DAY) >= env.block.time.seconds())
        .map(|deposit| deposit.amount)
        .sum();

    //Get claimables per deposit
    for deposit in deposits {
        add_deposit_claimables(
            storage,
            config.clone(), 
            incentive_schedule.clone(), 
            env.clone(), 
            fee_events.clone(), 
            deposit.clone(), 
            delegated_to.clone(),
            delegated.clone(),
            &mut claimables, 
            &mut accrued_interest,
            total_rewarding_stake,
        )?;

        //Total deposits
        total_deposits += deposit.amount;
    }

    //Save new condensed deposit for user
    STAKED.save(storage, staker.clone(), &vec![
        StakeDeposit {
            staker,
            amount: total_deposits,
            //Subtract wait period so this deposit isn't waiting anymore but add 1 to put past its claimed events
            stake_time: env.block.time.seconds() - (config.fee_wait_period * SECONDS_PER_DAY) + 1,
            unstake_start_time: None,
    }])?;

    Ok((claimables, accrued_interest))
}

/// Trim fee events to only include events after the earliest deposit
fn trim_fee_events(
    storage: &mut dyn Storage,
    info: MessageInfo,
) -> Result<Response, ContractError>{

    let config = CONFIG.load(storage)?;

    if info.sender != config.owner { return Err( ContractError::Unauthorized {  } )}

    let mut fee_events = FEE_EVENTS.load(storage)?;

    //Initialize earliest deposit
    let mut earliest_deposit = None;

    let _iter = STAKED
        .range(storage, None, None, cosmwasm_std::Order::Ascending)
        .map(|stakers| {
            let (_, deposits) = stakers.unwrap();

            //Set earliest deposit to first deposit
            let mut earliest_deposit_loop = deposits[0].clone().stake_time;

            //Find the earliest deposit
            for deposit in deposits {
                if deposit.stake_time < earliest_deposit_loop {
                    earliest_deposit_loop = deposit.stake_time;
                }
            }

            earliest_deposit = Some(earliest_deposit_loop);
        })
        .collect::<Vec<()>>();

    //Filter for fee events that are after the earliest deposit to trim state
    if let Some(earliest_deposit) = earliest_deposit{
        fee_events = fee_events.clone()
            .into_iter()
            .filter(|event| event.time_of_event > earliest_deposit)
            .collect::<Vec<FeeEvent>>();
    }
    //In a situation where no one is staked the contract will need to be upgraded to handle its assets
    //This won't happen as long as the builder's allocation is vesting so the functionality isn't necessary rn
    
    //Save Fee events
    FEE_EVENTS.save(storage, &fee_events)?;

    Ok(Response::new().add_attribute("trimmed", "true"))
}

pub fn get_delegation_commission(
    storage: &dyn Storage,
    delegated: Vec<Delegation>,
    delegated_to: Vec<Delegation>,
    total_rewarding_stake: Uint128,
) -> StdResult<(Decimal, Decimal)>{
    if total_rewarding_stake == Uint128::zero() {
        return Ok((Decimal::zero(), Decimal::zero()))
    }

    //Initialize the total the amount of MBRN delegated_to a delegate
    let mut total_delegated_to = Uint128::zero();

    //Get the average commission rate of the delegations
    let commission_rate = {
        let mut commission_rates: Vec<(Decimal, Uint128)> = vec![];

        //Create tuples for (Commission rate + total delegated_to) for each delegate
        for delegation in delegated_to.clone() {
            let delegator_commission = DELEGATIONS.load(storage, delegation.delegator.clone())?.commission;

            let like_delegations = delegated_to.clone()
                .into_iter()
                .filter(|delegation| delegation.delegator == delegation.delegator)
                .collect::<Vec<Delegation>>();

            let delegated_to_sum: Uint128 = like_delegations
                .into_iter()
                .map(|delegation| delegation.amount)
                .collect::<Vec<Uint128>>()
                .iter()
                .sum();
            total_delegated_to += delegated_to_sum;

            commission_rates.push((delegator_commission, delegated_to_sum));
        }

        //Get the average commission rate, weighted by the amount delegated_to
        let mut weighted_commission_rate = Decimal::zero();
        for (commission_rate, delegated_to) in commission_rates {
            weighted_commission_rate += commission_rate * Decimal::from_ratio(delegated_to, total_delegated_to);
        }

        weighted_commission_rate
    };
    
    //Calc the ratio of the total delegated_to to the total stake
    let ratio = Decimal::from_ratio(total_delegated_to, total_rewarding_stake);

    //Calculate the per deposit commission rate
    let per_deposit_commission_subtraction = decimal_multiplication(ratio, commission_rate)?;

    
    ///////Now do the same for delegated, to add to this deposit's claimables///////
    /// //Initialize the total the amount of MBRN delegated to the depositor
    let mut total_delegated = Uint128::zero();

    //Get the average commission rate of the delegations
    let commission_rate = {
        let mut commission_rates: Vec<(Decimal, Uint128)> = vec![];

        //Create tuples for (Commission rate + total delegated) for each delegate
        for delegation in delegated.clone() {
            let delegator_commission = DELEGATIONS.load(storage, delegation.delegator.clone())?.commission;

            let like_delegations = delegated_to.clone()
                .into_iter()
                .filter(|delegation| delegation.delegator == delegation.delegator)
                .collect::<Vec<Delegation>>();

            let delegated_sum: Uint128 = like_delegations
                .into_iter()
                .map(|delegation| delegation.amount)
                .collect::<Vec<Uint128>>()
                .iter()
                .sum();
            total_delegated += delegated_sum;

            commission_rates.push((delegator_commission, delegated_sum));
        }

        //Get the average commission rate, weighted by the amount delegated_to
        let mut weighted_commission_rate = Decimal::zero();
        for (commission_rate, delegated) in commission_rates {
            weighted_commission_rate += commission_rate * Decimal::from_ratio(delegated, total_delegated);
        }

        weighted_commission_rate
    };
    
    //Calc the ratio of the total delegated_to to the total stake
    let ratio = Decimal::from_ratio(total_delegated, total_rewarding_stake);

    //Calculate the per deposit commission rate
    let per_deposit_commission_addition = decimal_multiplication(ratio, commission_rate)?;

    Ok((per_deposit_commission_subtraction, per_deposit_commission_addition))
}

/// Get deposit's claimable fee assets based on which FeeEvents it experienced
pub fn get_deposit_claimables(
    storage: &dyn Storage,
    mut config: Config,
    incentive_schedule: StakeDistributionLog,
    env: Env,
    fee_events: Vec<FeeEvent>,
    mut deposit: StakeDeposit,
    delegated: Vec<Delegation>,
    delegated_to: Vec<Delegation>,
    total_rewarding_stake: Uint128, //stake thats being rewarded
) -> StdResult<(Vec<Asset>, Uint128)> {
    let mut claimables: Vec<Asset> = vec![];

    //Filter for events that the deposit was staked for
    //ie event times after the deposit + fee_wait_period
    //&& events before the current block time, signifiying that the event has happened
    let wait_period_in_seconds = config.fee_wait_period * SECONDS_PER_DAY;
    let events_experienced = fee_events
        .into_iter()
        .filter(|event| event.time_of_event >= deposit.stake_time + wait_period_in_seconds && event.time_of_event <= env.block.time.seconds())
        .collect::<Vec<FeeEvent>>();
        
    //Filter for delegations that have past the waiting period
    let delegated = delegated
        .into_iter()
        .filter(|delegation| delegation.time_of_delegation + wait_period_in_seconds >= env.block.time.seconds())
        .collect::<Vec<Delegation>>();
    let delegated_to = delegated_to
        .into_iter()
        .filter(|delegation| delegation.time_of_delegation + wait_period_in_seconds >= env.block.time.seconds())
        .collect::<Vec<Delegation>>();

    //Get commission rates per deposit
    let (per_deposit_commission_subtraction, per_deposit_commission_addition) = get_delegation_commission(
        storage, 
        delegated.clone(), 
        delegated_to.clone(), 
        total_rewarding_stake,
    )?;

    //Subtract commission from deposit
    if per_deposit_commission_subtraction > Decimal::zero() {
        deposit.amount = decimal_multiplication(
            (Decimal::one() - per_deposit_commission_subtraction), 
            Decimal::from_ratio(deposit.amount, Uint128::one())
        )?.to_uint_floor();
    }

    //Add commission to deposit
    if per_deposit_commission_addition > Decimal::zero() {
        deposit.amount = decimal_multiplication(
            (Decimal::one() + per_deposit_commission_addition), 
            Decimal::from_ratio(deposit.amount, Uint128::one())
        )?.to_uint_floor();
    }

    //Add delegations into per deposit fee calculation && Condense like Assets in claimables
    for event in events_experienced {

        //Check if asset is already in the list of claimables and add accordingly
        match claimables
            .clone()
            .into_iter()
            .enumerate()
            .find(|(_i, asset)| asset.info == event.fee.info)
        {
            Some((index, _asset)) => claimables[index].amount += event.fee.amount * deposit.amount,
            None => claimables.push(Asset {
                info: event.fee.info,
                amount: event.fee.amount * deposit.amount,
            }),
        }
    }

    //Assert staking rate is still active, if not set to 0
    let rate_duration = incentive_schedule.ownership_distribution.duration * SECONDS_PER_DAY;
    if env.block.time.seconds() - incentive_schedule.start_time > rate_duration {
        config.incentive_schedule.rate = Decimal::zero();
    }

    //Calc MBRN denominated rewards
    let deposit_interest = if !config.incentive_schedule.rate.is_zero() {
        let time_elapsed = env.block.time.seconds() - deposit.stake_time;
        accumulate_interest(deposit.amount, config.incentive_schedule.rate, time_elapsed)?
    } else {
        Uint128::zero()
    };

    Ok((claimables, deposit_interest))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::UserStake { staker } => to_binary(&query_user_stake(deps, staker)?),
        QueryMsg::StakerRewards { staker } => to_binary(&query_staker_rewards(deps, env, staker)?),
        QueryMsg::Staked {
            limit,
            start_after,
            end_before,
            unstaking,
        } => to_binary(&query_staked(
            deps,
            env,
            limit,
            start_after,
            end_before,
            unstaking,
        )?),
        QueryMsg::Delegations { limit, start_after, user } => {
            to_binary(&query_delegations(deps, limit, start_after, user)?)
        }
        QueryMsg::FeeEvents { limit, start_after } => {
            to_binary(&query_fee_events(deps, limit, start_after)?)
        }
        QueryMsg::TotalStaked {} => to_binary(&query_totals(deps)?),
        QueryMsg::IncentiveSchedule {  } => to_binary(&INCENTIVE_SCHEDULING.load(deps.storage)?),
    }
}

