///Expanded Fork of: https://github.com/astroport-fi/astroport-governance/tree/main/contracts/builder_unlock

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    attr, coin, to_binary, Addr, BankMsg, Binary, Coin, CosmosMsg, Decimal, Deps, DepsMut, Env,
    MessageInfo, QueryRequest, Response, StdError, StdResult, Uint128, WasmMsg, WasmQuery, QuerierWrapper,
};
use cw2::set_contract_version;
use cw20::Cw20ExecuteMsg;

use membrane::builder_vesting::{ ExecuteMsg, InstantiateMsg, QueryMsg };
use membrane::governance::{ExecuteMsg as GovExecuteMsg, ProposalMessage, ProposalVoteOption};
use membrane::math::decimal_division;
use membrane::osmosis_proxy::ExecuteMsg as OsmoExecuteMsg;
use membrane::staking::{
    ExecuteMsg as StakingExecuteMsg, QueryMsg as StakingQueryMsg, RewardsResponse, StakerResponse,
};
use membrane::types::{Allocation, Asset, AssetInfo, VestingPeriod};

use crate::error::ContractError;
use crate::query::{query_allocation, query_unlocked, query_receivers, query_receiver};
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
            expedited
        } => submit_proposal(deps, info, title, description, link, messages, expedited),
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

//Calls the Governance contract SubmitProposalMsg
fn submit_proposal(
    deps: DepsMut,
    info: MessageInfo,
    title: String,
    description: String,
    link: Option<String>,
    messages: Option<Vec<ProposalMessage>>,
    expedited: bool,
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
                    expedited,
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

//Calls the Governance contract CastVoteMsg
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

//Claim a receiver's proportion of staking rewards that were previously claimed using ClaimFeesForContract
fn claim_fees_for_receiver(deps: DepsMut, info: MessageInfo) -> Result<Response, ContractError> {

    //Load Receivers
    let mut receivers = RECEIVERS.load(deps.storage)?;

    let mut messages: Vec<CosmosMsg> = vec![];
    let claimables: Vec<Asset> = vec![];

    //Find Receiver claimables
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

            //Create withdraw msg for each claimable asset
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

//Claim staking rewards for all contract owned staked MBRN
fn claim_fees_for_contract(deps: DepsMut, env: Env) -> Result<Response, ContractError> {

    //Load Config
    let config = CONFIG.load(deps.storage)?;

    //Query Rewards
    let res: RewardsResponse = deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
        contract_addr: config.staking_contract.to_string(),
        msg: to_binary(&StakingQueryMsg::StakerRewards {
            staker: env.contract.address.to_string(),
        })?,
    }))?;

    //Split rewards w/ Receivers based on allocation amounts
    if res.claimables != vec![] {
        let receivers = RECEIVERS.load(deps.storage)?;

        let mut allocated_receivers: Vec<Receiver> = receivers
            .clone()
            .into_iter()
            .filter(|receiver| receiver.allocation.is_some())
            .collect::<Vec<Receiver>>();

        //Calculate allocation ratios
        let allocation_ratios = get_allocation_ratios(deps.querier, env.clone(), config.clone(), allocated_receivers.clone())?;
        
        //Add receiver's ratio of each claim asset to position
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

fn get_allocation_ratios(querier: QuerierWrapper, env: Env, config: Config, receivers: Vec<Receiver>) -> StdResult<Vec<Decimal>> {

    let mut allocation_ratios: Vec<Decimal> = vec![];

    //Get Contract's MBRN staked amount
    let staked_mbrn = querier.query_wasm_smart::<StakerResponse>(
        config.staking_contract, 
        &StakingQueryMsg::UserStake { staker: env.contract.address.to_string() }
    )?
    .total_staked;

    for receiver in receivers {        

        //Ratio of allocation.amount to total_staked
        allocation_ratios.push(decimal_division(
            Decimal::from_ratio(
                receiver.clone().allocation.unwrap().amount,
                Uint128::new(1u128),
            ),
            Decimal::from_ratio(staked_mbrn, Uint128::new(1u128)),
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


//Withdraw unvested MBRN
//If there is none to distribute in the contract, the amount will be unstaked
fn withdraw_unlocked(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let config = CONFIG.load(deps.storage)?;

    let receivers = RECEIVERS.load(deps.storage)?;

    let mut message: Option<CosmosMsg> = None;

    let unlocked_amount: Uint128;
    let mut unstaked_amount: Uint128 = Uint128::zero();
    let new_allocation: Allocation;

    //Find Receiver
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

                //If there is enough MBRN to send, send the unlocked amount
                //If not, unstake unlocked amount
                let mbrn_balance = get_contract_mbrn_balance(deps.querier, env, config.clone())?;
                
                if !unlocked_amount.is_zero(){
                    if mbrn_balance < unlocked_amount {
                        //MBRN unstake msg
                        message = Some(
                        CosmosMsg::Wasm(WasmMsg::Execute { 
                            contract_addr: config.staking_contract.to_string(), 
                            msg: to_binary(&StakingExecuteMsg::Unstake { mbrn_amount: Some(unlocked_amount) })?, 
                            funds: vec![],
                        }));

                        //Set to 0 for response
                        unstaked_amount = unlocked_amount;
                    } else {
                        //MBRN send msg
                        message = Some( CosmosMsg::Bank(BankMsg::Send {
                            to_address: receiver.receiver.to_string(),
                            amount: vec![coin(unlocked_amount.u128(), config.mbrn_denom)],
                        }) );
                    }  
                }            
                
                
            } else {
                return Err(ContractError::InvalidAllocation {});
            }
        }
        None => return Err(ContractError::InvalidReceiver {}),
    };


    if !unstaked_amount.is_zero() && message.is_some(){
        Ok(Response::new()
            .add_message(message.unwrap())
            .add_attributes(vec![
                attr("method", "withdraw_unlocked"),
                attr("receiver", info.sender),
                attr("unstaked_amount", String::from(unlocked_amount)),
            ])
        )
    } else if !unlocked_amount.is_zero() && message.is_some(){
        Ok(Response::new()
            .add_message(message.unwrap())
            .add_attributes(vec![
                attr("method", "withdraw_unlocked"),
                attr("receiver", info.sender),
                attr("withdrawn_amount", String::from(unlocked_amount)),
            ])
        )
    } else {
        Ok(Response::new()
            .add_attributes(vec![
                attr("method", "withdraw_unlocked"),
                attr("receiver", info.sender),
                attr("withdrawn_amount", String::from(unlocked_amount)),
            ])
        )
    }
}

//Get unvested amount 
pub fn get_unlocked_amount(
    //This is an option bc the receiver's allocation is. Its existence is confirmed beforehand.
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

//Add allocation to a receiver
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

    //Add allocation to a receiver
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

//Decrease allocation for receiver
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

//Add new Receiver
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

//Remove existing Receiver
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
        QueryMsg::Config {} => to_binary(&CONFIG.load(deps.storage)?),
        QueryMsg::Allocation { receiver } => to_binary(&query_allocation(deps, receiver)?),
        QueryMsg::UnlockedTokens { receiver } => to_binary(&query_unlocked(deps, env, receiver)?),
        QueryMsg::Receivers {} => to_binary(&query_receivers(deps)?),
        QueryMsg::Receiver { receiver } => to_binary(&query_receiver(deps, receiver)?),
    }
}


//Helper functions
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

pub fn get_contract_mbrn_balance(
    querier: QuerierWrapper,
    env: Env,
    config: Config,
) -> StdResult<Uint128> {

    Ok(
        querier
            .query_balance(env.clone().contract.address, config.mbrn_denom)?
            .amount
    )                

}