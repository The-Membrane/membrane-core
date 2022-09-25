///Expanded Fork of: https://github.com/astroport-fi/astroport-governance/tree/main/contracts/builder_unlock

//use std::error::Error;

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, coin, to_binary, Addr, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env,
    MessageInfo, QueryRequest, Response, StdError, StdResult, Uint128, WasmMsg, WasmQuery,
};
use cw2::set_contract_version;
use cw20::Cw20ExecuteMsg;

use membrane::builder_vesting::{
    AllocationResponse, ConfigResponse, ExecuteMsg, InstantiateMsg, QueryMsg, ReceiverResponse,
    UnlockedResponse,
};
use membrane::governance::{ExecuteMsg as GovExecuteMsg, ProposalMessage, ProposalVoteOption};
use membrane::math::decimal_division;
use membrane::osmosis_proxy::ExecuteMsg as OsmoExecuteMsg;
use membrane::staking::{
    ExecuteMsg as StakingExecuteMsg, QueryMsg as StakingQueryMsg, RewardsResponse,
};
use membrane::types::{Allocation, Asset, AssetInfo, VestingPeriod};

use crate::error::ContractError;
use crate::state::{Config, Receiver, CONFIG, RECEIVERS};

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:builder-vesting";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

//Constants
const SECONDS_IN_A_DAY: u64 = 86400u64;

/////////////////////
///**Make sure everything is allocated before fees are sent**
/////////////////////

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let mut config = Config {
        owner: info.sender,
        initial_allocation: msg.initial_allocation,
        mbrn_denom: msg.mbrn_denom,
        osmosis_proxy: deps.api.addr_validate(&msg.osmosis_proxy)?,
        staking_contract: deps.api.addr_validate(&msg.staking_contract)?,
    };

    //Set Optionals
    match msg.owner {
        Some(address) => match deps.api.addr_validate(&address) {
            Ok(addr) => config.owner = addr,
            Err(_) => {}
        },
        None => {}
    };

    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    CONFIG.save(deps.storage, &config)?;
    RECEIVERS.save(deps.storage, &vec![])?;

    let mut res = mint_initial_allocation(env.clone(), config.clone())?;

    let mut attrs = vec![
        attr("method", "instantiate"),
        attr("owner", config.owner.to_string()),
        attr("owner", env.contract.address.to_string()),
    ];
    attrs.extend(res.attributes);
    res.attributes = attrs;

    Ok(res)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Receive(_msg) => Ok(Response::new()),
        ExecuteMsg::AddReceiver { receiver } => add_receiver(deps, info, receiver),
        ExecuteMsg::RemoveReceiver { receiver } => remove_receiver(deps, info, receiver),
        ExecuteMsg::AddAllocation {
            receiver,
            allocation,
            vesting_period,
        } => add_allocation(deps, env, info, receiver, allocation, vesting_period),
        ExecuteMsg::DecreaseAllocation {
            receiver,
            allocation,
        } => decrease_allocation(deps, info, receiver, allocation),
        ExecuteMsg::WithdrawUnlocked {} => withdraw_unlocked(deps, env, info),
        ExecuteMsg::ClaimFeesforContract {} => claim_fees_for_contract(deps, env),
        ExecuteMsg::ClaimFeesforReceiver {} => claim_fees_for_receiver(deps, info),
        ExecuteMsg::SubmitProposal {
            title,
            description,
            link,
            messages,
        } => submit_proposal(deps, info, title, description, link, messages),
        ExecuteMsg::CastVote { proposal_id, vote } => cast_vote(deps, info, proposal_id, vote),
        ExecuteMsg::UpdateConfig {
            owner,
            mbrn_denom,
            osmosis_proxy,
            staking_contract,
        } => update_config(
            deps,
            info,
            owner,
            mbrn_denom,
            osmosis_proxy,
            staking_contract,
        ),
    }
}

fn submit_proposal(
    deps: DepsMut,
    info: MessageInfo,
    title: String,
    description: String,
    link: Option<String>,
    messages: Option<Vec<ProposalMessage>>,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let receivers = RECEIVERS.load(deps.storage)?;

    match receivers
        
        .into_iter()
        .find(|receiver| receiver.receiver == info.sender)
    {
        Some(receiver) => {
            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.owner.to_string(),
                msg: to_binary(&GovExecuteMsg::SubmitProposal {
                    title,
                    description,
                    link,
                    messages,
                    receiver: Some(receiver.receiver.to_string()),
                })?,
                funds: vec![],
            });

            Ok(Response::new()
                .add_attributes(vec![
                    attr("method", "submit_proposal"),
                    attr("proposer", receiver.receiver.to_string()),
                ])
                .add_message(message))
        }
        None => Err(ContractError::InvalidReceiver {}),
    }
}

fn cast_vote(
    deps: DepsMut,
    info: MessageInfo,
    proposal_id: u64,
    vote: ProposalVoteOption,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;
    let receivers = RECEIVERS.load(deps.storage)?;

    match receivers
        
        .into_iter()
        .find(|receiver| receiver.receiver == info.sender)
    {
        Some(receiver) => {
            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: config.owner.to_string(),
                msg: to_binary(&GovExecuteMsg::CastVote {
                    proposal_id,
                    vote,
                    receiver: Some(receiver.receiver.to_string()),
                })?,
                funds: vec![],
            });

            Ok(Response::new()
                .add_attributes(vec![
                    attr("method", "cast_vote"),
                    attr("voter", receiver.receiver.to_string()),
                ])
                .add_message(message))
        }
        None => Err(ContractError::InvalidReceiver {}),
    }
}

fn claim_fees_for_receiver(deps: DepsMut, info: MessageInfo) -> Result<Response, ContractError> {
    let mut receivers = RECEIVERS.load(deps.storage)?;

    let mut messages: Vec<CosmosMsg> = vec![];
    let claimables: Vec<Asset> = vec![];

    match receivers
        .clone()
        .into_iter()
        .enumerate()
        .find(|(_i, receiver)| receiver.receiver == info.sender)
    {
        Some((i, receiver)) => {
            if receiver.claimables == vec![] {
                return Err(ContractError::CustomError {
                    val: String::from("Nothing to claim"),
                });
            }

            //Create withdraw msg for each
            for claimable in receiver.clone().claimables {
                messages.push(withdrawal_msg(claimable, receiver.clone().receiver)?);
            }

            //Set claims to Empty Vec
            receivers[i].claimables = vec![];
        }
        None => return Err(ContractError::InvalidReceiver {}),
    }
    //Save Edited claims
    RECEIVERS.save(deps.storage, &receivers)?;

    //Claimables into String List
    let claimables_string: Vec<String> = claimables
        .into_iter()
        .map(|claim| claim.to_string())
        .collect::<Vec<String>>();

    Ok(Response::new().add_messages(messages).add_attributes(vec![
        attr("method", "claim_fees_for_receiver"),
        attr("claimables", format!("{:?}", claimables_string)),
    ]))
}

fn claim_fees_for_contract(deps: DepsMut, env: Env) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    //Query Rewards
    let res: RewardsResponse = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.staking_contract.to_string(),
        msg: to_binary(&StakingQueryMsg::StakerRewards {
            staker: env.contract.address.to_string(),
        })?,
    }))?;

    //Split rewards w/ Receivers
    if res.claimables != vec![] {
        let receivers = RECEIVERS.load(deps.storage)?;

        let mut allocated_receivers: Vec<Receiver> = receivers
            .clone()
            .into_iter()
            .filter(|receiver| receiver.allocation.is_some())
            .collect::<Vec<Receiver>>();

        //Calculate allocation ratios
        let allocation_ratios = get_allocation_ratios(config.clone(), allocated_receivers.clone())?;

        //Split between receivers
        for claim_asset in res.clone().claimables {
            for (i, receiver) in allocated_receivers.clone().into_iter().enumerate() {
                match receiver
                    .clone()
                    .claimables
                    .into_iter()
                    .enumerate()
                    .find(|(_index, claim)| claim.info == claim_asset.info)
                {
                    //If found in claimables, add amount to position
                    Some((index, _claim)) => {
                        allocated_receivers[i].claimables[index].amount +=
                            claim_asset.amount * allocation_ratios[i]
                    }
                    //If None, add asset as if new
                    None => allocated_receivers[i].claimables.push(Asset {
                        amount: claim_asset.amount * allocation_ratios[i],
                        ..claim_asset.clone()
                    }),
                }
            }
        }

        //Filter out, Extend, Save
        let mut new_receivers: Vec<Receiver> = receivers
            
            .into_iter()
            .filter(|receiver| receiver.allocation.is_none())
            .collect::<Vec<Receiver>>();
        new_receivers.extend(allocated_receivers);
        RECEIVERS.save(deps.storage, &new_receivers)?;
    }

    //Construct ClaimRewards Msg to Staking Contract
    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.staking_contract.to_string(),
        msg: to_binary(&StakingExecuteMsg::ClaimRewards {
            claim_as_cw20: None,
            claim_as_native: None,
            restake: false,
            send_to: None,
        })?,
        funds: vec![],
    });

    //Claimables into String List
    let claimables_string: Vec<String> = res
        .claimables
        .into_iter()
        .map(|claim| claim.to_string())
        .collect::<Vec<String>>();

    Ok(Response::new().add_message(msg).add_attributes(vec![
        attr("method", "claim_fees_for_contract"),
        attr("claimables", format!("{:?}", claimables_string)),
    ]))
}

fn get_allocation_ratios(config: Config, receivers: Vec<Receiver>) -> StdResult<Vec<Decimal>> {
    let mut allocation_ratios: Vec<Decimal> = vec![];
    for receiver in receivers {
        //Ratio of allocation.amount to initial_allocation
        allocation_ratios.push(decimal_division(
            Decimal::from_ratio(
                receiver.clone().allocation.unwrap().amount,
                Uint128::new(1u128),
            ),
            Decimal::from_ratio(config.clone().initial_allocation, Uint128::new(1u128)),
        ))
    }

    Ok(allocation_ratios)
}

fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    owner: Option<String>,
    mbrn_denom: Option<String>,
    osmosis_proxy: Option<String>,
    staking_contract: Option<String>,
) -> Result<Response, ContractError> {
    let mut config = CONFIG.load(deps.storage)?;

    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let mut attrs = vec![attr("method", "update_config")];

    match owner {
        Some(owner) => {
            config.owner = deps.api.addr_validate(&owner)?;
            attrs.push(attr("new_owner", owner));
        }
        None => {}
    };
    match osmosis_proxy {
        Some(osmosis_proxy) => {
            config.osmosis_proxy = deps.api.addr_validate(&osmosis_proxy)?;
            attrs.push(attr("new_osmosis_proxy", osmosis_proxy));
        }
        None => {}
    };
    match mbrn_denom {
        Some(mbrn_denom) => {
            config.mbrn_denom = mbrn_denom.clone();
            attrs.push(attr("new_mbrn_denom", mbrn_denom));
        }
        None => {}
    };
    if let Some(staking_contract) = staking_contract {
        config.staking_contract = deps.api.addr_validate(&staking_contract)?;
        attrs.push(attr("new_staking_contract", staking_contract));
    };

    CONFIG.save(deps.storage, &config)?;

    Ok(Response::new().add_attributes(attrs))
}

fn withdraw_unlocked(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let receivers = RECEIVERS.load(deps.storage)?;

    let message: CosmosMsg;

    let unlocked_amount: Uint128;
    let new_allocation: Allocation;

    match receivers
        .clone()
        .into_iter()
        .find(|receiver| receiver.receiver == info.sender)
    {
        Some(mut receiver) => {
            if receiver.allocation.is_some() {
                (unlocked_amount, new_allocation) =
                    get_unlocked_amount(receiver.allocation, env.block.time.seconds());

                //Save new allocation
                receiver.allocation = Some(new_allocation);

                let mut new_receivers = receivers
                    .into_iter()
                    .filter(|receiver| receiver.receiver != info.sender)
                    .collect::<Vec<Receiver>>();
                new_receivers.push(receiver.clone());

                RECEIVERS.save(deps.storage, &new_receivers)?;

                message = CosmosMsg::Bank(BankMsg::Send {
                    to_address: receiver.receiver.to_string(),
                    amount: vec![coin(unlocked_amount.u128(), config.mbrn_denom)],
                });
            } else {
                return Err(ContractError::InvalidAllocation {});
            }
        }
        None => return Err(ContractError::InvalidReceiver {}),
    };

    Ok(Response::new().add_message(message).add_attributes(vec![
        attr("method", "withdraw_unlocked"),
        attr("receiver", info.sender),
        attr("withdrawn_amount", String::from(unlocked_amount)),
    ]))
}

fn get_unlocked_amount(
    allocation: Option<Allocation>,
    current_block_time: u64, //in seconds
) -> (Uint128, Allocation) {
    let mut allocation = allocation.unwrap();

    let mut unlocked_amount = Uint128::zero();

    //Calculate unlocked amount
    let time_passed = current_block_time - allocation.clone().start_time_of_allocation;

    let cliff_in_seconds = allocation.clone().vesting_period.cliff * SECONDS_IN_A_DAY;

    //If cliff has been passed then calculate linear unlock
    if time_passed >= cliff_in_seconds {
        let time_passed_cliff = time_passed - cliff_in_seconds;

        let linear_in_seconds = allocation.clone().vesting_period.linear * SECONDS_IN_A_DAY;

        if time_passed_cliff < linear_in_seconds {
            //Unlock amount based off time into linear vesting period
            let ratio_unlocked = decimal_division(
                Decimal::from_ratio(Uint128::new(time_passed_cliff as u128), Uint128::new(1u128)),
                Decimal::from_ratio(Uint128::new(linear_in_seconds as u128), Uint128::new(1u128)),
            );

            let newly_unlocked: Uint128;
            if !ratio_unlocked.is_zero() {
                newly_unlocked = (ratio_unlocked * allocation.clone().amount)
                    - allocation.clone().amount_withdrawn;
            } else {
                newly_unlocked = Uint128::zero();
            }

            unlocked_amount += newly_unlocked;

            //Edit Allocation object
            allocation.amount_withdrawn += newly_unlocked;
        } else {
            //Unlock full amount
            unlocked_amount += allocation.clone().amount - allocation.clone().amount_withdrawn;

            allocation.amount_withdrawn += allocation.clone().amount;
        }
    }

    (unlocked_amount, allocation)
}

fn add_allocation(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    receiver: String,
    allocation: Uint128,
    vesting_period: VestingPeriod,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    //panic!("{:?}", RECEIVERS.load( deps.storage )?);

    //Add allocation for receiver
    RECEIVERS.update(
        deps.storage,
        |mut receivers| -> Result<Vec<Receiver>, ContractError> {
            //Add allocation
            receivers = receivers
                .into_iter()
                .map(|mut current_receiver| {
                    if current_receiver.receiver == receiver {
                        current_receiver.allocation = Some(Allocation {
                            amount: allocation,
                            amount_withdrawn: Uint128::zero(),
                            start_time_of_allocation: env.block.time.seconds(),
                            vesting_period: vesting_period.clone(),
                        });
                    }

                    current_receiver
                })
                .collect::<Vec<Receiver>>();

            Ok(receivers)
        },
    )?;

    //Get allocation total
    let mut allocation_total: Uint128 = Uint128::zero();

    for receiver in RECEIVERS.load(deps.storage)?.into_iter() {
        if receiver.allocation.is_some() {
            allocation_total += receiver.allocation.unwrap().amount;
        }
    }

    //Error if over allocating
    if allocation_total > config.initial_allocation {
        return Err(ContractError::OverAllocated {});
    }

    Ok(Response::new().add_attributes(vec![
        attr("method", "increase_allocation"),
        attr("receiver", receiver),
        attr("allocation_increase", String::from(allocation)),
    ]))
}

fn decrease_allocation(
    deps: DepsMut,
    info: MessageInfo,
    receiver: String,
    allocation: Uint128,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let error: Option<ContractError> = None;
    //Decrease allocation for receiver
    //If trying to decrease more than allocation, allocation set to 0
    RECEIVERS.update(
        deps.storage,
        |receivers| -> Result<Vec<Receiver>, ContractError> {
            //Decrease allocation
            Ok(receivers
                .into_iter()
                .map(|mut current_receiver| {
                    if current_receiver.receiver == receiver && current_receiver.allocation.is_some() {
                        match current_receiver
                            .allocation
                            .clone()
                            .unwrap()
                            .amount
                            .checked_sub(allocation)
                        {
                            Ok(difference) => {
                                current_receiver.allocation = Some(Allocation {
                                    amount: difference,
                                    ..current_receiver.allocation.clone().unwrap()
                                });
                            }
                            Err(_) => {
                                current_receiver.allocation = Some(Allocation {
                                    amount: Uint128::zero(),
                                    ..current_receiver.allocation.clone().unwrap()
                                });
                            }
                        };
                    }

                    current_receiver
                })
                .collect::<Vec<Receiver>>())
        },
    )?;

    if error.is_some() {
        return Err(error.unwrap());
    }

    Ok(Response::new().add_attributes(vec![
        attr("method", "decrease_allocation"),
        attr("receiver", receiver),
        attr("allocation_decrease", String::from(allocation)),
    ]))
}

fn add_receiver(
    deps: DepsMut,
    info: MessageInfo,
    receiver: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    let valid_receiver = deps.api.addr_validate(&receiver)?;

    //Add new Receiver
    RECEIVERS.update(
        deps.storage,
        |mut receivers| -> Result<Vec<Receiver>, ContractError> {
            if receivers
                .iter()
                .any(|receiver| receiver.receiver == valid_receiver)
            {
                return Err(ContractError::CustomError {
                    val: String::from("Duplicate receiver"),
                });
            }

            receivers.push(Receiver {
                receiver: valid_receiver,
                allocation: None,
                claimables: vec![],
            });

            Ok(receivers)
        },
    )?;

    Ok(Response::new().add_attributes(vec![
        attr("method", "add_receiver"),
        attr("receiver", receiver),
    ]))
}

fn remove_receiver(
    deps: DepsMut,
    info: MessageInfo,
    receiver: String,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    if info.sender != config.owner {
        return Err(ContractError::Unauthorized {});
    }

    //Remove Receiver
    RECEIVERS.update(
        deps.storage,
        |receivers| -> Result<Vec<Receiver>, ContractError> {
            //Filter out receiver and save
            Ok(receivers
                .into_iter()
                .filter(|current_receiver| current_receiver.receiver != receiver)
                .collect::<Vec<Receiver>>())
        },
    )?;

    Ok(Response::new().add_attributes(vec![
        attr("method", "remove_receiver"),
        attr("receiver", receiver),
    ]))
}

//Mint and stake initial allocation
fn mint_initial_allocation(env: Env, config: Config) -> Result<Response, ContractError> {
    let mut messages: Vec<CosmosMsg> = vec![];

    //Mint token msg in Osmosis Proxy
    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.osmosis_proxy.to_string(),
        msg: to_binary(&OsmoExecuteMsg::MintTokens {
            denom: config.clone().mbrn_denom,
            amount: config.initial_allocation,
            mint_to_address: env.contract.address.to_string(),
        })?,
        funds: vec![],
    }));

    //Stake msg to Staking contract
    messages.push(CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: config.staking_contract.to_string(),
        msg: to_binary(&StakingExecuteMsg::Stake { user: None })?,
        funds: vec![coin(config.initial_allocation.u128(), config.mbrn_denom)],
    }));

    Ok(Response::new().add_messages(messages).add_attributes(vec![
        attr("action", "mint_initial_allocation"),
        attr("allocation", config.initial_allocation.to_string()),
    ]))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&query_config(deps)?),
        QueryMsg::Allocation { receiver } => to_binary(&query_allocation(deps, receiver)?),
        QueryMsg::UnlockedTokens { receiver } => to_binary(&query_unlocked(deps, env, receiver)?),
        QueryMsg::Receivers {} => to_binary(&query_receivers(deps)?),
        QueryMsg::Receiver { receiver } => to_binary(&query_receiver(deps, receiver)?),
    }
}

fn query_config(deps: Deps) -> StdResult<ConfigResponse> {
    let config = CONFIG.load(deps.storage)?;

    Ok(ConfigResponse {
        owner: config.owner.to_string(),
        initial_allocation: config.initial_allocation.to_string(),
        mbrn_denom: config.mbrn_denom,
        osmosis_proxy: config.osmosis_proxy.to_string(),
        staking_contract: config.staking_contract.to_string(),
    })
}

fn query_allocation(deps: Deps, receiver: String) -> StdResult<AllocationResponse> {
    let receiver = match RECEIVERS
        .load(deps.storage)?
        .into_iter()
        .find(|stored_receiver| stored_receiver.receiver == receiver)
    {
        Some(receiver) => receiver,
        None => {
            return Err(StdError::GenericErr {
                msg: String::from("Invalid receiver"),
            })
        }
    };

    if receiver.allocation.is_some() {
        let allocation = receiver.allocation.unwrap();
        Ok(AllocationResponse {
            amount: allocation.amount.to_string(),
            amount_withdrawn: allocation.amount_withdrawn.to_string(),
            start_time_of_allocation: allocation.start_time_of_allocation.to_string(),
            vesting_period: allocation.vesting_period,
        })
    } else {
        Err(StdError::GenericErr {
            msg: String::from("Receiver has no allocation"),
        })
    }
}

fn query_unlocked(deps: Deps, env: Env, receiver: String) -> StdResult<UnlockedResponse> {
    let receiver = match RECEIVERS
        .load(deps.storage)?
        .into_iter()
        .find(|stored_receiver| stored_receiver.receiver == receiver)
    {
        Some(receiver) => receiver,
        None => {
            return Err(StdError::GenericErr {
                msg: String::from("Invalid receiver"),
            })
        }
    };

    if receiver.allocation.is_some() {
        let unlocked_amount = get_unlocked_amount(receiver.allocation, env.block.time.seconds()).0;
        Ok(UnlockedResponse { unlocked_amount })
    } else {
        Err(StdError::GenericErr {
            msg: String::from("Receiver has no allocation"),
        })
    }
}

fn query_receivers(deps: Deps) -> StdResult<Vec<ReceiverResponse>> {
    let receivers = RECEIVERS.load(deps.storage)?;

    let mut resp_list = vec![];

    for receiver in receivers {
        resp_list.push(ReceiverResponse {
            receiver: receiver.receiver.to_string(),
            allocation: receiver.allocation,
            claimables: receiver.claimables,
        })
    }

    Ok(resp_list)
}

fn query_receiver(deps: Deps, receiver: String) -> StdResult<ReceiverResponse> {
    let receivers = RECEIVERS.load(deps.storage)?;

    match receivers
        .into_iter()
        .find(|stored_receiver| stored_receiver.receiver == receiver)
    {
        Some(stored_receiver) => Ok(ReceiverResponse {
            receiver: stored_receiver.receiver.to_string(),
            allocation: stored_receiver.allocation,
            claimables: stored_receiver.claimables,
        }),
        None => {
            Err(StdError::GenericErr {
                msg: String::from("Invalid receiver"),
            })
        }
    }
}

pub fn withdrawal_msg(asset: Asset, recipient: Addr) -> StdResult<CosmosMsg> {
    match asset.clone().info {
        AssetInfo::Token { address } => {
            let message = CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr: address.to_string(),
                msg: to_binary(&Cw20ExecuteMsg::Transfer {
                    recipient: recipient.to_string(),
                    amount: asset.amount,
                })?,
                funds: vec![],
            });
            Ok(message)
        }
        AssetInfo::NativeToken { denom: _ } => {
            let coin: Coin = asset_to_coin(asset)?;
            let message = CosmosMsg::Bank(BankMsg::Send {
                to_address: recipient.to_string(),
                amount: vec![coin],
            });
            Ok(message)
        }
    }
}

pub fn asset_to_coin(asset: Asset) -> StdResult<Coin> {
    match asset.info {
        //
        AssetInfo::Token { address: _ } => {
            Err(StdError::GenericErr {
                msg: String::from("CW20 Assets can't be converted into Coin"),
            })
        }
        AssetInfo::NativeToken { denom } => Ok(Coin {
            denom,
            amount: asset.amount,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{from_binary, CosmosMsg, SubMsg};

    #[test]
    fn receivers() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {
            owner: Some(String::from("owner0000")),
            initial_allocation: Uint128::new(30_000_000_000_000u128),
            mbrn_denom: String::from("mbrn_denom"),
            osmosis_proxy: String::from("osmosis_proxy"),
            staking_contract: String::from("staking_contract"),
        };

        //Instantiating contract
        let v_info = mock_info("sender88", &[]);
        let _res = instantiate(deps.as_mut(), mock_env(), v_info.clone(), msg.clone()).unwrap();

        //AddReceiver
        let add_msg = ExecuteMsg::AddReceiver {
            receiver: String::from("receiver0000"),
        };
        let _res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            add_msg,
        )
        .unwrap();

        //Error: Duplicate Receiver
        let add_msg = ExecuteMsg::AddReceiver {
            receiver: String::from("receiver0000"),
        };
        let err = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            add_msg,
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            String::from("Custom Error val: \"Duplicate receiver\"")
        );

        //RemoveReceiver
        let add_msg = ExecuteMsg::RemoveReceiver {
            receiver: String::from("receiver0000"),
        };
        let _res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            add_msg,
        )
        .unwrap();

        //AddReceiver
        let add_msg = ExecuteMsg::AddReceiver {
            receiver: String::from("receiver0000"),
        };
        let _res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            add_msg,
        )
        .unwrap();

        //Query Receivers
        let msg = QueryMsg::Receivers {};
        let res = query(deps.as_ref(), mock_env(), msg).unwrap();

        let resp: Vec<ReceiverResponse> = from_binary(&res).unwrap();
        assert_eq!(resp[0].receiver, String::from("receiver0000"));
        assert_eq!(resp.len().to_string(), String::from("1"));
    }

    #[test]
    fn allocations() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {
            owner: Some(String::from("owner0000")),
            initial_allocation: Uint128::new(30_000_000_000_000u128),
            mbrn_denom: String::from("mbrn_denom"),
            osmosis_proxy: String::from("osmosis_proxy"),
            staking_contract: String::from("staking_contract"),
        };

        //Instantiating contract
        let v_info = mock_info("sender88", &[]);
        let _res = instantiate(deps.as_mut(), mock_env(), v_info.clone(), msg.clone()).unwrap();

        //AddReceiver
        let add_msg = ExecuteMsg::AddReceiver {
            receiver: String::from("receiver0000"),
        };
        let _res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            add_msg,
        )
        .unwrap();

        //AddAllocation
        let allocation_msg = ExecuteMsg::AddAllocation {
            receiver: String::from("receiver0000"),
            allocation: Uint128::new(1_000_000_000_000u128),
            vesting_period: VestingPeriod {
                cliff: 365u64,
                linear: 365u64,
            },
        };
        let _res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            allocation_msg,
        )
        .unwrap();

        //Decrease Allocation
        let allocation_msg = ExecuteMsg::DecreaseAllocation {
            receiver: String::from("receiver0000"),
            allocation: Uint128::new(500_000_000_000u128),
        };
        let _res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            allocation_msg,
        )
        .unwrap();

        //Query Allocation and assert Decrease
        let msg = QueryMsg::Allocation {
            receiver: String::from("receiver0000"),
        };
        let res = query(deps.as_ref(), mock_env(), msg).unwrap();

        let resp: AllocationResponse = from_binary(&res).unwrap();
        assert_eq!(resp.amount, String::from("500000000000"));

        //AddReceiver
        let add_msg = ExecuteMsg::AddReceiver {
            receiver: String::from("receiver1"),
        };
        let _res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            add_msg,
        )
        .unwrap();

        //Error: AddAllocation over Allocation limit
        let allocation_msg = ExecuteMsg::AddAllocation {
            receiver: String::from("receiver1"),
            allocation: Uint128::new(29_500_000_000_001u128),
            vesting_period: VestingPeriod {
                cliff: 365u64,
                linear: 365u64,
            },
        };
        let err = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            allocation_msg,
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            String::from("Increase is over contract's allocation")
        );
    }

    #[test]
    fn vesting_unlocks() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {
            owner: Some(String::from("owner0000")),
            initial_allocation: Uint128::new(30_000_000_000_000u128),
            mbrn_denom: String::from("mbrn_denom"),
            osmosis_proxy: String::from("osmosis_proxy"),
            staking_contract: String::from("staking_contract"),
        };

        //Instantiating contract
        let v_info = mock_info("sender88", &[]);
        let _res = instantiate(deps.as_mut(), mock_env(), v_info.clone(), msg.clone()).unwrap();

        //AddReceiver
        let add_msg = ExecuteMsg::AddReceiver {
            receiver: String::from("receiver0000"),
        };
        let _res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            add_msg,
        )
        .unwrap();

        //AddAllocation
        let allocation_msg = ExecuteMsg::AddAllocation {
            receiver: String::from("receiver0000"),
            allocation: Uint128::new(1_000_000_000_000u128),
            vesting_period: VestingPeriod {
                cliff: 365u64,
                linear: 365u64,
            },
        };
        let _res = execute(
            deps.as_mut(),
            mock_env(),
            mock_info("owner0000", &[]),
            allocation_msg,
        )
        .unwrap();

        //Query Unlocked
        let query_msg = QueryMsg::UnlockedTokens {
            receiver: String::from("receiver0000"),
        };
        //
        let mut env = mock_env();
        env.block.time = env.block.time.plus_seconds(47304000u64); //1.5 years
                                                                   //
        let res = query(deps.as_ref(), env.clone(), query_msg).unwrap();

        let resp: UnlockedResponse = from_binary(&res).unwrap();
        assert_eq!(resp.unlocked_amount, Uint128::new(500_000_000_000u128));

        ///Withdraw unlocked
        let withdraw_msg = ExecuteMsg::WithdrawUnlocked {};
        let res = execute(
            deps.as_mut(),
            env.clone(),
            mock_info("receiver0000", &[]),
            withdraw_msg,
        )
        .unwrap();

        //Can withdraw half since halfway thru linear vesting
        assert_eq!(
            res.attributes,
            vec![
                attr("method", "withdraw_unlocked"),
                attr("receiver", String::from("receiver0000")),
                attr("withdrawn_amount", String::from("500000000000")),
            ]
        );

        ///Withdraw unlocked but nothing to withdraw
        let withdraw_msg = ExecuteMsg::WithdrawUnlocked {};
        let res = execute(
            deps.as_mut(),
            env,
            mock_info("receiver0000", &[]),
            withdraw_msg,
        )
        .unwrap();

        //Can't withdraw anything bc no time has past since last withdrawal
        assert_eq!(
            res.attributes,
            vec![
                attr("method", "withdraw_unlocked"),
                attr("receiver", String::from("receiver0000")),
                attr("withdrawn_amount", String::from("0")),
            ]
        );
    }

    #[test]
    fn initial_allocation() {
        let mut deps = mock_dependencies();

        let msg = InstantiateMsg {
            owner: Some(String::from("owner0000")),
            initial_allocation: Uint128::new(30_000_000_000_000u128),
            mbrn_denom: String::from("mbrn_denom"),
            osmosis_proxy: String::from("osmosis_proxy"),
            staking_contract: String::from("staking_contract"),
        };

        //Instantiating contract
        let v_info = mock_info("sender88", &[]);
        let res = instantiate(deps.as_mut(), mock_env(), v_info.clone(), msg.clone()).unwrap();

        //Assert Mint and Stake Msgs
        assert_eq!(
            res.messages,
            vec![
                SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: String::from("osmosis_proxy"),
                    funds: vec![],
                    msg: to_binary(&OsmoExecuteMsg::MintTokens {
                        denom: String::from("mbrn_denom"),
                        amount: Uint128::new(30_000_000_000_000u128),
                        mint_to_address: String::from("cosmos2contract")
                    })
                    .unwrap()
                })),
                SubMsg::new(CosmosMsg::Wasm(WasmMsg::Execute {
                    contract_addr: String::from("staking_contract"),
                    funds: vec![coin(30_000_000_000_000, "mbrn_denom")],
                    msg: to_binary(&StakingExecuteMsg::Stake { user: None }).unwrap()
                })),
            ]
        );
    }
}
